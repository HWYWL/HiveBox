//! The host functions, as an application sees them.
//!
//! Every one of these is a call out of the sandbox, which costs more than a line of arithmetic and
//! far less than a mistake about what time it is. Nothing here blocks: a host service that waits is
//! a frame that does not get drawn, so the ones that talk to the world are asynchronous in a later
//! ABI and this one has only the question-and-answer kind.
//!
//! # Permissions are the package's, not the call's
//!
//! An application cannot ask for a permission at run time. The package's metadata lists the bits it
//! needs, the host grants exactly those, and a call whose bit is absent comes back as
//! [`app_abi::code::DENIED`] (or the matching state, when the call has no room for a code). The
//! wrappers below turn that into `false` or `None`, so an application that forgot to declare a
//! permission draws its "not available" screen instead of trapping.
//!
//! # Requests are started and polled
//!
//! [`http_state`], [`cloud_state`] and the NAS calls are snapshots: safe to ask once a frame, and
//! never blocking. A request is started once and its answer taken once — [`http_take`] and
//! [`cloud_take`] *consume*.

use app_abi::{
    code, AudioState, HttpProgressWord, NasSnapshot, SystemReadings, WifiSnapshot,
};

/// One line on the serial log, without a trailing newline.
pub fn log(message: &str) {
    imp::log(message.as_ptr() as u32, message.len() as u32)
}

/// Milliseconds since boot.
pub fn now_ms() -> i64 {
    imp::now_ms()
}

/// The width the host would draw a string at, in logical pixels.
pub fn text_width(text: &str, size: f32) -> i32 {
    imp::text_width(text.as_ptr() as u32, text.len() as u32, size as i32)
}

/// A remembered value. Returns how many bytes were written into `out`.
pub fn pref_get(key: &str, out: &mut [u8]) -> Option<usize> {
    let len = imp::pref_get(
        key.as_ptr() as u32,
        key.len() as u32,
        out.as_mut_ptr() as u32,
        out.len() as u32,
    );

    (len >= 0).then_some(len as usize)
}

/// Remembers a value. `false` when the host could not write it.
pub fn pref_set(key: &str, value: &[u8]) -> bool {
    imp::pref_set(
        key.as_ptr() as u32,
        key.len() as u32,
        value.as_ptr() as u32,
        value.len() as u32,
    ) == 0
}

/// Reads a file from the application's private directory.
pub fn file_read(name: &str, out: &mut [u8]) -> Option<usize> {
    let len = imp::file_read(
        name.as_ptr() as u32,
        name.len() as u32,
        out.as_mut_ptr() as u32,
        out.len() as u32,
    );

    (len >= 0).then_some(len as usize)
}

/// Writes a file in the application's private directory.
pub fn file_write(name: &str, data: &[u8]) -> bool {
    imp::file_write(
        name.as_ptr() as u32,
        name.len() as u32,
        data.as_ptr() as u32,
        data.len() as u32,
    ) == 0
}

/// Asks for another frame without an input event: what an application calls when something it
/// started in the background has an answer.
pub fn invalidate() {
    imp::invalidate()
}

/// Closes the application.
pub fn exit() {
    imp::exit()
}

// ---------------------------------------------------------------------------------------------
// The network (permission `NETWORK`).
// ---------------------------------------------------------------------------------------------

/// Asks the host to fetch `url`. `false` when the request was refused — no permission, a host with
/// no client, or this application already holding one request and one waiting behind it.
///
/// The board has one HTTP slot and the store shares it. A second request is queued by the host and
/// reported as [`app_abi::state::RUNNING`] while it waits, so a page can show "waiting" instead of
/// retrying at a moment it cannot know.
pub fn http_start(url: &str) -> bool {
    imp::http_start(url.as_ptr() as u32, url.len() as u32) == code::OK
}

/// One of [`app_abi::state`].
pub fn http_state() -> u32 {
    imp::http_state()
}

/// One of [`app_abi::fault`]: why the last request failed.
pub fn http_fault() -> u32 {
    imp::http_fault()
}

