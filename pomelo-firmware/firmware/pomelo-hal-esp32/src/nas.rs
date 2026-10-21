//! The NAS this board watches, over SSH.
//!
//! A pure-Rust SSH client — [`sunset`], over the Wi-Fi this image already has — that runs one command
//! on the NAS per look and hands the text back to `pomelo_hal::nas_sample`, which is where the parsing
//! lives and why the parsing can be tested on a desktop against a recorded sample.
//!
//! # There is no C SSH library any more
//!
//! This used to be a thin `extern "C"` binding over `components/board_hal/board_nas.c`, which linked
//! libssh and carried its flash with it. [`sunset`] is SSH written in Rust, with its own key exchange
//! and ciphers over the RustCrypto crates this image already builds, so the whole session is below:
//! what crossed the C boundary before — a host, a port, a password, a command, and the text that came
//! back — is now two fields and a thread. [`sunset`] uses `getrandom` for its randomness, which on
//! this target reads ESP-IDF's own `esp_fill_random`.
//!
//! # One thread, and one look at a time
//!
//! A look is a TCP connection, a key exchange and a command — hundreds of milliseconds of work a UI
//! frame cannot spend. So a worker thread owns the watching, and [`NasBackend::refresh`] only *asks*
//! for a look and picks up whatever the last one produced: the start-and-poll shape [`NasBackend`]
//! documents. Looks never overlap, and never come faster than the credentials' interval, however
//! often a caller asks.
//!
//! A fresh session is opened for every look rather than kept across them. A session kept across looks
//! has to survive the NAS rebooting, the router dropping an idle connection and the server's own idle
//! timeout — and each of those fails *quietly*, on the look where somebody was watching the panel. The
//! handshake is the cheap half of a look against a machine that is awake, so this pays it every time
//! and starts from a connection it knows is good.
//!
//! # The host key
//!
//! Trust on first use, the way `ssh` does it: the first look at a machine records the key it offered,
//! in `known_hosts` beside the credentials file, and every look after that refuses a *different* key
//! rather than opening a session anyway. The key is compared by its SSH wire bytes — the server's
//! identity, not the spelling of its fingerprint — so an encoding the panel might draw differently is
//! not mistaken for a different machine.
//!
//! # The credentials, and where they are written
//!
//! In AppData as a file, like the Wi-Fi ones and for the reason `pomelo_hal::nas_credentials` gives:
//! an app that read the file itself would work on a desktop and do nothing on the board, where there
//! is no `std::fs` that can create it. Reading and writing go through `hal_storage_*`, as the Wi-Fi
//! backend's do. The password is handed to the SSH client and nothing else ever sees it.
//!
//! # Who writes that file, and why this file reads it again
//!
//! The management page does — three fields and a save button on the device page, because typing an
//! address and a password on a 480-pixel panel with no keyboard is nobody's idea of a good time. But
//! the page is served by the *C* side and that file is read by this one, so something has to notice.
//! That something is [`NasBackend::refresh`]: every look, the file is read and compared with what this
//! backend last pointed the worker at, and a difference re-points it. Which is also why deleting the
//! file by hand stops the watching — "forget" and "delete it" are the same sentence said two ways,
//! and neither needs a page to be open.
//!
//! # The flash, and why the worker never touches it
//!
//! The worker's stack is in PSRAM — there was nowhere else big enough to put it — and on this chip a
//! task with a stack there may not touch the flash at all. `spi_flash_disable_interrupts_caches_and_
//! other_cpu` asserts exactly that, and LittleFS reaches it even for a *read*: `fopen` ends in
//! `esp_partition_read` and a cache-off flash read. So `known_hosts` is never read or written by the
//! worker. [`EspNas::new`] reads it once, on the thread that builds the board; [`Inner`] is where the
//! worker looks a host key up and records a new one; and [`NasBackend::refresh`] — the app thread,
//! whose stack is internal — writes it out. See [`check_hostkey`] and [`write_known_hosts`].

use std::ffi::CString;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pomelo_hal::app_data::BOARD_APP_DATA;
use pomelo_hal::error::HalError;
use pomelo_hal::nas_credentials::{self, NasCredentials};
use pomelo_hal::nas_sample::{round_uptime, split_pair, COMMAND, COMMAND_TWICE, Sampler};
use pomelo_hal::traits::NasBackend;
use pomelo_hal::types::{NasFault, NasReading, NasShell, NasShellStatus, NasStatus};

use sunset::event::{CheckHostkey, CliEvent, Event};
use sunset::sshwire::{SSHEncode, SSHSink, WireResult};
use sunset::{ChanData, ChanHandle, Pty, Runner};

mod ffi {
    use core::ffi::{c_char, c_int};

    // The radio's own state, shared with the Wi-Fi backend rather than declared a second time: this is
    // how the NAS backend tells "this board has no network" from "the NAS did not answer", and two
    // `extern` declarations of one C symbol have to agree on the types they use — the status struct is
    // the part that would not.
    pub use crate::wifi::ffi::{hal_wifi_get_status, HalWifiStatus, IP_MAX_LEN, SSID_MAX_LEN};

    extern "C" {
        // The same file API the Wi-Fi credentials go through: ESP-IDF's VFS is not something Rust's
        // `std::fs` can create files on this target (see `wifi.rs`), so the C side owns the file.
        pub fn hal_storage_read_file(
            path: *const c_char,
            out: *mut c_char,
            capacity: usize,
            out_len: *mut usize,
        ) -> c_int;
        pub fn hal_storage_write_file(path: *const c_char, data: *const c_char, len: usize) -> c_int;
        pub fn hal_storage_remove_file(path: *const c_char) -> c_int;
    }
}

/// Asking ESP-IDF for a pthread stack in PSRAM, for the one thread that needs it.
///
/// The worker's stack cannot come from internal RAM. ESP-IDF's `pthread_create` allocates stacks with
/// `MALLOC_CAP_INTERNAL` unless the *creating* thread has said otherwise, and by the time this backend
/// is built the internal heap has no 32 KB run left to give — which is what the first boot on hardware
/// said, as `[nas] the SSH worker could not be started: Not enough space (os error 12)` and a NAS that
/// could never be watched. The C task this replaced asked for PSRAM by name
/// (`xTaskCreatePinnedToCoreWithCaps` with `MALLOC_CAP_SPIRAM`); a pthread asks the same way, through
/// its creating thread's configuration.
///
/// The configuration is *borrowed*: the caller's thread is put back exactly as it was found, so a
/// thread that later spawns something of its own is not quietly given a PSRAM stack.
mod pthread {
    use core::ffi::{c_char, c_int};
    use core::ptr;

    /// `esp_pthread_cfg_t`, from ESP-IDF's `esp_pthread.h`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Cfg {
        stack_size: usize,
        prio: usize,
        inherit_cfg: bool,
        thread_name: *const c_char,
        pin_to_core: c_int,
        stack_alloc_caps: u32,
    }

    /// `MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT`, from `esp_heap_caps.h`.
    const PSRAM_8BIT: u32 = (1 << 10) | (1 << 2);

    const ESP_OK: c_int = 0;

    extern "C" {
        fn esp_pthread_get_cfg(cfg: *mut Cfg) -> c_int;
        fn esp_pthread_set_cfg(cfg: *const Cfg) -> c_int;
        fn esp_pthread_get_default_config() -> Cfg;
    }

    /// What the calling thread's configuration was, so [`Borrowed::restore`] can put it back. `None`
    /// means it had none, which behaves exactly like the default one.
    pub struct Borrowed(Option<Cfg>);

    /// Make the calling thread's next spawn allocate its stack in PSRAM.
    pub fn ask_for_psram_stack() -> Borrowed {
        let mut current = Cfg {
            stack_size: 0,
            prio: 0,
            inherit_cfg: false,
            thread_name: ptr::null(),
            pin_to_core: -1,
            stack_alloc_caps: 0,
        };
        let had_one = unsafe { esp_pthread_get_cfg(&mut current) } == ESP_OK;

        let mut wanted = if had_one {
            current
        } else {
            // Nothing of the caller's to keep, so start from what `pthread_create` would have used.
            unsafe { esp_pthread_get_default_config() }
        };
        // Only the memory the stack comes from is changed. The *size* is deliberately left alone:
        // Rust's `Builder::stack_size` travels as a `pthread_attr_t`, and an attribute wins over this.
        wanted.stack_alloc_caps = PSRAM_8BIT;
        unsafe { esp_pthread_set_cfg(&wanted) };

        Borrowed(if had_one { Some(current) } else { None })
    }

    impl Borrowed {
        /// Put the calling thread's configuration back the way it was found.
        pub fn restore(self) {
            let cfg = match self.0 {
                Some(cfg) => cfg,
                None => unsafe { esp_pthread_get_default_config() },
            };
            unsafe { esp_pthread_set_cfg(&cfg) };
        }
    }
}

