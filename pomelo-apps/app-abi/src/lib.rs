//! The contract between an application package and the host that runs it.
//!
//! One crate, two sides: a wasm application links it to name the functions it exports and the host
//! services it imports; the firmware links it to bind those same names and to read the package
//! container. Both sides therefore agree about a byte offset without either one restating it.
//!
//! # What crosses the boundary, and what does not
//!
//! Not widgets, not `Element`s, not Rust values: **a display list**. The application writes a flat
//! array of [`Command`]s into its own linear memory during [`export::FRAME`], and the host reads that
//! array once, after the call returns, and draws it. Going the other way, the host writes one
//! [`Event`] into a scratch buffer the application handed it and calls [`export::EVENT`].
//!
//! That shape is the whole design. A call across the interpreter boundary costs far more than a
//! word written to memory, so nothing is called per rectangle and nothing is called per pixel: a
//! frame is one call in and one buffer read out.
//!
//! # Versioning
//!
//! [`ABI_MAJOR`] changes only when something that already exists changes meaning; a host refuses a
//! package whose major it does not implement. [`ABI_MINOR`] grows when a host service or command is
//! *added*, which an older host ignores and an older application never calls. [`version_word`] packs
//! both into the single integer [`export::ABI_VERSION`] returns.
//!
//! # The package container
//!
//! [`PackageHeader`] is a fixed 32-byte header followed by four sections in a fixed order: metadata
//! (UTF-8 JSON), the wasm module, the icon (raw RGB565 + alpha, or empty), and reserved resource
//! bytes. The format is little-endian, because every target this project builds for is.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// The major version of this contract. A host refuses a package that does not match it.
pub const ABI_MAJOR: u16 = 1;

/// The minor version: the highest service or command this crate knows about.
///
/// * `1` added the network, the box's own readings and the speaker.
/// * `2` added the radio.
/// * `3` added the machines beside the box and the ones in an account.
///
/// An application built against `1.0` still runs on this host: every addition is an import it never
/// calls. An application built against `1.3` on a `1.0` host fails at instantiation, which is the
/// answer the store draws as "needs a newer system".
pub const ABI_MINOR: u16 = 3;

/// Packs a major and a minor into the word [`export::ABI_VERSION`] returns.
pub const fn version_word(major: u16, minor: u16) -> u32 {
    ((major as u32) << 16) | minor as u32
}

/// The major half of a word built by [`version_word`].
pub const fn major_of(word: u32) -> u16 {
    (word >> 16) as u16
}

/// The minor half of a word built by [`version_word`].
pub const fn minor_of(word: u32) -> u16 {
    word as u16
}

/// Every wasm function a package exports, by name.
///
/// The host looks these up by string once, when it instantiates a module — never per frame.
pub mod export {
    /// `() -> u32`: [`super::version_word`] of the ABI the package was built against.
    pub const ABI_VERSION: &str = "hb_abi_version";
    /// `(width: i32, height: i32, scale_q8: i32, now_ms: i64) -> u32`: `0` when the application
    /// accepted the surface, non-zero when it cannot run on it (too small, wrong orientation).
    pub const INIT: &str = "hb_init";
    /// `() -> u32`: the address, in linear memory, of the scratch buffer the host writes an
    /// [`super::Event`] into before calling [`EVENT`].
    pub const SCRATCH_PTR: &str = "hb_scratch_ptr";
    /// `() -> u32`: how many bytes that buffer is. The host refuses to write a larger event.
    pub const SCRATCH_LEN: &str = "hb_scratch_len";
    /// `(len: u32) -> u32`: one event, already in the scratch buffer.
    pub const EVENT: &str = "hb_event";
    /// `(now_ms: i64) -> u32`: build this frame's display list and return how many commands it holds.
    /// Zero means "nothing to draw", which is a valid frame for an application that only reacts to
    /// events.
    pub const FRAME: &str = "hb_frame";
    /// `() -> u32`: the address the display list starts at. Read after [`FRAME`] returned its count.
    pub const DISPLAY_PTR: &str = "hb_display_ptr";
    /// `() -> u32`: how many commands the application's display buffer can hold. The host clamps
    /// [`FRAME`]'s answer to it rather than trusting the answer.
    pub const DISPLAY_CAPACITY: &str = "hb_display_capacity";
    /// `()`: the application is losing the screen but is not being destroyed — free what can be
    /// rebuilt, keep what a person would notice.
    pub const PAUSE: &str = "hb_pause";
    /// `()`: the application is getting the screen back.
    pub const RESUME: &str = "hb_resume";
    /// `()`: the application is being destroyed. The host drops the instance afterwards either way;
    /// this is the application's one chance to flush state it keeps in memory.
    pub const STOP: &str = "hb_stop";
}

/// Every host function a package may import, by name, in the module called [`MODULE`].
pub mod host {
    /// The wasm import module every host function lives in.
    pub const MODULE: &str = "host";

    /// `(ptr: u32, len: u32)`: one line on the serial log, prefixed with the application's id. The
    /// host truncates a line at [`super::MAX_LOG_BYTES`].
    pub const LOG: &str = "hb_log";
    /// `() -> i64`: milliseconds since boot. The same clock the frame call is given.
    pub const NOW_MS: &str = "hb_now_ms";
    /// `(ptr: u32, len: u32, size: i32) -> i32`: the width in logical pixels the host would draw this
    /// UTF-8 string at, with the interface's own font. `-1` for a string that is not UTF-8.
    ///
    /// The size is an integer on purpose: it is the pixel height of a line of the interface, and
    /// keeping floats out of the boundary means an engine that cannot pass an `f32` as an argument
    /// can still be the host.
    pub const TEXT_WIDTH: &str = "hb_text_width";
    /// `(key_ptr, key_len, out_ptr, out_len) -> i32`: reads a preference into `out`, returns its
    /// length, or `-1` when it is not set. Preferences are per application.
    pub const PREF_GET: &str = "hb_pref_get";
    /// `(key_ptr, key_len, val_ptr, val_len) -> i32`: writes a preference. `0` on success.
    pub const PREF_SET: &str = "hb_pref_set";
    /// `(name_ptr, name_len, out_ptr, out_len) -> i32`: reads a file from the application's private
    /// directory into `out`. `-1` when it does not exist, `-2` when the path is refused.
    pub const FILE_READ: &str = "hb_file_read";
    /// `(name_ptr, name_len, data_ptr, data_len) -> i32`: writes a file in the application's private
    /// directory. `0` on success.
    pub const FILE_WRITE: &str = "hb_file_write";

    // -------------------------------------------------------------------------------------------
    // v1.1: the network, the box's own readings, and the speaker.
    // -------------------------------------------------------------------------------------------