/// How much of the answer has arrived.
pub fn http_progress() -> Option<HttpProgressWord> {
    let mut bytes = [0u8; HttpProgressWord::LEN];
    let written = imp::http_progress(bytes.as_mut_ptr() as u32, bytes.len() as u32);

    if written < HttpProgressWord::LEN as i32 {
        return None;
    }

    HttpProgressWord::from_bytes(&bytes)
}

/// Takes the answer: its HTTP status into `status`, its body into `body`.
///
/// `Some(len)` is the body's real length, which may be larger than `body`: the host keeps the
/// answer and the next call with a bigger buffer collects it. `None` when there is nothing to take.
pub fn http_take(status: &mut u16, body: &mut [u8]) -> Option<usize> {
    let mut word: u32 = 0;
    let written = imp::http_take(
        &mut word as *mut u32 as u32,
        body.as_mut_ptr() as u32,
        body.len() as u32,
    );

    if written < 0 {
        return None;
    }

    *status = word as u16;

    Some(written as usize)
}

/// Abandons the request in flight.
pub fn http_cancel() -> bool {
    imp::http_cancel() == code::OK
}

// ---------------------------------------------------------------------------------------------
// The box's own readings (permission `SYSTEM`).
// ---------------------------------------------------------------------------------------------

/// Battery, memory, storage, uptime and the radio, as one snapshot.
///
/// A snapshot is cheap enough for a status bar but not free: the host takes several readings behind
/// it. Once a frame is fine; several times in one frame is not.
pub fn system() -> Option<SystemReadings> {
    let mut bytes = [0u8; SystemReadings::LEN];
    let written = imp::system(bytes.as_mut_ptr() as u32, bytes.len() as u32);

    if written < SystemReadings::LEN as i32 {
        return None;
    }

    SystemReadings::from_bytes(&bytes)
}

