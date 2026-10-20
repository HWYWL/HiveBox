//! What the host does on an application's behalf.
//!
//! Every call that leaves the sandbox lands here. The trait is small on purpose: it is the set of
//! questions an interface has to ask that a sandbox cannot answer itself — what time it is, how wide
//! this string is in the host's font, what was remembered from last time, what is in this file — and
//! everything else an application wants is drawing commands, which never cross this boundary at all.
//!
//! Implementations: [`DesktopServices`] for a PC and for tests, and (behind the `board` feature) a
//! version backed by `pomelo-hal` on the panel. The trait exists so the runtime can be tested without
//! a board and so a service can be refused without the VM knowing why.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use app_abi::{
    code, fault, nas, nas_shell, scan, state, AudioState, HttpProgressWord, NasSnapshot,
    SystemReadings, WifiSnapshot,
};

/// The board's implementation, on the panel and on the desktop simulator alike.
#[cfg(feature = "board")]
pub mod board;

#[cfg(feature = "board")]
pub use board::BoardServices;

/// One access point, as the radio reported it.
///
/// A `String` and not bytes: an SSID is a name somebody typed, and the one place this project reads
/// one is a page that has to draw it. A name that is not UTF-8 is read lossily, which is the same
/// answer a terminal gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiAp {
    pub ssid: String,
    /// dBm.
    pub rssi: i32,
    pub secure: bool,
    pub channel: u32,
}

/// One disk of the machine beside the box.
#[derive(Clone, Debug, PartialEq)]
pub struct NasDisk {
    pub device: String,
    pub model: String,
    pub size_bytes: u64,
    pub temperature_c: Option<f32>,
    pub busy_percent: f32,
    pub read_per_sec: u64,
    pub write_per_sec: u64,
    pub purpose: String,
}

/// What a cloud account's saved key looks like from this side: the masked id and its region, and
/// never the secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudKey {
    pub masked: String,
    pub region: String,
}

/// The questions an application may ask the host.
///
/// Default implementations are the "this host does not have that" answers, so an implementation for
/// a test can provide the two it cares about and leave the rest alone. None of them may block: a
/// service that waits is a frame that does not get drawn.
pub trait Services {
    /// Milliseconds since boot. The clock the frame call and [`event::TICK`] are given.
    fn now_ms(&mut self) -> i64 {
        0
    }

    /// The width of a string in the host's interface font, in logical pixels.
    fn text_width(&mut self, _text: &str, _size: f32) -> i32 {
        0
    }

    /// A remembered value, written into `out`; `None` when the key is not set.
    fn pref_get(&mut self, _key: &str, _out: &mut [u8]) -> Option<usize> {
        None
    }

    /// Remembers a value. `false` when it could not be written.
    fn pref_set(&mut self, _key: &str, _value: &[u8]) -> bool {
        false
    }

    /// A file from the application's private directory.
    fn file_read(&mut self, _name: &str, _out: &mut [u8]) -> Option<usize> {
        None
    }

    /// Writes a file in the application's private directory.
    fn file_write(&mut self, _name: &str, _data: &[u8]) -> bool {
        false
    }

    // -------------------------------------------------------------------------------------------
    // The network (permission `NETWORK`).
    // -------------------------------------------------------------------------------------------

    /// Asks for a document. Returns at once; [`Services::http_state`] is what came of it.
    fn http_start(&mut self, _url: &str) -> i32 {
        code::UNSUPPORTED
    }

    /// One of [`app_abi::state`].
    fn http_state(&mut self) -> u32 {
        state::IDLE
    }

    /// One of [`app_abi::fault`]: why the last request failed.
    fn http_fault(&mut self) -> u32 {
        fault::NONE
    }

    /// How much of the answer has arrived, when the backend keeps a counter.
    fn http_progress(&mut self) -> Option<HttpProgressWord> {
        None
    }

    /// Takes the answer of a request that finished: its status and its body. Called once.
    fn http_take(&mut self) -> Option<(u16, Vec<u8>)> {
        None
    }

    fn http_cancel(&mut self) -> i32 {
        code::UNSUPPORTED
    }

    // -------------------------------------------------------------------------------------------
    // The box's own readings (permission `SYSTEM`).
    // -------------------------------------------------------------------------------------------

