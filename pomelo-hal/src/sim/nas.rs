//! A NAS that is not there, answering like one that is.

use std::time::Duration;

use crate::error::HalError;
use crate::nas_credentials::NasCredentials;
use crate::traits::NasBackend;
use crate::types::{
    MemoryInfo, NasCpu, NasDisk, NasFault, NasNetwork, NasReading, NasShell, NasShellStatus,
    NasStatus,
};

use super::nas_shell::SimShell;

/// What a look at the simulator's NAS does when it is asked for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Answer with a fresh reading.
    Data,
    /// Fail, in the way this fault says.
    Fail(NasFault),
}

/// The desktop's NAS: a machine that never existed, with numbers that look like one's.
///
/// It exists so the app can be built, drawn and tested with no NAS, no network and no SSH client —
/// the same reason every backend here has a simulator. What it models beyond the numbers is the three
/// things the app has to get right and cannot check against a machine on a desk: a look that *fails*
/// ([`SimNas::failing`]), a look that has not happened yet — [`NasBackend::reading`] answers
/// [`NasStatus::Connecting`] between the `watch` and the first answer, exactly as the device's does —
/// and a first look that carries a host key for somebody to accept.
pub struct SimNas {
    /// Every machine this desktop board has been told about, in the order it was told about them.
    devices: Vec<NasCredentials>,
    /// The one being looked at, or `None`. A copy rather than an index: the list can be edited under
    /// it, and what is being watched is a *machine* and not a row — see [`SimNas::watch`].
    watching: Option<NasCredentials>,
    /// Whether the machines in the list are there. What a *check* asks, as against [`Answer`], which is
    /// what a look does: a box that is switched off fails both, and a box whose password was changed
    /// fails only the look. See [`SimNas::unreachable`].
    reachable: bool,
    answer: Answer,
    /// How many looks have arrived, so that the numbers move. A window left open on a simulated NAS
    /// should show a machine doing something rather than a photograph.
    looks: u64,
    reading: NasReading,
    /// The terminal's session, if somebody has opened one. Beside the reading rather than inside it:
    /// the two are different connections to the same machine on purpose — see the trait's note on why —
    /// so that a page watching the numbers keeps watching them while somebody types.
    shell: Option<SimSession>,
}

/// The simulated machine's interactive session: a terminal's other end.
///
/// A wrapper and not the shell itself, because the shell is the machine's and the *status* is the
/// backend's to report — and because a session that never logged in has no shell to report at all.
/// Which is what [`NasShellStatus::Lost`] is here for: there is no wire on this side to spend a second
/// on a login, so this backend is never in [`NasShellStatus::Opening`] by the time anybody looks, and a
/// machine that is off goes straight to the state it would have reached a second later.
struct SimSession {
    /// Where it stands.
    status: NasShellStatus,
    /// The shell at the other end, once there is one. `None` for a session that never logged in — a
    /// machine that is switched off, which fails at the login rather than after it.
    shell: Option<SimShell>,
}

/// The host key the simulated server offers. Obviously not a real one, and that is the point: a
/// plausible-looking blob of base64 in a file is one somebody might mistake for a credential.
const HOST_KEY: &str =
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBpomeloSimulated";

const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const GB: u64 = 1024 * MB;

/// A byte rate, rounded to whole bytes. The numbers below are rates per second and are written as
/// floats so that the wobble can be a fraction of one; this is where the fraction stops.
fn per_sec(rate: f32) -> u64 {
    rate.max(0.0).round() as u64
}

