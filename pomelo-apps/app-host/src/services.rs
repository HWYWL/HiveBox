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
