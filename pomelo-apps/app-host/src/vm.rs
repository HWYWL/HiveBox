//! One wasm instance, with a budget and a memory.
//!
//! # Why `wasmi`
//!
//! A pure-Rust interpreter, so the runtime is the same code on a PC and on the panel. The price is
//! speed — an interpreter runs wasm an order of magnitude or two slower than the same logic compiled
//! natively — and that price is paid only by application logic, never by drawing: a frame is a
//! display list the host replays with its own rasteriser, not a wasm function per pixel. A
//! calculator, a clock, a weather page and a settings form are small amounts of arithmetic between
//! touches; a video decoder is not, and would not be an application this board could run.
//!
//! The interpreter is behind this module and not behind the ABI: [`app_abi`] names functions the
//! package exports and host functions it imports, and an engine that implements those names could
//! replace `wasmi` without a package noticing.
//!
//! # Two budgets, both enforced here
//!
//! *Fuel* is the wasm instruction budget: every entry into the module adds [`Limits::fuel_per_call`]
//! and an application that runs past it traps. That is what stops `loop {}` from taking the panel
//! with it, and it is why an application that hangs is a screen that says so rather than a box that
//! has to be reset.
//!
//! *Memory* is the linear memory the application may have. The module's own declaration is checked
//! at instantiation, and the size is checked after every call — cheap, and it bounds what a runaway
//! `memory.grow` can take out of the PSRAM the interface is using.
//!
//! # A trap is not a panic
//!
//! Every failure of the module — a trap, a missing export, a bad pointer — comes back as a
//! [`VmError`]. The host keeps its own state and its own frame; the worst outcome is an application
//! that does not draw, which [`crate::hosted`] turns into a message and, after enough of them, a
//! package that is no longer offered.

use std::fmt;
use std::time::SystemTime;

use std::fmt::Write as _;

use app_abi::{
    code, export, fault, host, key, nas_shell, perm, scan, state, Event, ABI_MAJOR, MAX_COMMANDS,
    MAX_LOG_BYTES, MAX_MEMORY_KB, MAX_PACKAGE_BYTES,
};
use wasmi::{Caller, Config, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

use crate::display::DisplayList;
use crate::services::{Services, WifiAp};

/// What one instance may spend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Instructions one call into the module may run.
    pub fuel_per_call: u64,
    /// Linear memory, in kilobytes.
    pub memory_kb: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // Two million wasm instructions is generous for a frame of layout arithmetic and mean to
            // a loop that will not end: on the panel it is a fraction of a second.
            fuel_per_call: 2_000_000,
            memory_kb: 256,
        }
    }
}

impl Limits {
    pub fn with_memory_kb(mut self, memory_kb: u32) -> Self {
        self.memory_kb = memory_kb.min(MAX_MEMORY_KB);
        self
    }

    pub fn with_fuel(mut self, fuel: u64) -> Self {
        self.fuel_per_call = fuel;
        self
    }

    fn max_pages(&self) -> u64 {
        (self.memory_kb as u64).div_ceil(64)
    }
}

/// The state shared with the host functions.
pub struct HostState {
    pub services: Box<dyn Services>,
    /// What the package's metadata said it may use. A host function whose bit is absent answers
    /// [`code::DENIED`] without asking the service anything.
    pub permissions: u32,
    /// Resolved once, after instantiation: every host function needs to read or write application
    /// memory, and looking the export up per call would be a string comparison per string.
    memory: Option<Memory>,
    /// Set by `hb_invalidate`, read and cleared by the caller of [`Vm::frame`].
    pub invalidated: bool,
    /// Set by `hb_exit`, read by the launcher.
    pub exit_requested: bool,
    /// An HTTP answer that did not fit the buffer it was handed, kept for the next attempt. A body
    /// taken out of the service and dropped would be a request the application paid for and never
    /// saw.
    pending_http: Option<(u16, Vec<u8>)>,
    /// A cloud answer that did not fit, for the same reason.
    pending_cloud: Option<String>,
    /// Shell output that did not fit the buffer it was handed.
    pending_shell: Vec<u8>,
    /// Where the NAS session stands, as the last take saw it. `hb_nas_shell_status` is a question
    /// about the last look rather than a second look, because a second look would consume the bytes
    /// the first one was asked for.
    shell_status: u32,
    /// Why a lost session was lost, cached beside the status.
    shell_fault: u32,
    /// The last scan's access points, read once and answered as lines.
    scan: Vec<WifiAp>,
    scan_fresh: bool,
}

/// A running application.
pub struct Vm {
    store: Store<HostState>,
    #[allow(dead_code)]
    instance: Instance,
    memory: Memory,
    exports: Exports,
    limits: Limits,
    abi: u32,
}

struct Exports {
    init: TypedFunc<(i32, i32, i32, i64), u32>,
    scratch_ptr: TypedFunc<(), u32>,
    scratch_len: TypedFunc<(), u32>,
    event: TypedFunc<u32, u32>,
    frame: TypedFunc<i64, u32>,
    display_ptr: TypedFunc<(), u32>,
    display_capacity: TypedFunc<(), u32>,
    pause: Option<TypedFunc<(), ()>>,
    resume: Option<TypedFunc<(), ()>>,
    stop: Option<TypedFunc<(), ()>>,
}