    /// `(url_ptr: u32, url_len: u32) -> i32`: asks the host to fetch `url`. Returns at once; what
    /// came of it is [`HTTP_STATE`]. Requires [`super::perm::NETWORK`].
    pub const HTTP_START: &str = "hb_http_start";
    /// `() -> u32`: one of [`super::state`].
    pub const HTTP_STATE: &str = "hb_http_state";
    /// `() -> u32`: why the last request failed, one of [`super::fault`]; `0` when it did not.
    pub const HTTP_FAULT: &str = "hb_http_fault";
    /// `(out_ptr, out_len) -> i32`: writes a [`super::HttpProgressWord`]. Returns its length, or a
    /// negative [`super::code`] when one is not readable.
    pub const HTTP_PROGRESS: &str = "hb_http_progress";
    /// `(status_ptr, body_ptr, body_len) -> i32`: takes the answer. Writes the HTTP status as a
    /// `u32`, copies the body, and returns the body's length. A buffer too small is not a loss: the
    /// host keeps the answer and returns the length it needs.
    pub const HTTP_TAKE: &str = "hb_http_take";
    /// `() -> i32`: abandons the request in flight.
    pub const HTTP_CANCEL: &str = "hb_http_cancel";
    /// `(out_ptr, out_len) -> i32`: writes a [`super::SystemReadings`]. Requires
    /// [`super::perm::SYSTEM`].
    pub const SYSTEM: &str = "hb_system";
    /// `(out_ptr, out_len) -> i32`: copies the firmware version string, `v0.10.5-…`.
    pub const FIRMWARE_VERSION: &str = "hb_firmware_version";
    /// `(path_ptr, path_len) -> i32`: starts playing an audio file on the box's one stream.
    /// Requires [`super::perm::AUDIO`].
    pub const AUDIO_PLAY: &str = "hb_audio_play";
    /// `(op: u32, value: i32) -> i32`: one of [`super::audio`]. `value` is the position in
    /// milliseconds for a seek and the percentage for a volume change.
    pub const AUDIO_CONTROL: &str = "hb_audio_control";
    /// `(out_ptr, out_len) -> i32`: writes a [`super::AudioState`].
    pub const AUDIO_STATE: &str = "hb_audio_state";

    // -------------------------------------------------------------------------------------------
    // v1.2: the radio.
    // -------------------------------------------------------------------------------------------

    /// `(out_ptr, out_len) -> i32`: writes a [`super::WifiSnapshot`]. Requires
    /// [`super::perm::WIFI`].
    pub const WIFI_STATUS: &str = "hb_wifi_status";
    /// `() -> i32`: begins a scan.
    pub const WIFI_SCAN_START: &str = "hb_wifi_scan_start";
    /// `() -> u32`: one of [`super::scan`].
    pub const WIFI_SCAN_STATE: &str = "hb_wifi_scan_state";
    /// `() -> u32`: how many access points the last completed scan found.
    pub const WIFI_SCAN_COUNT: &str = "hb_wifi_scan_count";
    /// `(index: u32, out_ptr, out_len) -> i32`: writes one access point as one line,
    /// `ssid\trssi\tsecure\tchannel`. Returns the line's length.
    pub const WIFI_SCAN_AP: &str = "hb_wifi_scan_ap";
    /// `(ssid_ptr, ssid_len, password_ptr, password_len) -> i32`: joins a network.
    pub const WIFI_CONNECT: &str = "hb_wifi_connect";

    // -------------------------------------------------------------------------------------------
    // v1.3: the machines beside the box, and the ones in an account.
    // -------------------------------------------------------------------------------------------

    /// `(out_ptr, out_len) -> i32`: writes a [`super::NasSnapshot`]. Requires
    /// [`super::perm::NAS`].
    pub const NAS_SNAPSHOT: &str = "hb_nas_snapshot";
    /// `(out_ptr, out_len) -> i32`: writes one line per disk, tab separated:
    /// `device\tmodel\tsize_bytes\ttemperature_c\tbusy_percent\tread_per_sec\twrite_per_sec\tpurpose`.
    pub const NAS_DISKS: &str = "hb_nas_disks";
    /// `() -> i32`: asks for a fresh look at the machine.
    pub const NAS_REFRESH: &str = "hb_nas_refresh";
    /// `(host_ptr, host_len, columns: u32, rows: u32) -> i32`: opens a shell session.
    pub const NAS_SHELL_OPEN: &str = "hb_nas_shell_open";
    /// `(bytes_ptr, bytes_len) -> i32`: types into the open session.
    pub const NAS_SHELL_WRITE: &str = "hb_nas_shell_write";
    /// `() -> u32`: one of [`super::nas_shell`].
    pub const NAS_SHELL_STATUS: &str = "hb_nas_shell_status";
    /// `() -> u32`: why a [`super::nas_shell::LOST`] session was lost, one of [`super::fault`].
    pub const NAS_SHELL_FAULT: &str = "hb_nas_shell_fault";
    /// `(out_ptr, out_len) -> i32`: copies what the session said since the last call.
    pub const NAS_SHELL_TAKE: &str = "hb_nas_shell_take";
    /// `() -> i32`: hangs the session up.
    pub const NAS_SHELL_CLOSE: &str = "hb_nas_shell_close";
    /// `(vendor: u32, out_ptr, out_len) -> i32`: writes `masked\tregion` for one vendor
    /// ([`super::vendor`]). Requires [`super::perm::CLOUD`].
    pub const CLOUD_KEY: &str = "hb_cloud_key";
    /// `(target: u32, action: u32, instance_ptr, instance_len, region_ptr, region_len) -> i32`:
    /// asks for a list or a power command. `target` is a [`super::vendor`] code for a list and
    /// `16 + provider` for a machine, exactly as `pomelo-hal` numbers `CloudTarget`.
    pub const CLOUD_START: &str = "hb_cloud_start";
    /// `() -> u32`: one of [`super::state`].
    pub const CLOUD_STATE: &str = "hb_cloud_state";
    /// `(out_ptr, out_len) -> i32`: copies the answer as lines — see [`super::cloud`].
    pub const CLOUD_TAKE: &str = "hb_cloud_take";
    /// `() -> i32`: abandons the request in flight.
    pub const CLOUD_CANCEL: &str = "hb_cloud_cancel";

    /// `()`: asks the host for another frame without an input event — what an application does when
    /// something it started asynchronously has finished.
    pub const INVALIDATE: &str = "hb_invalidate";
    /// `()`: the application wants to close, exactly as if it had been sent [`super::event::CLOSE`].
    pub const EXIT: &str = "hb_exit";
}

