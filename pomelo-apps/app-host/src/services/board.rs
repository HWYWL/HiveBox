//! The host services, backed by the board the launcher was handed.
//!
//! This is the implementation that matters on the panel, and it is also the one the desktop build
//! uses: `pomelo-hal`'s simulator implements the same traits, so a package is refused by the same
//! permission check and asks the same questions of the same types whether it is running on a PC or
//! on the box. What changes is only what is behind the traits.
//!
//! # Where an application's own files live
//!
//! Under the package's `data/` directory, and written through [`StorageBackend::write_file`] rather
//! than `std::fs`: on this board `std::fs` reads, stats, lists and makes directories, and cannot
//! create a *file*. Reads go through `std::fs` because reading was never the broken half.
//!
//! # What is not here
//!
//! No caching and no policy. Each call is answered by the backend that owns the reading; the ones
//! that are asynchronous — a request, a scan, a look at the NAS — are started and polled exactly as
//! their traits describe, and the application polls them once a frame. A service that blocked would
//! be a frame that did not get drawn, which is the one thing this layer may not do.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use app_abi::{
    audio as audio_op, cloud as cloud_wire, code, dir_entry, fault, nas, nas_shell, scan, state,
    AudioState, HttpProgressWord, NasSnapshot, SystemReadings, WifiSnapshot, TARGET_MACHINE,
};
use pomelo_hal::{
    AudioMeta, Board, CloudAction, CloudAnswer, CloudFault, CloudInstanceState, CloudProvider,
    CloudState, CloudTarget, CloudVendor, HalError, HttpFault, HttpProgress, HttpState, NasFault,
    NasShellStatus, NasStatus, ScanState, WebStatus, WifiState,
};

use super::{safe_component, CloudKey, NasDisk, Services, WifiAp};

/// The board's services for one application.
pub struct BoardServices {
    board: Arc<Board>,
    id: String,
    /// The package's own directory: preferences under `prefs/`, files beside them.
    dir: PathBuf,
    started: Instant,
    log: bool,
    /// Why the last request failed, kept because [`HttpState`] carries a fault only while the
    /// state is read, and the ABI asks for it in a second call.
    http_fault: u32,
    /// A request waiting for the one HTTP slot the board has. The store and an application share
    /// that slot, and a `BUSY` a person can neither see nor do anything about is worse than a wait:
    /// the second request is held here and started when the first answer has been taken.
    queued_url: Option<String>,
    /// A held request the backend refused when its turn came. Kept as a state because by then the
    /// application is already polling, and the answer it needs is `FAILED`, not `IDLE`.
    http_failed: bool,
    /// How long the current track is, read when it was opened.
    duration_ms: u32,
    /// The directory the application has open, read once and held rather than re-read per entry: a
    /// listing is a snapshot, and a page that asked the card again for every row would be a page
    /// that waits on the card. One directory at a time, which is what `hb_dir_next` means by "the
    /// next".
    listing: Vec<DirEntry>,
    listing_at: usize,
    /// Set by the application and read by the launcher.
    pub invalidated: bool,
    pub exited: bool,
}

/// One line of a listing, held between [`Services::dir_open`] and [`Services::dir_next`].
struct DirEntry {
    kind: u32,
    name: String,
    size: u64,
}

impl BoardServices {
    /// Services for `id`, whose private directory is `dir`.
    pub fn new(board: Arc<Board>, dir: impl Into<PathBuf>) -> Self {
        Self {
            board,
            id: String::new(),
            dir: dir.into(),
            started: Instant::now(),
            log: std::env::var_os("HB_APP_LOG").is_some(),
            http_fault: fault::NONE,
            queued_url: None,
            http_failed: false,
            duration_ms: 0,
            listing: Vec::new(),
            listing_at: 0,
            invalidated: false,
            exited: false,
        }
    }

    /// The same services, named for the log line.
    pub fn for_app(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_log(mut self, log: bool) -> Self {
        self.log = log;
        self
    }

    fn file_path(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).map(|name| self.dir.join(name))
    }

    fn pref_path(&self, key: &str) -> Option<PathBuf> {
        safe_component(key).map(|key| self.dir.join("prefs").join(key))
    }