impl Vm {
    /// Compiles and instantiates a module.
    ///
    /// `permissions` is the package's own [`app_abi::perm`] bitset. It is an argument rather than
    /// something the services decide because the refusal has to happen *before* the service is
    /// asked: a denied call must not open a socket, start a scan or write a preference that the
    /// implementation would then have to undo.
    pub fn new(
        wasm: &[u8],
        services: Box<dyn Services>,
        limits: Limits,
        permissions: u32,
    ) -> Result<Self, VmError> {
        if wasm.len() > MAX_PACKAGE_BYTES {
            return Err(VmError::TooLarge(wasm.len()));
        }

        let config = {
            let mut config = Config::default();
            config.consume_fuel(true);
            config
        };

        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|error| VmError::Compile(error.to_string()))?;

        let mut store = Store::new(
            &engine,
            HostState {
                services,
                permissions,
                memory: None,
                invalidated: false,
                exit_requested: false,
                pending_http: None,
                pending_cloud: None,
                pending_shell: Vec::new(),
                shell_status: nas_shell::CLOSED,
                shell_fault: fault::NONE,
                scan: Vec::new(),
                scan_fresh: false,
            },
        );

        store
            .add_fuel(limits.fuel_per_call)
            .map_err(|error| VmError::Fuel(error.to_string()))?;

