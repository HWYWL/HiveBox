//! A NAS that is not there, answering like one that is.

use std::time::Duration;

use crate::error::HalError;
use crate::nas_credentials::NasCredentials;
use crate::traits::NasBackend;
use crate::types::{MemoryInfo, NasCpu, NasDisk, NasFault, NasNetwork, NasReading, NasStatus};

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
}