impl SimNas {
    /// A NAS that answers.
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            watching: None,
            reachable: true,
            answer: Answer::Data,
            looks: 0,
            reading: NasReading::default(),
            shell: None,
        }
    }

    /// A NAS that is not there at all: switched off, or unplugged from the network.
    ///
    /// Both of its answers are failures — the look cannot connect and the list's check cannot reach the
    /// port — which is the pair a machine that is off produces. A machine that is up with a changed
    /// password is the *other* combination, and is [`SimNas::failing`].
    pub fn unreachable() -> Self {
        Self {
            reachable: false,
            answer: Answer::Fail(NasFault::Unreachable),
            ..Self::new()
        }
    }

    /// Looks at `device`, or at nothing, dropping whatever the last one said.
    ///
    /// Whatever the old machine said is not the new machine's: a reading from somewhere else drawn
    /// under this machine's name is worse than no reading. Watching nothing is not a failure either —
    /// it is the board's own state instead, which is [`NasStatus::Unconfigured`].
    fn watch(&mut self, device: Option<NasCredentials>) {
        self.watching = device;
        self.looks = 0;
        self.reading = NasReading::default();

        // And the terminal's session with it, for the reason the reading goes: a shell logged into a
        // machine that this board is no longer watching is a shell nobody asked for.
        self.shell = None;

        self.reading.status = if self.watching.is_some() {
            NasStatus::Connecting
        } else {
            NasStatus::Unconfigured
        };
    }

    /// A NAS that refuses, in the way `fault` says — a machine that is off, a password that is wrong,
    /// a key that changed. Every failure the page has to be able to draw, without unplugging anything.
    pub fn failing(fault: NasFault) -> Self {
        Self {
            answer: Answer::Fail(fault),
            ..Self::new()
        }
    }

    /// What a machine's *first* look comes back with: everything that is a fact about it — its uptime,
    /// its memory, its disks and what they are for — and nothing that is a difference between two looks.
    ///
    /// Which is what the device's backend gets from `pomelo_hal::nas_sample::Sampler`, said here in the
    /// simulator's own words: `/proc` reports totals since boot, so a rate is a subtraction and one look
    /// cannot make one. Without this the desktop would draw a CPU and a network the board cannot have
    /// yet, and the dashes the page has to be able to draw would be a state nobody had ever seen.
    ///
    /// The app asks again a second later for exactly this reason — see its `PRIME` — which is the same
    /// second the device's worker waits before its second look.
    fn first_look(&self) -> NasReading {
        NasReading {
            cpu: None,
            network: None,
            // The disks are already facts, and how busy each has been is not: it is the same
            // subtraction, and a first look is the one that has nothing to subtract from.
            disks: self
                .fresh()
                .disks
                .into_iter()
                .map(|disk| NasDisk {
                    busy_percent: 0.0,
                    read_per_sec: 0,
                    write_per_sec: 0,
                    ..disk
                })
                .collect(),
            ..self.fresh()
        }
    }

    /// A fresh reading, over the files that describe the simulated machine: a system SSD and two data
    /// drives, the shape of the box this app was written for.
    ///
    /// `fresh` and not `answering`, which is what [`NasBackend::answering`] answers the *list's* checks
    /// with: the two are one word apart and mean quite different things, and an inherent method of that
    /// name would shadow the trait's.
    fn fresh(&self) -> NasReading {
        // The wobble: a few values that move with each look, so a page watched for a minute is a page
        // that changes. Deliberately small and deliberately not random — a test that reads a number
        // twice gets the same one.
        let step = (self.looks % 8) as f32;
        let busy = 2.0 + step * 1.3;

        NasReading {
            status: NasStatus::Online,
            at: Some(std::time::Instant::now()),
            uptime: Some(Duration::from_secs(17 * 86_400 + 4 * 3_600 + 21 * 60)),
            cpu: Some(NasCpu {
                usage_percent: 11.0 + step * 2.5,
                temperature_c: Some(41.0 + step * 0.5),
            }),
            // 16 GB with about a third of it handed out: a NAS that is doing its job and not
            // shuffling anything.
            memory: Some(MemoryInfo {
                total_bytes: 16 * GB,
                free_bytes: ((16 * GB) as f32 * (0.66 - step * 0.005)) as u64,
            }),
            network: Some(NasNetwork {
                sent_per_sec: per_sec(MB as f32 + step * 180.0 * KB as f32),
                received_per_sec: per_sec(8.0 * MB as f32 + step * 320.0 * KB as f32),
            }),
            disks: vec![
                NasDisk {
                    device: String::from("sda"),
                    model: String::from("Samsung SSD 870 EVO 500GB"),
                    size_bytes: Some(500 * GB),
                    // The SSD is the one drive here with no temperature to read the usual way, which
                    // is the ordinary case on a real box and worth having in the simulator.
                    temperature_c: None,
                    busy_percent: busy,
                    read_per_sec: per_sec(0.5 * MB as f32 + step * 64.0 * KB as f32),
                    write_per_sec: per_sec(MB as f32 + step * 256.0 * KB as f32),
                    purpose: String::from("系统安装"),
                },
                NasDisk {
                    device: String::from("sdb"),
                    model: String::from("WDC WD40EFRX-68N32N0"),
                    size_bytes: Some(4_000 * GB),
                    temperature_c: Some(37.0 + step * 0.4),
                    busy_percent: busy * 3.0,
                    read_per_sec: per_sec(8.0 * MB as f32 + step * 512.0 * KB as f32),
                    write_per_sec: per_sec(step * 64.0 * KB as f32),
                    purpose: String::from("存储空间 1"),
                },
                NasDisk {
                    device: String::from("sdc"),
                    model: String::from("WDC WD40EFRX-68N32N0"),
                    size_bytes: Some(4_000 * GB),
                    temperature_c: Some(36.0 + step * 0.4),
                    busy_percent: busy * 2.0,
                    read_per_sec: per_sec(4.0 * MB as f32 + step * 256.0 * KB as f32),
                    write_per_sec: per_sec(step * 32.0 * KB as f32),
                    purpose: String::from("存储空间 2"),
                },
            ],
            // What the server offers, unless the file already remembers a key — in which case this is
            // the key that was agreed on, which is what a real server's would have to be for a reading
            // to arrive at all.
            host_key: Some(
                self.watching
                    .as_ref()
                    .and_then(|credentials| credentials.host_key.clone())
                    .unwrap_or_else(|| String::from(HOST_KEY)),
            ),
        }
    }
}