/// One display-list command, as ten 32-bit words.
///
/// Coordinates are logical pixels of the 480 × 480 surface, with the origin at the top left; the host
/// scales nothing. Angles and colours are not floats: a colour is `0x00RRGGBB`, and the two commands
/// that need a radius carry it in a word of their own. A command the host does not understand is
/// skipped, which is what makes a newer application on an older host degrade to a plainer screen
/// rather than to nothing.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    /// One of [`op`].
    pub op: u32,
    /// `x` for a rectangle, the start x for a line.
    pub x: u32,
    /// `y` for a rectangle, the start y for a line.
    pub y: u32,
    /// Width, or the end x of a line.
    pub w: u32,
    /// Height, or the end y of a line.
    pub h: u32,
    /// `0x00RRGGBB`.
    pub color: u32,
    /// The command's own first argument (radius, string address, glyph, clip depth).
    pub arg0: u32,
    /// The command's own second argument (stroke width, string length, alignment).
    pub arg1: u32,
    /// The command's own third argument.
    pub arg2: u32,
    /// Reserved for a later minor version; must be zero.
    pub reserved: u32,
}

impl Command {
    /// How many words one command is.
    pub const WORDS: usize = 10;

    /// How many bytes one command is.
    pub const LEN: usize = Self::WORDS * 4;

    /// A command with every argument zeroed.
    pub const fn new(op: u32) -> Self {
        Self {
            op,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            color: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
            reserved: 0,
        }
    }

    /// Reads a command out of ten words, little-endian, as they are laid out in linear memory.
    pub fn from_words(words: [u32; Self::WORDS]) -> Self {
        Self {
            op: words[0],
            x: words[1],
            y: words[2],
            w: words[3],
            h: words[4],
            color: words[5],
            arg0: words[6],
            arg1: words[7],
            arg2: words[8],
            reserved: words[9],
        }
    }

    /// Writes a command as ten words, little-endian.
    pub fn to_words(self) -> [u32; Self::WORDS] {
        [
            self.op,
            self.x,
            self.y,
            self.w,
            self.h,
            self.color,
            self.arg0,
            self.arg1,
            self.arg2,
            self.reserved,
        ]
    }
}

/// The operations a display list may hold.
pub mod op {
    /// Fill the whole surface with `color`. Resets the clip stack.
    pub const CLEAR: u32 = 0;
    /// `x, y, w, h, color`.
    pub const RECT: u32 = 1;
    /// `x, y, w, h, color`, `arg0` = corner radius.
    pub const ROUNDED_RECT: u32 = 2;
    /// `x, y, w, h, color`, `arg0` = corner radius, `arg1` = stroke width.
    pub const ROUNDED_STROKE: u32 = 3;
    /// `x, y` to `w, h`, `color`, `arg0` = line width.
    pub const LINE: u32 = 4;
    /// `x, y` left/top of the first line, `w` = max width (`0` = unbounded), `h` = font size,
    /// `color`, `arg0` = string address, `arg1` = byte length, `arg2` = [`align`].
    pub const TEXT: u32 = 5;
    /// `x, y` top left, `h` = size, `color`, `arg0` = Unicode scalar of a Material Symbols glyph.
    pub const ICON: u32 = 6;
    /// `x, y, w, h`: everything drawn after this is clipped to the rectangle, until a
    /// [`POP_CLIP`]. Clips nest.
    pub const PUSH_CLIP: u32 = 7;
    /// Ends the innermost clip.
    pub const POP_CLIP: u32 = 8;
    /// Filled circle: `x, y` centre, `w` = radius, `color`.
    pub const CIRCLE: u32 = 9;
}

/// Horizontal alignment of a [`op::TEXT`] command.
pub mod align {
    pub const LEFT: u32 = 0;
    pub const CENTER: u32 = 1;
    pub const RIGHT: u32 = 2;
}

/// The same choice as a type, for the SDK's drawing functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    /// The word an [`op::TEXT`] command carries.
    pub const fn code(self) -> u32 {
        match self {
            Self::Left => align::LEFT,
            Self::Center => align::CENTER,
            Self::Right => align::RIGHT,
        }
    }
}

/// One input event, written by the host into the application's scratch buffer.
///
/// `kind` is one of [`event`]; `x`/`y` are logical pixels; `a`/`b` are the kind's own arguments.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: u32,
    pub x: i32,
    pub y: i32,
    pub a: i32,
    pub b: i32,
}

impl Event {
    /// How many bytes one event is.
    pub const LEN: usize = 20;

    pub const fn new(kind: u32, x: i32, y: i32, a: i32, b: i32) -> Self {
        Self { kind, x, y, a, b }
    }
}

/// The kinds of event a host may send.
///
/// A touch is a press, any number of moves and one release: an application that only wants taps
/// ignores the moves, and one that wants a drag has everything it needs. There is no double tap,
/// because the panel has the same 480 px and a finger has the same width as on every other page here.
pub mod event {
    pub const TOUCH_DOWN: u32 = 0;
    pub const TOUCH_MOVE: u32 = 1;
    pub const TOUCH_UP: u32 = 2;
    /// `a` = key code (see [`key`]), `b` = 1 for a repeat.
    pub const KEY_DOWN: u32 = 3;
    pub const KEY_UP: u32 = 4;
    /// `a` = milliseconds since boot. Sent at most once a frame, and only to an application that has
    /// asked to be woken (see [`host::INVALIDATE`]) or that is animating.
    pub const TICK: u32 = 5;
    /// `a` = width, `b` = height: the surface changed size. Rare on this panel, but it is the one
    /// thing an application cannot discover on its own.
    pub const RESIZE: u32 = 6;
    /// The screen is gone; keep state but stop drawing. Not the same as [`CLOSE`].
    pub const PAUSE: u32 = 7;
    pub const RESUME: u32 = 8;
    /// The application should close itself: the back gesture, or the launcher unloading it.
    pub const CLOSE: u32 = 9;
}

/// Key codes for [`event::KEY_DOWN`] and [`event::KEY_UP`].
pub mod key {
    pub const BACK: u32 = 0x1_0000;
    pub const ENTER: u32 = 0x1_0001;
    pub const DELETE: u32 = 0x1_0002;
    pub const LEFT: u32 = 0x1_0003;
    pub const RIGHT: u32 = 0x1_0004;
    pub const UP: u32 = 0x1_0005;
    pub const DOWN: u32 = 0x1_0006;
}

/// What an application may ask the host to do on its behalf.
///
/// A package's metadata carries the union of the bits it needs; the host refuses an import whose bit
/// is not set, and the store shows the list before a person installs it. The bits are powers of two
/// and grow by appending, so the meaning of an old bit never changes.
pub mod perm {
    /// Nothing: the application may draw and keep its own data.
    pub const NONE: u32 = 0;
    /// HTTPS requests, through the host's one-request-at-a-time queue.
    pub const NETWORK: u32 = 1 << 0;
    /// Battery, memory, storage, uptime and the firmware version.
    pub const SYSTEM: u32 = 1 << 1;
    /// Start, stop, seek and set the volume of the box's one audio stream.
    pub const AUDIO: u32 = 1 << 2;
    /// SSH sessions to the machines the owner has configured.
    pub const NAS: u32 = 1 << 3;
    /// Wi-Fi state and scans.
    pub const WIFI: u32 = 1 << 4;
    /// The cloud provider requests the settings page configures.
    pub const CLOUD: u32 = 1 << 5;
    /// The two volumes' file listings, beyond the application's own directory.
    pub const FILES: u32 = 1 << 6;