/// `HAL_WIFI_STATE_CONNECTED`, from `board_hal.h`.
const WIFI_STATE_CONNECTED: i32 = 3;

// The fault codes, from `board_hal.h`'s `hal_nas_fault_t`. They are the boundary's vocabulary and
// the words the panel draws are chosen from them in `fault_of`.
const FAULT_NONE: i32 = 0;
const FAULT_NO_NETWORK: i32 = 1;
const FAULT_UNREACHABLE: i32 = 2;
const FAULT_AUTHENTICATION: i32 = 3;
const FAULT_HOST_KEY: i32 = 4;
const FAULT_PROTOCOL: i32 = 5;

/// How long to wait for the TCP connection. Generous, because a sleeping NAS's ARP can take a moment,
/// and bounded, because a look that never returns is a reading that never updates.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a socket read may block. Short enough that a look notices output the protocol wants to
/// send while it waits for the server, long enough not to spin.
const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// How long the whole look may take, connection and all. Past this the NAS is not answering in any
/// sense this board can use, and the next look should get its turn.
const LOOK_DEADLINE: Duration = Duration::from_secs(30);

/// How often the worker wakes to check for a request or the interval. Also the ceiling on how late a
/// [`NasBackend::refresh`] can be answered.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long after a machine's first look the second one is taken.
///
/// `/proc` reports totals since boot, so every rate on this page is a difference between two looks — and
/// a machine's first look has none of them: its CPU and its network come back as nothing, and its disks
/// at zero. The page draws that as a dash, which is honest and is not what somebody opened it to see, so
/// the second look is taken a second after the first rather than an interval after it.
///
/// A second from both sides: the app asks again on the same beat (its own `PRIME`, where the other half
/// of this is written down). The two do not have to agree exactly — an ask that arrives before the look
/// is due is a flag the worker answers on its next pass.
const PRIME: Duration = Duration::from_secs(1);

/// How long a check waits for a machine's port to answer.
///
/// What this decides, in practice, is how long "not there" takes to say: a machine on the LAN answers in
/// milliseconds and a machine that is switched off never does. A second and a half is long enough for a
/// slow link to answer across and short enough that a row does not sit on 检测中… for a whole pass.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// How long between passes of checks.
///
/// A machine that comes back or goes away is a thing somebody looks at a list to see, and neither happens
/// between two seconds: ten is fast enough to watch for it and slow enough that a list left open overnight
/// is not a set of machines being knocked on every second.
const CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// The terminal type this board asks a session's machine for.
///
/// What a program at the other end reads to decide how much it may say: a pager asks for a screen's
/// worth, a colourful `ls` asks whether it may use colour at all. `xterm-256color` is one of the two
/// answers nearly everything understands, and [`crate::nas`]'s screen interpreter already draws the
/// colours it implies.
const SHELL_TERM: &str = "xterm-256color";

/// What the worker knows about the machine it is watching: which one it was, when it last looked, and
/// whether that machine has had the two looks a rate needs.
///
/// One value rather than three locals because they are all about the *same* machine and go together: a
/// machine the worker has just been pointed at is one it has no clock for at all, and a look at it is
/// due at once — the interval is the spacing *between* looks, not a delay before the first one.
#[derive(Default)]
struct Watched {
    /// The host of the last look.
    host: Option<String>,
    /// When that look finished. `None` until there has been one.
    at: Option<Instant>,
    /// That machine has had its pair of looks, so the interval takes over from [`PRIME`].
    paired: bool,
}

impl Watched {
    /// Whether a look at `config` is due.
    fn due(&self, config: &NasCredentials, now: Instant) -> bool {
        if self.host.as_deref() != Some(config.host.as_str()) {
            return true;
        }

        let spacing = if self.paired {
            config.interval()
        } else {
            PRIME
        };

        self.at.is_none_or(|at| now >= at + spacing)
    }

    /// Takes down a look at `config` that has just finished.
    fn looked(&mut self, config: &NasCredentials, now: Instant) {
        // The look that just finished is this machine's first unless the clock was already on it, which
        // is what makes the look after a first one the second: see [`PRIME`].
        self.paired = self.host.as_deref() == Some(config.host.as_str());
        self.host = Some(config.host.clone());
        self.at = Some(now);
    }
}

/// The largest SSH packet allowed, from RFC 4253: what sunset's packet buffers have to hold. Sunset's
/// own `config::SSH_MAX_PACKET` is `pub(crate)`, so the number lives here too.
const SSH_MAX_PACKET: usize = 35000;

/// How much socket data to move per call. Smaller than a packet, so a big one is fed in pieces.
const SOCKET_CHUNK: usize = 4096;

/// How much of a file to read. The credentials are five short lines; `known_hosts` is a host and a
/// hex key per machine.
const FILE_CAPACITY: usize = 4 * 1024;

/// The worker's stack. The SSH key exchange and the ciphers run on it, so it is not the 8 KB thread
/// default — see sunset's own demos, which budget tens of kilobytes for exactly this.
///
/// It is also more than the internal heap can give as one run, so this stack lives in PSRAM: see
/// [`pthread::ask_for_psram_stack`], which is how the thread says so before it is created.
const WORKER_STACK: usize = 32 * 1024;

/// The `known_hosts` file's name, beside the credentials file.
const KNOWN_HOSTS_FILE: &str = "known_hosts";

/// This board's NAS-watching backend.
pub struct EspNas {
    /// The previous look's counters, which is what turns this look's totals into rates.
    sampler: Mutex<Sampler>,
    /// The last reading that arrived, as the app will draw it.
    last: Mutex<NasReading>,
    /// What the worker has been pointed at, so that a change to the file can be noticed.
    ///
    /// Held here rather than asked of the worker because the question is "has the *file* changed",
    /// and only two copies of what it said can answer that.
    pointed_at: Option<NasCredentials>,
    /// The worker and the little state the two sides of the thread share.
    shared: Arc<Shared>,
    /// The terminal's session, if one has ever been asked for: its own worker, and the state the two
    /// sides of *that* thread share.
    ///
    /// A thread of its own and not the look's, because the trait insists the two connections not get in
    /// each other's way: a look can spend [`LOOK_DEADLINE`] on a machine that has stopped answering, and
    /// a terminal sharing that thread would be frozen for the whole of it — with the shell at the other
    /// end still reading, and somebody at this one waiting to see what they typed. Spawned the first
    /// time a session is opened rather than at boot, because its stack is 32 KB of PSRAM and a board
    /// nobody has opened a terminal on should not be paying for one.
    shell: Option<Arc<ShellShared>>,
    /// Which machine the terminal is pointed at, kept here rather than asked of the worker.
    ///
    /// The worker has the same host inside the session it is running, but it owns it and the app-side
    /// must not have to wait on the worker to answer "is the terminal on the machine being forgotten?" —
    /// see [`NasBackend::forget`]. `None` is a terminal with no session: before the first open, and
    /// after a [`NasBackend::shell_close`].
    shell_host: Option<String>,
}

/// The state [`EspNas`] and its worker thread share.
struct Shared {
    inner: Mutex<Inner>,
    /// Signalled whenever the app-side wants the worker to look at something.
    wake: Condvar,
}

#[derive(Default)]
struct Inner {
    /// The machine the worker is watching, or `None` if it is watching nothing.
    config: Option<NasCredentials>,
    /// A look has been asked for and not yet taken.
    requested: bool,
    /// The worker should stop. Never set in practice; the thread lives as long as the board.
    stop: bool,
    /// A look is in flight right now.
    busy: bool,
    /// The last fault, as a `hal_nas_fault_t`, or [`FAULT_NONE`].
    fault: i32,
    /// What completed looks have produced and nobody has taken yet, each with the instant its sample
    /// belongs to.
    ///
    /// A queue and not one text, because a machine's first look hands over *two* samples of one connection
    /// — see `COMMAND_TWICE` — and the instant travels with the text because the right to say "per second"
    /// comes from the gap between two samples and not from when somebody got round to reading them.
    pending: Vec<(Instant, Vec<u8>)>,
    /// The `known_hosts` file's contents, mirrored here because the worker cannot reach the flash —
    /// its stack is in PSRAM. Read once by [`EspNas::new`], looked up and extended by
    /// [`check_hostkey`] as looks bring host keys, and written back by [`NasBackend::refresh`].
    known_hosts: String,
    /// [`Inner::known_hosts`] holds a host key the file does not yet: the app thread writes it on the
    /// next [`NasBackend::refresh`], being a thread whose stack can reach the flash.
    known_hosts_dirty: bool,
    /// A pass of checks has been asked for and not yet taken: the machines to check, and the port to knock
    /// on. The list is handed over by the app thread rather than read here, because reading it is a flash
    /// read and the worker's stack is in PSRAM — see the module notes, and [`NasBackend::check`], which is
    /// where the handing over happens.
    checking: Option<Vec<(String, u16)>>,
    /// What the last pass found, per host: whether that machine's port answered. A host that is not in it
    /// has not been checked — which on a page is 检测中… and is not 离线.
    answering: Vec<(String, bool)>,
    /// The app has forgotten the counters the last look was measured against, having just pointed this
    /// board at a machine — or at one again.
    ///
    /// What the worker does with it is to forget its own clock too, and that is the whole of why the flag
    /// exists: [`Watched`] decides when the *next* look is due, and a look whose counters are gone is a
    /// first look — the one that gets the priming beat rather than the interval. Without this the board
    /// would take a first look (because the counters went) and then wait a whole interval to take the
    /// second (because the clock was still on the machine), which is the wait the priming exists to
    /// remove. See [`EspNas::select`] and [`PRIME`].
    forgotten: bool,
}