        let mut linker = Linker::new(&engine);
        bind(&mut linker)?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|error| VmError::Instantiate(error.to_string()))?
            .start(&mut store)
            .map_err(|error| VmError::Instantiate(error.to_string()))?;

        let memory = instance.get_memory(&store, "memory");
        store.data_mut().memory = memory;

        // The ABI word is read before any other export, because it decides whether the others mean
        // what this host thinks they mean. A module that does not export it is not one of ours — and
        // that is the answer for an empty module too, which is why this comes before the memory
        // check: "not an application" is more useful than "no memory".
        let abi_fn = instance
            .get_typed_func::<(), u32>(&store, export::ABI_VERSION)
            .map_err(|_| VmError::MissingExport(export::ABI_VERSION.to_string()))?;

        let abi = abi_fn
            .call(&mut store, ())
            .map_err(|error| VmError::Trap(error.to_string()))?;

        // A newer minor is a package that imports a function this host was not built with, and
        // refusing it here says "needs a newer system" instead of letting the linker say "unknown
        // import" in a sentence about wasm. An older minor is fine: every addition is an import it
        // never calls.
        if app_abi::major_of(abi) != ABI_MAJOR || app_abi::minor_of(abi) > app_abi::ABI_MINOR {
            return Err(VmError::Abi(abi));
        }

        let memory = memory.ok_or(VmError::NoMemory)?;

        let typed = |store: &Store<HostState>, name: &'static str| {
            instance
                .get_typed_func::<(), u32>(store, name)
                .map_err(|_| VmError::MissingExport(name.to_string()))
        };

        let exports = Exports {
            init: instance
                .get_typed_func::<(i32, i32, i32, i64), u32>(&store, export::INIT)
                .map_err(|_| VmError::MissingExport(export::INIT.to_string()))?,
            scratch_ptr: typed(&store, export::SCRATCH_PTR)?,
            scratch_len: typed(&store, export::SCRATCH_LEN)?,
            event: instance
                .get_typed_func::<u32, u32>(&store, export::EVENT)
                .map_err(|_| VmError::MissingExport(export::EVENT.to_string()))?,
            frame: instance
                .get_typed_func::<i64, u32>(&store, export::FRAME)
                .map_err(|_| VmError::MissingExport(export::FRAME.to_string()))?,
            display_ptr: typed(&store, export::DISPLAY_PTR)?,
            display_capacity: typed(&store, export::DISPLAY_CAPACITY)?,
            pause: instance
                .get_typed_func::<(), ()>(&store, export::PAUSE)
                .ok(),
            resume: instance
                .get_typed_func::<(), ()>(&store, export::RESUME)
                .ok(),
            stop: instance
                .get_typed_func::<(), ()>(&store, export::STOP)
                .ok(),
        };

        let vm = Self {
            store,
            instance,
            memory,
            exports,
            limits,
            abi,
        };

        vm.check_memory()?;

        Ok(vm)
    }

    /// The ABI word the module reported.
    pub fn abi(&self) -> u32 {
        self.abi
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Hands the application its surface and lets it build its first frame.
    pub fn init(&mut self, width: i32, height: i32, scale: f32, now_ms: i64) -> Result<(), VmError> {
        let scale_q8 = (scale * 256.0) as i32;
        self.fuel()?;

        let answer = self
            .exports
            .init
            .call(&mut self.store, (width, height, scale_q8, now_ms))
            .map_err(|error| VmError::Trap(error.to_string()))?;

        self.check_memory()?;

        if answer != 0 {
            return Err(VmError::Refused(answer));
        }

        Ok(())
    }

    /// Sends one event, writing it into the scratch buffer first.
    pub fn event(&mut self, event: Event) -> Result<(), VmError> {
        let ptr = self.scratch_ptr()?;
        let len = self.scratch_len()?;

        if len < Event::LEN as u32 {
            return Err(VmError::ScratchTooSmall(len));
        }

        let mut bytes = [0u8; Event::LEN];
        bytes[0..4].copy_from_slice(&event.kind.to_le_bytes());
        bytes[4..8].copy_from_slice(&event.x.to_le_bytes());
        bytes[8..12].copy_from_slice(&event.y.to_le_bytes());
        bytes[12..16].copy_from_slice(&event.a.to_le_bytes());
        bytes[16..20].copy_from_slice(&event.b.to_le_bytes());

        self.memory
            .write(&mut self.store, ptr as usize, &bytes)
            .map_err(|error| VmError::Memory(error.to_string()))?;

        self.fuel()?;

        self.exports
            .event
            .call(&mut self.store, Event::LEN as u32)
            .map_err(|error| VmError::Trap(error.to_string()))?;

        self.check_memory()
    }

    /// Builds and returns one frame.
    pub fn frame(&mut self, now_ms: i64) -> Result<DisplayList, VmError> {
        self.fuel()?;

        let count = self
            .exports
            .frame
            .call(&mut self.store, now_ms)
            .map_err(|error| VmError::Trap(error.to_string()))?;

        self.check_memory()?;

        let capacity = self
            .exports
            .display_capacity
            .call(&mut self.store, ())
            .map_err(|error| VmError::Trap(error.to_string()))?;

        let ptr = self
            .exports
            .display_ptr
            .call(&mut self.store, ())
            .map_err(|error| VmError::Trap(error.to_string()))?;

        let count = (count as usize)
            .min(capacity as usize)
            .min(MAX_COMMANDS);

        let mut bytes = vec![0u8; count * app_abi::Command::LEN];

        if !bytes.is_empty() {
            self.memory
                .read(&self.store, ptr as usize, &mut bytes)
                .map_err(|error| VmError::Memory(error.to_string()))?;
        }

        Ok(DisplayList::parse(&bytes, count, |ptr, len| {
            read_string(&self.store, Some(self.memory), ptr, len).unwrap_or_default()
        }))
    }

    pub fn pause(&mut self) -> Result<(), VmError> {
        if let Some(pause) = self.exports.pause {
            self.fuel()?;
            pause
                .call(&mut self.store, ())
                .map_err(|error| VmError::Trap(error.to_string()))?;
        }

        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), VmError> {
        if let Some(resume) = self.exports.resume {
            self.fuel()?;
            resume
                .call(&mut self.store, ())
                .map_err(|error| VmError::Trap(error.to_string()))?;
        }

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), VmError> {
        if let Some(stop) = self.exports.stop {
            self.fuel()?;
            stop
                .call(&mut self.store, ())
                .map_err(|error| VmError::Trap(error.to_string()))?;
        }

        Ok(())
    }

    /// Whether the application asked for a frame since this was last read.
    pub fn take_invalidated(&mut self) -> bool {
        let invalidated = self.store.data().invalidated;
        self.store.data_mut().invalidated = false;
        invalidated
    }

    /// Whether the application asked to close.
    pub fn exit_requested(&self) -> bool {
        self.store.data().exit_requested
    }

    /// The application's services, for a caller that needs to read a flag the services set.
    pub fn services_mut(&mut self) -> &mut dyn Services {
        &mut *self.store.data_mut().services
    }

    /// Linear memory in bytes.
    pub fn memory_bytes(&self) -> u64 {
        self.memory
            .current_pages(&self.store)
            .to_bytes()
            .unwrap_or(0) as u64
    }

    fn fuel(&mut self) -> Result<(), VmError> {
        self.store
            .add_fuel(self.limits.fuel_per_call)
            .map_err(|error| VmError::Fuel(error.to_string()))
    }

    fn check_memory(&self) -> Result<(), VmError> {
        let bytes = self.memory_bytes();
        let allowed = self.limits.max_pages() * 65536;

        if bytes > allowed {
            return Err(VmError::MemoryOverBudget { bytes, allowed });
        }

        Ok(())
    }

    fn scratch_ptr(&mut self) -> Result<u32, VmError> {
        self.exports
            .scratch_ptr
            .call(&mut self.store, ())
            .map_err(|error| VmError::Trap(error.to_string()))
    }

    fn scratch_len(&mut self) -> Result<u32, VmError> {
        self.exports
            .scratch_len
            .call(&mut self.store, ())
            .map_err(|error| VmError::Trap(error.to_string()))
    }
}