    /// Every bit this ABI defines.
    pub const ALL: u32 = NETWORK | SYSTEM | AUDIO | NAS | WIFI | CLOUD | FILES;

    /// The permission a host function needs, by its name, or `None` for the calls that need none.
    ///
    /// The host binds this once, when it builds the imports; the SDK uses it to describe what a
    /// package will ask for before it is installed.
    pub fn of(import: &str) -> Option<u32> {
        Some(match import {
            super::host::HTTP_START
            | super::host::HTTP_STATE
            | super::host::HTTP_FAULT
            | super::host::HTTP_PROGRESS
            | super::host::HTTP_TAKE
            | super::host::HTTP_CANCEL => NETWORK,
            super::host::SYSTEM | super::host::FIRMWARE_VERSION => SYSTEM,
            super::host::AUDIO_PLAY | super::host::AUDIO_CONTROL | super::host::AUDIO_STATE => AUDIO,
            super::host::WIFI_STATUS
            | super::host::WIFI_SCAN_START
            | super::host::WIFI_SCAN_STATE
            | super::host::WIFI_SCAN_COUNT
            | super::host::WIFI_SCAN_AP
            | super::host::WIFI_CONNECT => WIFI,
            super::host::NAS_SNAPSHOT
            | super::host::NAS_DISKS
            | super::host::NAS_REFRESH
            | super::host::NAS_SHELL_OPEN
            | super::host::NAS_SHELL_WRITE
            | super::host::NAS_SHELL_STATUS
            | super::host::NAS_SHELL_FAULT
            | super::host::NAS_SHELL_TAKE
            | super::host::NAS_SHELL_CLOSE => NAS,
            super::host::CLOUD_KEY
            | super::host::CLOUD_START
            | super::host::CLOUD_STATE
            | super::host::CLOUD_TAKE
            | super::host::CLOUD_CANCEL => CLOUD,
            _ => return None,
        })
    }
}

/// What a call that starts or controls something answered.
///
/// Negative on purpose: a positive return from one of those calls is a length or a count, and a
/// caller that read `-1` as one byte would be reading a refusal as an answer.
pub mod code {
    pub const OK: i32 = 0;
    /// The package did not ask for the permission this call needs.
    pub const DENIED: i32 = -1;
    /// This host has no backend for the service: a desktop run of an image with no radio, or a
    /// board built without the SSH client.
    pub const UNSUPPORTED: i32 = -2;
    /// The one slot the host has for this service is already taken.
    pub const BUSY: i32 = -3;
    /// The arguments were refused — a pointer that is not in memory, a path that leaves the
    /// application's directory, an address that is not UTF-8.
    pub const INVALID: i32 = -4;
    /// The storage or the transport answered with a failure.
    pub const IO: i32 = -5;
}

/// How a request that has a start and a poll is going.
pub mod state {
    pub const IDLE: u32 = 0;
    pub const RUNNING: u32 = 1;
    pub const DONE: u32 = 2;
    pub const FAILED: u32 = 3;
    /// The package did not ask for the permission the request needs. A state rather than a code,
    /// because the call that reports it has no room for a negative answer.
    pub const DENIED: u32 = 4;
}

/// Why a request did not come back.
///
/// One list for the network, the NAS and an account's API, because it is one question a page asks
/// — "what do I tell the person" — and three lists would be three translations of the same answer.
pub mod fault {
    pub const NONE: u32 = 0;
    /// The box is not on a network.
    pub const NETWORK: u32 = 1;
    /// An HTTPS request with a clock that has never been set.
    pub const CLOCK: u32 = 2;
    /// Nothing answered, or the name did not resolve.
    pub const UNREACHABLE: u32 = 3;
    pub const TIMEOUT: u32 = 4;
    /// The secure connection did not complete.
    pub const TLS: u32 = 5;
    /// The answer was larger than the host's buffer.
    pub const TOO_LARGE: u32 = 6;
    /// Writing the answer to a volume failed.
    pub const STORAGE: u32 = 7;
    /// This host has no client for the service at all.
    pub const UNSUPPORTED: u32 = 8;
    /// The server refused the user or the password.
    pub const AUTHENTICATION: u32 = 9;
    /// The machine offered a host key that is not the one this box remembers.
    pub const HOST_KEY: u32 = 10;
    /// The two ends share no protocol, or the answer made no sense.
    pub const PROTOCOL: u32 = 11;
    /// The request itself was malformed.
    pub const ARGS: u32 = 12;
    /// No key is saved for this provider.
    pub const NO_CREDENTIALS: u32 = 13;
    /// The provider answered and refused the request.
    pub const API: u32 = 14;
    /// The package did not ask for the permission the call needs.
    pub const DENIED: u32 = 15;
}

/// The controls [`host::AUDIO_CONTROL`] understands.
pub mod audio {
    pub const PAUSE: u32 = 0;
    pub const RESUME: u32 = 1;
    pub const STOP: u32 = 2;
    /// `value` is a position in milliseconds.
    pub const SEEK: u32 = 3;
    /// `value` is a percentage, `0` to `100`.
    pub const VOLUME: u32 = 4;
}

/// The radio's connection state, as [`WifiSnapshot::state`] carries it.
pub mod wifi {
    pub const DISCONNECTED: u32 = 0;
    pub const SCANNING: u32 = 1;
    pub const CONNECTING: u32 = 2;
    pub const CONNECTED: u32 = 3;
}

/// A scan's progress, as [`host::WIFI_SCAN_STATE`] carries it.
pub mod scan {
    pub const IDLE: u32 = 0;
    pub const SCANNING: u32 = 1;
    pub const DONE: u32 = 2;
    pub const ERROR: u32 = 3;
    /// The package did not ask for [`super::perm::WIFI`].
    pub const DENIED: u32 = 4;
}

/// How the last look at the NAS went, as [`NasSnapshot::status`] carries it.
pub mod nas {
    pub const UNCONFIGURED: u32 = 0;
    pub const CONNECTING: u32 = 1;
    pub const ONLINE: u32 = 2;
    pub const OFFLINE: u32 = 3;
}

/// Where an interactive session on the NAS stands, as [`host::NAS_SHELL_STATUS`] carries it.
pub mod nas_shell {
    pub const CLOSED: u32 = 0;
    pub const OPENING: u32 = 1;
    pub const READY: u32 = 2;
    /// A session that ended with a fault; [`fault`] says which.
    pub const LOST: u32 = 3;
    /// The package did not ask for [`super::perm::NAS`].
    pub const DENIED: u32 = 4;
}

/// A cloud account's company, in the order `pomelo-hal` numbers them.
pub mod vendor {
    pub const TENCENT: u32 = 0;
    pub const ALIYUN: u32 = 1;
    pub const AWS: u32 = 2;
}