// ---------------------------------------------------------------------------------------------
// The terminal's session: a second connection, and a second thread to keep it.
// ---------------------------------------------------------------------------------------------

/// The state [`EspNas`] and its terminal worker share.
///
/// The same shape as [`Shared`] and for the same reason: the two sides are a UI thread that may never
/// block and a worker whose whole job is to, so everything they agree on is behind one mutex and one
/// condvar that says when there is something new to look at.
struct ShellShared {
    inner: Mutex<ShellInner>,
    /// Signalled whenever the app-side has something for the worker: a session to open, bytes to type,
    /// or a session to hang up.
    wake: Condvar,
}

#[derive(Default)]
struct ShellInner {
    /// A session to open, handed over by [`NasBackend::shell_open`] and taken by the worker.
    ///
    /// One slot rather than one per machine, because there is one terminal: a *second* open replaces
    /// whatever session is up — the trait says so — and a worker that finds this set while a session is
    /// running tears that one down and starts this one instead.
    opening: Option<ShellRequest>,
    /// Bytes typed and not yet on the wire. Queued rather than written by the caller because the
    /// caller is a UI thread that must not block, and because a byte typed while the login is still
    /// coming up has nowhere to go yet but is still in order.
    outgoing: Vec<u8>,
    /// What the session has said and nobody has taken yet, in the order it arrived.
    ///
    /// Bytes and not text: a multi-byte character can be split across two reads, and it is
    /// [`crate::nas`]'s screen that joins them — see `pomelo_apps::terminal::screen`.
    output: Vec<u8>,
    /// Where the session stands, as the worker last knew it.
    status: NasShellStatus,
    /// The app asked the session to be hung up. The worker closes the channel and stops; the status is
    /// already [`NasShellStatus::Closed`][pomelo_hal::types::NasShellStatus::Closed], because hanging up
    /// is this board's own act and not something it has to wait on the machine to confirm.
    closing: bool,
    /// The session was hung up by [`NasBackend::shell_close`] and not merely ended. It is the difference
    /// between a terminal somebody left and one whose shell exited: typing at the first is a mistake the
    /// backend reports, and at the second it is a key with nowhere to go.
    hung_up: bool,
    /// The worker should stop. Never set in practice; the thread lives as long as the board.
    stop: bool,
}

/// A session to open: which machine, how to log in, and the screen's size.
struct ShellRequest {
    /// The machine and its account, copied out of the devices file.
    credentials: NasCredentials,
    /// The screen's size in characters, as the terminal drawing it stated — a pseudo-terminal is how a
    /// program at the other end asks how wide its answers should be.
    columns: u16,
    rows: u16,
}

/// Why a session loop came back.
///
/// Two reasons and not one, because a session replaced by a *newer* open must not write a status: the
/// newer open has already set [`NasShellStatus::Opening`], and a worker finishing the old one writing
/// `Closed` over it would have the terminal draw "gone" for a session on its way up.
enum Ended {
    /// The session is over, and this is where it stands: a clean [`NasShellStatus::Closed`] or a
    /// [`NasShellStatus::Lost`] with the fault that ended it.
    Final(NasShellStatus),
    /// A newer [`NasBackend::shell_open`] arrived. The outer loop has it; nothing here is written.
    Replaced,
}

impl EspNas {
    /// The backend, pointed at whatever NAS was written down.
    ///
    /// Starts the worker thread and hands it the saved credentials, if there are any. Infallible, like
    /// every constructor here: a box that has been told which machine to watch should be watching it
    /// from boot rather than waiting for somebody to open a page. A machine that cannot be watched — a
    /// file that will not parse — is not a boot that fails; it is a reading that says so, which is what
    /// the panel draws.
    pub fn new() -> Self {
        // The known-hosts file, read now: this is a thread whose stack can reach the flash, and the
        // worker's cannot — see the module notes. An absent file is simply no keys yet.
        let mut inner = Inner::default();
        inner.known_hosts = read_known_hosts().unwrap_or_default();

        let shared = Arc::new(Shared {
            inner: Mutex::new(inner),
            wake: Condvar::new(),
        });

        let worker = Arc::clone(&shared);
        let borrowed = pthread::ask_for_psram_stack();
        let spawned = thread::Builder::new()
            .name(String::from("nas-ssh"))
            .stack_size(WORKER_STACK)
            .spawn(move || run_worker(worker));
        // Put the caller's thread back before looking at the result: whether the worker started or
        // not, the thread that asked for it is not this backend's to keep configured.
        borrowed.restore();

        if let Err(error) = spawned {
            // No worker means no looks ever happen; the panel says "connecting" and stays honest about
            // not knowing, rather than the board refusing to boot over a machine it cannot watch.
            eprintln!("[nas] the SSH worker could not be started: {error}");
        }

        let mut backend = Self {
            sampler: Mutex::new(Sampler::new()),
            last: Mutex::new(NasReading::default()),
            pointed_at: None,
            shared,
            // No terminal worker yet: it is spawned the first time somebody opens a session — see the
            // field's own note on why its 32 KB of PSRAM are not spent at boot.
            shell: None,
            shell_host: None,
        };

        // From boot the board watches the first machine in the list. Which one is *not* written down —
        // see the trait's note on `watching` — so this is a starting point the page re-makes the
        // moment somebody picks a row, rather than a setting.
        if let Some(credentials) = backend.devices().into_iter().next() {
            backend.select(&credentials);
        }

        backend
    }

    /// Points the worker at `credentials`, dropping whatever the last machine said.
    ///
    /// The half of [`NasBackend::remember`] that does not touch the file: coming up and picking a row
    /// are both "watch this machine", and neither is a reason to write down what is written down
    /// already.
    fn select(&mut self, credentials: &NasCredentials) {
        {
            let mut inner = self
                .shared
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.config = Some(credentials.clone());
            inner.requested = true;
            inner.pending.clear();
            inner.fault = FAULT_NONE;
            // A pass of checks queued for the list is dropped here: the list is not what anybody is looking
            // at now, and a socket per machine would be this thread's time before the session this page
            // wants. What the checks had answered stays where it is — the page that asked for them is one
            // tap away — and the list asks again when it comes back. See [`NasBackend::check`].
            inner.checking = None;
            // And the worker's clock goes with the counters this call is about to drop: see [`Inner`]'s
            // `forgotten`, which is where the whole of that argument is.
            inner.forgotten = true;
        }
        self.shared.wake.notify_all();

        // Different machine, different counters: the worker has already dropped whatever reading it
        // had, and this drops the arithmetic that went with it.
        if let Ok(mut sampler) = self.sampler.lock() {
            sampler.forget();
        }
        if let Ok(mut last) = self.last.lock() {
            *last = NasReading::default();
        }

        self.pointed_at = Some(credentials.clone());
    }

    /// Stops watching anything, leaving the list as it is.
    ///
    /// What a page ends on when the row it was looking at is dropped: the machine is gone from the
    /// list, and a reading from a machine this board no longer has is a reading from nowhere.
    fn unselect(&mut self) {
        {
            let mut inner = self
                .shared
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.config = None;
            inner.requested = false;
            inner.pending.clear();
            inner.fault = FAULT_NONE;
        }
        self.shared.wake.notify_all();

        if let Ok(mut sampler) = self.sampler.lock() {
            sampler.forget();
        }
        if let Ok(mut last) = self.last.lock() {
            *last = NasReading::default();
        }

        self.pointed_at = None;
    }