/// Binds every host function an application may import.
///
/// All of them take pointers and lengths and none of them trusts either: a call with a pointer into
/// nowhere is answered with a failure code, never with a panic and never with a write.
fn bind(linker: &mut Linker<HostState>) -> Result<(), VmError> {
    let link = |result: Result<&mut Linker<HostState>, wasmi::errors::LinkerError>| {
        result
            .map(|_| ())
            .map_err(|error| VmError::Link(error.to_string()))
    };

    link(linker.func_wrap(
        host::MODULE,
        host::LOG,
        |mut caller: Caller<'_, HostState>, ptr: u32, len: u32| {
            if let Some(text) = read_string(&caller, memory_of(&caller), ptr, len.min(MAX_LOG_BYTES as u32)) {
                caller.data_mut().services.log(&text);
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NOW_MS,
        |mut caller: Caller<'_, HostState>| -> i64 { caller.data_mut().services.now_ms() },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::TEXT_WIDTH,
        |mut caller: Caller<'_, HostState>, ptr: u32, len: u32, size: i32| -> i32 {
            let Some(text) = read_string(&caller, memory_of(&caller), ptr, len.min(MAX_LOG_BYTES as u32))
            else {
                return -1;
            };

            caller.data_mut().services.text_width(&text, size as f32)
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::PREF_GET,
        |mut caller: Caller<'_, HostState>,
         key_ptr: u32,
         key_len: u32,
         out_ptr: u32,
         out_len: u32|
         -> i32 {
            let memory = memory_of(&caller);
            let Some(key) = read_string(&caller, memory, key_ptr, key_len) else {
                return -1;
            };

            let mut buffer = vec![0u8; out_len.min(4096) as usize];

            match caller.data_mut().services.pref_get(&key, &mut buffer) {
                Some(len) => match memory {
                    Some(memory)
                        if memory
                            .write(&mut caller, out_ptr as usize, &buffer[..len])
                            .is_ok() =>
                    {
                        len as i32
                    }
                    _ => -1,
                },
                None => -1,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::PREF_SET,
        |mut caller: Caller<'_, HostState>,
         key_ptr: u32,
         key_len: u32,
         value_ptr: u32,
         value_len: u32|
         -> i32 {
            let memory = memory_of(&caller);
            let Some(key) = read_string(&caller, memory, key_ptr, key_len) else {
                return -1;
            };
            let Some(value) = read_bytes(&caller, memory, value_ptr, value_len) else {
                return -1;
            };

            if caller.data_mut().services.pref_set(&key, &value) {
                0
            } else {
                -1
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::FILE_READ,
        |mut caller: Caller<'_, HostState>,
         name_ptr: u32,
         name_len: u32,
         out_ptr: u32,
         out_len: u32|
         -> i32 {
            let memory = memory_of(&caller);
            let Some(name) = read_string(&caller, memory, name_ptr, name_len) else {
                return -1;
            };

            let mut buffer = vec![0u8; out_len.min(64 * 1024) as usize];

            match caller.data_mut().services.file_read(&name, &mut buffer) {
                Some(len) => match memory {
                    Some(memory)
                        if memory
                            .write(&mut caller, out_ptr as usize, &buffer[..len])
                            .is_ok() =>
                    {
                        len as i32
                    }
                    _ => -1,
                },
                None => -1,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::FILE_WRITE,
        |mut caller: Caller<'_, HostState>,
         name_ptr: u32,
         name_len: u32,
         data_ptr: u32,
         data_len: u32|
         -> i32 {
            let memory = memory_of(&caller);
            let Some(name) = read_string(&caller, memory, name_ptr, name_len) else {
                return -1;
            };
            let Some(data) = read_bytes(&caller, memory, data_ptr, data_len) else {
                return -1;
            };

            if caller.data_mut().services.file_write(&name, &data) {
                0
            } else {
                -1
            }
        },
    ))?;

    // -------------------------------------------------------------------------------------------
    // v1.1: the network, the box's own readings, and the speaker.
    // -------------------------------------------------------------------------------------------

    link(linker.func_wrap(
        host::MODULE,
        host::HTTP_START,
        |mut caller: Caller<'_, HostState>, url_ptr: u32, url_len: u32| -> i32 {
            if !granted(&caller, perm::NETWORK) {
                return code::DENIED;
            }

            let memory = memory_of(&caller);
            let Some(url) = read_string(&caller, memory, url_ptr, url_len.min(2048)) else {
                return code::INVALID;
            };

            if url.trim().is_empty() {
                return code::INVALID;
            }

            caller.data_mut().services.http_start(&url)
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::HTTP_STATE,
        |mut caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::NETWORK) {
                return state::DENIED;
            }

            caller.data_mut().services.http_state()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::HTTP_FAULT,
        |mut caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::NETWORK) {
                return fault::DENIED;
            }

            caller.data_mut().services.http_fault()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::HTTP_PROGRESS,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::NETWORK) {
                return code::DENIED;
            }

            let progress = caller.data_mut().services.http_progress();

            match progress {
                Some(progress) => write_fixed(&mut caller, out_ptr, out_len, &progress.to_bytes()),
                None => code::INVALID,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::HTTP_TAKE,
        |mut caller: Caller<'_, HostState>,
         status_ptr: u32,
         body_ptr: u32,
         body_len: u32|
         -> i32 {
            if !granted(&caller, perm::NETWORK) {
                return code::DENIED;
            }

            if caller.data().pending_http.is_none() {
                let answer = caller.data_mut().services.http_take();
                caller.data_mut().pending_http = answer;
            }

            let Some((status, body)) = caller.data().pending_http.clone() else {
                return code::INVALID;
            };

            // Not a loss: an answer that does not fit is kept, and the caller is told how much
            // room it needs. The length of a package's metadata or a forecast is known long before
            // the buffer is.
            if (body_len as usize) < body.len() {
                return body.len() as i32;
            }

            let header = (status as u32).to_le_bytes();
            let written = write_fixed(&mut caller, status_ptr, 4, &header);

            if written < 0 {
                return written;
            }

            let written = write_bytes(&mut caller, body_ptr, body_len, &body);
            caller.data_mut().pending_http = None;
            written
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::HTTP_CANCEL,
        |mut caller: Caller<'_, HostState>| -> i32 {
            if !granted(&caller, perm::NETWORK) {
                return code::DENIED;
            }

            caller.data_mut().pending_http = None;
            caller.data_mut().services.http_cancel()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::SYSTEM,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::SYSTEM) {
                return code::DENIED;
            }

            let readings = caller.data_mut().services.system();

            match readings {
                Some(readings) => write_fixed(&mut caller, out_ptr, out_len, &readings.to_bytes()),
                None => code::UNSUPPORTED,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::FIRMWARE_VERSION,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::SYSTEM) {
                return code::DENIED;
            }

            let version = caller.data_mut().services.firmware_version();

            match version {
                Some(version) => write_bytes(&mut caller, out_ptr, out_len, version.as_bytes()),
                None => code::UNSUPPORTED,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::AUDIO_PLAY,
        |mut caller: Caller<'_, HostState>, path_ptr: u32, path_len: u32| -> i32 {
            if !granted(&caller, perm::AUDIO) {
                return code::DENIED;
            }

            let memory = memory_of(&caller);
            let Some(path) = read_string(&caller, memory, path_ptr, path_len.min(1024)) else {
                return code::INVALID;
            };

            caller.data_mut().services.audio_play(&path)
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::AUDIO_CONTROL,
        |mut caller: Caller<'_, HostState>, op: u32, value: i32| -> i32 {
            if !granted(&caller, perm::AUDIO) {
                return code::DENIED;
            }

            caller.data_mut().services.audio_control(op, value)
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::AUDIO_STATE,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::AUDIO) {
                return code::DENIED;
            }

            let state = caller.data_mut().services.audio_state();

            match state {
                Some(state) => write_fixed(&mut caller, out_ptr, out_len, &state.to_bytes()),
                None => code::UNSUPPORTED,
            }
        },
    ))?;

    // -------------------------------------------------------------------------------------------
    // v1.2: the radio.
    // -------------------------------------------------------------------------------------------

    link(linker.func_wrap(
        host::MODULE,
        host::WIFI_STATUS,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::WIFI) {
                return code::DENIED;
            }

            let status = caller.data_mut().services.wifi_status();

            match status {
                Some(status) => write_fixed(&mut caller, out_ptr, out_len, &status.to_bytes()),
                None => code::UNSUPPORTED,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::WIFI_SCAN_START,
        |mut caller: Caller<'_, HostState>| -> i32 {
            if !granted(&caller, perm::WIFI) {
                return code::DENIED;
            }

            caller.data_mut().scan_fresh = false;
            caller.data_mut().scan.clear();
            caller.data_mut().services.wifi_scan_start()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::WIFI_SCAN_STATE,
        |mut caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::WIFI) {
                return scan::DENIED;
            }

            caller.data_mut().services.wifi_scan_state()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::WIFI_SCAN_COUNT,
        |mut caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::WIFI) {
                return 0;
            }

            refresh_scan(&mut caller);

            caller.data().scan.len() as u32
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::WIFI_SCAN_AP,
        |mut caller: Caller<'_, HostState>, index: u32, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::WIFI) {
                return code::DENIED;
            }

            refresh_scan(&mut caller);

            let ap = caller.data().scan.get(index as usize).cloned();

            match ap {
                Some(ap) => {
                    let line = format!(
                        "{}\t{}\t{}\t{}",
                        one_line(&ap.ssid),
                        ap.rssi,
                        u8::from(ap.secure),
                        ap.channel
                    );

                    write_bytes(&mut caller, out_ptr, out_len, line.as_bytes())
                }
                None => code::INVALID,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::WIFI_CONNECT,
        |mut caller: Caller<'_, HostState>,
         ssid_ptr: u32,
         ssid_len: u32,
         password_ptr: u32,
         password_len: u32|
         -> i32 {
            if !granted(&caller, perm::WIFI) {
                return code::DENIED;
            }

            let memory = memory_of(&caller);
            let Some(ssid) = read_string(&caller, memory, ssid_ptr, ssid_len.min(64)) else {
                return code::INVALID;
            };
            let Some(password) =
                read_string(&caller, memory, password_ptr, password_len.min(128))
            else {
                return code::INVALID;
            };

            caller.data_mut().services.wifi_connect(&ssid, &password)
        },
    ))?;

    // -------------------------------------------------------------------------------------------
    // v1.3: the machines beside the box, and the ones in an account.
    // -------------------------------------------------------------------------------------------

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SNAPSHOT,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            let snapshot = caller.data_mut().services.nas_snapshot();
            write_fixed(&mut caller, out_ptr, out_len, &snapshot.to_bytes())
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_DISKS,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            let disks = caller.data_mut().services.nas_disks();
            let text = nas_disk_lines(&disks);

            write_bytes(&mut caller, out_ptr, out_len, text.as_bytes())
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_REFRESH,
        |mut caller: Caller<'_, HostState>| -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            caller.data_mut().services.nas_refresh()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SHELL_OPEN,
        |mut caller: Caller<'_, HostState>,
         host_ptr: u32,
         host_len: u32,
         columns: u32,
         rows: u32|
         -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            let memory = memory_of(&caller);
            let Some(host) = read_string(&caller, memory, host_ptr, host_len.min(128)) else {
                return code::INVALID;
            };

            let answer = caller.data_mut().services.nas_shell_open(
                &host,
                columns.min(u16::MAX as u32) as u16,
                rows.min(u16::MAX as u32) as u16,
            );

            if answer == code::OK {
                caller.data_mut().shell_status = nas_shell::OPENING;
                caller.data_mut().pending_shell.clear();
            }

            answer
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SHELL_WRITE,
        |mut caller: Caller<'_, HostState>, bytes_ptr: u32, bytes_len: u32| -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            let memory = memory_of(&caller);
            let Some(bytes) = read_bytes(&caller, memory, bytes_ptr, bytes_len.min(4096)) else {
                return code::INVALID;
            };

            caller.data_mut().services.nas_shell_write(&bytes)
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SHELL_STATUS,
        |caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::NAS) {
                return nas_shell::DENIED;
            }

            caller.data().shell_status
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SHELL_FAULT,
        |caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::NAS) {
                return fault::DENIED;
            }

            caller.data().shell_fault
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SHELL_TAKE,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            if caller.data().pending_shell.is_empty() {
                let (status, why, output) = caller.data_mut().services.nas_shell_take();
                caller.data_mut().shell_status = status;
                caller.data_mut().shell_fault = why;
                caller.data_mut().pending_shell = output;
            }

            let want = (out_len as usize).min(caller.data().pending_shell.len());
            let chunk: Vec<u8> = caller.data_mut().pending_shell.drain(..want).collect();
            let written = write_bytes(&mut caller, out_ptr, out_len, &chunk);

            if written < 0 {
                // Put back what could not be written, in front of what is still queued: a terminal
                // that lost a frame of output would lose the middle of somebody's command.
                let mut rest = chunk;
                rest.extend(caller.data_mut().pending_shell.drain(..));
                caller.data_mut().pending_shell = rest;
            }

            written
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::NAS_SHELL_CLOSE,
        |mut caller: Caller<'_, HostState>| -> i32 {
            if !granted(&caller, perm::NAS) {
                return code::DENIED;
            }

            caller.data_mut().pending_shell.clear();
            caller.data_mut().shell_status = nas_shell::CLOSED;
            caller.data_mut().services.nas_shell_close()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::CLOUD_KEY,
        |mut caller: Caller<'_, HostState>, vendor: u32, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::CLOUD) {
                return code::DENIED;
            }

            let key = caller.data_mut().services.cloud_key(vendor);

            match key {
                Some(key) => {
                    let text = format!("{}\t{}", one_line(&key.masked), one_line(&key.region));
                    write_bytes(&mut caller, out_ptr, out_len, text.as_bytes())
                }
                None => code::UNSUPPORTED,
            }
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::CLOUD_START,
        |mut caller: Caller<'_, HostState>,
         target: u32,
         action: u32,
         instance_ptr: u32,
         instance_len: u32,
         region_ptr: u32,
         region_len: u32|
         -> i32 {
            if !granted(&caller, perm::CLOUD) {
                return code::DENIED;
            }

            let memory = memory_of(&caller);
            let Some(instance) =
                read_string(&caller, memory, instance_ptr, instance_len.min(128))
            else {
                return code::INVALID;
            };
            let Some(region) = read_string(&caller, memory, region_ptr, region_len.min(64)) else {
                return code::INVALID;
            };

            caller
                .data_mut()
                .services
                .cloud_start(target, action, &instance, &region)
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::CLOUD_STATE,
        |mut caller: Caller<'_, HostState>| -> u32 {
            if !granted(&caller, perm::CLOUD) {
                return state::DENIED;
            }

            caller.data_mut().services.cloud_state()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::CLOUD_TAKE,
        |mut caller: Caller<'_, HostState>, out_ptr: u32, out_len: u32| -> i32 {
            if !granted(&caller, perm::CLOUD) {
                return code::DENIED;
            }

            if caller.data().pending_cloud.is_none() {
                let answer = caller.data_mut().services.cloud_take();
                caller.data_mut().pending_cloud = answer;
            }

            let Some(text) = caller.data().pending_cloud.clone() else {
                return code::INVALID;
            };

            if (out_len as usize) < text.len() {
                return text.len() as i32;
            }

            let written = write_bytes(&mut caller, out_ptr, out_len, text.as_bytes());
            caller.data_mut().pending_cloud = None;
            written
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::CLOUD_CANCEL,
        |mut caller: Caller<'_, HostState>| -> i32 {
            if !granted(&caller, perm::CLOUD) {
                return code::DENIED;
            }

            caller.data_mut().pending_cloud = None;
            caller.data_mut().services.cloud_cancel()
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::INVALIDATE,
        |mut caller: Caller<'_, HostState>| {
            caller.data_mut().invalidated = true;
            caller.data_mut().services.invalidate();
        },
    ))?;

    link(linker.func_wrap(
        host::MODULE,
        host::EXIT,
        |mut caller: Caller<'_, HostState>| {
            caller.data_mut().exit_requested = true;
            caller.data_mut().services.exit();
        },
    ))?;

    Ok(())
}

