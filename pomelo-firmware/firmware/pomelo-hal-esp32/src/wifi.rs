//! Wi-Fi backend (ESP-IDF `esp_wifi`, via `firmware/components/board_hal/board_wifi.c`).
//!
//! Raw `extern "C"` bindings live in the private `ffi` module below; the safe
//! trait impl is a thin wrapper that maps the C status codes and `char[]`
//! buffers into Rust types. All calls are non-blocking — the C side caches
//! state in its event handlers.

use std::ffi::{c_char, CString};
use std::sync::Mutex;

use pomelo_hal::wifi_credentials::{self, WifiCredentials};
use pomelo_hal::{ApInfo, HalError, ScanState, WifiBackend, WifiState, WifiStatus};

mod ffi {
    use std::ffi::c_char;

    pub const SSID_MAX_LEN: usize = 33;
    pub const IP_MAX_LEN: usize = 16;
    pub const MAX_AP_RECORDS: usize = 20;

    /// Mirrors `hal_wifi_ap_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalWifiAp {
        pub ssid: [c_char; SSID_MAX_LEN],
        pub rssi: i8,
        pub channel: u8,
        pub secure: bool,
    }

    /// Mirrors `hal_wifi_status_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalWifiStatus {
        pub state: i32,
        pub ssid: [c_char; SSID_MAX_LEN],
        pub ip: [c_char; IP_MAX_LEN],
        pub netmask: [c_char; IP_MAX_LEN],
        pub gateway: [c_char; IP_MAX_LEN],
        pub rssi: i8,
    }

    extern "C" {
        pub fn hal_wifi_init() -> i32;
        pub fn hal_wifi_set_enabled(on: bool) -> i32;
        pub fn hal_wifi_is_enabled() -> bool;
        pub fn hal_wifi_scan_start() -> i32;
        pub fn hal_wifi_scan_get_state() -> i32;
        pub fn hal_wifi_scan_get_results(
            records: *mut HalWifiAp,
            max_count: u16,
            out_count: *mut u16,
        ) -> i32;
        pub fn hal_wifi_connect(ssid: *const c_char, password: *const c_char) -> i32;
        pub fn hal_wifi_disconnect() -> i32;
        pub fn hal_wifi_get_status(out_status: *mut HalWifiStatus) -> i32;

        /// Files on the built-in partition, through the C `stdio` shim — see the note above
        /// `read_credentials` for why these are not `std::fs`.
        pub fn hal_storage_read_file(
            path: *const c_char,
            out: *mut c_char,
            capacity: usize,
            out_len: *mut usize,
        ) -> i32;
        pub fn hal_storage_write_file(path: *const c_char, data: *const c_char, len: usize) -> i32;
        pub fn hal_storage_remove_file(path: *const c_char) -> i32;
    }
}

// C state codes (see board_hal.h).
const SCAN_STATE_SCANNING: i32 = 1;
const SCAN_STATE_DONE: i32 = 2;
const SCAN_STATE_ERROR: i32 = -1;

const STATE_SCANNING: i32 = 1;
const STATE_CONNECTING: i32 = 2;
const STATE_CONNECTED: i32 = 3;