    /// Points the terminal at `credentials` and asks for a session of the given size.
    ///
    /// The worker is spawned on the first call and kept afterwards: a terminal opened, left and opened
    /// again is one thread doing two sessions rather than two threads, and the second session is not a
    /// login thread born behind a first one still winding down.
    fn open_shell(
        &mut self,
        credentials: &NasCredentials,
        columns: u16,
        rows: u16,
    ) -> Result<(), HalError> {
        let shell = match self.shell.as_ref() {
            Some(shell) => Arc::clone(shell),
            None => {
                let shell = Arc::new(ShellShared {
                    inner: Mutex::new(ShellInner::default()),
                    wake: Condvar::new(),
                });

                // The same PSRAM-stack dance as the look worker's, and for the same reason: the key
                // exchange runs on this thread and the internal heap has no room for its stack. See
                // [`pthread::ask_for_psram_stack`].
                let worker = Arc::clone(&shell);
                let known_hosts = Arc::clone(&self.shared);
                let borrowed = pthread::ask_for_psram_stack();
                let spawned = thread::Builder::new()
                    .name(String::from("nas-shell"))
                    .stack_size(WORKER_STACK)
                    .spawn(move || run_shell_worker(worker, known_hosts));
                borrowed.restore();

                if let Err(error) = spawned {
                    // Told at once rather than as a status, because there is no session to report on:
                    // the terminal draws an unreachable machine, which is the honest reading of a board
                    // whose image cannot hold one more thread.
                    eprintln!("[nas] the shell worker could not be started: {error}");
                    return Err(HalError::NotSupported);
                }

                shell
            }
        };
        // Kept for the next session: one thread, however many terminals are opened on it and left.
        self.shell = Some(Arc::clone(&shell));

        // Handed over and answered at once: whether the machine comes up is the *status*, a look later,
        // the same way [`NasBackend::refresh`] asks for a reading rather than waiting for one.
        let mut inner = shell
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.opening = Some(ShellRequest {
            credentials: credentials.clone(),
            columns,
            rows,
        });
        inner.hung_up = false;
        inner.closing = false;
        inner.output.clear();
        inner.outgoing.clear();
        inner.status = NasShellStatus::Opening;
        drop(inner);

        self.shell_host = Some(credentials.host.clone());

        shell.wake.notify_all();

        Ok(())
    }

    /// Hangs the terminal's session up, if there is one.
    ///
    /// The status is set here and not left for the worker, because hanging up is this board's own act:
    /// there is nothing to wait on a machine for, and a terminal that kept drawing a live prompt until
    /// the socket had finished closing would be drawing something that is not true.
    fn close_shell(&mut self) {
        // Said before the borrow below, and said even when there is no worker: a terminal with no
        // session is on no machine, which is the same `None` as one just hung up.
        self.shell_host = None;

        let Some(shell) = &self.shell else {
            return;
        };

        let mut inner = shell
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A session still coming up is called off with the one that is up: `opening` is taken so that a
        // worker about to start it does not, and the status says closed rather than opening.
        inner.opening = None;
        inner.outgoing.clear();
        inner.output.clear();
        inner.closing = true;
        inner.hung_up = true;
        inner.status = NasShellStatus::Closed;
        drop(inner);

        shell.wake.notify_all();
    }
}

impl Default for EspNas {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------
// The worker thread: wait, look, report.
// ---------------------------------------------------------------------------------------------

fn run_worker(shared: Arc<Shared>) {
    // The two packet buffers, allocated once and reused for every look. Sunset needs buffers big
    // enough for any SSH packet; taking them from the heap once keeps a 70 KB allocation from churning
    // — and fragmenting the heap — on every look, which on a board that runs for months is the
    // difference between a steady image and one that slowly runs out of memory.
    let mut inbuf = vec![0u8; SSH_MAX_PACKET];
    let mut outbuf = vec![0u8; SSH_MAX_PACKET];

    // The clock the worker keeps about the machine it is watching. See [`Watched`], which is where the
    // whole spacing rule lives: a machine it has not looked at yet, then that machine's pair of looks,
    // then the interval.
    let mut watched = Watched::default();

    // When the last pass of checks finished. See [`CHECK_INTERVAL`], which is what paces them.
    let mut last_check: Option<Instant> = None;

    loop {
        // Wait for a request, or for one poll interval to pass.
        {
            let guard = shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let (guard, _) = shared
                .wake
                .wait_timeout(guard, POLL_INTERVAL)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if guard.stop {
                return;
            }
        }

        let now = Instant::now();

        // The list's checks, if a page asked for them and the last pass is old enough. A pass is one socket
        // per machine rather than a session, so it does not carry a look's cost — but it is this thread's
        // time, and this is the thread a look needs, which is why a page asking on every tick gets a pass
        // every [`CHECK_INTERVAL`] rather than one per tick.
        {
            let asked = {
                let mut inner = shared
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());

                if inner.checking.is_none() || last_check.is_some_and(|at| now < at + CHECK_INTERVAL) {
                    None
                } else {
                    inner.checking.take()
                }
            };

            if let Some(hosts) = asked {
                let mut answers: Vec<(String, bool)> = Vec::new();
                let mut left: Vec<(String, u16)> = Vec::new();

                for (index, (host, port)) in hosts.iter().enumerate() {
                    // A look outranks a check: this is the thread a session runs on, and something asking
                    // for one is a page somebody is waiting on. The rest of the pass goes back for the next
                    // time round, and what has been answered so far is kept.
                    if wants_look(&shared) {
                        left = hosts[index..].to_vec();
                        break;
                    }

                    answers.push((host.clone(), probe(host, *port)));
                }

                let mut inner = shared
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());

                // Merged rather than replaced, so that what a cut-short pass did not get to keeps the
                // answer it had: a row that slipped back to 检测中… because a look went first would be a
                // row telling a lie about how much is known.
                for (host, answered) in answers {
                    match inner.answering.iter_mut().find(|(checked, _)| *checked == host) {
                        Some(entry) => entry.1 = answered,
                        None => inner.answering.push((host, answered)),
                    }
                }

                if !left.is_empty() {
                    inner.checking = Some(left);
                }

                last_check = Some(Instant::now());
            }
        }

        // A look is due if something asked for one, there is a machine to watch, and the clock since the
        // last look at *that* machine has run out. `twice` is whether this look is a machine's *first* one,
        // which is the one that asks the machine for two samples on one connection. See [`COMMAND_TWICE`].
        let (config, twice) = {
            let mut inner = shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

            let Some(config) = inner.config.clone() else {
                // Nothing watched: a look now would be a look at nothing. The clock goes with it, so
                // that the next machine configured is looked at at once.
                watched = Watched::default();
                continue;
            };

            // First, because the flag and the clock are both about to be cleared: a look with no counters
            // behind it needs two samples, and so does one at a machine the clock has never been on.
            let twice = inner.forgotten || watched.host.is_none();

            if inner.forgotten {
                // The counters went, and the clock that measured them goes with them: the next look is this
                // machine's *first* one, and the one after that is due on the priming beat. See [`Inner`]'s
                // `forgotten`.
                inner.forgotten = false;
                watched = Watched::default();
            }

            if !inner.requested || !watched.due(&config, now) {
                continue;
            }

            inner.requested = false;
            inner.busy = true;
            (config, twice)
        };

        let outcome = look(&config, &mut inbuf, &mut outbuf, &shared, twice);
        watched.looked(&config, Instant::now());