impl Default for SimNas {
    fn default() -> Self {
        Self::new()
    }
}

impl NasBackend for SimNas {
    fn devices(&self) -> Vec<NasCredentials> {
        self.devices.clone()
    }

    fn watching(&self) -> Option<NasCredentials> {
        self.watching.clone()
    }

    fn remember(&mut self, credentials: &NasCredentials) -> Result<(), HalError> {
        // Kept and not written anywhere: the desktop has no flash to be remembered in, and what the
        // device writes is its own backend's business — see the trait's note on who touches the file.
        // By host, like the device's: saving an address twice corrects the device rather than adding a
        // second one.
        match self
            .devices
            .iter_mut()
            .find(|held| held.host == credentials.host)
        {
            Some(held) => *held = credentials.clone(),
            None => self.devices.push(credentials.clone()),
        }

        self.watch(Some(credentials.clone()));

        Ok(())
    }

    fn point_at(&mut self, host: &str) -> Result<(), HalError> {
        let Some(device) = self
            .devices
            .iter()
            .find(|device| device.host == host)
            .cloned()
        else {
            return Err(HalError::InvalidArg);
        };

        self.watch(Some(device));

        Ok(())
    }

    fn forget(&mut self, host: &str) -> Result<(), HalError> {
        self.devices.retain(|device| device.host != host);

        // Dropping the machine being watched leaves this watching nothing, and the next reading says
        // so — rather than leaving a reading from a machine the list no longer has.
        if self
            .watching
            .as_ref()
            .is_some_and(|device| device.host == host)
        {
            self.watch(None);
        }

        Ok(())
    }

    fn refresh(&mut self) -> Result<(), HalError> {
        // Nothing to look at is a setting, not a failure.
        if self.watching.is_none() {
            return Ok(());
        }

        self.looks += 1;

        match self.answer {
            // The numbers already gathered stay where they are, with their instant, so that the page
            // keeps drawing them and `age` keeps growing. That is what the device's backend has to do
            // too: a NAS that went to sleep did not become a NAS with no disks.
            Answer::Fail(fault) => self.reading.status = NasStatus::Offline(fault),
            Answer::Data => {
                self.reading = if self.looks <= 1 {
                    self.first_look()
                } else {
                    self.fresh()
                };
            }
        }

        Ok(())
    }

    fn reading(&self) -> NasReading {
        self.reading.clone()
    }

    fn check(&mut self) -> Result<(), HalError> {
        // Nothing to ask for: a simulated socket opens at once, so the answer is there before the
        // question — see [`SimNas::answering`]. A backend with a real port to open is the device's, and
        // what it does about it is in the trait's note.
        Ok(())
    }

    fn answering(&self, host: &str) -> Option<bool> {
        // A machine this simulator has been told about is one it has an answer about, and a machine it
        // has never heard of is one it has nothing to say about — which is not the same as "not there".
        self.devices
            .iter()
            .any(|device| device.host == host)
            .then_some(self.reachable)
    }