/// Copies the firmware's version string into `out`; the length written.
pub fn firmware_version(out: &mut [u8]) -> Option<usize> {
    let written = imp::firmware_version(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

// ---------------------------------------------------------------------------------------------
// The speaker (permission `AUDIO`).
// ---------------------------------------------------------------------------------------------

/// Plays an audio file. The path is the board's — `/sdcard/Music/a.mp3` — rather than the
/// application's private directory.
pub fn audio_play(path: &str) -> bool {
    imp::audio_play(path.as_ptr() as u32, path.len() as u32) == code::OK
}

/// One of [`app_abi::audio`]: pause, resume, stop, seek (milliseconds) or volume (percent).
pub fn audio_control(op: u32, value: i32) -> bool {
    imp::audio_control(op, value) == code::OK
}

/// Where the track is, how loud, and how long it is.
pub fn audio_state() -> Option<AudioState> {
    let mut bytes = [0u8; AudioState::LEN];
    let written = imp::audio_state(bytes.as_mut_ptr() as u32, bytes.len() as u32);

    if written < AudioState::LEN as i32 {
        return None;
    }

    AudioState::from_bytes(&bytes)
}

// ---------------------------------------------------------------------------------------------
// The radio (permission `WIFI`).
// ---------------------------------------------------------------------------------------------

pub fn wifi_status() -> Option<WifiSnapshot> {
    let mut bytes = [0u8; WifiSnapshot::LEN];
    let written = imp::wifi_status(bytes.as_mut_ptr() as u32, bytes.len() as u32);

    if written < WifiSnapshot::LEN as i32 {
        return None;
    }

    WifiSnapshot::from_bytes(&bytes)
}

pub fn wifi_scan_start() -> bool {
    imp::wifi_scan_start() == code::OK
}

/// One of [`app_abi::scan`].
pub fn wifi_scan_state() -> u32 {
    imp::wifi_scan_state()
}

/// How many access points the last completed scan found.
pub fn wifi_scan_count() -> u32 {
    imp::wifi_scan_count()
}

/// One access point, as the ABI's line: `ssid\trssi\tsecure\tchannel`.
pub fn wifi_scan_ap(index: u32, out: &mut [u8]) -> Option<usize> {
    let written = imp::wifi_scan_ap(index, out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

/// Joins a network. The password may be empty for an open one.
pub fn wifi_connect(ssid: &str, password: &str) -> bool {
    imp::wifi_connect(
        ssid.as_ptr() as u32,
        ssid.len() as u32,
        password.as_ptr() as u32,
        password.len() as u32,
    ) == code::OK
}

// ---------------------------------------------------------------------------------------------
// The machines beside the box (permission `NAS`).
// ---------------------------------------------------------------------------------------------

pub fn nas_snapshot() -> Option<NasSnapshot> {
    let mut bytes = [0u8; NasSnapshot::LEN];
    let written = imp::nas_snapshot(bytes.as_mut_ptr() as u32, bytes.len() as u32);

    if written < NasSnapshot::LEN as i32 {
        return None;
    }

    NasSnapshot::from_bytes(&bytes)
}

/// One line per disk: `device\tmodel\tsize_bytes\ttemperature_c\tbusy_percent\tread_per_sec\t
/// write_per_sec\tpurpose`.
pub fn nas_disks(out: &mut [u8]) -> Option<usize> {
    let written = imp::nas_disks(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

pub fn nas_refresh() -> bool {
    imp::nas_refresh() == code::OK
}

/// 板子被交代过的那些机器，一行一台：`host\tuser`。
///
/// 列表页要画几行，靠的就是这一条：快照只有一份，而「有哪几台」在板子读过的那份配置里。
pub fn nas_devices(out: &mut [u8]) -> Option<usize> {
    let written = imp::nas_devices(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

pub fn nas_shell_open(host: &str, columns: u16, rows: u16) -> bool {
    imp::nas_shell_open(
        host.as_ptr() as u32,
        host.len() as u32,
        u32::from(columns),
        u32::from(rows),
    ) == code::OK
}

pub fn nas_shell_write(bytes: &[u8]) -> bool {
    imp::nas_shell_write(bytes.as_ptr() as u32, bytes.len() as u32) == code::OK
}

/// One of [`app_abi::nas_shell`].
pub fn nas_shell_status() -> u32 {
    imp::nas_shell_status()
}

/// One of [`app_abi::fault`], when the session was lost.
pub fn nas_shell_fault() -> u32 {
    imp::nas_shell_fault()
}

/// What the session said since the last call.
pub fn nas_shell_take(out: &mut [u8]) -> Option<usize> {
    let written = imp::nas_shell_take(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

pub fn nas_shell_close() -> bool {
    imp::nas_shell_close() == code::OK
}

// ---------------------------------------------------------------------------------------------
// An account's machines (permission `CLOUD`).
// ---------------------------------------------------------------------------------------------

/// `masked\tregion` for one vendor ([`app_abi::vendor`]).
pub fn cloud_key(vendor: u32, out: &mut [u8]) -> Option<usize> {
    let written = imp::cloud_key(vendor, out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

/// A list names a vendor; a power command names `16 + provider`.
pub fn cloud_start(target: u32, action: u32, instance: &str, region: &str) -> bool {
    imp::cloud_start(
        target,
        action,
        instance.as_ptr() as u32,
        instance.len() as u32,
        region.as_ptr() as u32,
        region.len() as u32,
    ) == code::OK
}

pub fn cloud_state() -> u32 {
    imp::cloud_state()
}

/// The answer as the lines [`app_abi::cloud`] describes. Consumes it.
pub fn cloud_take(out: &mut [u8]) -> Option<usize> {
    let written = imp::cloud_take(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

pub fn cloud_cancel() -> bool {
    imp::cloud_cancel() == code::OK
}

// ---------------------------------------------------------------------------------------------
// What is on the two volumes (permission `FILES`).
// ---------------------------------------------------------------------------------------------

/// Opens `path` for reading. One directory at a time: opening another closes the first, and the
/// entries are read once — the listing is a snapshot, not a cursor into a disk that is still turning.
///
/// A path outside the two mount points, or one with a `..` in it, is refused.
pub fn dir_open(path: &str) -> bool {
    imp::dir_open(path.as_ptr() as u32, path.len() as u32) == code::OK
}

/// The next entry, as `kind\tname\tsize` — see [`app_abi::dir_entry`]. `None` at the end.
pub fn dir_next(out: &mut [u8]) -> Option<usize> {
    let written = imp::dir_next(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

/// Closes the open directory, which the host would otherwise keep until the next one is opened.
pub fn dir_close() {
    imp::dir_close();
}

/// Deletes a file, or a directory with everything under it.
pub fn path_remove(path: &str) -> bool {
    imp::path_remove(path.as_ptr() as u32, path.len() as u32) == code::OK
}

// ---------------------------------------------------------------------------------------------
// The board's own web server (permission `WEB`).
// ---------------------------------------------------------------------------------------------

/// `running\turl`, where `running` is `1` or `0` and the address is empty while there is none to
/// open. Read this rather than believing [`web_set`]: the board is the authority on its own socket.
pub fn web_status(out: &mut [u8]) -> Option<usize> {
    let written = imp::web_status(out.as_mut_ptr() as u32, out.len() as u32);

    (written >= 0).then_some(written as usize)
}

/// Starts the server on its default port, or stops it.
pub fn web_set(on: bool) -> bool {
    imp::web_set(u32::from(on)) == code::OK
}

#[cfg(target_arch = "wasm32")]
mod imp {
    // The imported names are the ABI's (`hb_*`); the names on this side are what the wrappers above
    // call. `link_name` is the seam, and each `unsafe` is one call out of the sandbox — nothing in
    // an application outside this module is unsafe.
    #[link(wasm_import_module = "host")]
    extern "C" {
        #[link_name = "hb_log"]
        fn raw_log(ptr: u32, len: u32);
        #[link_name = "hb_now_ms"]
        fn raw_now_ms() -> i64;
        #[link_name = "hb_text_width"]
        fn raw_text_width(ptr: u32, len: u32, size: i32) -> i32;
        #[link_name = "hb_pref_get"]
        fn raw_pref_get(key_ptr: u32, key_len: u32, out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_pref_set"]
        fn raw_pref_set(key_ptr: u32, key_len: u32, value_ptr: u32, value_len: u32) -> i32;
        #[link_name = "hb_file_read"]
        fn raw_file_read(name_ptr: u32, name_len: u32, out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_file_write"]
        fn raw_file_write(name_ptr: u32, name_len: u32, data_ptr: u32, data_len: u32) -> i32;
        #[link_name = "hb_invalidate"]
        fn raw_invalidate();
        #[link_name = "hb_exit"]
        fn raw_exit();

        #[link_name = "hb_http_start"]
        fn raw_http_start(url_ptr: u32, url_len: u32) -> i32;
        #[link_name = "hb_http_state"]
        fn raw_http_state() -> u32;
        #[link_name = "hb_http_fault"]
        fn raw_http_fault() -> u32;
        #[link_name = "hb_http_progress"]
        fn raw_http_progress(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_http_take"]
        fn raw_http_take(status_ptr: u32, body_ptr: u32, body_len: u32) -> i32;
        #[link_name = "hb_http_cancel"]
        fn raw_http_cancel() -> i32;

        #[link_name = "hb_system"]
        fn raw_system(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_firmware_version"]
        fn raw_firmware_version(out_ptr: u32, out_len: u32) -> i32;

        #[link_name = "hb_audio_play"]
        fn raw_audio_play(path_ptr: u32, path_len: u32) -> i32;
        #[link_name = "hb_audio_control"]
        fn raw_audio_control(op: u32, value: i32) -> i32;
        #[link_name = "hb_audio_state"]
        fn raw_audio_state(out_ptr: u32, out_len: u32) -> i32;

        #[link_name = "hb_wifi_status"]
        fn raw_wifi_status(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_wifi_scan_start"]
        fn raw_wifi_scan_start() -> i32;
        #[link_name = "hb_wifi_scan_state"]
        fn raw_wifi_scan_state() -> u32;
        #[link_name = "hb_wifi_scan_count"]
        fn raw_wifi_scan_count() -> u32;
        #[link_name = "hb_wifi_scan_ap"]
        fn raw_wifi_scan_ap(index: u32, out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_wifi_connect"]
        fn raw_wifi_connect(
            ssid_ptr: u32,
            ssid_len: u32,
            password_ptr: u32,
            password_len: u32,
        ) -> i32;

        #[link_name = "hb_nas_snapshot"]
        fn raw_nas_snapshot(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_nas_disks"]
        fn raw_nas_disks(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_nas_refresh"]
        fn raw_nas_refresh() -> i32;
        #[link_name = "hb_nas_devices"]
        fn raw_nas_devices(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_nas_shell_open"]
        fn raw_nas_shell_open(
            host_ptr: u32,
            host_len: u32,
            columns: u32,
            rows: u32,
        ) -> i32;
        #[link_name = "hb_nas_shell_write"]
        fn raw_nas_shell_write(bytes_ptr: u32, bytes_len: u32) -> i32;
        #[link_name = "hb_nas_shell_status"]
        fn raw_nas_shell_status() -> u32;
        #[link_name = "hb_nas_shell_fault"]
        fn raw_nas_shell_fault() -> u32;
        #[link_name = "hb_nas_shell_take"]
        fn raw_nas_shell_take(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_nas_shell_close"]
        fn raw_nas_shell_close() -> i32;

        #[link_name = "hb_cloud_key"]
        fn raw_cloud_key(vendor: u32, out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_cloud_start"]
        fn raw_cloud_start(
            target: u32,
            action: u32,
            instance_ptr: u32,
            instance_len: u32,
            region_ptr: u32,
            region_len: u32,
        ) -> i32;
        #[link_name = "hb_cloud_state"]
        fn raw_cloud_state() -> u32;
        #[link_name = "hb_cloud_take"]
        fn raw_cloud_take(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_cloud_cancel"]
        fn raw_cloud_cancel() -> i32;

        #[link_name = "hb_dir_open"]
        fn raw_dir_open(path_ptr: u32, path_len: u32) -> i32;
        #[link_name = "hb_dir_next"]
        fn raw_dir_next(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_dir_close"]
        fn raw_dir_close() -> i32;
        #[link_name = "hb_path_remove"]
        fn raw_path_remove(path_ptr: u32, path_len: u32) -> i32;
        #[link_name = "hb_web_status"]
        fn raw_web_status(out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_web_set"]
        fn raw_web_set(on: u32) -> i32;
    }

    pub fn log(ptr: u32, len: u32) {
        unsafe { raw_log(ptr, len) }
    }

    pub fn now_ms() -> i64 {
        unsafe { raw_now_ms() }
    }

    pub fn text_width(ptr: u32, len: u32, size: i32) -> i32 {
        unsafe { raw_text_width(ptr, len, size) }
    }

    pub fn pref_get(key_ptr: u32, key_len: u32, out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_pref_get(key_ptr, key_len, out_ptr, out_len) }
    }

    pub fn pref_set(key_ptr: u32, key_len: u32, value_ptr: u32, value_len: u32) -> i32 {
        unsafe { raw_pref_set(key_ptr, key_len, value_ptr, value_len) }
    }

    pub fn file_read(name_ptr: u32, name_len: u32, out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_file_read(name_ptr, name_len, out_ptr, out_len) }
    }

    pub fn file_write(name_ptr: u32, name_len: u32, data_ptr: u32, data_len: u32) -> i32 {
        unsafe { raw_file_write(name_ptr, name_len, data_ptr, data_len) }
    }

    pub fn invalidate() {
        unsafe { raw_invalidate() }
    }

    pub fn exit() {
        unsafe { raw_exit() }
    }

    pub fn http_start(url_ptr: u32, url_len: u32) -> i32 {
        unsafe { raw_http_start(url_ptr, url_len) }
    }

    pub fn http_state() -> u32 {
        unsafe { raw_http_state() }
    }

    pub fn http_fault() -> u32 {
        unsafe { raw_http_fault() }
    }

    pub fn http_progress(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_http_progress(out_ptr, out_len) }
    }

    pub fn http_take(status_ptr: u32, body_ptr: u32, body_len: u32) -> i32 {
        unsafe { raw_http_take(status_ptr, body_ptr, body_len) }
    }

    pub fn http_cancel() -> i32 {
        unsafe { raw_http_cancel() }
    }

    pub fn system(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_system(out_ptr, out_len) }
    }

    pub fn firmware_version(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_firmware_version(out_ptr, out_len) }
    }

    pub fn audio_play(path_ptr: u32, path_len: u32) -> i32 {
        unsafe { raw_audio_play(path_ptr, path_len) }
    }

    pub fn audio_control(op: u32, value: i32) -> i32 {
        unsafe { raw_audio_control(op, value) }
    }

    pub fn audio_state(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_audio_state(out_ptr, out_len) }
    }

    pub fn wifi_status(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_wifi_status(out_ptr, out_len) }
    }

    pub fn wifi_scan_start() -> i32 {
        unsafe { raw_wifi_scan_start() }
    }

    pub fn wifi_scan_state() -> u32 {
        unsafe { raw_wifi_scan_state() }
    }

    pub fn wifi_scan_count() -> u32 {
        unsafe { raw_wifi_scan_count() }
    }

    pub fn wifi_scan_ap(index: u32, out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_wifi_scan_ap(index, out_ptr, out_len) }
    }

    pub fn wifi_connect(
        ssid_ptr: u32,
        ssid_len: u32,
        password_ptr: u32,
        password_len: u32,
    ) -> i32 {
        unsafe { raw_wifi_connect(ssid_ptr, ssid_len, password_ptr, password_len) }
    }

    pub fn nas_snapshot(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_nas_snapshot(out_ptr, out_len) }
    }

    pub fn nas_disks(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_nas_disks(out_ptr, out_len) }
    }

    pub fn nas_refresh() -> i32 {
        unsafe { raw_nas_refresh() }
    }

    pub fn nas_devices(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_nas_devices(out_ptr, out_len) }
    }

    pub fn nas_shell_open(host_ptr: u32, host_len: u32, columns: u32, rows: u32) -> i32 {
        unsafe { raw_nas_shell_open(host_ptr, host_len, columns, rows) }
    }

    pub fn nas_shell_write(bytes_ptr: u32, bytes_len: u32) -> i32 {
        unsafe { raw_nas_shell_write(bytes_ptr, bytes_len) }
    }

    pub fn nas_shell_status() -> u32 {
        unsafe { raw_nas_shell_status() }
    }

    pub fn nas_shell_fault() -> u32 {
        unsafe { raw_nas_shell_fault() }
    }

    pub fn nas_shell_take(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_nas_shell_take(out_ptr, out_len) }
    }

    pub fn nas_shell_close() -> i32 {
        unsafe { raw_nas_shell_close() }
    }

    pub fn cloud_key(vendor: u32, out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_cloud_key(vendor, out_ptr, out_len) }
    }

    pub fn cloud_start(
        target: u32,
        action: u32,
        instance_ptr: u32,
        instance_len: u32,
        region_ptr: u32,
        region_len: u32,
    ) -> i32 {
        unsafe {
            raw_cloud_start(
                target,
                action,
                instance_ptr,
                instance_len,
                region_ptr,
                region_len,
            )
        }
    }

    pub fn cloud_state() -> u32 {
        unsafe { raw_cloud_state() }
    }

    pub fn cloud_take(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_cloud_take(out_ptr, out_len) }
    }

    pub fn cloud_cancel() -> i32 {
        unsafe { raw_cloud_cancel() }
    }

    pub fn dir_open(path_ptr: u32, path_len: u32) -> i32 {
        unsafe { raw_dir_open(path_ptr, path_len) }
    }

    pub fn dir_next(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_dir_next(out_ptr, out_len) }
    }

    pub fn dir_close() -> i32 {
        unsafe { raw_dir_close() }
    }

    pub fn path_remove(path_ptr: u32, path_len: u32) -> i32 {
        unsafe { raw_path_remove(path_ptr, path_len) }
    }

    pub fn web_status(out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_web_status(out_ptr, out_len) }
    }

    pub fn web_set(on: u32) -> i32 {
        unsafe { raw_web_set(on) }
    }
}

/// The same names on a PC, answered as "this host does not have that".
///
/// They exist so that an application's arithmetic — and every test of it — can be compiled and run
/// with `cargo test`, where there is no sandbox and no board. Nothing in the SDK calls them on the
/// panel.
#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn log(_ptr: u32, _len: u32) {}
    pub fn now_ms() -> i64 {
        0
    }
    pub fn text_width(_ptr: u32, _len: u32, _size: i32) -> i32 {
        0
    }
    pub fn pref_get(_key_ptr: u32, _key_len: u32, _out_ptr: u32, _out_len: u32) -> i32 {
        -1
    }
    pub fn pref_set(_key_ptr: u32, _key_len: u32, _value_ptr: u32, _value_len: u32) -> i32 {
        -1
    }
    pub fn file_read(_name_ptr: u32, _name_len: u32, _out_ptr: u32, _out_len: u32) -> i32 {
        -1
    }
    pub fn file_write(_name_ptr: u32, _name_len: u32, _data_ptr: u32, _data_len: u32) -> i32 {
        -1
    }
    pub fn invalidate() {}
    pub fn exit() {}

    pub fn http_start(_url_ptr: u32, _url_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn http_state() -> u32 {
        app_abi::state::IDLE
    }
    pub fn http_fault() -> u32 {
        app_abi::fault::UNSUPPORTED
    }
    pub fn http_progress(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn http_take(_status_ptr: u32, _body_ptr: u32, _body_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn http_cancel() -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn system(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn firmware_version(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn audio_play(_path_ptr: u32, _path_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn audio_control(_op: u32, _value: i32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn audio_state(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn wifi_status(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn wifi_scan_start() -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn wifi_scan_state() -> u32 {
        app_abi::scan::IDLE
    }
    pub fn wifi_scan_count() -> u32 {
        0
    }
    pub fn wifi_scan_ap(_index: u32, _out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn wifi_connect(_ssid_ptr: u32, _ssid_len: u32, _password_ptr: u32, _password_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn nas_snapshot(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn nas_disks(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn nas_refresh() -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn nas_devices(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn nas_shell_open(_host_ptr: u32, _host_len: u32, _columns: u32, _rows: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn nas_shell_write(_bytes_ptr: u32, _bytes_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn nas_shell_status() -> u32 {
        app_abi::nas_shell::CLOSED
    }
    pub fn nas_shell_fault() -> u32 {
        app_abi::fault::UNSUPPORTED
    }
    pub fn nas_shell_take(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn nas_shell_close() -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn cloud_key(_vendor: u32, _out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn cloud_start(
        _target: u32,
        _action: u32,
        _instance_ptr: u32,
        _instance_len: u32,
        _region_ptr: u32,
        _region_len: u32,
    ) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn cloud_state() -> u32 {
        app_abi::state::IDLE
    }
    pub fn cloud_take(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn cloud_cancel() -> i32 {
        app_abi::code::UNSUPPORTED
    }

    pub fn dir_open(_path_ptr: u32, _path_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn dir_next(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::END
    }
    pub fn dir_close() -> i32 {
        app_abi::code::OK
    }
    pub fn path_remove(_path_ptr: u32, _path_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn web_status(_out_ptr: u32, _out_len: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
    pub fn web_set(_on: u32) -> i32 {
        app_abi::code::UNSUPPORTED
    }
}
