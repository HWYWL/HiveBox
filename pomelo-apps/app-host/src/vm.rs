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

use app_abi::{
    export, host, key, Event, ABI_MAJOR, MAX_COMMANDS, MAX_LOG_BYTES, MAX_MEMORY_KB, MAX_PACKAGE_BYTES,
};
use wasmi::{Caller, Config, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

use crate::display::DisplayList;
use crate::services::Services;

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
    /// Resolved once, after instantiation: every host function needs to read or write application
    /// memory, and looking the export up per call would be a string comparison per string.
    memory: Option<Memory>,
    /// Set by `hb_invalidate`, read and cleared by the caller of [`Vm::frame`].
    pub invalidated: bool,
    /// Set by `hb_exit`, read by the launcher.
    pub exit_requested: bool,
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
    pub fn new(
        wasm: &[u8],
        services: Box<dyn Services>,
        limits: Limits,
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
                memory: None,
                invalidated: false,
                exit_requested: false,
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

        if app_abi::major_of(abi) != ABI_MAJOR {
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
        let error = Vm::new(NOT_OURS, Box::new(MemoryServices::default()), Limits::default());

        assert!(
            matches!(error, Err(VmError::MissingExport(name)) if name == export::ABI_VERSION),
            "a module with no exports is not an application"
        );
    }

    #[test]
    fn garbage_is_a_compile_error_not_a_panic() {
        let error = Vm::new(b"not wasm at all", Box::new(MemoryServices::default()), Limits::default());
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