    /// The box as it is right now. `None` on a host that cannot measure it.
    fn system(&mut self) -> Option<SystemReadings> {
        None
    }

    /// The version string built into the image.
    fn firmware_version(&mut self) -> Option<String> {
        None
    }

    // -------------------------------------------------------------------------------------------
    // The speaker (permission `AUDIO`).
    // -------------------------------------------------------------------------------------------

    /// Starts playing `path`. The path is the backend's to interpret; [`safe_component`] is for the
    /// application's own files and is not applied here, because a track may live anywhere on a
    /// mounted volume.
    fn audio_play(&mut self, _path: &str) -> i32 {
        code::UNSUPPORTED
    }

    /// One of [`app_abi::audio`].
    fn audio_control(&mut self, _op: u32, _value: i32) -> i32 {
        code::UNSUPPORTED
    }

    fn audio_state(&mut self) -> Option<AudioState> {
        None
    }

    // -------------------------------------------------------------------------------------------
    // The radio (permission `WIFI`).
    // -------------------------------------------------------------------------------------------

    fn wifi_status(&mut self) -> Option<WifiSnapshot> {
        None
    }

    fn wifi_scan_start(&mut self) -> i32 {
        code::UNSUPPORTED
    }

    /// One of [`app_abi::scan`].
    fn wifi_scan_state(&mut self) -> u32 {
        scan::IDLE
    }

    /// Every access point the last completed scan found, in the order the radio reported them.
    fn wifi_scan_results(&mut self) -> Vec<WifiAp> {
        Vec::new()
    }

    fn wifi_connect(&mut self, _ssid: &str, _password: &str) -> i32 {
        code::UNSUPPORTED
    }

    // -------------------------------------------------------------------------------------------
    // The machines beside the box (permission `NAS`).
    // -------------------------------------------------------------------------------------------

    fn nas_snapshot(&mut self) -> NasSnapshot {
        NasSnapshot {
            status: nas::UNCONFIGURED,
            ..NasSnapshot::default()
        }
    }

    fn nas_disks(&mut self) -> Vec<NasDisk> {
        Vec::new()
    }

    fn nas_refresh(&mut self) -> i32 {
        code::UNSUPPORTED
    }

    fn nas_shell_open(&mut self, _host: &str, _columns: u16, _rows: u16) -> i32 {
        code::UNSUPPORTED
    }

    fn nas_shell_write(&mut self, _bytes: &[u8]) -> i32 {
        code::UNSUPPORTED
    }

    /// What the session said since the last call: where it stands, why it was lost, and the bytes.
    /// Takes.
    fn nas_shell_take(&mut self) -> (u32, u32, Vec<u8>) {
        (nas_shell::CLOSED, fault::NONE, Vec::new())
    }

    fn nas_shell_close(&mut self) -> i32 {
        code::UNSUPPORTED
    }

    // -------------------------------------------------------------------------------------------
    // An account's machines (permission `CLOUD`).
    // -------------------------------------------------------------------------------------------

    fn cloud_key(&mut self, _vendor: u32) -> Option<CloudKey> {
        None
    }

    /// A vendor code for a list, `16 + provider` for a machine: the ABI's numbering.
    fn cloud_start(&mut self, _target: u32, _action: u32, _instance: &str, _region: &str) -> i32 {
        code::UNSUPPORTED
    }

    fn cloud_state(&mut self) -> u32 {
        state::IDLE
    }

    /// Takes the answer, encoded as [`app_abi::cloud`] describes.
    fn cloud_take(&mut self) -> Option<String> {
        None
    }

    fn cloud_cancel(&mut self) -> i32 {
        code::UNSUPPORTED
    }

    /// One line on the serial log.
    fn log(&mut self, _message: &str) {}

    /// The application asked for a frame of its own accord.
    fn invalidate(&mut self) {}

    /// The application asked to close.
    fn exit(&mut self) {}
}

/// The host services a PC run of the runtime uses.
///
/// Files and preferences live under one base directory, which is the application's own directory in
/// miniature: `<base>/prefs/<id>/<key>` and `<base>/files/<id>/<name>`. Nothing here may escape that
/// directory, and the checks are the same checks the board's implementation will make — a name is a
/// name, not a path.
pub struct DesktopServices {
    id: String,
    base: PathBuf,
    started: Instant,
    /// Text metrics without a font engine: a CJK character is one em, a Latin one is half. It is a
    /// ruler good enough for layout to be tested, and the board replaces it with the real one.
    cjk_per_em: f32,
    latin_per_em: f32,
    log: bool,
    pub invalidated: bool,
    pub exited: bool,
    /// Preferences held in memory as well as on disk, so a test does not have to read a file to see
    /// whether the write happened.
    pub prefs: HashMap<String, Vec<u8>>,
}