/// A cloud product, in the order `pomelo-hal` numbers them. A power command adds
/// [`TARGET_MACHINE`] to one of these to name the machine's product; a list names a vendor
/// directly.
pub mod provider {
    pub const CVM: u32 = 0;
    pub const LIGHTHOUSE: u32 = 1;
    pub const ECS: u32 = 2;
    pub const EC2: u32 = 3;
}

/// Added to a [`provider`] code to say "a machine, not a vendor".
pub const TARGET_MACHINE: u32 = 16;

/// A cloud action, as [`host::CLOUD_START`] takes it.
pub mod cloud_action {
    pub const LIST: u32 = 0;
    pub const START: u32 = 1;
    pub const STOP: u32 = 2;
    pub const REBOOT: u32 = 3;
}

/// How a cloud request stands on the wire, as one line per item.
///
/// `I` is an instance, `A` an accepted command, `R` a refusal, `F` a product whose call failed.
/// Every field is tab separated and every line ends with `\n`:
///
/// ```text
/// I\t<provider>\t<state>\t<id>\t<name>\t<ip>\t<region>\n
/// A\t<provider>\t<action>\t<request id>\n
/// R\t<fault>\t<provider code>\t<provider message>\n
/// F\t<provider>\n
/// ```
///
/// Text rather than a struct because the fields have no fixed length — an instance name is
/// somebody's, and a refusal is the provider's own sentence in its own language. Fields have their
/// tabs and newlines replaced with spaces on the way out, so a line is always one line.
pub mod cloud {
    pub const INSTANCE: u8 = b'I';
    pub const ACCEPTED: u8 = b'A';
    pub const REFUSED: u8 = b'R';
    pub const FAILED_PRODUCT: u8 = b'F';

    /// The state bytes of an instance line, in `pomelo-hal`'s `CloudInstanceState` order.
    pub const RUNNING: u32 = 0;
    pub const STOPPED: u32 = 1;
    pub const STARTING: u32 = 2;
    pub const STOPPING: u32 = 3;
    pub const REBOOTING: u32 = 4;
    pub const OTHER: u32 = 5;
}

/// The most commands a display list may hold, whatever an application's capacity says.
pub const MAX_COMMANDS: usize = 4096;

/// The most bytes of text one [`op::TEXT`] command may name.
pub const MAX_TEXT_BYTES: usize = 4096;

/// The most bytes of a log line the host keeps.
pub const MAX_LOG_BYTES: usize = 256;

/// The most preferences an application may hold, so that one package cannot fill the filesystem.
pub const MAX_PREFS: usize = 64;

// ---------------------------------------------------------------------------------------------
// The fixed-size answers of the host services
// ---------------------------------------------------------------------------------------------
//
// Each one writes itself as little-endian words, and each one is the whole answer rather than a
// pointer to something the host would have to keep alive: a snapshot is taken inside the host call
// that answered it, and the application owns the bytes afterwards.
//
// The serialisation is written by hand rather than derived from `repr(C)` because the two sides are
// two compilers — one of them a wasm32 target — and "the same layout" is a claim that costs more to
// check than a hundred lines of `to_le_bytes` cost to write.

/// The box's own readings, for [`host::SYSTEM`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemReadings {
    /// Battery charge, `0` to `100`.
    pub battery_percent: u32,
    /// `1` while on a charger.
    pub charging: u32,
    /// Millivolts at the cell, or `0` when the board cannot measure one.
    pub battery_mv: u32,
    /// The built-in heap's free bytes, or `0` when this host does not measure it.
    pub heap_free: u32,
    pub heap_total: u32,
    /// The external RAM, which the panel uses; zeroes on a board without it.
    pub psram_free: u32,
    pub psram_total: u32,
    /// The first volume that is mounted — `/internal` on the panel.
    pub storage_free: u32,
    pub storage_total: u32,
    /// Seconds since the board started.
    pub uptime_secs: u32,
    /// One of [`wifi`].
    pub wifi_state: u32,
    /// `0` to `4`.
    pub wifi_bars: u32,
}

impl SystemReadings {
    pub const LEN: usize = 48;

    pub fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        let fields = [
            self.battery_percent,
            self.charging,
            self.battery_mv,
            self.heap_free,
            self.heap_total,
            self.psram_free,
            self.psram_total,
            self.storage_free,
            self.storage_total,
            self.uptime_secs,
            self.wifi_state,
            self.wifi_bars,
        ];

        for (at, value) in fields.into_iter().enumerate() {
            out[at * 4..at * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        out
    }

    /// `None` for fewer than [`Self::LEN`] bytes: a short answer is not a reading with zeroes in
    /// the missing half.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::LEN {
            return None;
        }

        let word = |at: usize| {
            u32::from_le_bytes([
                bytes[at * 4],
                bytes[at * 4 + 1],
                bytes[at * 4 + 2],
                bytes[at * 4 + 3],
            ])
        };

        Some(Self {
            battery_percent: word(0),
            charging: word(1),
            battery_mv: word(2),
            heap_free: word(3),
            heap_total: word(4),
            psram_free: word(5),
            psram_total: word(6),
            storage_free: word(7),
            storage_total: word(8),
            uptime_secs: word(9),
            wifi_state: word(10),
            wifi_bars: word(11),
        })
    }
}

/// How the speaker is doing, for [`host::AUDIO_STATE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioState {
    pub playing: u32,
    /// `0` to `100`.
    pub volume: u32,
    pub position_ms: u32,
    /// `0` when nothing has said how long the track is.
    pub duration_ms: u32,
}

impl AudioState {
    pub const LEN: usize = 16;

    pub fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];

        for (at, value) in [
            self.playing,
            self.volume,
            self.position_ms,
            self.duration_ms,
        ]
        .into_iter()
        .enumerate()
        {
            out[at * 4..at * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::LEN {
            return None;
        }

        let word = |at: usize| {
            u32::from_le_bytes([
                bytes[at * 4],
                bytes[at * 4 + 1],
                bytes[at * 4 + 2],
                bytes[at * 4 + 3],
            ])
        };

        Some(Self {
            playing: word(0),
            volume: word(1),
            position_ms: word(2),
            duration_ms: word(3),
        })
    }
}

/// Process into how much of an answer has arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HttpProgressWord {
    pub received: u32,
    pub total: u32,
}

impl HttpProgressWord {
    pub const LEN: usize = 8;

