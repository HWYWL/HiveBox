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
pub const ABI_MINOR: u16 = 0;

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
}