impl DesktopServices {
    /// Services for an application whose private directory is `<base>/<id>`.
    pub fn new(base: impl Into<PathBuf>, id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base: base.into(),
            started: Instant::now(),
            cjk_per_em: 1.0,
            latin_per_em: 0.5,
            log: std::env::var_os("HB_APP_LOG").is_some(),
            invalidated: false,
            exited: false,
            prefs: HashMap::new(),
        }
    }

    pub fn with_log(mut self, log: bool) -> Self {
        self.log = log;
        self
    }

    fn prefs_dir(&self) -> PathBuf {
        self.base.join("prefs").join(&self.id)
    }

    fn files_dir(&self) -> PathBuf {
        self.base.join("files").join(&self.id)
    }

    /// A file name is one component under the application's own directory, and nothing else.
    fn file_path(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).map(|name| self.files_dir().join(name))
    }
}

impl Services for DesktopServices {
    fn now_ms(&mut self) -> i64 {
        self.started.elapsed().as_millis() as i64
    }

    fn text_width(&mut self, text: &str, size: f32) -> i32 {
        text.chars()
            .map(|c| {
                let per_em = if c.is_ascii() {
                    self.latin_per_em
                } else {
                    self.cjk_per_em
                };

                (per_em * size) as i32
            })
            .sum()
    }

    fn pref_get(&mut self, key: &str, out: &mut [u8]) -> Option<usize> {
        if let Some(value) = self.prefs.get(key) {
            let len = value.len().min(out.len());
            out[..len].copy_from_slice(&value[..len]);
            return Some(len);
        }

        let path = self.prefs_dir().join(safe_component(key)?);
        let value = std::fs::read(path).ok()?;
        let len = value.len().min(out.len());
        out[..len].copy_from_slice(&value[..len]);
        self.prefs.insert(key.to_string(), value);

        Some(len)
    }

    fn pref_set(&mut self, key: &str, value: &[u8]) -> bool {
        let Some(name) = safe_component(key) else {
            return false;
        };

        self.prefs.insert(key.to_string(), value.to_vec());

        let dir = self.prefs_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }

        std::fs::write(dir.join(name), value).is_ok()
    }

    fn file_read(&mut self, name: &str, out: &mut [u8]) -> Option<usize> {
        let path = self.file_path(name)?;
        let value = std::fs::read(path).ok()?;
        let len = value.len().min(out.len());
        out[..len].copy_from_slice(&value[..len]);
        Some(len)
    }

    fn file_write(&mut self, name: &str, data: &[u8]) -> bool {
        let Some(path) = self.file_path(name) else {
            return false;
        };

        let dir = self.files_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }

        std::fs::write(path, data).is_ok()
    }

    fn log(&mut self, message: &str) {
        if !self.log {
            return;
        }

        let _ = writeln!(std::io::stderr(), "[app {}] {message}", self.id);
    }

    fn invalidate(&mut self) {
        self.invalidated = true;
    }

    fn exit(&mut self) {
        self.exited = true;
    }

    /// A desktop has no battery, no radio and no panel heap. What it does have is a clock, and the
    /// uptime is the one reading that is true rather than invented.
    fn system(&mut self) -> Option<SystemReadings> {
        Some(SystemReadings {
            uptime_secs: self.started.elapsed().as_secs().min(u32::MAX as u64) as u32,
            wifi_state: app_abi::wifi::DISCONNECTED,
            ..SystemReadings::default()
        })
    }

    fn firmware_version(&mut self) -> Option<String> {
        Some(format!("{}-desktop", env!("CARGO_PKG_VERSION")))
    }
}