    fn shell_open(&mut self, host: &str, _columns: u16, _rows: u16) -> Result<(), HalError> {
        // A session is opened on a machine this board has been *told* about, the way a look is: a host
        // in no file is not a machine that is down, it is a mistake in the caller, and the two want
        // different screens.
        let Some(credentials) = self
            .devices
            .iter()
            .find(|device| device.host == host)
            .cloned()
        else {
            return Err(HalError::InvalidArg);
        };

        // A machine that is off fails at the *login* rather than at the asking, which is where a real
        // one fails it too: opening the session is the request, and whether it comes up is the status.
        self.shell = Some(SimSession {
            status: if self.reachable {
                NasShellStatus::Opening
            } else {
                NasShellStatus::Lost(NasFault::Unreachable)
            },
            shell: self
                .reachable
                .then(|| SimShell::new(&credentials.user, &credentials.host)),
        });

        Ok(())
    }

    fn shell_write(&mut self, bytes: &[u8]) -> Result<(), HalError> {
        let Some(session) = self.shell.as_mut() else {
            return Err(HalError::NotInitialized);
        };

        // Whatever is typed goes to the shell, even while the login is still being reported as going
        // on: a session being opened is a shell reading its own input from the moment it is up, and the
        // bytes were typed in order. Which is why there is nothing to queue here — the shell exists
        // already, and what waits is the report.
        if let Some(shell) = session.shell.as_mut() {
            shell.write(bytes);
        }

        Ok(())
    }

    fn shell_take(&mut self) -> NasShell {
        let Some(session) = self.shell.as_mut() else {
            return NasShell::default();
        };

        // A simulated login takes no time at all: there is no wire here to take it over, so the look
        // *after* the ask is the one that finds the shell up. `Opening` is therefore a state this
        // backend passes through rather than one a caller sees — the device's is the one that spends a
        // second in it, and a terminal drawing "logging in" is drawing that one.
        if session.status == NasShellStatus::Opening {
            session.status = NasShellStatus::Ready;
        }

        let Some(shell) = session.shell.as_mut() else {
            return NasShell {
                status: session.status,
                output: Vec::new(),
            };
        };

        let output = shell.take();
        let goodbye = shell.ended();

        // A shell that said goodbye is a session that is over, and over *cleanly*: `exit` is not a
        // fault, and the trait asks for a session somebody ended not to be drawn as one that went
        // wrong.
        if goodbye {
            session.shell = None;
            session.status = NasShellStatus::Closed;
        }

        NasShell {
            status: session.status,
            output,
        }
    }

