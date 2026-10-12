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
use pomelo_hal::nas_sample::{COMMAND, Sampler};
use pomelo_hal::traits::NasBackend;
use pomelo_hal::types::{NasFault, NasReading, NasStatus};

use sunset::event::{CheckHostkey, CliEvent, Event};
use sunset::sshwire::{SSHEncode, SSHSink, WireResult};
use sunset::{ChanData, ChanHandle, Runner};

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
    /// The text of the last completed look, waiting for [`NasBackend::refresh`] to take it.
    pending: Option<Vec<u8>>,
    /// The `known_hosts` file's contents, mirrored here because the worker cannot reach the flash —
    /// its stack is in PSRAM. Read once by [`EspNas::new`], looked up and extended by
    /// [`check_hostkey`] as looks bring host keys, and written back by [`NasBackend::refresh`].
    known_hosts: String,
    /// [`Inner::known_hosts`] holds a host key the file does not yet: the app thread writes it on the
    /// next [`NasBackend::refresh`], being a thread whose stack can reach the flash.
    known_hosts_dirty: bool,
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
            inner.pending = None;
            inner.fault = FAULT_NONE;
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
            inner.pending = None;
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

    // When the last look finished, so that looks are spaced by the interval rather than by how often a
    // caller asks. `None` until the first look, which is therefore immediate.
    let mut last_look: Option<Instant> = None;

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

        // A look is due if something asked for one, there is a machine to watch, and the interval
        // since the last look has passed.
        let config = {
            let mut inner = shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

            let Some(config) = inner.config.clone() else {
                // Nothing watched: a look now would be a look at nothing. Forgetting clears the clock
                // so that the next machine configured is looked at at once.
                last_look = None;
                continue;
            };

            let due = last_look.is_none_or(|at| now >= at + config.interval());
            if !inner.requested || !due {
                continue;
            }

            inner.requested = false;
            inner.busy = true;
            config
        };

        let outcome = look(&config, &mut inbuf, &mut outbuf, &shared);
        last_look = Some(Instant::now());

        let mut inner = shared.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.busy = false;
        match outcome {
            Ok(text) => {
                inner.fault = FAULT_NONE;
                inner.pending = Some(text);
            }
            // A failed look keeps no output: whatever the last good look said is still the least
            // wrong thing the panel can draw, and `reading` reports the fault beside it.
            Err(fault) => inner.fault = fault,
        }
    }
}

/// One look: connect, authenticate, run the command, read everything it printed.
///
/// `Err` is a `hal_nas_fault_t` code — what went wrong, in the vocabulary the panel speaks.
fn look(
    config: &NasCredentials,
    inbuf: &mut [u8],
    outbuf: &mut [u8],
    shared: &Shared,
) -> Result<Vec<u8>, i32> {
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
                        opener.exec(COMMAND).map_err(|_| FAULT_PROTOCOL)?;
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

    Ok(output)
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

        // Take whatever the last completed look produced.
        let pending = {
            self.shared
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pending
                .take()
        };
        let Some(output) = pending else {
            // Nothing new: a look still in flight, or one that failed — and a failure is reported by
            // [`NasBackend::reading`]'s status rather than by throwing the last good reading away.
            return Ok(());
        };

        let Ok(text) = String::from_utf8(output) else {
            // Not a reply this board can read. The status stays as it was, and the next look may well
            // be fine — a session that dropped mid-line is the usual reason.
            return Ok(());
        };

        let reading = match self.sampler.lock() {
            Ok(mut sampler) => sampler.reading(&text, Instant::now()),
            Err(_) => return Ok(()),
        };

        if let Ok(mut last) = self.last.lock() {
            *last = reading;
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
}