/// A path component that cannot escape the directory it is joined to.
///
/// The sandbox checks this on every host service that takes a name: an application may write `a.txt`
/// or `notes/2026.md` but not `../wifi.conf`, and not an absolute path. Slash is allowed because
/// nested names are useful; `..`, a leading slash and an empty segment are not.
pub fn safe_component(name: &str) -> Option<&str> {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') || name.contains('\0') {
        return None;
    }

    if name.starts_with('.') && name != "." {
        // A leading dot is how the installer keeps its own directories out of the catalog; an
        // application's files do not need one.
        return None;
    }

    if name
        .split(['/', '\\'])
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }

    Some(name)
}

/// A services implementation for tests: everything in memory, no filesystem.
#[derive(Default)]
pub struct MemoryServices {
    pub prefs: HashMap<String, Vec<u8>>,
    pub files: HashMap<String, Vec<u8>>,
    pub logs: Vec<String>,
    pub invalidated: bool,
    pub exited: bool,
    pub now: i64,
}

impl Services for MemoryServices {
    fn now_ms(&mut self) -> i64 {
        self.now
    }

    fn text_width(&mut self, text: &str, size: f32) -> i32 {
        text.chars()
            .map(|c| if c.is_ascii() { size * 0.5 } else { size } as i32)
            .sum()
    }

    fn pref_get(&mut self, key: &str, out: &mut [u8]) -> Option<usize> {
        let value = self.prefs.get(key)?;
        let len = value.len().min(out.len());
        out[..len].copy_from_slice(&value[..len]);
        Some(len)
    }

    fn pref_set(&mut self, key: &str, value: &[u8]) -> bool {
        self.prefs.insert(key.to_string(), value.to_vec());
        true
    }

    fn file_read(&mut self, name: &str, out: &mut [u8]) -> Option<usize> {
        let value = self.files.get(name)?;
        let len = value.len().min(out.len());
        out[..len].copy_from_slice(&value[..len]);
        Some(len)
    }

    fn file_write(&mut self, name: &str, data: &[u8]) -> bool {
        self.files.insert(name.to_string(), data.to_vec());
        true
    }

    fn log(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }

    fn invalidate(&mut self) {
        self.invalidated = true;
    }

    fn exit(&mut self) {
        self.exited = true;
    }
}

/// The directory a package's files live in, given the volume root and the id. Kept here so the
/// desktop and the board agree about the layout the catalog and the installer already use.
pub fn private_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id).join("data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_may_not_leave_the_applications_directory() {
        for name in ["../wifi.conf", "/etc/passwd", "a/../../b", "", ".hidden", "a//b"] {
            assert!(safe_component(name).is_none(), "{name:?} must be refused");
        }

        for name in ["notes.txt", "notes/2026.md", "a-b_c.json"] {
            assert!(safe_component(name).is_some(), "{name:?} must be allowed");
        }
    }

    /// A host with no radio, no speaker and no account says so with the ABI's own code rather than
    /// with a zero that would read as success.
    #[test]
    fn a_host_without_a_service_refuses_it_by_name() {
        let mut services = MemoryServices::default();

        assert_eq!(services.http_start("https://example.com/"), code::UNSUPPORTED);
        assert_eq!(services.http_state(), state::IDLE);
        assert_eq!(services.http_take(), None);

        assert_eq!(services.audio_play("/sdcard/a.wav"), code::UNSUPPORTED);
        assert_eq!(services.wifi_scan_start(), code::UNSUPPORTED);
        assert!(services.wifi_scan_results().is_empty());
        assert_eq!(services.nas_refresh(), code::UNSUPPORTED);
        assert_eq!(services.nas_shell_take().0, nas_shell::CLOSED);
        assert_eq!(services.nas_shell_take().1, fault::NONE);
        assert_eq!(services.cloud_start(0, 0, "", ""), code::UNSUPPORTED);
        assert_eq!(services.cloud_take(), None);

        // The first look is "nothing has been configured", which is a screen and not a failure.
        assert_eq!(services.nas_snapshot().status, nas::UNCONFIGURED);
    }

    #[test]
    fn memory_services_answer_the_questions_a_test_asks() {
        let mut services = MemoryServices::default();
        assert_eq!(services.now_ms(), 0);

        assert!(services.pref_set("last", b"7"));
        let mut out = [0u8; 8];
        assert_eq!(services.pref_get("last", &mut out), Some(1));
        assert_eq!(&out[..1], b"7");

        assert!(services.file_write("a.txt", b"hello"));
        assert_eq!(services.file_read("a.txt", &mut out), Some(5));
        assert_eq!(&out[..5], b"hello");
    }
}