        let mut inner = shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.busy = false;
        match outcome {
            Ok(samples) => {
                inner.fault = FAULT_NONE;
                inner.pending.extend(samples);
            }
            // A failed look keeps no output: whatever the last good look said is still the least
            // wrong thing the panel can draw, and `reading` reports the fault beside it.
            Err(fault) => inner.fault = fault,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The terminal's worker thread: wait for a session, run it, report what became of it.
// ---------------------------------------------------------------------------------------------

/// Runs sessions, one after another, on the terminal's own connection.
///
/// One thread for the life of the board, and one session at a time on it: a terminal is one page with
/// one prompt on it, so a second session is somebody pointing it somewhere else rather than two. What
/// it does *not* do is block the look worker, which is the whole reason it exists — see
/// [`EspNas`]'s `shell` field.
fn run_shell_worker(shared: Arc<ShellShared>, known_hosts: Arc<Shared>) {
    // Two packet buffers, allocated once and reused, exactly as the look worker's: a session can run
    // for hours and the heap is not to be churned.
    let mut inbuf = vec![0u8; SSH_MAX_PACKET];
    let mut outbuf = vec![0u8; SSH_MAX_PACKET];

    loop {
        // Wait for a session to be asked for, or a poll interval to pass. The timeout is only so that
        // the loop comes round to look for its stop flag; nothing starts a session but a request.
        let opening = {
            let guard = shared
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (mut guard, _) = shared
                .wake
                .wait_timeout(guard, POLL_INTERVAL)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if guard.stop {
                return;
            }
            guard.opening.take()
        };

        let Some(request) = opening else {
            continue;
        };

        match run_shell_session(&request, &mut inbuf, &mut outbuf, &shared, &known_hosts) {
            // The session was replaced by a newer open, which has already set the status and cleared
            // the page: writing either from here would be the old session drawing on the new one's.
            Ended::Replaced => {}
            Ended::Final(status) => {
                let mut inner = shared
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                // Unless the app hung it up, in which case the status is already the one it asked for
                // and the worker's reading of a socket it just closed is not worth contradicting.
                if !inner.hung_up {
                    inner.status = status;
                }
            }
        }
    }
}

/// The pseudo-terminal to ask a session's machine for.
///
/// The size in *characters*, because that is what the protocol carries and what a program at the other
/// end lays its output out in; there is no pixel size to give on a character-cell terminal, and zero is
/// what the protocol means by "not known". No modes either: the defaults are the ones that make a shell
/// behave like a shell, and a mode this board cannot honour is a promise to the machine it would break.
fn terminal_of(columns: u16, rows: u16) -> Pty {
    Pty {
        term: SHELL_TERM.try_into().unwrap_or_default(),
        cols: u32::from(columns),
        rows: u32::from(rows),
        width: 0,
        height: 0,
        modes: Default::default(),
    }
}

/// One terminal session, from opening the socket to the last byte either way.
///
/// The shape of [`look`]'s loop with two differences that matter. There is no deadline: a look is a
/// bounded question and a terminal is meant to sit there for as long as somebody is typing at it. And
/// there is a second direction — what was typed goes out on the same channel the output arrives on, so
/// each pass both reads the socket and writes the queue.
///
/// Returns why it came back; the caller is what turns that into a status. It never touches
/// [`NasShellStatus::Opening`]: that is the app's, set when it asked, and set to
/// [`NasShellStatus::Ready`] here once the machine's shell is up at the other end.
fn run_shell_session(
    request: &ShellRequest,
    inbuf: &mut [u8],
    outbuf: &mut [u8],
    shell: &ShellShared,
    known_hosts: &Shared,
) -> Ended {
    if !has_network() {
        return Ended::Final(NasShellStatus::Lost(NasFault::NoNetwork));
    }

    let Some(address) = resolve(&request.credentials.host, request.credentials.port) else {
        return Ended::Final(NasShellStatus::Lost(NasFault::Unreachable));
    };

    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        // The machine that is off, or the address that is not there — the same fault a look gets, and
        // the one a terminal draws as a session it could not raise.
        return Ended::Final(NasShellStatus::Lost(NasFault::Unreachable));
    };
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));

    let mut runner = Runner::new_client(inbuf, outbuf);

    // The lock, taken the patient way: a panic elsewhere must not turn a live session into a dead
    // terminal, the same reasoning as the look worker's.
    macro_rules! inner {
        () => {
            shell
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
    }

    let mut channel: Option<ChanHandle> = None;
    let mut open_session = false;
    // Set when the *socket* has gone quiet (a zero-length read), which the protocol reports as a
    // channel EOF just as it does a shell exiting. It is what tells the two apart at the end.
    let mut transport_closed = false;

    // Typed bytes a full window refused, kept here rather than in the shared queue so that what was
    // typed first is what goes out first across passes.
    let mut unsent: Vec<u8> = Vec::new();

    let mut from_socket = [0u8; SOCKET_CHUNK];
    let mut channel_data = [0u8; SOCKET_CHUNK];
    let mut unread: Vec<u8> = Vec::new();

    loop {
        // Whatever the app has asked for since the last pass, before this thread blocks on a read.
        {
            let inner = inner!();
            if inner.opening.is_some() {
                return Ended::Replaced;
            }
            if inner.stop || inner.closing {
                return Ended::Final(NasShellStatus::Closed);
            }
        }

        // What was typed goes out *before* the blocking read below, so a line sent on Return is not
        // held behind a socket that is waiting for the machine to say something.
        if let Some(handle) = &channel {
            let typed = {
                let mut inner = inner!();
                let mut typed = std::mem::take(&mut unsent);
                typed.append(&mut inner.outgoing);
                typed
            };

            let mut sent = 0;
            while sent < typed.len() {
                match runner.write_channel(handle, ChanData::Normal, &typed[sent..]) {
                    Ok(0) => break,
                    Ok(length) => sent += length,
                    // The channel is gone. Read as a goodbye rather than a fault: the machine's shell
                    // has ended, and a terminal typing into a channel nobody reads is worse than one
                    // that says so.
                    Err(_) => return Ended::Final(NasShellStatus::Closed),
                }
            }
            unsent = typed[sent..].to_vec();
        }

        // Send whatever the protocol has queued. The same pair of calls as [`look`], for the reason
        // written there: [`Runner::output`] is broken in sunset 0.6.0 and this is the pair its own
        // examples use.
        while runner.is_output_pending() {
            let length = {
                let queued = runner.output_buf();
                if queued.is_empty() {
                    break;
                }
                if stream.write_all(queued).is_err() {
                    return Ended::Final(NasShellStatus::Lost(NasFault::Unreachable));
                }
                queued.len()
            };
            runner.consume_output(length);
        }

        // Feed the socket into the protocol, then hand back anything it would not take.
        if runner.is_input_ready() {
            let mut fed = 0;
            while fed < unread.len() && runner.is_input_ready() {
                match runner.input(&unread[fed..]) {
                    Ok(0) => break,
                    Ok(length) => fed += length,
                    Err(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
                }
            }
            unread.drain(..fed);

            if unread.is_empty() {
                match stream.read(&mut from_socket) {
                    Ok(0) => {
                        // The machine has stopped talking. The protocol will call this a channel EOF;
                        // `transport_closed` is what stops that being read as a shell exiting.
                        transport_closed = true;
                        runner.close_input();
                    }
                    Ok(length) => {
                        let mut taken = 0;
                        while taken < length && runner.is_input_ready() {
                            match runner.input(&from_socket[taken..length]) {
                                Ok(0) => break,
                                Ok(n) => taken += n,
                                Err(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
                            }
                        }
                        if taken < length {
                            unread.extend_from_slice(&from_socket[taken..length]);
                        }
                    }
                    // A quiet socket is not an idle one: the shell waits on a person, and the person
                    // waits on the shell. Both timeouts are the protocol's own keep-alive business.
                    Err(error) if is_would_block(&error) => {}
                    Err(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Unreachable)),
                }
            }
        } else {
            thread::sleep(Duration::from_millis(2));
        }

        // Handle everything the protocol has to say.
        loop {
            let event = match runner.progress() {
                Ok(event) => event,
                Err(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
            };

            match event {
                Event::None => break,
                Event::Progressed => {}
                Event::Serv(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
                Event::Cli(event) => match event {
                    CliEvent::Hostkey(hostkey) => {
                        match check_hostkey(&hostkey, &request.credentials, known_hosts) {
                            Ok(()) => {
                                if hostkey.accept().is_err() {
                                    return Ended::Final(NasShellStatus::Lost(NasFault::Protocol));
                                }
                            }
                            Err(fault) => {
                                let _ = hostkey.reject();
                                return Ended::Final(NasShellStatus::Lost(fault_of(fault)));
                            }
                        }
                    }
                    CliEvent::Username(request_) => {
                        if request_.username(&request.credentials.user).is_err() {
                            return Ended::Final(NasShellStatus::Lost(NasFault::Protocol));
                        }
                    }
                    CliEvent::Password(request_) => {
                        if request_.password(&request.credentials.password).is_err() {
                            return Ended::Final(NasShellStatus::Lost(NasFault::Authentication));
                        }
                    }
                    // No key, so nothing to offer and nothing to sign with: the account this board
                    // signs in as is a password one, and a server that insists still has `keyboard-interactive`
                    // to fall back on.
                    CliEvent::Pubkey(request_) => {
                        let _ = request_.skip();
                    }
                    CliEvent::AgentSign(request_) => {
                        let _ = request_.skip();
                    }
                    CliEvent::Banner(_) => {}
                    CliEvent::Authenticated => open_session = true,
                    CliEvent::SessionOpened(mut opener) => {
                        // A pseudo-terminal first, then a shell *on* it: the order the protocol
                        // requires, and the pair that says there is a person here — which is what makes
                        // the machine print a prompt and keep the connection, rather than run one
                        // command and hang up.
                        if opener.pty(terminal_of(request.columns, request.rows)).is_err() {
                            return Ended::Final(NasShellStatus::Lost(NasFault::Protocol));
                        }
                        if opener.shell().is_err() {
                            return Ended::Final(NasShellStatus::Lost(NasFault::Protocol));
                        }

                        // The session is up: the channel is confirmed and the shell asked for.
                        // Everything from here is the machine's prompt and whatever is typed at it.
                        let mut inner = inner!();
                        // Not if somebody has since pointed the terminal elsewhere — see [`Ended`].
                        if inner.opening.is_none() && !inner.closing {
                            inner.status = NasShellStatus::Ready;
                        }
                    }
                    // The shell exited with a status. The channel's own EOF follows and is where the
                    // session is ended, so there is nothing to do with the code here.
                    CliEvent::SessionExit(_) => {}
                    CliEvent::Defunct => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
                    CliEvent::PollAgain => {}
                },
            }
        }

        if open_session {
            open_session = false;
            match runner.open_client_session() {
                Ok(handle) => channel = Some(handle),
                Err(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
            }
        }

        // What the machine has said. `ChannelEOF` is the shell exiting, and that is a clean goodbye: a
        // person typing `exit`, or a shell whose job is done, is not a fault. A channel EOF with the
        // transport already closed is not that — the machine went away mid-session — and is a fault.
        if let Some(handle) = &channel {
            let mut said: Vec<u8> = Vec::new();
            let gone = loop {
                match runner.read_channel(handle, ChanData::Normal, &mut channel_data) {
                    Ok(0) => break false,
                    Ok(length) => said.extend_from_slice(&channel_data[..length]),
                    Err(sunset::Error::ChannelEOF) => break true,
                    Err(_) => return Ended::Final(NasShellStatus::Lost(NasFault::Protocol)),
                }
            };

            if !said.is_empty() {
                let mut inner = inner!();
                inner.output.extend_from_slice(&said);
            }

            if gone {
                return Ended::Final(if transport_closed {
                    NasShellStatus::Lost(NasFault::Unreachable)
                } else {
                    NasShellStatus::Closed
                });
            }
        }
    }
}

/// Whether something is waiting for a look: what a pass of checks asks between two sockets.
///
/// A socket is the cheap thing this thread does and a session is the expensive one, so a look is what a
/// check gives way to. Asked of the flag rather than of the machine: the only question is whether anybody
/// else wants the thread, and a check is the work it can put down. See [`NasBackend::check`].
fn wants_look(shared: &Shared) -> bool {
    shared
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .requested
}

/// One check: open the machine's SSH port and close it again.
///
/// Not a look — no handshake, no login, and nothing runs on the machine — which is the whole point of it.
/// What a *list* of machines is asked is whether each is there, and an SSH session per row would be a login
/// per row for a question a socket answers. It is also the question this board can act on: whether the port
/// it would open a session through is open.
///
/// Any address the name resolves to will do. A name that resolves to nothing is a machine this board cannot
/// reach, which is the answer and not an error — the caller is drawing a dot, not diagnosing a network.
fn probe(host: &str, port: u16) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };

    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok())
}

/// One look: connect, authenticate, run the command, read everything it printed.
///
/// `Err` is a `hal_nas_fault_t` code — what went wrong, in the vocabulary the panel speaks. `Ok` is the
/// samples this look is worth — one, or two when `twice` asked the command for a second round on the same
/// connection — each with the instant it belongs to. See [`COMMAND_TWICE`] and [`uptime_of`].
fn look(
    config: &NasCredentials,
    inbuf: &mut [u8],
    outbuf: &mut [u8],
    shared: &Shared,
    twice: bool,
) -> Result<Vec<(Instant, Vec<u8>)>, i32> {
    if !has_network() {
        return Err(FAULT_NO_NETWORK);
    }

    let address = resolve(&config.host, config.port).ok_or(FAULT_UNREACHABLE)?;
    let mut stream =
        TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|_| FAULT_UNREACHABLE)?;
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));

    let mut runner = Runner::new_client(inbuf, outbuf);

    let mut channel: Option<ChanHandle> = None;
    // Set when the server says authentication succeeded, acted on after the event loop has released
    // its borrow of the runner — `open_client_session` cannot be called while an event is alive.
    let mut open_session = false;
    let mut output = Vec::new();
    let deadline = Instant::now() + LOOK_DEADLINE;

    let mut from_socket = [0u8; SOCKET_CHUNK];
    let mut channel_data = [0u8; SOCKET_CHUNK];
    // Bytes read from the socket that the protocol would not take all at once.
    let mut unread: Vec<u8> = Vec::new();

    loop {
        if Instant::now() >= deadline {
            return Err(FAULT_UNREACHABLE);
        }

        // Send whatever the protocol has queued.
        //
        // The obvious call here, `Runner::output(buf)`, is broken in sunset 0.6.0 — it ends in
        // `buf.copy_from_slice(&out[..l])`, which `copy_from_slice` rejects unless the two are the same
        // length, so it panics on any write that is not exactly buffer-sized. That is nearly every
        // write, and it is how the first boot on hardware died: `source slice length (22) does not
        // match destination slice length (4096)`. The pair below is the one sunset's own examples use;
        // it asks the queue for its bytes instead of guessing a size, and needs no buffer of ours.
        while runner.is_output_pending() {
            let length = {
                let queued = runner.output_buf();
                if queued.is_empty() {
                    break;
                }
                stream.write_all(queued).map_err(|_| FAULT_UNREACHABLE)?;
                queued.len()
            };
            runner.consume_output(length);
        }

        // Feed the socket into the protocol.
        if runner.is_input_ready() {
            // Anything the protocol could not take last time, first.
            let mut fed = 0;
            while fed < unread.len() && runner.is_input_ready() {
                match runner.input(&unread[fed..]) {
                    Ok(0) => break,
                    Ok(n) => fed += n,
                    Err(_) => return Err(FAULT_PROTOCOL),
                }
            }
            unread.drain(..fed);

            if unread.is_empty() {
                match stream.read(&mut from_socket) {
                    Ok(0) => runner.close_input(),
                    Ok(length) => {
                        let mut taken = 0;
                        while taken < length && runner.is_input_ready() {
                            match runner.input(&from_socket[taken..length]) {
                                Ok(0) => break,
                                Ok(n) => taken += n,
                                Err(_) => return Err(FAULT_PROTOCOL),
                            }
                        }
                        if taken < length {
                            unread.extend_from_slice(&from_socket[taken..length]);
                        }
                    }
                    Err(error) if is_would_block(&error) => {}
                    Err(_) => return Err(FAULT_UNREACHABLE),
                }
            }
        } else {
            // Waiting for the output queue to drain before accepting packets. Let it.
            thread::sleep(Duration::from_millis(2));
        }

        // Handle everything the protocol has to say.
        loop {
            let event = runner.progress().map_err(|_| FAULT_PROTOCOL)?;

            match event {
                Event::None => break,
                Event::Progressed => {}
                // A client runner never produces a server event.
                Event::Serv(_) => return Err(FAULT_PROTOCOL),
                Event::Cli(event) => match event {
                    CliEvent::Hostkey(hostkey) => match check_hostkey(&hostkey, config, shared) {
                        Ok(()) => hostkey.accept().map_err(|_| FAULT_PROTOCOL)?,
                        Err(fault) => {
                            let _ = hostkey.reject();
                            return Err(fault);
                        }
                    },
                    CliEvent::Username(request) => {
                        request
                            .username(&config.user)
                            .map_err(|_| FAULT_PROTOCOL)?;
                    }
                    CliEvent::Password(request) => {
                        request
                            .password(&config.password)
                            .map_err(|_| FAULT_AUTHENTICATION)?;
                    }
                    // Password is the only method this board offers; the others are declined so the
                    // server does not wait for a key this board will never have.
                    CliEvent::Pubkey(request) => {
                        let _ = request.skip();
                    }
                    CliEvent::AgentSign(request) => {
                        let _ = request.skip();
                    }
                    CliEvent::Banner(_) => {}
                    CliEvent::Authenticated => open_session = true,
                    CliEvent::SessionOpened(mut opener) => {
                        // The command — or the command twice, for a machine's first look. One connection,
                        // one session: the second round costs a `sleep` and a `cat`, not a login.
                        let command = if twice { COMMAND_TWICE } else { COMMAND };

                        opener.exec(command).map_err(|_| FAULT_PROTOCOL)?;
                    }
                    CliEvent::SessionExit(_) => {}
                    CliEvent::Defunct => return Err(FAULT_PROTOCOL),
                    CliEvent::PollAgain => {}
                },
            }
        }

        if open_session {
            open_session = false;
            channel = Some(
                runner
                    .open_client_session()
                    .map_err(|_| FAULT_PROTOCOL)?,
            );
        }

        // Collect the command's output.
        let mut finished = false;
        if let Some(handle) = &channel {
            loop {
                match runner.read_channel(handle, ChanData::Normal, &mut channel_data) {
                    Ok(0) => break,
                    Ok(length) => output.extend_from_slice(&channel_data[..length]),
                    // The end of the command's output: everything it printed has been read.
                    Err(sunset::Error::ChannelEOF) => {
                        finished = true;
                        break;
                    }
                    Err(_) => return Err(FAULT_PROTOCOL),
                }
            }
        }

        if finished {
            break;
        }
    }

    if let Some(handle) = channel {
        let _ = runner.channel_done(handle);
    }

    // What the command printed, as samples: one round, or the two a first look asked for, split where the
    // second begins. Bytes that are not text are a look that produced nothing this board can read — the next
    // look may well be fine — which is why this is an empty answer rather than a fault.
    let Ok(text) = String::from_utf8(output) else {
        return Ok(Vec::new());
    };

    let now = Instant::now();

    if twice {
        if let Some((first, second)) = split_pair(&text) {
            // The *gap* between the two samples is the machine's own, read out of the two rounds'
            // `/proc/uptime`: on a LAN both rounds arrive in one burst, so when the bytes got here says
            // nothing about when the counters were read, and a rate divided by no time is not a rate. See
            // [`uptime_of`].
            let between = match (uptime_of(first), uptime_of(second)) {
                (Some(first), Some(second)) if second > first => {
                    Duration::from_secs_f64(second - first)
                }
                // Something the machine did not answer with. The command was told to leave [`PRIME`], and a
                // rate over a second is nearer the truth than one over no time at all.
                _ => PRIME,
            };

            return Ok(vec![
                (now - between, first.as_bytes().to_vec()),
                (now, second.as_bytes().to_vec()),
            ]);
        }

        // Not two rounds: a `cat` that is not there, a shell that would not run the command twice. One
        // sample is still a look, and the page draws its rates on the next one.
    }

    Ok(vec![(now, text.into_bytes())])
}