    /// Whether `path` is somewhere an application may look: under one of the mounts the storage
    /// backend reports, and with no `..` anywhere in it.
    ///
    /// Refused rather than cleaned up. A path with a `..` in it is not a path somebody mistyped, and
    /// an answer of `INVALID` is one the application can draw; silently walking it back inside the
    /// mount would be a listing of somewhere the person did not ask for, drawn as if it were.
    fn volume_path(&self, path: &str) -> Option<String> {
        if path.is_empty() || path.split('/').any(|part| part == "..") {
            return None;
        }

        let mounts: Vec<String> = self
            .board
            .storage()
            .volumes()
            .ok()?
            .into_iter()
            .map(|volume| volume.mount_point.trim_end_matches('/').to_string())
            .collect();

        let path = path.trim_end_matches('/').to_string();

        mounts
            .iter()
            .any(|mount| path == *mount || path.starts_with(&format!("{mount}/")))
            .then_some(path)
    }

    /// Writes through the backend, which is the only writer that makes a file on the panel.
    fn write(&self, path: &Path, data: &[u8]) -> bool {
        let path = path.to_string_lossy();

        self.board.storage().write_file(&path, data).is_ok()
    }

    /// Starts the held request once the slot is free.
    ///
    /// Only an `Idle` slot counts as free: `Done` is an answer nobody has taken yet, and starting
    /// over it would throw away the very bytes the request was made for.
    fn pump_http(&mut self) {
        if !matches!(self.board.http().state(), HttpState::Idle) {
            return;
        }

        let Some(url) = self.queued_url.take() else {
            return;
        };

        match self.board.http().start(&url) {
            Ok(()) => {}
            // Somebody slipped in between the state read and the start. The URL keeps its place.
            Err(HalError::Busy) => self.queued_url = Some(url),
            Err(error) => {
                // A request that could not start is still a request that did not come back. Its
                // fault is the one that describes a start rather than an exchange.
                self.http_fault = match error {
                    HalError::NotSupported => fault::UNSUPPORTED,
                    HalError::InvalidArg => fault::ARGS,
                    _ => fault::UNREACHABLE,
                };
                self.http_failed = true;
            }
        }
    }
}

impl Services for BoardServices {
    fn now_ms(&mut self) -> i64 {
        // The board's own uptime, so that two services of the same application agree about when
        // "now" is — the frame clock, a request's age, and a session's age are all the same clock.
        match self.board.system().uptime() {
            Ok(uptime) => uptime.as_millis() as i64,
            Err(_) => self.started.elapsed().as_millis() as i64,
        }
    }

    fn text_width(&mut self, text: &str, size: f32) -> i32 {
        // A ruler without a glyph cache: one em for a CJK character, half for a Latin one. The
        // launcher's own font is the authority on the drawn width, and this is deliberately the
        // approximation the desktop runner uses so that a layout tested there is the same shape.
        text.chars()
            .map(|c| {
                let per_em = if c.is_ascii() { 0.5 } else { 1.0 };
                (per_em * size) as i32
            })
            .sum()
    }

    fn pref_get(&mut self, key: &str, out: &mut [u8]) -> Option<usize> {
        let path = self.pref_path(key)?;
        let value = std::fs::read(path).ok()?;
        let len = value.len().min(out.len());
        out[..len].copy_from_slice(&value[..len]);

        Some(len)
    }