    fn shell_close(&mut self) -> Result<(), HalError> {
        // Nothing open is nothing to do: closing a session that is not there is somebody leaving a page
        // they had already left, which is not an error in anything.
        self.shell = None;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials(password: &str) -> NasCredentials {
        NasCredentials {
            host: String::from("192.168.1.10"),
            port: 22,
            user: String::from("nasstat"),
            password: password.to_string(),
            interval_secs: 5,
            host_key: None,
        }
    }

    fn watching() -> SimNas {
        let mut nas = SimNas::new();
        nas.remember(&credentials("hunter2")).unwrap();

        nas
    }

    /// A board watching no NAS has nothing to report and nothing wrong.
    #[test]
    fn nothing_watched_is_not_a_failure() {
        let nas = SimNas::new();

        assert_eq!(nas.reading().status, NasStatus::Unconfigured);
        assert!(nas.reading().disks.is_empty());
    }

    /// 点一行就换一眼：换到清单里那台，看的是它；换到清单里没有的那台，什么也不换。
    ///
    /// 后一种是过时的一行，不是一台机器 —— 页面该接着画原来那台，而不是画一片空白。
    #[test]
    fn pointing_at_a_listed_machine_moves_the_look() {
        let mut nas = watching();

        let mut second = credentials("hunter2");
        second.host = String::from("192.168.1.11");
        nas.remember(&second).unwrap();

        assert_eq!(
            nas.watching().map(|held| held.host),
            Some(String::from("192.168.1.11")),
            "remembering a machine already points at it"
        );

        nas.point_at("192.168.1.10").unwrap();

        assert_eq!(nas.watching().map(|held| held.host), Some(String::from("192.168.1.10")));
        assert_eq!(
            nas.reading().status,
            NasStatus::Connecting,
            "a machine just picked has not answered yet"
        );

        assert_eq!(nas.point_at("192.168.1.99"), Err(HalError::InvalidArg));
        assert_eq!(
            nas.watching().map(|held| held.host),
            Some(String::from("192.168.1.10")),
            "and the look stays where it was"
        );
    }

    /// Between the watch and the first answer the state is *connecting*, which is the state a page has
    /// to draw differently from both "fine" and "broken".
    #[test]
    fn a_watched_nas_is_connecting_until_it_answers() {
        let mut nas = watching();

        assert_eq!(nas.reading().status, NasStatus::Connecting);
        assert!(nas.reading().age().is_none(), "nothing has arrived yet");

        nas.refresh().unwrap();

        assert_eq!(nas.reading().status, NasStatus::Online);
        assert!(nas.reading().age().is_some());
    }

    /// A machine's first look carries the facts and none of the rates, and the second carries both.
    ///
    /// The shape the page's dashes hang on: without it a board's first reading would be drawn with a CPU
    /// and a network in it that the board cannot have. See [`SimNas::first_look`].
    #[test]
    fn a_first_look_has_no_rates_in_it() {
        let mut nas = watching();

        nas.refresh().unwrap();

        let first = nas.reading();

        assert_eq!(first.status, NasStatus::Online);
        assert!(first.cpu.is_none(), "one look cannot know how busy the CPU is");
        assert!(first.network.is_none());
        assert_eq!(
            first.uptime.map(|up| up.as_secs()),
            Some(17 * 86_400 + 4 * 3_600 + 21 * 60),
            "and the uptime is a fact about the machine, not a difference"
        );
        assert!(first.memory.is_some());
        assert_eq!(first.disks.len(), 3, "the disks are facts too");
        assert_eq!(first.disks[0].purpose, "系统安装");
        assert_eq!(first.disks[0].read_per_sec, 0, "with no speed yet");

        nas.refresh().unwrap();

        let second = nas.reading();

        assert!(second.cpu.is_some(), "the second look is what makes a rate");
        assert!(second.network.is_some());
        assert!(second.disks[0].busy_percent > 0.0);
    }

    /// An answer carries the whole machine: its uptime, its CPU and memory, its network, its disks,
    /// and the host key somebody has to decide about.
    #[test]
    fn an_answer_carries_the_machine() {
        let mut nas = watching();

        // Twice, because a machine's first look has no rates in it — see [`SimNas::first_look`] — and
        // this test is about what a reading carries once there is one.
        nas.refresh().unwrap();
        nas.refresh().unwrap();

        let reading = nas.reading();

        assert!(reading.uptime.unwrap() > Duration::from_secs(17 * 86_400));
        assert!(reading.cpu.unwrap().usage_percent > 0.0);
        assert!(reading.cpu.unwrap().temperature_c.is_some());
        let memory = reading.memory.unwrap();
        assert!(
            (25.0..50.0).contains(&memory.used_percent()),
            "a machine that is up and not busy: {memory:?}"
        );
        assert!(reading.network.unwrap().received_per_sec > reading.network.unwrap().sent_per_sec);

        assert_eq!(reading.disks.len(), 3);
        assert_eq!(reading.disks[0].purpose, "系统安装");
        assert!(reading.disks[1].size_bytes.is_some());
        assert!(
            reading.disks[0].temperature_c.is_none(),
            "and one drive the account cannot read a temperature for is a drive the page still lists"
        );

        assert!(
            reading.host_key.as_deref().unwrap().starts_with("ecdsa-"),
            "the first look offers a key to accept"
        );
    }

    /// A failed look says why, and keeps the last numbers rather than emptying the page.
    #[test]
    fn a_failed_look_keeps_the_numbers_it_had() {
        let mut nas = watching();
        nas.refresh().unwrap();

        let answered = nas.reading();
        assert!(!answered.disks.is_empty());

        let mut refusing = SimNas::failing(NasFault::Authentication);
        refusing.remember(&credentials("wrong")).unwrap();

        refusing.refresh().unwrap();

        let reading = refusing.reading();
        assert_eq!(reading.status, NasStatus::Offline(NasFault::Authentication));
        assert!(
            reading.disks.is_empty(),
            "and a NAS that has never answered has nothing to keep"
        );

        // The same fault on a NAS that *had* answered keeps what it had.
        let mut half = watching();
        half.refresh().unwrap(); // the first look: disks, and no rates yet
        half.refresh().unwrap(); // the second: the rates with them
        half.answer = Answer::Fail(NasFault::Unreachable);
        half.refresh().unwrap();

        let reading = half.reading();
        assert_eq!(reading.status, NasStatus::Offline(NasFault::Unreachable));
        assert_eq!(reading.disks.len(), 3, "the last good reading stays on it");
        assert!(reading.cpu.is_some());
    }

    /// A check says whether a machine is there, about the machines this board has been told about and
    /// about no others — and a machine that is off fails it, which is what the list draws in red.
    #[test]
    fn a_check_says_whether_a_machine_is_there() {
        let mut nas = watching();

        assert_eq!(nas.answering("192.168.1.10"), Some(true));
        assert_eq!(
            nas.answering("10.0.0.1"),
            None,
            "a machine it has never been told about is not a machine that is down"
        );
        assert!(nas.check().is_ok(), "and asking for one is never an error");

        // A machine that is switched off: the check fails *and* the look does, which is the pair the
        // list has to be able to tell from a machine that is up with something wrong with its login.
        let mut off = SimNas::unreachable();
        off.remember(&credentials("hunter2")).unwrap();

        assert_eq!(off.answering("192.168.1.10"), Some(false));

        off.refresh().unwrap();

        assert_eq!(
            off.reading().status,
            NasStatus::Offline(NasFault::Unreachable)
        );
    }

    /// Dropping the machine being watched forgets it, and leaves nothing being watched.
    #[test]
    fn watching_nothing_forgets_the_machine() {
        let mut nas = watching();
        nas.refresh().unwrap();

        nas.forget("192.168.1.10").unwrap();

        assert!(nas.watching().is_none());
        assert!(nas.devices().is_empty());
        assert_eq!(nas.reading().status, NasStatus::Unconfigured);
        assert!(nas.reading().disks.is_empty());

        // And a look at nothing is not an error.
        assert!(nas.refresh().is_ok());
        assert_eq!(nas.reading().status, NasStatus::Unconfigured);
    }

    /// Being pointed at a *different* machine drops the old one's numbers rather than drawing them
    /// under the new machine's name.
    #[test]
    fn another_machine_starts_from_nothing() {
        let mut nas = watching();
        nas.refresh().unwrap();
        assert_eq!(nas.reading().disks.len(), 3);

        nas.remember(&NasCredentials {
            host: String::from("192.168.1.11"),
            ..credentials("hunter2")
        })
        .unwrap();

        assert_eq!(nas.watching().unwrap().host, "192.168.1.11");
        assert_eq!(nas.reading().status, NasStatus::Connecting);
        assert!(
            nas.reading().disks.is_empty(),
            "and the previous machine's disks went with it"
        );
    }

    /// The list holds one entry per host however many times it is saved, the one being watched is the
    /// one last saved, and dropping it leaves the rest of the list alone.
    #[test]
    fn the_device_list_holds_one_entry_per_host() {
        let mut nas = SimNas::new();

        nas.remember(&credentials("hunter2")).unwrap();
        nas.remember(&NasCredentials {
            host: String::from("192.168.1.11"),
            ..credentials("hunter2")
        })
        .unwrap();

        assert_eq!(nas.devices().len(), 2);
        assert_eq!(
            nas.watching().unwrap().host,
            "192.168.1.11",
            "and the one last saved is the one being watched"
        );

        // Saving the first address again corrects it rather than adding a third entry.
        nas.remember(&NasCredentials {
            password: String::from("corrected"),
            ..credentials("hunter2")
        })
        .unwrap();

        assert_eq!(nas.devices().len(), 2);
        assert_eq!(nas.devices()[0].password, "corrected");
        assert_eq!(nas.devices()[0].host, "192.168.1.10", "in the place it was");
        assert_eq!(nas.watching().unwrap().host, "192.168.1.10");

        // Dropping the one being watched leaves the board watching nothing, with the other still on it.
        nas.forget("192.168.1.10").unwrap();

        assert_eq!(nas.devices().len(), 1);
        assert_eq!(nas.devices()[0].host, "192.168.1.11");
        assert!(nas.watching().is_none());
        assert_eq!(nas.reading().status, NasStatus::Unconfigured);
    }

    /// A board watching a NAS, with a terminal logged into it and the welcome already read.
    fn logged_in() -> SimNas {
        let mut nas = watching();
        nas.shell_open("192.168.1.10", 60, 20).unwrap();
        nas.shell_take();

        nas
    }

    /// What the session has said since the last call, as text — what a screen would be drawing.
    fn said(nas: &mut SimNas) -> String {
        String::from_utf8_lossy(&nas.shell_take().output).into_owned()
    }

    /// A session is opened on a machine this board has been told about, and a host it has never heard of
    /// is a mistake in the caller rather than a machine that is down.
    #[test]
    fn a_session_is_opened_on_a_machine_the_board_knows() {
        let mut nas = watching();

        assert_eq!(
            nas.shell_open("10.0.0.9", 60, 20),
            Err(HalError::InvalidArg),
            "a host in no file is not a host that is off"
        );
        assert_eq!(nas.shell_take().status, NasShellStatus::Closed);

        nas.shell_open("192.168.1.10", 60, 20).unwrap();

        let first = nas.shell_take();
        assert_eq!(first.status, NasShellStatus::Ready);
        assert!(
            String::from_utf8_lossy(&first.output).contains("$ "),
            "and the shell has already drawn its prompt: {first:?}"
        );
    }

    /// The reason a session exists rather than a command per keystroke: what it was told stays told.
    #[test]
    fn a_session_stays_where_it_was_left() {
        let mut nas = logged_in();

        nas.shell_write(b"cd /volume2\r").unwrap();
        assert!(said(&mut nas).contains("/volume2$"));

        nas.shell_write(b"pwd\r").unwrap();
        assert!(said(&mut nas).contains("/volume2"));
    }

    /// A machine that is off fails the *login*, which is what the terminal has to draw — and not the
    /// asking, which is answered at once.
    #[test]
    fn a_machine_that_is_off_loses_its_session() {
        let mut nas = SimNas::unreachable();
        nas.remember(&credentials("hunter2")).unwrap();

        nas.shell_open("192.168.1.10", 60, 20).unwrap();

        assert_eq!(
            nas.shell_take().status,
            NasShellStatus::Lost(NasFault::Unreachable)
        );
    }

    /// A shell that was asked to leave ends the session, and *cleanly*: `exit` is not a fault, and the
    /// terminal must not draw its own way out as one.
    #[test]
    fn a_shell_that_hangs_up_ends_the_session() {
        let mut nas = logged_in();

        nas.shell_write(b"exit\r").unwrap();

        let goodbye = nas.shell_take();
        assert!(String::from_utf8_lossy(&goodbye.output).contains("logout"));
        assert_eq!(goodbye.status, NasShellStatus::Closed);
        assert_eq!(
            nas.shell_take().status,
            NasShellStatus::Closed,
            "and it stays ended rather than coming back"
        );
    }

    /// Closing a session is not a fault, and closing one that is not there is not an error either.
    #[test]
    fn closing_a_session_is_not_a_failure() {
        let mut nas = logged_in();

        nas.shell_close().unwrap();

        assert_eq!(nas.shell_take().status, NasShellStatus::Closed);
        assert_eq!(
            nas.shell_write(b"pwd\r"),
            Err(HalError::NotInitialized),
            "and there is nothing left to type at"
        );
        assert!(nas.shell_close().is_ok(), "and closing it again is fine");
    }

    /// A session goes when the machine being watched does, for the reason the reading does.
    #[test]
    fn a_session_goes_when_the_machine_does() {
        let mut nas = logged_in();

        nas.forget("192.168.1.10").unwrap();

        assert_eq!(nas.shell_take().status, NasShellStatus::Closed);
    }

    /// The two connections are separate on purpose: a terminal is open on a machine and the page
    /// watching its numbers keeps watching them, in both directions.
    #[test]
    fn a_terminal_and_a_look_do_not_get_in_each_others_way() {
        let mut nas = logged_in();

        nas.shell_write(b"cd /volume2\r").unwrap();
        nas.refresh().unwrap();
        nas.refresh().unwrap();

        assert_eq!(nas.reading().status, NasStatus::Online);
        assert_eq!(nas.reading().disks.len(), 3, "the numbers carried on arriving");

        nas.shell_write(b"pwd\r").unwrap();
        assert!(
            said(&mut nas).contains("/volume2"),
            "and the shell is still where it was"
        );
    }
}