fn c_buf_to_string(buf: &[c_char]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

pub struct EspWifi;

impl EspWifi {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for EspWifi {
    fn default() -> Self {
        Self::new()
    }
}

/// The network the file named when this backend was initialised.
///
/// A `static` and not a field, because `EspWifi` is a unit struct the board builds in a `const`
/// context, and a cache is not worth giving that up. The file is read once, in `init`, and never
/// again: `saved` is called by whatever draws the page, and a filesystem read behind a getter is a
/// cost nobody would see coming.
static SAVED: Mutex<Option<WifiCredentials>> = Mutex::new(None);

/// The largest credentials file this backend will read or write.
///
/// Four fields for a network name and a password. The bound is not a limitation to be raised later:
/// it is what lets a file too big to be one of these be an error rather than a truncated password.
const CREDENTIALS_CAPACITY: usize = 4096;

/// `ESP_ERR_NOT_FOUND`: the C side's "there is no such file" — the ordinary answer from a board that
/// has never been asked to remember anything, and not a fault.
const ESP_ERR_NOT_FOUND: i32 = 0x105;

/// The path as a C string. A NUL inside a path is impossible here and would be a bad argument.
fn c_path(path: &std::path::Path) -> Result<CString, HalError> {
    CString::new(path.to_string_lossy().as_ref()).map_err(|_| HalError::InvalidArg)
}

/// Read the credentials file.
///
/// # Why this is not `std::fs`
///
/// `std::fs` is how a file is written on every other platform, and on this board it cannot create one.
/// It was what wrote this file, and it failed silently in the one direction that looks like success:
/// `metadata`, `read_dir` and `create_dir_all` all work, reading an existing file works, and *every*
/// way of opening a file for writing — `File::create`, `OpenOptions` with and without truncate, with
/// `create_new`, with append — comes back ENOENT. So `save` returned `Err`, the settings page logged
/// it and moved on, and the board kept nothing. "It has forgotten my network" was never the flash
/// being rewritten; there was never a file.
///
/// The measurement is the reason for the shim and not a guess about which syscall the Rust runtime
/// picks: the C side's `fopen` has written this partition since `main.c` mounted it, so the write goes
/// through `fopen`, and the read goes through the same pair so that a file written cannot be one the
/// reader would not find.
///
/// `Ok(None)` is a board that has never been on a network; an `Err` is a file that is *there* and
/// wrong, which is a different problem and must not be answered as the first.
fn read_credentials(root: &str) -> Result<Option<WifiCredentials>, HalError> {
    let path = c_path(&wifi_credentials::path(root))?;

    let mut buffer = vec![0u8; CREDENTIALS_CAPACITY];
    let mut len = 0usize;

    let code = unsafe {
        ffi::hal_storage_read_file(
            path.as_ptr(),
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len(),
            &mut len,
        )
    };

    if code == ESP_ERR_NOT_FOUND {
        return Ok(None);
    }

    if code != 0 {
        return Err(HalError::Internal(code));
    }

    let text = String::from_utf8_lossy(&buffer[..len]);

    WifiCredentials::parse(&text)
        .map(Some)
        .map_err(|error| HalError::Io(error.to_string()))
}

/// Write the credentials file, over an existing one.
///
/// The C side makes the directories above it, so the `AppData/WIFI` path does not have to exist
/// first — which is what a board fresh out of the box has: no file, and no directory to put one in.
fn write_credentials(credentials: &WifiCredentials, root: &str) -> Result<(), HalError> {
    let text = credentials
        .to_file()
        .map_err(|error| HalError::Io(error.to_string()))?;

    let path = c_path(&wifi_credentials::path(root))?;
    let data = CString::new(text).map_err(|_| HalError::InvalidArg)?;

    let code = unsafe {
        ffi::hal_storage_write_file(path.as_ptr(), data.as_ptr(), data.as_bytes().len())
    };

    if code != 0 {
        return Err(HalError::Internal(code));
    }

    Ok(())
}

/// Remove the credentials file. Gone, or never there, are the same answer.
fn remove_credentials(root: &str) -> Result<(), HalError> {
    let path = c_path(&wifi_credentials::path(root))?;

    let code = unsafe { ffi::hal_storage_remove_file(path.as_ptr()) };

    if code != 0 {
        return Err(HalError::Internal(code));
    }

    Ok(())
}

impl WifiBackend for EspWifi {
    fn init(&mut self) -> Result<(), HalError> {
        // Read once, here. `Ok(None)` is a board that has never been on a network and is not worth a
        // line; an `Err` is a file that is there and unreadable, which is a different problem from
        // no file at all and must not be reported as one.
        match read_credentials(wifi_credentials::BOARD_APP_DATA) {
            Ok(saved) => {
                match &saved {
                    Some(saved) => eprintln!(
                        "[wifi] remembered {:?} (enabled={}, autoconnect={})",
                        saved.ssid, saved.enabled, saved.autoconnect
                    ),
                    // Said out loud rather than passed over: "the board forgot where it belongs" and
                    // "there was nothing to remember" look the same from the panel, and this line is
                    // the difference between them.
                    None => eprintln!("[wifi] no remembered network"),
                }
                *SAVED.lock().unwrap() = saved;
            }
            Err(error) => eprintln!("[wifi] the credentials file is unreadable: {error}"),
        }

        HalError::from_code(unsafe { ffi::hal_wifi_init() })?;

        Ok(())
    }

    /// What the board does, unasked, when it comes up.
    ///
    /// The file was read in `init`; this acts on what it said. The radio comes up **on** unless the
    /// file says a finger turned it off — the default is the page's, and it is where a person starts:
    /// a board that came up with the radio down could not even look for a network until someone found
    /// the switch. One connection is attempted only if the file names a network that is wanted.
    ///
    /// Called from a thread of its own by `rust_main`, so the panel is up before the radio is, and
    /// again by the settings page when the switch is turned back on.
    fn autoconnect(&mut self) -> Result<(), HalError> {
        let saved = self.saved();

        // No file is not "nothing was said", it is "nothing was said *yet*": a board out of the box
        // comes up ready to look for a network, and the first thing a person does with it is turn the
        // radio on. A file that carries the key is believed, because the only thing that writes it is
        // a finger on the switch.
        let wanted = saved.as_ref().map_or(true, |saved| saved.enabled);

        if !wanted {
            eprintln!("[wifi] the switch was left off; the radio stays down");
            return Ok(());
        }

        self.set_enabled(true)?;

        let Some(saved) = saved else {
            eprintln!("[wifi] the radio is up; nothing is remembered to join yet");
            return Ok(());
        };

        if !saved.autoconnect {
            eprintln!(
                "[wifi] {:?} is remembered, but not wanted at boot",
                saved.ssid
            );
            return Ok(());
        }

        eprintln!(
            "[wifi] boot: connecting to {:?}, because the file says autoconnect",
            saved.ssid
        );
        self.connect(&saved.ssid, &saved.password)
    }

    fn saved(&self) -> Option<WifiCredentials> {
        SAVED.lock().unwrap().clone()
    }

    fn remember(&mut self, credentials: &WifiCredentials) -> Result<(), HalError> {
        // Written before it is cached, and the cache is only touched once the write has returned: a
        // board that says it remembered a network it could not write down would reconnect happily
        // until the next boot and then have nothing, which is the failure this used to be.
        write_credentials(credentials, wifi_credentials::BOARD_APP_DATA)?;

        eprintln!("[wifi] remembered {:?}", credentials.ssid);
        *SAVED.lock().unwrap() = Some(credentials.clone());
        Ok(())
    }

    fn forget(&mut self) -> Result<(), HalError> {
        remove_credentials(wifi_credentials::BOARD_APP_DATA)?;

        eprintln!("[wifi] forgot the remembered network");
        *SAVED.lock().unwrap() = None;
        Ok(())
    }

    /// The switch, and nothing else.
    ///
    /// It does not connect and it does not write. Connecting here would mean the radio came up and
    /// immediately reached for a network nobody asked for — coming up and connecting are
    /// `autoconnect`'s job, and it is the only path to either. Writing here would put the file back
    /// in the driver's hands, which is the thing the file exists to stop.
    fn set_enabled(&mut self, on: bool) -> Result<(), HalError> {
        HalError::from_code(unsafe { ffi::hal_wifi_set_enabled(on) })
    }

    #[inline]
    fn is_enabled(&self) -> bool {
        unsafe { ffi::hal_wifi_is_enabled() }
    }

    fn scan_start(&mut self) -> Result<(), HalError> {
        HalError::from_code(unsafe { ffi::hal_wifi_scan_start() })
    }

    #[inline]
    fn scan_state(&self) -> ScanState {
        match unsafe { ffi::hal_wifi_scan_get_state() } {
            SCAN_STATE_SCANNING => ScanState::Scanning,
            SCAN_STATE_DONE => ScanState::Done,
            SCAN_STATE_ERROR => ScanState::Error,
            _ => ScanState::Idle,
        }
    }

    fn scan_results(&self) -> Result<Vec<ApInfo>, HalError> {
        if self.scan_state() != ScanState::Done {
            return Err(HalError::Busy);
        }

        let mut records = [ffi::HalWifiAp {
            ssid: [0; ffi::SSID_MAX_LEN],
            rssi: 0,
            channel: 0,
            secure: false,
        }; ffi::MAX_AP_RECORDS];
        let mut count: u16 = 0;

        let ret = unsafe {
            ffi::hal_wifi_scan_get_results(
                records.as_mut_ptr(),
                ffi::MAX_AP_RECORDS as u16,
                &mut count,
            )
        };
        if ret != 0 {
            return Err(HalError::Internal(ret));
        }

        let mut aps = Vec::with_capacity(count as usize);
        for rec in records.iter().take(count as usize) {
            aps.push(ApInfo::new(
                c_buf_to_string(&rec.ssid),
                rec.rssi,
                rec.secure,
                rec.channel,
            ));
        }
        Ok(aps)
    }

    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), HalError> {
        let c_ssid = CString::new(ssid).map_err(|_| HalError::InvalidArg)?;
        let c_pwd = CString::new(password).map_err(|_| HalError::InvalidArg)?;

        // Nothing is written here. The file belongs to the app: it is the app that holds the
        // password, and it is the app that hears the connection come up, so `remember` is called
        // from there. Writing at the *attempt* would mean a mistyped password destroys the one that
        // worked — which is exactly what the NVS copy this replaces did.
        HalError::from_code(unsafe { ffi::hal_wifi_connect(c_ssid.as_ptr(), c_pwd.as_ptr()) })
    }

    fn disconnect(&mut self) -> Result<(), HalError> {
        HalError::from_code(unsafe { ffi::hal_wifi_disconnect() })
    }

    fn status(&self) -> WifiStatus {
        let mut st = ffi::HalWifiStatus {
            state: 0,
            ssid: [0; ffi::SSID_MAX_LEN],
            ip: [0; ffi::IP_MAX_LEN],
            netmask: [0; ffi::IP_MAX_LEN],
            gateway: [0; ffi::IP_MAX_LEN],
            rssi: 0,
        };
        if unsafe { ffi::hal_wifi_get_status(&mut st) } != 0 {
            return WifiStatus::default();
        }

        WifiStatus {
            state: match st.state {
                STATE_SCANNING => WifiState::Scanning,
                STATE_CONNECTING => WifiState::Connecting,
                STATE_CONNECTED => WifiState::Connected,
                _ => WifiState::Disconnected,
            },
            ssid: c_buf_to_string(&st.ssid),
            ip: c_buf_to_string(&st.ip),
            netmask: c_buf_to_string(&st.netmask),
            gateway: c_buf_to_string(&st.gateway),
            rssi: st.rssi,
        }
    }
}