    fn pref_set(&mut self, key: &str, value: &[u8]) -> bool {
        let Some(path) = self.pref_path(key) else {
            return false;
        };

        self.write(&path, value)
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

        self.write(&path, data)
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

    // -------------------------------------------------------------------------------------------
    // The network.
    // -------------------------------------------------------------------------------------------

    fn http_start(&mut self, url: &str) -> i32 {
        self.http_failed = false;

        match self.board.http().start(url) {
            Ok(()) => {
                self.http_fault = fault::NONE;
                code::OK
            }
            // Somebody else — the store, another frame's request — owns the one slot. Hold this
            // one behind it rather than telling the application to try again: it cannot know when
            // "again" is, and a person watching a progress bar cannot either.
            Err(HalError::Busy) => {
                if self.queued_url.is_some() {
                    return code::BUSY;
                }

                self.queued_url = Some(url.to_string());
                self.http_fault = fault::NONE;
                code::OK
            }
            Err(error) => hal_code(error),
        }
    }

    fn http_state(&mut self) -> u32 {
        if self.http_failed {
            return state::FAILED;
        }

        if self.queued_url.is_some() {
            self.pump_http();

            if self.http_failed {
                return state::FAILED;
            }

            if self.queued_url.is_some() {
                return state::RUNNING;
            }
        }

        let state = self.board.http().state();

        match state {
            HttpState::Idle => state::IDLE,
            HttpState::Running => state::RUNNING,
            HttpState::Done => state::DONE,
            HttpState::Failed(error) => {
                self.http_fault = http_fault_code(error);
                state::FAILED
            }
        }
    }

    fn http_fault(&mut self) -> u32 {
        self.http_fault
    }

    fn http_progress(&mut self) -> Option<HttpProgressWord> {
        self.board
            .http()
            .progress()
            .map(|progress: HttpProgress| HttpProgressWord {
                received: word(progress.received),
                total: word(progress.total),
            })
    }

    fn http_take(&mut self) -> Option<(u16, Vec<u8>)> {
        self.board
            .http()
            .take_result()
            .ok()
            .map(|answer| (answer.status, answer.body))
    }

    fn http_cancel(&mut self) -> i32 {
        self.http_fault = fault::NONE;
        self.queued_url = None;
        self.http_failed = false;

        match self.board.http().cancel() {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    // -------------------------------------------------------------------------------------------
    // The box's own readings.
    // -------------------------------------------------------------------------------------------

    fn system(&mut self) -> Option<SystemReadings> {
        let mut readings = SystemReadings::default();

        if let Ok(percent) = self.board.power().battery_percent() {
            readings.battery_percent = u32::from(percent);
        }

        if let Ok(charging) = self.board.power().is_charging() {
            readings.charging = u32::from(charging);
        }

        if let Ok(millivolts) = self.board.power().battery_voltage_mv() {
            readings.battery_mv = millivolts;
        }

        if let Ok(memory) = self.board.system().memory() {
            readings.heap_free = word(memory.free_bytes);
            readings.heap_total = word(memory.total_bytes);
        }

        if let Ok(Some(psram)) = self.board.system().psram() {
            readings.psram_free = word(psram.free_bytes);
            readings.psram_total = word(psram.total_bytes);
        }

        if let Ok(uptime) = self.board.system().uptime() {
            readings.uptime_secs = word(uptime.as_secs());
        }

        if let Ok(volumes) = self.board.storage().volumes() {
            if let Some(volume) = volumes.first() {
                readings.storage_free = word(volume.free_bytes);
                readings.storage_total = word(volume.total_bytes);
            }
        }

        let status = self.board.wifi().status();
        readings.wifi_state = wifi_state_code(status.state);
        readings.wifi_bars = u32::from(status.signal_bars());

        Some(readings)
    }

    fn firmware_version(&mut self) -> Option<String> {
        self.board.system().firmware().ok().map(|firmware| firmware.version)
    }

    // -------------------------------------------------------------------------------------------
    // The speaker.
    // -------------------------------------------------------------------------------------------

    fn audio_play(&mut self, path: &str) -> i32 {
        let played = self.board.audio().play(path);

        match played {
            Ok(AudioMeta { duration_secs, .. }) => {
                self.duration_ms = (duration_secs.max(0.0) * 1000.0) as u32;
                code::OK
            }
            Err(error) => hal_code(error),
        }
    }

    fn audio_control(&mut self, op: u32, value: i32) -> i32 {
        match op {
            audio_op::PAUSE => {
                self.board.audio().pause();
                code::OK
            }
            audio_op::RESUME => {
                self.board.audio().resume();
                code::OK
            }
            audio_op::STOP => {
                self.board.audio().stop();
                self.duration_ms = 0;
                code::OK
            }
            audio_op::SEEK => {
                let seconds = (value.max(0) as f32) / 1000.0;

                match self.board.audio().seek(seconds) {
                    Ok(()) => code::OK,
                    Err(error) => hal_code(error),
                }
            }
            audio_op::VOLUME => {
                self.board.audio().set_volume(value.clamp(0, 100) as u8);
                code::OK
            }
            _ => code::INVALID,
        }
    }

    fn audio_state(&mut self) -> Option<AudioState> {
        let playing = self.board.audio().is_playing();
        let volume = self.board.audio().volume();
        let position = self.board.audio().position_secs();

        Some(AudioState {
            playing: u32::from(playing),
            volume: u32::from(volume),
            position_ms: (position.max(0.0) * 1000.0) as u32,
            duration_ms: self.duration_ms,
        })
    }

    // -------------------------------------------------------------------------------------------
    // The radio.
    // -------------------------------------------------------------------------------------------

    fn wifi_status(&mut self) -> Option<WifiSnapshot> {
        let status = self.board.wifi().status();

        Some(WifiSnapshot::new(
            wifi_state_code(status.state),
            i32::from(status.rssi),
            u32::from(status.signal_bars()),
            status.ssid.as_bytes(),
        ))
    }

    fn wifi_scan_start(&mut self) -> i32 {
        match self.board.wifi().scan_start() {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    fn wifi_scan_state(&mut self) -> u32 {
        scan_state_code(self.board.wifi().scan_state())
    }

    fn wifi_scan_results(&mut self) -> Vec<WifiAp> {
        self.board
            .wifi()
            .scan_results()
            .unwrap_or_default()
            .into_iter()
            .map(|ap| WifiAp {
                ssid: ap.ssid,
                rssi: i32::from(ap.rssi),
                secure: ap.secure,
                channel: u32::from(ap.channel),
            })
            .collect()
    }

    fn wifi_connect(&mut self, ssid: &str, password: &str) -> i32 {
        match self.board.wifi().connect(ssid, password) {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    // -------------------------------------------------------------------------------------------
    // The machines beside the box.
    // -------------------------------------------------------------------------------------------

    fn nas_snapshot(&mut self) -> NasSnapshot {
        let reading = self.board.nas().reading();
        let mut snapshot = NasSnapshot {
            status: nas::UNCONFIGURED,
            // The one "no answer" this struct can carry without a separate flag.
            temperature_c_x100: i32::MIN,
            ..NasSnapshot::default()
        };

        match reading.status {
            NasStatus::Unconfigured => snapshot.status = nas::UNCONFIGURED,
            NasStatus::Connecting => snapshot.status = nas::CONNECTING,
            NasStatus::Online => snapshot.status = nas::ONLINE,
            NasStatus::Offline(reason) => {
                snapshot.status = nas::OFFLINE;
                snapshot.fault = nas_fault_code(reason);
            }
        }

        if let Some(cpu) = reading.cpu {
            snapshot.has_cpu = 1;
            snapshot.cpu_x100 = percent_x100(cpu.usage_percent);
            snapshot.temperature_c_x100 = cpu
                .temperature_c
                .map(|temperature| (temperature * 100.0).round() as i32)
                .unwrap_or(i32::MIN);
        }

        if let Some(memory) = reading.memory {
            snapshot.has_memory = 1;
            snapshot.memory_used_kb = word(memory.used_bytes() / 1024);
            snapshot.memory_total_kb = word(memory.total_bytes / 1024);
        }

        if let Some(network) = reading.network {
            snapshot.net_sent_bps = word(network.sent_per_sec);
            snapshot.net_received_bps = word(network.received_per_sec);
        }

        if let Some(uptime) = reading.uptime {
            snapshot.uptime_secs = word(uptime.as_secs());
        }

        if let Some(age) = reading.age() {
            snapshot.age_secs = word(age.as_secs());
        }

        snapshot.disks = reading.disks.len() as u32;
        snapshot
    }

    fn nas_disks(&mut self) -> Vec<NasDisk> {
        let reading = self.board.nas().reading();

        reading
            .disks
            .into_iter()
            .map(|disk| NasDisk {
                device: disk.device,
                model: disk.model,
                size_bytes: disk.size_bytes.unwrap_or(0),
                temperature_c: disk.temperature_c,
                busy_percent: disk.busy_percent,
                read_per_sec: disk.read_per_sec,
                write_per_sec: disk.write_per_sec,
                purpose: disk.purpose,
            })
            .collect()
    }

    fn nas_refresh(&mut self) -> i32 {
        match self.board.nas().refresh() {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    fn nas_devices(&mut self) -> Vec<(String, String)> {
        // 板子读过的那份配置就是答案：应用既不读文件，也不该知道密码。
        self.board
            .nas()
            .devices()
            .into_iter()
            .map(|device| (device.host, device.user))
            .collect()
    }

    fn nas_shell_open(&mut self, host: &str, columns: u16, rows: u16) -> i32 {
        match self.board.nas().shell_open(host, columns, rows) {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    fn nas_shell_write(&mut self, bytes: &[u8]) -> i32 {
        match self.board.nas().shell_write(bytes) {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    fn nas_shell_take(&mut self) -> (u32, u32, Vec<u8>) {
        let shell = self.board.nas().shell_take();

        let (status, reason) = match shell.status {
            NasShellStatus::Closed => (nas_shell::CLOSED, fault::NONE),
            NasShellStatus::Opening => (nas_shell::OPENING, fault::NONE),
            NasShellStatus::Ready => (nas_shell::READY, fault::NONE),
            NasShellStatus::Lost(reason) => (nas_shell::LOST, nas_fault_code(reason)),
        };

        (status, reason, shell.output)
    }

    fn nas_shell_close(&mut self) -> i32 {
        match self.board.nas().shell_close() {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    // -------------------------------------------------------------------------------------------
    // An account's machines.
    // -------------------------------------------------------------------------------------------

    fn cloud_key(&mut self, vendor: u32) -> Option<CloudKey> {
        let vendor = vendor_of(vendor)?;
        let key = self.board.cloud().key(vendor);

        Some(CloudKey {
            masked: key.masked,
            region: key.region,
        })
    }

    fn cloud_start(&mut self, target: u32, action: u32, instance: &str, region: &str) -> i32 {
        let Some(action) = cloud_action(action) else {
            return code::INVALID;
        };

        let Some(target) = cloud_target(target, action) else {
            return code::INVALID;
        };

        match self
            .board
            .cloud()
            .start(target, action, instance, region)
        {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    fn cloud_state(&mut self) -> u32 {
        match self.board.cloud().state() {
            CloudState::Idle => state::IDLE,
            CloudState::Running => state::RUNNING,
            CloudState::Done => state::DONE,
            CloudState::Failed(_) => state::FAILED,
        }
    }

    fn cloud_take(&mut self) -> Option<String> {
        self.board
            .cloud()
            .take_answer()
            .ok()
            .map(|answer| cloud_answer_text(&answer))
    }

    fn cloud_cancel(&mut self) -> i32 {
        match self.board.cloud().cancel() {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }

    // -----------------------------------------------------------------------------------------
    // What is on the two volumes.
    // -----------------------------------------------------------------------------------------

    fn dir_open(&mut self, path: &str) -> i32 {
        let Some(path) = self.volume_path(path) else {
            return code::INVALID;
        };

        let Ok(entries) = std::fs::read_dir(&path) else {
            return code::IO;
        };

        let mut listing: Vec<DirEntry> = entries
            .flatten()
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                let kind = match entry.file_type() {
                    Ok(kind) if kind.is_dir() => dir_entry::DIR,
                    _ => dir_entry::FILE,
                };

                DirEntry { kind, name, size }
            })
            .collect();

        // Directories first, then files, each by name: the order a person looking for something
        // expects, and one the application would otherwise have to sort forty lines to get.
        listing.sort_by(|left, right| {
            right
                .kind
                .cmp(&left.kind)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });

        self.listing = listing;
        self.listing_at = 0;

        code::OK
    }

    fn dir_next(&mut self, out: &mut [u8]) -> Option<usize> {
        let entry = self.listing.get(self.listing_at)?;
        self.listing_at += 1;

        let mut line = String::new();
        let _ = write!(line, "{}\t{}\t{}", entry.kind, entry.name, entry.size);
        let bytes = line.as_bytes();

        let len = bytes.len().min(out.len());
        out[..len].copy_from_slice(&bytes[..len]);

        Some(len)
    }

    fn dir_close(&mut self) {
        self.listing.clear();
        self.listing_at = 0;
    }

    fn path_remove(&mut self, path: &str) -> i32 {
        let Some(path) = self.volume_path(path) else {
            return code::INVALID;
        };

        let target = Path::new(&path);
        let result = if target.is_dir() {
            // The backend removes files and not trees, so a directory goes through `std::fs`, which
            // is the same call the file manager in the image makes for the same button.
            std::fs::remove_dir_all(&path)
        } else {
            self.board.storage().remove_file(&path).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "the backend refused")
            })
        };

        match result {
            Ok(()) => code::OK,
            Err(_) => code::IO,
        }
    }

    // -----------------------------------------------------------------------------------------
    // The board's own web server.
    // -----------------------------------------------------------------------------------------

    fn web_status(&mut self) -> Option<(bool, String)> {
        let status = self.board.web().status();
        let url = status.url(&self.board.wifi().status());

        // Empty rather than absent while it is up but unreachable: "running with no address" is a
        // thing a page has to be able to say, and it is not the same as "this host has no server".
        Some((status.running, url.unwrap_or_default()))
    }

    fn web_set(&mut self, on: bool) -> i32 {
        let mut web = self.board.web();

        let result = if on {
            web.start(WebStatus::DEFAULT_PORT)
        } else {
            web.stop()
        };

        match result {
            Ok(()) => code::OK,
            Err(error) => hal_code(error),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The translations between this ABI and the HAL's own vocabulary
// ---------------------------------------------------------------------------------------------

/// A 64-bit count as the 32-bit word the wire carries. Sizes, byte counters and millivolt readings
/// are all far below the limit on this board; saturating is the honest answer if one ever is not.
fn word(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

/// A share, `0.0..=100.0`, as hundredths.
fn percent_x100(percent: f32) -> u32 {
    (percent.max(0.0) * 100.0).round().min(10_000.0) as u32
}

/// The ABI's code for a backend failure.
fn hal_code(error: HalError) -> i32 {
    match error {
        HalError::NotSupported => code::UNSUPPORTED,
        HalError::Busy => code::BUSY,
        HalError::InvalidArg => code::INVALID,
        HalError::NotInitialized | HalError::Timeout | HalError::Io(_) | HalError::Internal(_) => {
            code::IO
        }
    }
}

fn http_fault_code(fault: HttpFault) -> u32 {
    match fault {
        HttpFault::NoNetwork => app_abi::fault::NETWORK,
        HttpFault::ClockNotSet => app_abi::fault::CLOCK,
        HttpFault::Unreachable => app_abi::fault::UNREACHABLE,
        HttpFault::Timeout => app_abi::fault::TIMEOUT,
        HttpFault::Tls => app_abi::fault::TLS,
        HttpFault::TooLarge => app_abi::fault::TOO_LARGE,
        HttpFault::Storage => app_abi::fault::STORAGE,
        HttpFault::Unsupported => app_abi::fault::UNSUPPORTED,
    }
}

fn nas_fault_code(fault: NasFault) -> u32 {
    match fault {
        NasFault::NoNetwork => app_abi::fault::NETWORK,
        NasFault::Unreachable => app_abi::fault::UNREACHABLE,
        NasFault::Authentication => app_abi::fault::AUTHENTICATION,
        NasFault::HostKeyChanged => app_abi::fault::HOST_KEY,
        NasFault::Protocol => app_abi::fault::PROTOCOL,
        NasFault::Unsupported => app_abi::fault::UNSUPPORTED,
    }
}

fn cloud_fault_code(fault: CloudFault) -> u32 {
    match fault {
        CloudFault::Failed => app_abi::fault::UNSUPPORTED,
        CloudFault::Args => app_abi::fault::ARGS,
        CloudFault::NoCredentials => app_abi::fault::NO_CREDENTIALS,
        CloudFault::NoNetwork => app_abi::fault::NETWORK,
        CloudFault::ClockNotSet => app_abi::fault::CLOCK,
        CloudFault::Unreachable => app_abi::fault::UNREACHABLE,
        CloudFault::Tls => app_abi::fault::TLS,
        CloudFault::Http => app_abi::fault::UNREACHABLE,
        CloudFault::Api => app_abi::fault::API,
        CloudFault::Storage => app_abi::fault::STORAGE,
        CloudFault::Unsupported => app_abi::fault::UNSUPPORTED,
    }
}

fn wifi_state_code(state: WifiState) -> u32 {
    match state {
        WifiState::Disconnected => app_abi::wifi::DISCONNECTED,
        WifiState::Scanning => app_abi::wifi::SCANNING,
        WifiState::Connecting => app_abi::wifi::CONNECTING,
        WifiState::Connected => app_abi::wifi::CONNECTED,
    }
}

fn scan_state_code(state: ScanState) -> u32 {
    match state {
        ScanState::Idle => scan::IDLE,
        ScanState::Scanning => scan::SCANNING,
        ScanState::Done => scan::DONE,
        ScanState::Error => scan::ERROR,
    }
}

fn vendor_of(code: u32) -> Option<CloudVendor> {
    CloudVendor::ALL.get(code as usize).copied()
}

fn cloud_action(code: u32) -> Option<CloudAction> {
    CloudAction::ALL.get(code as usize).copied()
}

/// A vendor for a list, a product for a power command: the ABI's `16 + provider` convention.
fn cloud_target(target: u32, action: CloudAction) -> Option<CloudTarget> {
    match action {
        CloudAction::List => vendor_of(target).map(CloudTarget::Vendor),
        _ => CloudProvider::from_index(target.checked_sub(TARGET_MACHINE)? as usize)
            .map(CloudTarget::Machine),
    }
}

fn instance_state_code(state: CloudInstanceState) -> u32 {
    match state {
        CloudInstanceState::Running => cloud_wire::RUNNING,
        CloudInstanceState::Stopped => cloud_wire::STOPPED,
        CloudInstanceState::Starting => cloud_wire::STARTING,
        CloudInstanceState::Stopping => cloud_wire::STOPPING,
        CloudInstanceState::Rebooting => cloud_wire::REBOOTING,
        CloudInstanceState::Other => cloud_wire::OTHER,
    }
}

/// One field of a line-oriented answer without the characters that would end it early.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

/// [`CloudAnswer`] as the lines [`app_abi::cloud`] describes.
fn cloud_answer_text(answer: &CloudAnswer) -> String {
    let mut text = String::new();

    match answer {
        CloudAnswer::Instances {
            instances, failed, ..
        } => {
            for instance in instances {
                let _ = writeln!(
                    text,
                    "I\t{}\t{}\t{}\t{}\t{}\t{}",
                    instance.provider.index(),
                    instance_state_code(instance.state),
                    clean(&instance.id),
                    clean(&instance.name),
                    clean(&instance.ip),
                    clean(&instance.region)
                );
            }

            for provider in failed {
                let _ = writeln!(text, "F\t{}", provider.index());
            }
        }
        CloudAnswer::Accepted {
            provider,
            action,
            request_id,
        } => {
            let _ = writeln!(
                text,
                "A\t{}\t{}\t{}",
                provider.index(),
                action.index(),
                clean(request_id)
            );
        }
        CloudAnswer::Refused {
            fault,
            code,
            message,
        } => {
            let _ = writeln!(
                text,
                "R\t{}\t{}\t{}",
                cloud_fault_code(*fault),
                clean(code),
                clean(message)
            );
        }
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_hal::Board;

    /// The target convention is the ABI's, not the backend's: a list names a vendor, a command names
    /// a product, and the two do not overlap.
    #[test]
    fn a_cloud_target_names_a_vendor_for_a_list_and_a_product_for_a_command() {
        assert_eq!(
            cloud_target(app_abi::vendor::TENCENT, CloudAction::List),
            Some(CloudTarget::Vendor(CloudVendor::Tencent))
        );
        assert_eq!(
            cloud_target(TARGET_MACHINE + app_abi::provider::LIGHTHOUSE, CloudAction::Start),
            Some(CloudTarget::Machine(CloudProvider::Lighthouse))
        );

        assert_eq!(cloud_target(app_abi::vendor::AWS, CloudAction::Stop), None);
        assert_eq!(cloud_target(9, CloudAction::List), None);
        assert_eq!(cloud_target(TARGET_MACHINE + 9, CloudAction::Reboot), None);
    }

    /// A refused answer is still an answer, and the line it becomes carries the provider's own
    /// sentence rather than this side's summary of it.
    #[test]
    fn a_refusal_is_one_line_with_the_providers_own_words() {
        let answer = CloudAnswer::Refused {
            fault: CloudFault::Api,
            code: "AuthFailure".to_string(),
            message: "密钥\n无效".to_string(),
        };

        assert_eq!(
            cloud_answer_text(&answer),
            "R\t14\tAuthFailure\t密钥 无效\n"
        );
    }

    /// The two halves of the board path: what the services write is what `std::fs` reads back, in a
    /// directory the test owns.
    #[test]
    fn preferences_and_files_go_through_the_storage_backend() {
        let dir = std::env::temp_dir().join(format!(
            "hivebox-board-services-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let mut services = BoardServices::new(Arc::new(Board::simulated()), &dir).for_app("clock");

        assert!(services.pref_set("alarm", b"07:30"));
        let mut out = [0u8; 16];
        assert_eq!(services.pref_get("alarm", &mut out), Some(5));
        assert_eq!(&out[..5], b"07:30");

        assert!(services.file_write("notes/a.txt", b"hello"));
        assert_eq!(services.file_read("notes/a.txt", &mut out), Some(5));
        assert_eq!(&out[..5], b"hello");

        assert_eq!(services.file_write("../escape", b"x"), false);
        assert_eq!(services.file_read("../escape", &mut out), None);

        std::fs::remove_dir_all(&dir).ok();
    }
}
