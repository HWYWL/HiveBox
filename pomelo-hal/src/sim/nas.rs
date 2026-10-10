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
    credentials: Option<NasCredentials>,
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
            credentials: None,
            answer: Answer::Data,
            looks: 0,
            reading: NasReading::default(),
        }
    }

    /// A NAS that refuses, in the way `fault` says — a machine that is off, a password that is wrong,
    /// a key that changed. Every failure the page has to be able to draw, without unplugging anything.
    pub fn failing(fault: NasFault) -> Self {
        Self {
            answer: Answer::Fail(fault),
            ..Self::new()
        }
    }

    /// The reading the simulator would answer with, over the files that describe it: a system SSD and
    /// two data drives, the shape of the box this app was written for.
    fn answering(&self) -> NasReading {
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
                self.credentials
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
    fn watching(&self) -> Option<NasCredentials> {
        self.credentials.clone()
    }

    fn remember(&mut self, credentials: &NasCredentials) -> Result<(), HalError> {
        // Kept and not written anywhere: the desktop has no flash to be remembered in, and what the
        // device writes is its own backend's business — see the trait's note on who touches the file.
        self.credentials = Some(credentials.clone());
        self.looks = 0;

        // Whatever the old machine said is not the new machine's: a reading from somewhere else drawn
        // under this machine's name is worse than no reading.
        self.reading = NasReading::default();
        self.reading.status = NasStatus::Connecting;

        Ok(())
    }

    fn forget(&mut self) -> Result<(), HalError> {
        self.credentials = None;
        self.looks = 0;
        self.reading = NasReading::default();

        Ok(())
    }

    fn refresh(&mut self) -> Result<(), HalError> {
        // Nothing to look at is a setting, not a failure.
        if self.credentials.is_none() {
            return Ok(());
        }

        self.looks += 1;

        match self.answer {
            // The numbers already gathered stay where they are, with their instant, so that the page
            // keeps drawing them and `age` keeps growing. That is what the device's backend has to do
            // too: a NAS that went to sleep did not become a NAS with no disks.
            Answer::Fail(fault) => self.reading.status = NasStatus::Offline(fault),
            Answer::Data => self.reading = self.answering(),
        }

        Ok(())
    }

    fn reading(&self) -> NasReading {
        self.reading.clone()
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

    /// An answer carries the whole machine: its uptime, its CPU and memory, its network, its disks,
    /// and the host key somebody has to decide about.
    #[test]
    fn an_answer_carries_the_machine() {
        let mut nas = watching();
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
        half.refresh().unwrap();
        half.answer = Answer::Fail(NasFault::Unreachable);
        half.refresh().unwrap();

        let reading = half.reading();
        assert_eq!(reading.status, NasStatus::Offline(NasFault::Unreachable));
        assert_eq!(reading.disks.len(), 3, "the last good reading stays on it");
        assert!(reading.cpu.is_some());
    }

    /// Being told to watch nothing forgets the machine that was there.
    #[test]
    fn watching_nothing_forgets_the_machine() {
        let mut nas = watching();
        nas.refresh().unwrap();

        nas.forget().unwrap();

        assert!(nas.watching().is_none());
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
}