    pub fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.received.to_le_bytes());
        out[4..8].copy_from_slice(&self.total.to_le_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::LEN {
            return None;
        }

        Some(Self {
            received: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            total: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

/// The radio's connection, for [`host::WIFI_STATUS`].
///
/// The SSID travels inside the struct rather than behind a second call: an SSID is at most 32 bytes,
/// and two calls to read one would be two frames in which the radio could have joined something
/// else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiSnapshot {
    /// One of [`wifi`].
    pub state: u32,
    /// dBm, or `0` when there is no connection.
    pub rssi: i32,
    /// `0` to `4`.
    pub bars: u32,
    /// How many bytes of [`Self::ssid`] are the name. Zero when not connected.
    pub ssid_len: u32,
    pub ssid: [u8; Self::SSID_LEN],
}

impl WifiSnapshot {
    /// The longest SSID the standard allows, and therefore the longest one this carries.
    pub const SSID_LEN: usize = 32;
    pub const LEN: usize = 16 + Self::SSID_LEN;

    /// A snapshot for a network whose name is `ssid`, cut to what the struct can hold.
    pub fn new(state: u32, rssi: i32, bars: u32, ssid: &[u8]) -> Self {
        let mut name = [0u8; Self::SSID_LEN];
        let len = ssid.len().min(Self::SSID_LEN);
        name[..len].copy_from_slice(&ssid[..len]);

        Self {
            state,
            rssi,
            bars,
            ssid_len: len as u32,
            ssid: name,
        }
    }

    /// The name the radio reported, or `""` when it has not joined anything.
    pub fn ssid(&self) -> &[u8] {
        &self.ssid[..(self.ssid_len as usize).min(Self::SSID_LEN)]
    }

    pub fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.state.to_le_bytes());
        out[4..8].copy_from_slice(&self.rssi.to_le_bytes());
        out[8..12].copy_from_slice(&self.bars.to_le_bytes());
        out[12..16].copy_from_slice(&self.ssid_len.to_le_bytes());
        out[16..].copy_from_slice(&self.ssid);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::LEN {
            return None;
        }

        let mut ssid = [0u8; Self::SSID_LEN];
        ssid.copy_from_slice(&bytes[16..Self::LEN]);

        Some(Self {
            state: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            rssi: i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            bars: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            ssid_len: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            ssid,
        })
    }
}

/// The last look at the NAS, for [`host::NAS_SNAPSHOT`].
///
/// Floats are carried as hundredths, because an integer is the one number every engine that could
/// host this ABI agrees about: `cpu_x100 = 4230` is `42.30 %`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NasSnapshot {
    /// One of [`nas`].
    pub status: u32,
    /// One of [`fault`] when [`Self::status`] is [`nas::OFFLINE`].
    pub fault: u32,
    pub has_cpu: u32,
    pub cpu_x100: u32,
    /// `i32::MIN` when the machine cannot be asked for a temperature.
    pub temperature_c_x100: i32,
    pub has_memory: u32,
    pub memory_used_kb: u32,
    pub memory_total_kb: u32,
    pub net_sent_bps: u32,
    pub net_received_bps: u32,
    pub uptime_secs: u32,
    /// How long ago the last reading was taken.
    pub age_secs: u32,
    /// How many lines [`host::NAS_DISKS`] will write.
    pub disks: u32,
}

impl NasSnapshot {
    pub const LEN: usize = 52;

    pub fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        let words = [
            self.status,
            self.fault,
            self.has_cpu,
            self.cpu_x100,
            self.temperature_c_x100 as u32,
            self.has_memory,
            self.memory_used_kb,
            self.memory_total_kb,
            self.net_sent_bps,
            self.net_received_bps,
            self.uptime_secs,
            self.age_secs,
            self.disks,
        ];