/// Whether the package asked for the permission a host function needs.
fn granted(caller: &Caller<'_, HostState>, permission: u32) -> bool {
    caller.data().permissions & permission != 0
}

/// Copies `bytes` into application memory, cut to `out_len`.
///
/// Returns how many bytes the answer *is*, which is how a caller learns that a buffer was too
/// small: a return larger than the buffer it handed over is a length it now knows.
fn write_bytes(caller: &mut Caller<'_, HostState>, ptr: u32, out_len: u32, bytes: &[u8]) -> i32 {
    let len = bytes.len().min(out_len as usize);
    let Some(memory) = memory_of(caller) else {
        return code::UNSUPPORTED;
    };

    if len > 0 && memory.write(caller, ptr as usize, &bytes[..len]).is_err() {
        return code::INVALID;
    }

    bytes.len() as i32
}

/// Writes a value that has a fixed size and refuses a buffer smaller than it: half of a snapshot is
/// not a snapshot with the rest missing, it is a wrong number.
fn write_fixed(caller: &mut Caller<'_, HostState>, ptr: u32, out_len: u32, bytes: &[u8]) -> i32 {
    if (out_len as usize) < bytes.len() {
        return code::INVALID;
    }

    write_bytes(caller, ptr, out_len, bytes)
}

/// Reads the last scan once per scan, and not once per access point.
fn refresh_scan(caller: &mut Caller<'_, HostState>) {
    if caller.data().scan_fresh {
        return;
    }

    let scan = caller.data_mut().services.wifi_scan_results();
    caller.data_mut().scan = scan;
    caller.data_mut().scan_fresh = true;
}

