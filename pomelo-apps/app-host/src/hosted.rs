//! One application, open.
//!
//! The layer between a package on disk and a screen: it builds the VM, sends the events the launcher
//! receives, asks for frames, and turns any failure of the module into a state the interface can
//! draw. The launcher never touches `wasmi` and never interprets an error; it asks this type whether
//! there is a frame and what went wrong if there is not.
//!
//! # An application that keeps failing is stopped
//!
//! A trap is not fatal to the box — the VM is dropped and the launcher stays up — but an application
//! that traps on every launch is a tile that does nothing when pressed, forever. [`CRASH_LIMIT`]
//! consecutive failed starts write a marker next to the package ([`crate::catalog::DISABLED_FILE`]),
//! and the launcher then shows it as stopped and the store offers a rollback or an uninstall. One
//! successful frame clears the count, because that is the evidence that whatever was wrong is not
//! wrong any more.
//!
//! # State lives in the services, not in the VM
//!
//! A closed application is gone: its linear memory, its heap and every variable it kept are dropped.
//! What survives is what it put in a preference or a file, and what the host services themselves
//! keep — the audio stream, an HTTP request, a scan. That is what makes it honest to unload an
//! application when memory is tight, and what the SDK's documentation tells application authors to
//! rely on.

use std::path::{Path, PathBuf};

use app_abi::event as kind;
use app_abi::Event;

use crate::display::DisplayList;
use crate::meta::Meta;
use crate::package::Package;
use crate::services::Services;
use crate::vm::{Limits, Vm, VmError};

/// How many consecutive failed starts stop an application.
pub const CRASH_LIMIT: u32 = 3;

/// The file that counts consecutive failed starts.
pub const CRASH_FILE: &str = "crashes";

/// An application that is running, or that failed to.
pub struct HostedApp {
    pub meta: Meta,
    /// The directory the package came from; crash bookkeeping and the marker live here.
    dir: PathBuf,
    vm: Option<Vm>,
    /// Why the last start or the last call failed, when it did. The interface draws this sentence.
    error: Option<String>,
    /// Whether the application asked to close.
    exited: bool,
    /// The surface it was given.
    size: (f32, f32),
    scale: f32,
    /// The last frame, kept so that a redraw that is not a new frame (a theme change, a scroll of the
    /// launcher behind an overlay) does not run the application again.
    last: DisplayList,
}

impl HostedApp {
    /// Opens a package and gives it the surface.
    pub fn open(
        package: &Package,
        dir: impl Into<PathBuf>,
        services: Box<dyn Services>,
        size: (f32, f32),
        scale: f32,
        now_ms: i64,
    ) -> Self {
        let dir = dir.into();
        let limits = Limits::default().with_memory_kb(package.meta.memory_kb);

        let mut app = Self {
            meta: package.meta.clone(),
            dir,
            vm: None,
            error: None,
            exited: false,
            size,
            scale,
            last: DisplayList::default(),
        };

        match Vm::new(&package.wasm, services, limits, package.meta.permissions) {
            Ok(mut vm) => match vm.init(size.0 as i32, size.1 as i32, scale, now_ms) {
                Ok(()) => {
                    app.vm = Some(vm);
                }
                Err(error) => app.fail(error),
            },
            Err(error) => app.fail(error),
        }

        app
    }

    /// Whether there is a live VM behind this application.
    pub fn is_running(&self) -> bool {
        self.vm.is_some() && !self.exited
    }

    /// The sentence to draw when there is no live VM, or `None` when there is.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether the application closed itself.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// The last frame built, without asking the application for another.
    pub fn last_frame(&self) -> &DisplayList {
        &self.last
    }

    /// Sends one event and returns whether a redraw is needed.
    ///
    /// The frame itself is built by [`HostedApp::frame`], on the frame clock: an event that changes
    /// state but is never drawn is a state change nobody saw, and an event that draws twice is a
    /// frame the panel did not need.
    pub fn event(&mut self, event: Event) -> bool {
        if !self.is_running() {
            return false;
        }

        let Some(vm) = self.vm.as_mut() else {
            return false;
        };

        if let Err(error) = vm.event(event) {
            self.fail(error);
            return false;
        }

        if vm.exit_requested() {
            self.exited = true;
        }

        true
    }

    /// Asks for a frame. Returns it, or `None` when the application has failed or closed.
    pub fn frame(&mut self, now_ms: i64) -> Option<&DisplayList> {
        if !self.is_running() {
            return None;
        }

        let outcome = self.vm.as_mut().map(|vm| vm.frame(now_ms));

        match outcome {
            Some(Ok(list)) => {
                self.last = list;
                self.clear_crashes();
                Some(&self.last)
            }
            Some(Err(error)) => {
                self.fail(error);
                None
            }
            None => None,
        }
    }

    /// Drops the VM but keeps the application's identity and last frame, which is what a pause does
    /// when the launcher needs the memory more than it needs this application to keep its variables.
    pub fn unload(&mut self) {
        if let Some(mut vm) = self.vm.take() {
            let _ = vm.stop();
        }
    }