        for (at, value) in words.into_iter().enumerate() {
            out[at * 4..at * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::LEN {
            return None;
        }

        let word = |at: usize| {
            u32::from_le_bytes([
                bytes[at * 4],
                bytes[at * 4 + 1],
                bytes[at * 4 + 2],
                bytes[at * 4 + 3],
            ])
        };

        Some(Self {
            status: word(0),
            fault: word(1),
            has_cpu: word(2),
            cpu_x100: word(3),
            temperature_c_x100: word(4) as i32,
            has_memory: word(5),
            memory_used_kb: word(6),
            memory_total_kb: word(7),
            net_sent_bps: word(8),
            net_received_bps: word(9),
            uptime_secs: word(10),
            age_secs: word(11),
            disks: word(12),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// The package container
// ---------------------------------------------------------------------------------------------

/// Every package begins with these four bytes.
pub const PACKAGE_MAGIC: [u8; 4] = *b"HBAP";

/// The container format this crate describes. Independent of [`ABI_MAJOR`] on purpose: the layout of
/// the box a module arrives in can change without changing what the module says.
pub const PACKAGE_FORMAT: u16 = 1;

/// How many bytes the fixed header is.
pub const PACKAGE_HEADER_LEN: usize = 32;

/// The largest package the host will read: an application that needs more than this is not an
/// application this board should be downloading over its radio.
pub const MAX_PACKAGE_BYTES: usize = 2 * 1024 * 1024;

/// The largest metadata document a package may carry.
pub const MAX_META_BYTES: usize = 8 * 1024;

/// The largest wasm module a package may carry.
pub const MAX_WASM_BYTES: usize = MAX_PACKAGE_BYTES - PACKAGE_HEADER_LEN - MAX_META_BYTES;

/// The largest linear memory an application may ask for, in kilobytes.
pub const MAX_MEMORY_KB: u32 = 512;

/// The fixed header of a package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageHeader {
    pub format: u16,
    pub abi_major: u16,
    pub abi_minor: u16,
    pub flags: u16,
    pub meta_len: u32,
    pub wasm_len: u32,
    pub icon_len: u32,
    pub res_len: u32,
}

impl PackageHeader {
    /// A header for a package with the given section lengths.
    pub const fn new(meta_len: u32, wasm_len: u32, icon_len: u32, res_len: u32) -> Self {
        Self {
            format: PACKAGE_FORMAT,
            abi_major: ABI_MAJOR,
            abi_minor: ABI_MINOR,
            flags: 0,
            meta_len,
            wasm_len,
            icon_len,
            res_len,
        }
    }

    /// The four sections, in the order they are stored.
    pub fn sections(&self) -> [u32; 4] {
        [self.meta_len, self.wasm_len, self.icon_len, self.res_len]
    }

    /// How many bytes the whole package is.
    pub fn total_len(&self) -> u64 {
        PACKAGE_HEADER_LEN as u64
            + self.meta_len as u64
            + self.wasm_len as u64
            + self.icon_len as u64
            + self.res_len as u64
    }

    /// Reads the header at the front of `bytes`.
    ///
    /// Checks the magic, the format, the declared ABI and every length before returning, because a
    /// package is a file that arrived over a network: an offset that runs past the end of what
    /// arrived must be a refusal here rather than a slice panic in a drawing routine.
    pub fn parse(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() < PACKAGE_HEADER_LEN {
            return Err(PackageError::TooShort);
        }

        let header = Self::decode(&bytes[..PACKAGE_HEADER_LEN])?;

        if header.total_len() != bytes.len() as u64 {
            return Err(PackageError::LengthMismatch {
                declared: header.total_len(),
                actual: bytes.len() as u64,
            });
        }

        Ok(header)
    }

    /// Reads the fixed header alone, without a whole package behind it.
    ///
    /// What the catalog uses: the store and the launcher need a package's metadata on every boot,
    /// and reading 32 bytes plus a JSON document is not reading a megabyte of module.
    pub fn decode(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() < PACKAGE_HEADER_LEN {
            return Err(PackageError::TooShort);
        }

        if bytes[0..4] != PACKAGE_MAGIC {
            return Err(PackageError::BadMagic);
        }

        let format = u16::from_le_bytes([bytes[4], bytes[5]]);
        let abi_major = u16::from_le_bytes([bytes[6], bytes[7]]);
        let abi_minor = u16::from_le_bytes([bytes[8], bytes[9]]);
        let flags = u16::from_le_bytes([bytes[10], bytes[11]]);
        let meta_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let wasm_len = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let icon_len = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        let res_len = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);

        if format != PACKAGE_FORMAT {
            return Err(PackageError::UnsupportedFormat(format));
        }

        if abi_major != ABI_MAJOR {
            return Err(PackageError::UnsupportedAbi(abi_major));
        }

        let header = Self {
            format,
            abi_major,
            abi_minor,
            flags,
            meta_len,
            wasm_len,
            icon_len,
            res_len,
        };

        if meta_len as usize > MAX_META_BYTES {
            return Err(PackageError::MetadataTooLarge);
        }

        if wasm_len as usize > MAX_WASM_BYTES || wasm_len == 0 {
            return Err(PackageError::ModuleTooLarge);
        }

        Ok(header)
    }

    /// Encodes the header as the 32 bytes at the front of a package.
    pub fn encode(&self) -> [u8; PACKAGE_HEADER_LEN] {
        let mut out = [0u8; PACKAGE_HEADER_LEN];
        out[0..4].copy_from_slice(&PACKAGE_MAGIC);
        out[4..6].copy_from_slice(&self.format.to_le_bytes());
        out[6..8].copy_from_slice(&self.abi_major.to_le_bytes());
        out[8..10].copy_from_slice(&self.abi_minor.to_le_bytes());
        out[10..12].copy_from_slice(&self.flags.to_le_bytes());
        out[12..16].copy_from_slice(&self.meta_len.to_le_bytes());
        out[16..20].copy_from_slice(&self.wasm_len.to_le_bytes());
        out[20..24].copy_from_slice(&self.icon_len.to_le_bytes());
        out[24..28].copy_from_slice(&self.res_len.to_le_bytes());
        // The four bytes after `res_len` are reserved and stay zero.
        out
    }
}

/// The four sections of a package, borrowed from the bytes it was parsed out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sections<'a> {
    pub meta: &'a [u8],
    pub wasm: &'a [u8],
    pub icon: &'a [u8],
    pub resource: &'a [u8],
}

impl<'a> Sections<'a> {
    /// Splits a package whose header [`PackageHeader::parse`] accepted.
    pub fn of(bytes: &'a [u8], header: &PackageHeader) -> Self {
        let mut at = PACKAGE_HEADER_LEN;

        let mut take = |len: usize| {
            let slice = &bytes[at..at + len];
            at += len;
            slice
        };

        Self {
            meta: take(header.meta_len as usize),
            wasm: take(header.wasm_len as usize),
            icon: take(header.icon_len as usize),
            resource: take(header.res_len as usize),
        }
    }
}

/// Why a package was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageError {
    TooShort,
    BadMagic,
    UnsupportedFormat(u16),
    UnsupportedAbi(u16),
    MetadataTooLarge,
    ModuleTooLarge,
    LengthMismatch { declared: u64, actual: u64 },
}

impl PackageError {
    /// A sentence a person can act on, for the store's error line.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::TooShort => "文件太短，不是应用包",
            Self::BadMagic => "不是应用包（文件头不对）",
            Self::UnsupportedFormat(_) => "应用包格式太新，请先更新系统",
            Self::UnsupportedAbi(_) => "应用需要更新的系统",
            Self::MetadataTooLarge => "应用包的说明超出上限",
            Self::ModuleTooLarge => "应用体量超出上限",
            Self::LengthMismatch { .. } => "应用包不完整",
        }
    }
}

/// What a raw icon section is: width, height, RGB565 pixels and an alpha plane.
///
/// Kept in the ABI rather than in the host because the packaging tool and the tile that draws it are
/// two programs that must agree about where the alpha plane starts.
pub const ICON_HEADER_LEN: usize = 4;

/// Reads an icon section into its parts.
///
/// `None` for a section whose declared size does not match its pixels, which is a package built by
/// something other than this project's tooling and not a reason to draw a half-read icon.
pub fn icon_parts(section: &[u8]) -> Option<(u16, u16, &[u8], &[u8])> {
    if section.len() < ICON_HEADER_LEN {
        return None;
    }

    let width = u16::from_le_bytes([section[0], section[1]]);
    let height = u16::from_le_bytes([section[2], section[3]]);
    let pixels = width as usize * height as usize;

    if width == 0 || height == 0 || section.len() != ICON_HEADER_LEN + pixels * 3 {
        return None;
    }

    let rgb = &section[ICON_HEADER_LEN..ICON_HEADER_LEN + pixels * 2];
    let alpha = &section[ICON_HEADER_LEN + pixels * 2..];

    Some((width, height, rgb, alpha))
}