/// The machine's own uptime, in seconds, as the first line of a round reports it: `/proc/uptime`'s first
/// field.
///
/// What a rate is divided by. The board's own clock cannot answer that question for a doubled look — both
/// rounds arrive in the same burst — and the machine's clock can, because that is the clock its counters are
/// kept in. See [`round_uptime`], which is the parsing of it.
fn uptime_of(round: &str) -> Option<f64> {
    round_uptime(round).map(|uptime| uptime.as_secs_f64())
}

/// Whether this board has a network to reach a NAS through.
///
/// "No network" and "nothing answered" are different sentences on the panel, so they are different
/// faults, and this is the line between them.
fn has_network() -> bool {
    let mut status = ffi::HalWifiStatus {
        state: 0,
        ssid: [0; ffi::SSID_MAX_LEN],
        ip: [0; ffi::IP_MAX_LEN],
        netmask: [0; ffi::IP_MAX_LEN],
        gateway: [0; ffi::IP_MAX_LEN],
        rssi: 0,
    };

    let result = unsafe { ffi::hal_wifi_get_status(&mut status) };

    result == 0 && status.state == WIFI_STATE_CONNECTED && status.ip[0] != 0
}

/// Resolves the host, and takes the first address it comes to.
fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    (host, port).to_socket_addrs().ok()?.next()
}