/// A field of a text answer without the characters that would end it early.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

/// The NAS's disks as the ABI's one-line-per-disk text.
fn nas_disk_lines(disks: &[crate::services::NasDisk]) -> String {
    let mut text = String::new();

    for disk in disks {
        let temperature = disk
            .temperature_c
            .map(|c| (c * 100.0).round() as i64)
            .unwrap_or(i32::MIN as i64);
        let busy = (disk.busy_percent * 100.0).round().clamp(0.0, 10_000.0) as u32;

        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            one_line(&disk.device),
            one_line(&disk.model),
            disk.size_bytes,
            temperature,
            busy,
            disk.read_per_sec,
            disk.write_per_sec,
            one_line(&disk.purpose)
        );
    }

    text
}

fn memory_of(caller: &Caller<'_, HostState>) -> Option<Memory> {
    caller.data().memory
}

fn read_bytes(
    context: impl wasmi::AsContext<UserState = HostState>,
    memory: Option<Memory>,
    ptr: u32,
    len: u32,
) -> Option<Vec<u8>> {
    let memory = memory?;

    if len as usize > MAX_PACKAGE_BYTES {
        return None;
    }

    let mut buffer = vec![0u8; len as usize];
    memory.read(context, ptr as usize, &mut buffer).ok()?;

    Some(buffer)
}