/// Encodes an icon section from pixels and an alpha plane.
pub fn encode_icon(width: u16, height: u16, rgb: &[u8], alpha: &[u8]) -> Option<Vec<u8>> {
    let pixels = width as usize * height as usize;

    if pixels == 0 || rgb.len() != pixels * 2 || alpha.len() != pixels {
        return None;
    }

    let mut out = Vec::with_capacity(ICON_HEADER_LEN + pixels * 3);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(rgb);
    out.extend_from_slice(alpha);

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn package(meta: &[u8], wasm: &[u8], icon: &[u8]) -> Vec<u8> {
        let header = PackageHeader::new(meta.len() as u32, wasm.len() as u32, icon.len() as u32, 0);
        let mut out = Vec::new();
        out.extend_from_slice(&header.encode());
        out.extend_from_slice(meta);
        out.extend_from_slice(wasm);
        out.extend_from_slice(icon);
        out
    }

    /// A package is read back with its sections in the order they were written, and the header's
    /// length is the whole file's length.
    #[test]
    fn a_package_splits_into_its_four_sections() {
        let bytes = package(b"{\"id\":\"calculator\"}", b"\0asm\x01\0\0\0", b"icon");
        let header = PackageHeader::parse(&bytes).expect("a package this crate wrote");

        assert_eq!(header.format, PACKAGE_FORMAT);
        assert_eq!(header.abi_major, ABI_MAJOR);
        assert_eq!(header.total_len(), bytes.len() as u64);

        let sections = Sections::of(&bytes, &header);
        assert_eq!(sections.meta, b"{\"id\":\"calculator\"}");
        assert_eq!(sections.wasm, b"\0asm\x01\0\0\0");
        assert_eq!(sections.icon, b"icon");
        assert_eq!(sections.resource, b"");
    }

    /// Every refusal the host can make about a container is made here, before a byte is executed:
    /// truncation, a wrong magic, a wrong format, a foreign ABI and a declared length that disagrees
    /// with what arrived.
    #[test]
    fn a_broken_container_is_refused_not_run() {
        let good = package(b"{}", b"\0asm\x01\0\0\0", b"");

        assert_eq!(PackageHeader::parse(&[0u8; 8]), Err(PackageError::TooShort));

        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        assert_eq!(
            PackageHeader::parse(&wrong_magic),
            Err(PackageError::BadMagic)
        );

        let mut wrong_format = good.clone();
        wrong_format[4] = 9;
        assert_eq!(
            PackageHeader::parse(&wrong_format),
            Err(PackageError::UnsupportedFormat(9))
        );

        let mut wrong_abi = good.clone();
        wrong_abi[6] = 2;
        assert_eq!(
            PackageHeader::parse(&wrong_abi),
            Err(PackageError::UnsupportedAbi(2))
        );

        let mut short = good.clone();
        short.pop();
        assert!(matches!(
            PackageHeader::parse(&short),
            Err(PackageError::LengthMismatch { .. })
        ));

        let mut no_module = good;
        no_module[16..20].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            PackageHeader::parse(&no_module),
            Err(PackageError::LengthMismatch { .. }) | Err(PackageError::ModuleTooLarge)
        ));
    }

    /// An icon section carries its own dimensions, and a section whose lengths do not add up is
    /// refused rather than drawn at a guessed size.
    #[test]
    fn an_icon_is_read_only_when_its_planes_match_its_size() {
        let rgb = vec![0x11u8; 2 * 2 * 2];
        let alpha = vec![0xFFu8; 4];
        let section = encode_icon(2, 2, &rgb, &alpha).expect("a 2 x 2 icon");

        let (width, height, pixels, mask) = icon_parts(&section).expect("the icon just encoded");
        assert_eq!((width, height), (2, 2));
        assert_eq!(pixels, &rgb[..]);
        assert_eq!(mask, &alpha[..]);

        assert_eq!(icon_parts(&section[..section.len() - 1]), None);
        assert_eq!(encode_icon(2, 2, &rgb[..3], &alpha), None);
        assert_eq!(icon_parts(&[]), None);
    }

    /// The version word is two halves that do not borrow each other's bits: `1.0` is not `0.1`.
    #[test]
    fn the_version_word_keeps_the_two_halves_apart() {
        let word = version_word(1, 0);
        assert_eq!(major_of(word), 1);
        assert_eq!(minor_of(word), 0);
        assert_eq!(word, 0x0001_0000);

        let later = version_word(1, 7);
        assert_eq!(major_of(later), 1);
        assert_eq!(minor_of(later), 7);
        assert_eq!(major_of(version_word(2, 0)), 2);
    }

    /// A command's ten words survive the round trip, and the length constants agree with the layout.
    #[test]
    fn a_command_is_ten_words_and_reads_back_unchanged() {
        let mut command = Command::new(op::TEXT);
        command.x = 12;
        command.y = 34;
        command.color = 0x00F6_9E22;
        command.arg0 = 0x2000;
        command.arg1 = 5;
        command.arg2 = align::CENTER;

        let words = command.to_words();
        assert_eq!(Command::LEN, words.len() * 4);
        assert_eq!(Command::from_words(words), command);
    }

    /// Every fixed answer survives its round trip, and a short answer is refused rather than read as
    /// one with zeroes in the missing half.
    #[test]
    fn the_host_answers_round_trip_through_their_bytes() {
        let readings = SystemReadings {
            battery_percent: 87,
            charging: 1,
            battery_mv: 4_120,
            heap_free: 120_000,
            heap_total: 320_000,
            psram_free: 6_000_000,
            psram_total: 8_000_000,
            storage_free: 2_000_000,
            storage_total: 4_000_000,
            uptime_secs: 3_600,
            wifi_state: wifi::CONNECTED,
            wifi_bars: 3,
        };

        assert_eq!(SystemReadings::from_bytes(&readings.to_bytes()), Some(readings));
        assert_eq!(SystemReadings::from_bytes(&[0u8; SystemReadings::LEN - 1]), None);

        let audio = AudioState {
            playing: 1,
            volume: 40,
            position_ms: 91_000,
            duration_ms: 240_000,
        };

        assert_eq!(AudioState::from_bytes(&audio.to_bytes()), Some(audio));

        let progress = HttpProgressWord {
            received: 90_000,
            total: 230_000,
        };

        assert_eq!(HttpProgressWord::from_bytes(&progress.to_bytes()), Some(progress));

        let wifi = WifiSnapshot::new(wifi::CONNECTED, -57, 3, "HiveBox".as_bytes());
        let read = WifiSnapshot::from_bytes(&wifi.to_bytes()).expect("the snapshot just encoded");

        assert_eq!(read, wifi);
        assert_eq!(read.ssid(), b"HiveBox");
        assert_eq!(WifiSnapshot::LEN, 48);

        let nas = NasSnapshot {
            status: nas::OFFLINE,
            fault: fault::UNREACHABLE,
            has_cpu: 1,
            cpu_x100: 4_230,
            temperature_c_x100: -500,
            has_memory: 1,
            memory_used_kb: 2_048,
            memory_total_kb: 8_192,
            net_sent_bps: 1_000,
            net_received_bps: 2_000,
            uptime_secs: 86_400,
            age_secs: 12,
            disks: 4,
        };

        assert_eq!(NasSnapshot::from_bytes(&nas.to_bytes()), Some(nas));
    }

    /// An import's permission is the ABI's own table, including the calls that need none.
    #[test]
    fn every_service_import_names_its_permission() {
        assert_eq!(perm::of(host::HTTP_START), Some(perm::NETWORK));
        assert_eq!(perm::of(host::HTTP_TAKE), Some(perm::NETWORK));
        assert_eq!(perm::of(host::SYSTEM), Some(perm::SYSTEM));
        assert_eq!(perm::of(host::AUDIO_STATE), Some(perm::AUDIO));
        assert_eq!(perm::of(host::WIFI_SCAN_AP), Some(perm::WIFI));
        assert_eq!(perm::of(host::NAS_SHELL_TAKE), Some(perm::NAS));
        assert_eq!(perm::of(host::CLOUD_TAKE), Some(perm::CLOUD));
        assert_eq!(perm::of(host::LOG), None);
        assert_eq!(perm::of(host::PREF_SET), None);
        assert_eq!(perm::of("hb_not_a_function"), None);
    }
}