/// Whether a read error just means "nothing yet".
fn is_would_block(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

// ---------------------------------------------------------------------------------------------
// Trust on first use.
// ---------------------------------------------------------------------------------------------

/// A sink that appends SSH wire bytes to a `Vec`.
struct VecSink<'a>(&'a mut Vec<u8>);

impl SSHSink for VecSink<'_> {
    fn push(&mut self, bytes: &[u8]) -> WireResult<()> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

/// Decides whether the server's host key is the one this board expects, and records it the first time.
///
/// Returns `Err(FAULT_HOST_KEY)` for a key that is not the remembered one, and `Ok(())` for a key that
/// is — or for the first one ever seen, which is written down on the way past.
fn check_hostkey(
    hostkey: &CheckHostkey,
    config: &NasCredentials,
    shared: &Shared,
) -> Result<(), i32> {
    let key = hostkey.hostkey().map_err(|_| FAULT_PROTOCOL)?;

    let mut blob = Vec::new();
    key.enc(&mut VecSink(&mut blob)).map_err(|_| FAULT_PROTOCOL)?;

    // Decided from memory, because this runs on the worker: its stack is in PSRAM, and a flash read is
    // as forbidden there as a write — see the module notes. The file is mirrored in [`Inner`], and a
    // key trusted here is written out by the app thread.
    let mut inner = shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    match known_host_in(&inner.known_hosts, &config.host) {
        Some(known) if known == blob => Ok(()),
        Some(_) => Err(FAULT_HOST_KEY),
        None => {
            // The first look at this machine. Trust it and write it down, the way `ssh` does — the
            // write itself is the app thread's, which is why the mirror is marked dirty rather than
            // the file touched here.
            inner.known_hosts = with_known_host(&inner.known_hosts, &config.host, &blob);
            inner.known_hosts_dirty = true;
            Ok(())
        }
    }
}

/// The `known_hosts` file's path, beside the credentials file.
fn known_hosts_path() -> Result<CString, HalError> {
    let path = nas_credentials::path(BOARD_APP_DATA).with_file_name(KNOWN_HOSTS_FILE);
    CString::new(path.to_string_lossy().as_bytes()).map_err(|_| HalError::InvalidArg)
}

/// Reads the whole file back as text, if it is there.
fn read_known_hosts() -> Option<String> {
    let path = known_hosts_path().ok()?;
    let mut buffer = vec![0u8; FILE_CAPACITY];
    let mut length = 0usize;

    let result = unsafe {
        ffi::hal_storage_read_file(
            path.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut length,
        )
    };

    if result != 0 || length == 0 {
        return None;
    }

    buffer.truncate(length);
    String::from_utf8(buffer).ok()
}

/// The key remembered for `host` in a `known_hosts` text, if one is.
///
/// Pure, and reads no file: the caller holds the mirror in [`Inner`], because this is only ever asked
/// on the worker, whose PSRAM stack cannot reach the flash.
fn known_host_in(text: &str, host: &str) -> Option<Vec<u8>> {
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() == Some(host) {
            return fields.next().and_then(decode_hex);
        }
    }

    None
}

/// A `known_hosts` text with `blob` as `host`'s key, replacing whatever was there.
///
/// Pure, like [`known_host_in`]: the file is written later, by the app thread, and never here.
fn with_known_host(text: &str, host: &str, blob: &[u8]) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| line.split_whitespace().next() != Some(host))
        .map(str::to_string)
        .collect();

    lines.push(format!("{host} {}", encode_hex(blob)));
    lines.join("\n")
}

/// Writes a `known_hosts` text to the flash.
///
/// Only ever called from a thread whose stack is *not* in PSRAM — the app thread, in
/// [`NasBackend::refresh`]. Called from the worker it would assert: see the module notes.
fn write_known_hosts(text: &str) {
    let Ok(path) = known_hosts_path() else {
        return;
    };

    unsafe { ffi::hal_storage_write_file(path.as_ptr(), text.as_ptr().cast(), text.len()) };
}

/// Lowercase hex, so the key survives a text file with no binary in it.
fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }

    text
}

/// The inverse of [`encode_hex`], or `None` if the text is not hex of an even length.
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();

    if text.len() % 2 != 0 {
        return None;
    }

    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The credentials file, read and written through the C side's storage.
// ---------------------------------------------------------------------------------------------

/// The credentials file, as a C string.
fn credentials_path() -> Result<CString, HalError> {
    let path = nas_credentials::path(BOARD_APP_DATA);

    CString::new(path.to_string_lossy().as_bytes()).map_err(|_| HalError::InvalidArg)
}

/// Reads the devices written down, in the order the file names them.
///
/// Empty for a board that has never been told about a machine — and also for a file that is *there*
/// and will not parse. A page draws a list, and a file somebody has broken has to land somewhere;
/// what it must not do is take the machines that *are* readable down with it, which is why the
/// parser's error stops here rather than being handed to a caller that would have to invent a list
/// around it.
fn read_devices() -> Vec<NasCredentials> {
    let Some(path) = credentials_path().ok() else {
        return Vec::new();
    };

    let mut buffer = vec![0u8; FILE_CAPACITY];
    let mut length = 0usize;

    let result = unsafe {
        ffi::hal_storage_read_file(
            path.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut length,
        )
    };

    if result != 0 || length == 0 {
        return Vec::new();
    }

    buffer.truncate(length);

    String::from_utf8(buffer)
        .ok()
        .and_then(|text| NasCredentials::parse_devices(&text).ok())
        .unwrap_or_default()
}

/// Writes the devices down, beside the Wi-Fi credentials.
///
/// No devices is *no file*, rather than a file with nothing in it: that is the state
/// [`NasBackend::forget`] leaves behind and the one a board nobody has pointed at anything is in —
/// see `NasCredentials::save_all`, which is this same thing on a desk.
fn write_devices(devices: &[NasCredentials]) -> Result<(), HalError> {
    let path = credentials_path()?;

    if devices.is_empty() {
        // Whatever was there goes, and whether there was anything there is not a failure.
        unsafe { ffi::hal_storage_remove_file(path.as_ptr()) };

        return Ok(());
    }

    let text = NasCredentials::file_of_devices(devices).map_err(|_| HalError::InvalidArg)?;

    let result =
        unsafe { ffi::hal_storage_write_file(path.as_ptr(), text.as_ptr().cast(), text.len()) };

    HalError::from_code(result)
}