fn read_string(
    context: impl wasmi::AsContext<UserState = HostState>,
    memory: Option<Memory>,
    ptr: u32,
    len: u32,
) -> Option<String> {
    let bytes = read_bytes(context, memory, ptr, len)?;

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Why a VM could not be built or could not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmError {
    TooLarge(usize),
    Compile(String),
    Link(String),
    Instantiate(String),
    NoMemory,
    MissingExport(String),
    Abi(u32),
    Refused(u32),
    Trap(String),
    Fuel(String),
    Memory(String),
    ScratchTooSmall(u32),
    MemoryOverBudget { bytes: u64, allowed: u64 },
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(size) => write!(f, "应用模块太大（{size} 字节）"),
            Self::Compile(error) => write!(f, "应用无法编译: {error}"),
            Self::Link(error) | Self::Instantiate(error) => write!(f, "应用无法启动: {error}"),
            Self::NoMemory => write!(f, "应用没有导出 memory"),
            Self::MissingExport(name) => write!(f, "应用缺少入口 {name}"),
            Self::Abi(abi) => write!(f, "应用 ABI {} 与本机不兼容", app_abi::major_of(*abi)),
            Self::Refused(code) => write!(f, "应用拒绝了这个屏幕（{code}）"),
            Self::Trap(error) => write!(f, "应用出错: {error}"),
            Self::Fuel(error) => write!(f, "应用超出执行预算: {error}"),
            Self::Memory(error) => write!(f, "应用访问了不该访问的内存: {error}"),
            Self::ScratchTooSmall(len) => write!(f, "应用的输入缓冲区太小（{len} 字节）"),
            Self::MemoryOverBudget { bytes, allowed } => {
                write!(f, "应用内存超出预算（{bytes} / {allowed} 字节）")
            }
        }
    }
}

impl std::error::Error for VmError {}