    /// Whether the application asked for another frame by itself (a download finished, a scan
    /// answered, an animation is running).
    pub fn take_invalidated(&mut self) -> bool {
        self.vm
            .as_mut()
            .map(|vm| vm.take_invalidated())
            .unwrap_or(false)
    }

    /// 应用请外壳把终端打开在某台机器上，如果它刚这么说过。取走，所以只回来一次。
    ///
    /// 走服务而不是走一条新消息，是因为它是**请求**不是事件：外壳在画完这一帧之后看它一眼，看
    /// 到了就把屏幕交给终端；看不到，应用那一页什么也没变。
    pub fn take_terminal(&mut self) -> Option<String> {
        self.vm
            .as_mut()
            .and_then(|vm| vm.services_mut().take_terminal())
    }

    /// 外壳那块键盘此刻的样子：外壳照着它把键盘画在应用下面，应用照着它知道自己那一格被敲了
    /// 什么。`None` 是这块宿主没有键盘（桌面模拟器）。
    pub fn keyboard(&self) -> Option<&crate::Keyboard> {
        self.vm.as_ref().and_then(|vm| vm.services().keyboard())
    }

    /// 键盘上的一个动作，由外壳的画布送进来。
    pub fn keyboard_mut(&mut self) -> Option<&mut crate::Keyboard> {
        self.vm
            .as_mut()
            .and_then(|vm| vm.services_mut().keyboard_mut())
    }

    /// How much linear memory the application is holding, for the store's and the settings' readings.
    pub fn memory_bytes(&self) -> u64 {
        self.vm.as_ref().map(|vm| vm.memory_bytes()).unwrap_or(0)
    }

    fn fail(&mut self, error: VmError) {
        let message = error.to_string();
        let count = record_crash(&self.dir);
        self.error = Some(message);
        self.vm = None;
        self.exited = false;

        if count >= CRASH_LIMIT {
            let _ = crate::manager::set_disabled(
                &crate::catalog::Root::new(
                    self.dir.parent().unwrap_or_else(|| Path::new("/")).to_path_buf(),
                    crate::catalog::Source::Internal,
                ),
                &self.meta.id,
                true,
            );
        }
    }

    fn clear_crashes(&mut self) {
        let marker = self.dir.join(CRASH_FILE);

        if marker.exists() {
            std::fs::remove_file(marker).ok();
        }
    }

    /// The last frame as an element, for a launcher that hosts this application.
    ///
    /// The list is the one [`HostedApp::frame`] returned: a redraw that is not a new frame — a theme
    /// change, a sheet over the panel — costs a recording of the same commands, not a call into the
    /// sandbox.
    #[cfg(feature = "ui")]
    pub fn view<'a, Message: 'a, F>(
        &'a self,
        fonts: crate::ui::Fonts,
        input: F,
    ) -> iced::Element<'a, Message>
    where
        F: Fn(Event) -> Message + 'a,
    {
        crate::ui::view(&self.last, fonts, input)
    }

    /// The surface this application was opened on.
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// The scale the surface was opened at, kept for a resize that does not happen on this panel.
    pub fn scale(&self) -> f32 {
        self.scale
    }
}

/// How many consecutive failed starts an application has had.
pub fn crash_count(dir: &Path) -> u32 {
    std::fs::read_to_string(dir.join(CRASH_FILE))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Records one more failed start and returns the new count.
pub fn record_crash(dir: &Path) -> u32 {
    let count = crash_count(dir).saturating_add(1);
    std::fs::write(dir.join(CRASH_FILE), count.to_string()).ok();

    count
}

/// A back press, as the launcher sends it.
pub fn back_event() -> Event {
    Event::new(kind::KEY_DOWN, 0, 0, app_abi::key::BACK as i32, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::MemoryServices;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hivebox-hosted-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A package that is not an application fails to open, and the failure is a sentence rather than
    /// a panic — that is the whole of what crash isolation means at this layer.
    #[test]
    fn a_broken_package_opens_into_an_error_not_a_crash() {
        let dir = temp_dir("broken");
        let meta = crate::meta::Meta::parse(
            r#"{"id":"broken","name":"Broken","version":"v1","memory_kb":64}"#,
        )
        .unwrap();

        let package = Package {
            meta,
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            icon: None,
        };

        let app = HostedApp::open(
            &package,
            &dir,
            Box::new(MemoryServices::default()),
            (480.0, 480.0),
            1.0,
            0,
        );

        assert!(!app.is_running());
        assert!(app.error().is_some());
        assert_eq!(crash_count(&dir), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn crashes_are_counted_and_cleared() {
        let dir = temp_dir("count");
        assert_eq!(crash_count(&dir), 0);

        assert_eq!(record_crash(&dir), 1);
        assert_eq!(record_crash(&dir), 2);
        assert_eq!(crash_count(&dir), 2);

        std::fs::remove_file(dir.join(CRASH_FILE)).ok();
        assert_eq!(crash_count(&dir), 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