/// A fault code, in the vocabulary the app speaks.
fn fault_of(code: i32) -> NasFault {
    match code {
        FAULT_NO_NETWORK => NasFault::NoNetwork,
        FAULT_UNREACHABLE => NasFault::Unreachable,
        FAULT_AUTHENTICATION => NasFault::Authentication,
        FAULT_HOST_KEY => NasFault::HostKeyChanged,
        // Includes the "could not check the host key" case, which is also a conversation that could
        // not be had.
        _ => NasFault::Protocol,
    }
}

impl NasBackend for EspNas {
    fn devices(&self) -> Vec<NasCredentials> {
        read_devices()
    }

    fn watching(&self) -> Option<NasCredentials> {
        // The machine being *looked at*: a choice made on the page rather than a fact in the file —
        // [`NasBackend::devices`] is what was written down, and this is which of them the worker is
        // on. See the trait's note on why it is not the file's business which one that is.
        self.pointed_at.clone()
    }

    fn remember(&mut self, credentials: &NasCredentials) -> Result<(), HalError> {
        // One entry per host: saving an address twice corrects the machine rather than adding a second
        // one. `parse_devices` de-duplicates on the way back in for the same reason — the web page's
        // form appends to the file rather than rewriting it.
        let mut devices = read_devices();
        match devices.iter_mut().find(|held| held.host == credentials.host) {
            Some(held) => *held = credentials.clone(),
            None => devices.push(credentials.clone()),
        }
        write_devices(&devices)?;

        self.select(credentials);

        Ok(())
    }

    fn point_at(&mut self, host: &str) -> Result<(), HalError> {
        // 只是换一眼看哪一台：清单不动，所以读一次就够，写下去不是这一下的事。
        let Some(credentials) = read_devices()
            .into_iter()
            .find(|device| device.host == host)
        else {
            return Err(HalError::InvalidArg);
        };

        self.select(&credentials);

        Ok(())
    }

    fn forget(&mut self, host: &str) -> Result<(), HalError> {
        let mut devices = read_devices();
        devices.retain(|device| device.host != host);
        write_devices(&devices)?;

        // Dropping the machine being watched leaves this watching nothing — and when the list ran out
        // the write above left *no* file at all, which is what a board nobody has pointed at anything
        // looks like.
        if self
            .pointed_at
            .as_ref()
            .is_some_and(|device| device.host == host)
        {
            self.unselect();
        }

        // And the terminal, if it was on that machine: a session logged into a machine the list no
        // longer has is one nobody can find again. The look and the session are otherwise kept apart —
        // see the trait's note on why they are two connections — so re-pointing the look at another
        // machine leaves the session alone, and only *dropping* the machine it is on ends it.
        if self.shell_host.as_deref() == Some(host) {
            self.close_shell();
        }

        Ok(())
    }

    fn shell_open(&mut self, host: &str, columns: u16, rows: u16) -> Result<(), HalError> {
        // A session is opened on a machine this board has been *told* about, the way a look is: a host
        // in no file is a mistake in the caller rather than a machine that is down. Read here, on the
        // app's thread, which is the one whose stack can reach the flash — the worker's is in PSRAM.
        let Some(credentials) = read_devices()
            .into_iter()
            .find(|device| device.host == host)
        else {
            return Err(HalError::InvalidArg);
        };

        self.open_shell(&credentials, columns, rows)
    }

    fn shell_write(&mut self, bytes: &[u8]) -> Result<(), HalError> {
        let Some(shell) = &self.shell else {
            // No session has ever been opened on this board, so there is nothing to type at.
            return Err(HalError::NotInitialized);
        };

        let mut inner = shell
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Typing at a session somebody hung up is a mistake in the caller, and reported as one — the
        // same rule the sim keeps. A *shell* that exited on its own is the other case: the keystroke
        // has nowhere to go, but nothing about it is the caller's fault.
        if inner.hung_up {
            return Err(HalError::NotInitialized);
        }

        inner.outgoing.extend_from_slice(bytes);
        drop(inner);

        shell.wake.notify_all();

        Ok(())
    }

    fn shell_take(&mut self) -> NasShell {
        let Some(shell) = &self.shell else {
            return NasShell::default();
        };

        let mut inner = shell
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        NasShell {
            status: inner.status,
            // Taken rather than copied: the terminal keeps its own screen and this is only what has
            // arrived since the last time it looked.
            output: std::mem::take(&mut inner.output),
        }
    }

    fn shell_close(&mut self) -> Result<(), HalError> {
        // Closing a session that is not there is somebody leaving a page they had already left: it
        // undoes [`close_shell`][EspNas::close_shell]'s own work rather than failing at it.
        self.close_shell();
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), HalError> {
        // The worker may have trusted a host key it could not write, its stack being in PSRAM. Write it
        // here, on this thread, which can reach the flash — the same side that writes the credentials.
        let pending_hosts = {
            let mut inner = self.shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if inner.known_hosts_dirty {
                inner.known_hosts_dirty = false;
                Some(inner.known_hosts.clone())
            } else {
                None
            }
        };
        if let Some(text) = pending_hosts {
            write_known_hosts(&text);
        }

        // First, has the machine being watched changed under this board's feet? The page writes that
        // file and this backend is the side that can act on it — see the module notes. Only the
        // watched one is compared: another device gaining a password is not this session's business,
        // and a look is not disturbed by finding the entry it found last time, which is the usual
        // answer.
        if let Some(watched) = self.pointed_at.clone() {
            let saved = read_devices()
                .into_iter()
                .find(|device| device.host == watched.host);

            if saved.as_ref() != Some(&watched) {
                match saved {
                    Some(credentials) => {
                        self.select(&credentials);
                        eprintln!("[nas] the credentials changed; the session was re-pointed");
                    }
                    None => {
                        self.unselect();
                        eprintln!("[nas] the machine being watched is no longer in the list");
                    }
                }
            }
        }

        // Ask the worker for a look, if there is anything to watch. The answer arrives when it
        // arrives: this function never waits for it.
        {
            let mut inner = self.shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if inner.config.is_some() {
                inner.requested = true;
            }
        }
        self.shared.wake.notify_all();

        // Take everything the looks have produced, oldest first. A machine's *first* look hands over two
        // samples of one connection — see `COMMAND_TWICE` — and each carries the instant it belongs to,
        // because the right to say "per second" comes from the gap between two samples and not from when
        // somebody got round to reading them. Nothing to take is a look still in flight, or one that
        // failed: a failure is reported by [`NasBackend::reading`]'s status rather than by throwing the last
        // good reading away.
        let pending: Vec<(Instant, Vec<u8>)> = std::mem::take(
            &mut self
                .shared
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pending,
        );

        for (at, output) in pending {
            let Ok(text) = String::from_utf8(output) else {
                // Not a reply this board can read. The status stays as it was, and the next look may well
                // be fine — a session that dropped mid-line is the usual reason.
                continue;
            };

            let Ok(mut sampler) = self.sampler.lock() else {
                continue;
            };
            let reading = sampler.reading(&text, at);

            if let Ok(mut last) = self.last.lock() {
                *last = reading;
            }
        }

        Ok(())
    }

    fn reading(&self) -> NasReading {
        let mut reading = self
            .last
            .lock()
            .map(|last| last.clone())
            .unwrap_or_default();

        // Worked out here rather than cached, because it is about *now*: nothing configured, a fault,
        // or a reading.
        let inner = self.shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        reading.status = if inner.config.is_none() {
            NasStatus::Unconfigured
        } else if inner.fault != FAULT_NONE {
            NasStatus::Offline(fault_of(inner.fault))
        } else if reading.at.is_none() {
            NasStatus::Connecting
        } else {
            NasStatus::Online
        };

        reading
    }

    fn check(&mut self) -> Result<(), HalError> {
        // One pass at a time. The worker clears this when it takes it, and a caller on a timer that asked
        // again while a pass was in flight would have the list — the flash — read once a second for a pass
        // that is already coming.
        if self
            .shared
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .checking
            .is_some()
        {
            return Ok(());
        }

        // Read here and not by the worker, because this is a thread whose stack can reach the flash. See
        // the module notes, and `read_devices` — the same file the page's list is drawn from.
        let hosts: Vec<(String, u16)> = read_devices()
            .into_iter()
            .map(|device| (device.host, device.port))
            .collect();

        {
            let mut inner = self
                .shared
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.checking = Some(hosts);
        }
        self.shared.wake.notify_all();

        Ok(())
    }

    fn answering(&self, host: &str) -> Option<bool> {
        let inner = self
            .shared
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        inner
            .answering
            .iter()
            .find(|(checked, _)| checked == host)
            .map(|(_, answered)| *answered)
    }
}