/// Milliseconds since boot on a host with no board clock. Used by the desktop runner and by tests;
/// the panel supplies the real clock through [`Services`].
pub fn host_now_ms() -> i64 {
    static START: std::sync::OnceLock<SystemTime> = std::sync::OnceLock::new();
    let start = START.get_or_init(SystemTime::now);

    SystemTime::now()
        .duration_since(*start)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// The key a launcher sends when a person presses the back button.
pub fn back_key() -> Event {
    Event::new(app_abi::event::KEY_DOWN, 0, 0, key::BACK as i32, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::MemoryServices;

    /// A hand-written module that is nothing but the smallest legal implementation of the ABI: no
    /// memory export, no exports at all. It exists to make one point — a package that is not this
    /// project's package is refused at the door, before anything runs.
    const NOT_OURS: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn a_module_without_the_abi_is_refused_before_it_runs() {
        let error = Vm::new(
            NOT_OURS,
            Box::new(MemoryServices::default()),
            Limits::default(),
            app_abi::perm::ALL,
        );

        assert!(
            matches!(error, Err(VmError::MissingExport(name)) if name == export::ABI_VERSION),
            "a module with no exports is not an application"
        );
    }

    /// A module whose `hb_init` is nothing but the import under test. What it returns is therefore
    /// the host's own answer, which is what makes the gate visible from outside the sandbox.
    fn probe_module(import: &str, params: &str, arguments: &str) -> Vec<u8> {
        let source = format!(
            r#"
            (module
              (import "host" "{import}" (func $probe {params} (result i32)))
              (memory (export "memory") 1)
              (func (export "hb_abi_version") (result i32) i32.const 0x00010003)
              (func (export "hb_init") (param i32 i32 i32 i64) (result i32)
                {arguments}
                call $probe)
              (func (export "hb_scratch_ptr") (result i32) i32.const 0)
              (func (export "hb_scratch_len") (result i32) i32.const 64)
              (func (export "hb_event") (param i32) (result i32) i32.const 0)
              (func (export "hb_frame") (param i64) (result i32) i32.const 0)
              (func (export "hb_display_ptr") (result i32) i32.const 0)
              (func (export "hb_display_capacity") (result i32) i32.const 0)
            )
            "#
        );

        wat::parse_str(source).expect("a module this test wrote")
    }

    fn init_result(wasm: &[u8], permissions: u32) -> Result<(), VmError> {
        let mut vm = Vm::new(
            wasm,
            Box::new(MemoryServices::default()),
            Limits::default(),
            permissions,
        )?;

        vm.init(480, 480, 1.0, 0)
    }

    /// A package that did not ask for a permission is refused before the host is asked anything —
    /// importantly, before a socket, a scan or a preference write happens. Asking for the bit is
    /// what lets the same module through to the service behind it.
    #[test]
    fn a_service_without_its_permission_is_refused_before_the_host_is_asked() {
        let cases = [
            (host::HTTP_STATE, perm::NETWORK, "", "", state::DENIED),
            (host::WIFI_STATUS, perm::WIFI, "(param i32 i32)", "i32.const 0 i32.const 0", code::DENIED as u32),
            (host::SYSTEM, perm::SYSTEM, "(param i32 i32)", "i32.const 0 i32.const 0", code::DENIED as u32),
            (host::AUDIO_STATE, perm::AUDIO, "(param i32 i32)", "i32.const 0 i32.const 0", code::DENIED as u32),
            (host::NAS_SHELL_STATUS, perm::NAS, "", "", nas_shell::DENIED),
            (host::CLOUD_STATE, perm::CLOUD, "", "", state::DENIED),
        ];

        for (import, permission, params, arguments, refused) in cases {
            let wasm = probe_module(import, params, arguments);

            assert_eq!(
                init_result(&wasm, 0),
                Err(VmError::Refused(refused)),
                "{import} without its permission"
            );

            assert_ne!(
                init_result(&wasm, permission),
                Err(VmError::Refused(refused)),
                "{import} with its permission must reach the service"
            );
        }
    }

    /// A package built against a newer minor imports functions this host does not have. It is
    /// refused with the word it reported rather than by a linker error naming an import.
    #[test]
    fn a_package_from_a_newer_minor_is_refused_before_it_is_instantiated() {
        let wasm = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "hb_abi_version") (result i32) i32.const 0x00010004)
            )
            "#,
        )
        .expect("a module this test wrote");

        assert_eq!(
            Vm::new(
                &wasm,
                Box::new(MemoryServices::default()),
                Limits::default(),
                app_abi::perm::ALL,
            )
            .err(),
            Some(VmError::Abi(app_abi::version_word(1, 4)))
        );
    }

    #[test]
    fn garbage_is_a_compile_error_not_a_panic() {
        let error = Vm::new(
            b"not wasm at all",
            Box::new(MemoryServices::default()),
            Limits::default(),
            app_abi::perm::ALL,
        );
        assert!(matches!(error, Err(VmError::Compile(_))));
    }

    /// The limits are clamped: no package can ask for more memory than the sandbox has.
    #[test]
    fn a_memory_budget_cannot_exceed_the_abi_maximum() {
        let limits = Limits::default().with_memory_kb(u32::MAX);
        assert_eq!(limits.memory_kb, MAX_MEMORY_KB);
        assert_eq!(limits.max_pages(), (MAX_MEMORY_KB as u64).div_ceil(64));
    }

    #[test]
    fn the_host_clock_moves_forward() {
        let first = host_now_ms();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(host_now_ms() >= first);
    }
}
