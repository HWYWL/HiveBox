//! One look at the NAS: the command, the text it comes back with, and the reading that text means.
//!
//! # Why the parsing is here and not in the C
//!
//! The board reaches the NAS over SSH, and the session is C (`board_nas.c`, libssh2 over mbedTLS)
//! because that is where a session belongs. What it hands back is **text** — the output of one command
//! — and the text is parsed *here*, in Rust, for one reason: this is the half that goes wrong quietly.
//! A kernel that moves a column, a `lsblk` whose tree is drawn differently, a disk that appears between
//! two looks: every one of those is a parse that starts answering nonsense rather than an error, and
//! every one of them can be tested on a desktop against a sample. No branch in the C depends on the
//! shape of `/proc`.
//!
//! # One command, one round trip
//!
//! The machine is asked *once* per look (see [`COMMAND`]), because what is expensive about a look over
//! SSH is not the reading — it is the round trip, and six commands would pay for six. The command prints
//! a marker before each file's contents and [`Facts::parse`] reads the sections by their markers: a
//! section that is missing is a reading that is missing, not a parse that fails.
//!
//! # Rates need two looks
//!
//! Half of a dashboard is a *rate* — the network, the disks, how busy the CPU is — and `/proc` reports
//! totals since boot. So a look is kept for the next one ([`Sampler`]), and the first look after a
//! restart has no rates in it: the disks and their capacities are already facts, and the speeds are
//! dashes, which is an honest picture of what one look knows.
//!
//! # The NAS's own words
//!
//! One field comes from the machine's *arrangement* rather than its numbers: which pool a disk belongs
//! to is a fact about how somebody set the box up. The rule for reading it off the output is fnOS's and
//! it lives in [`purpose_of`], where its source is written down — this is the only place on the page
//! that knows anything about the software at the other end.

use std::time::{Duration, Instant};

use crate::types::{MemoryInfo, NasCpu, NasDisk, NasNetwork, NasReading, NasStatus};

/// The one command a look is made of.
///
/// A shell one-liner in the order the sections are read, printing `=== <name>` before each so that the
/// parse does not depend on the files being the length they were on the machine this was written
/// against. `/proc` for the counters, `hwmon` for the one temperature an ordinary login can read, and
/// `lsblk` for the disks themselves — which is also what tells the table which `/proc/diskstats` lines
/// are whole disks and which are partitions of one.
///
/// `lsblk -b -P` and not the default: `-b` for sizes in bytes, so that nothing here has to know what `G`
/// meant to whichever version of `util-linux` printed it, and `-P` for `KEY="value"` pairs — because a
/// drive's model is `Samsung SSD 870 EVO 500GB`, and a parse that reads its columns by position works
/// until the first drive with three words in its name. See [`lsblk`].
pub const COMMAND: &str = "for f in uptime stat meminfo net/dev diskstats; do echo \"=== $f\"; cat /proc/$f; \
done; echo \"=== hwmon\"; for h in /sys/class/hwmon/hwmon*; do echo \"-- $(cat $h/name 2>/dev/null)\"; \
cat $h/temp1_input 2>/dev/null; done; echo \"=== lsblk\"; \
lsblk -b -P -o NAME,SIZE,TYPE,MODEL,MOUNTPOINT 2>/dev/null";

/// The CPU's time counters, as `/proc/stat` keeps them since boot.
///
/// Two numbers and not ten, because only two are ever used: `idle` is what "not busy" means — idle plus
/// the time spent waiting on I/O, which is the convention every `top` follows — and `total` is
/// everything, so that the share between two looks is one subtraction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Bytes in and out since boot, as `/proc/net/dev` keeps them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub received: u64,
    pub sent: u64,
}

/// One disk's I/O since boot, as `/proc/diskstats` keeps it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Io {
    pub read: u64,
    pub written: u64,
    /// Milliseconds this disk spent with I/O in flight — what `iostat` turns into `%util`.
    pub busy_ms: u64,
}

/// A block device, as `lsblk` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub name: String,
    pub size: Option<u64>,
    pub model: String,
    /// Where it is mounted, taken from the *partition* that is mounted on it: `lsblk` puts the mount
    /// point on the child, and the child is what a person means by "the disk's mount point".
    pub mount: Option<String>,
}

/// Everything one look came back with, before any of it is turned into a rate.
///
/// The absolute numbers, and deliberately not a [`NasReading`]: a reading says how fast something is
/// going, and one look cannot know that. See [`Sampler`], which is the thing that has seen two.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub uptime: Option<Duration>,
    pub cpu: Option<CpuTimes>,
    pub memory: Option<MemoryInfo>,
    pub network: Option<Counters>,
    /// The CPU's temperature, in °C, when the machine let the account read one.
    pub cpu_temperature: Option<f32>,
    /// One entry per whole disk, keyed by the kernel's name for it.
    pub io: Vec<(String, Io)>,
    /// The disks, in the order `lsblk` listed them.
    pub blocks: Vec<Block>,
}

impl Facts {
    /// Reads one look's text.
    ///
    /// Total: every section is optional and every line inside one is read defensively, so a machine that
    /// answers with half of what was asked for — or with something that is not the shape it was — yields
    /// a [`Facts`] with holes in it rather than an error. A look that *failed* is the caller's business
    /// (the session did not come up, the command did not run) and says so in its own words; a look that
    /// arrived and made no sense is this function's, and the most it can say about it is "that number is
    /// not here".
    pub fn parse(text: &str) -> Self {
        let mut facts = Facts::default();
        let mut io = Vec::new();
        let mut blocks = Vec::new();

        for (name, body) in sections(text) {
            match name {
                "uptime" => facts.uptime = uptime(body),
                "stat" => facts.cpu = cpu_times(body),
                "meminfo" => facts.memory = memory(body),
                "net/dev" => facts.network = network(body),
                "diskstats" => io = diskstats(body),
                "hwmon" => facts.cpu_temperature = temperature(body),
                "lsblk" => blocks = lsblk(body),
                // A section from a later version of the command is a section this version does not
                // know — the same rule the credentials file follows.
                _ => {}
            }
        }

        // The join that makes `/proc/diskstats` usable. It lists partitions beside their disks, so a
        // disk's throughput would otherwise be counted twice, and `lsblk` is the authority on which of
        // its lines are whole disks: see [`lsblk`].
        let whole = |name: &str| blocks.iter().any(|block| block.name == name);

        facts.io = io.into_iter().filter(|(name, _)| whole(name)).collect();
        facts.blocks = blocks
            .into_iter()
            .filter(|block| !is_virtual(&block.name))
            .collect();

        facts
    }
}

/// A look at the NAS, and the arithmetic between it and the one before it.
///
/// Holds the previous look's counters and when it arrived. One per NAS watched: what is in it only means
/// something against the machine it came from, which is why [`Sampler::forget`] exists and why the
/// backend calls it when it is pointed somewhere else.
#[derive(Debug, Default)]
pub struct Sampler {
    previous: Option<Taken>,
}

#[derive(Debug)]
struct Taken {
    at: Instant,
    facts: Facts,
}

impl Sampler {
    /// A sampler that has seen nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Turns a look into a reading, using the one before it for everything that is a rate.
    ///
    /// `now` is when the look *arrived*, and it is the interval between two arrivals that the rates are
    /// divided by — not the interval between two asks. A look that took three seconds to come back is
    /// three seconds of traffic, and a sampler that assumed its own cadence would report a throughput
    /// this board never saw.
    pub fn reading(&mut self, text: &str, now: Instant) -> NasReading {
        let facts = Facts::parse(text);
        let before = self.previous.replace(Taken { at: now, facts });

        let Some(before) = before else {
            // A first look: the disks and their capacities are facts already, and everything that needs
            // two looks is a dash. See the module documentation.
            return NasReading {
                status: NasStatus::Online,
                at: Some(now),
                uptime: self.facts().and_then(|facts| facts.uptime),
                memory: self.facts().and_then(|facts| facts.memory),
                cpu: None,
                network: None,
                disks: self.disks(None, 0.0),
                host_key: None,
            };
        };

        let seconds = now
            .saturating_duration_since(before.at)
            .as_secs_f32()
            .max(f32::EPSILON);

        let Some(taken) = self.facts() else {
            return NasReading {
                status: NasStatus::Online,
                at: Some(now),
                ..NasReading::default()
            };
        };

        NasReading {
            status: NasStatus::Online,
            at: Some(now),
            uptime: taken.uptime,
            memory: taken.memory,
            cpu: usage(&before.facts, taken),
            network: throughput(&before.facts, taken, seconds),
            disks: self.disks(Some(&before.facts), seconds),
            host_key: None,
        }
    }

    /// Forgets the previous look, so that the next one is a first look again.
    ///
    /// What a backend calls when it is pointed at a different NAS: a rate computed across two machines
    /// is a rate between them, and nothing on a page is less true than a number that belongs to another
    /// computer.
    pub fn forget(&mut self) {
        self.previous = None;
    }

    /// The current look's facts.
    fn facts(&self) -> Option<&Facts> {
        self.previous.as_ref().map(|taken| &taken.facts)
    }

    /// The disks, with whatever the interval between the looks gives them.
    ///
    /// `previous` is the look before this one, or `None` on a first look — in which case the rates are
    /// zero, because a disk that has only been seen once has moved nothing *as far as this board knows*,
    /// and reporting a lifetime average as a speed would be the one thing a monitor must not do.
    fn disks(&self, previous: Option<&Facts>, seconds: f32) -> Vec<NasDisk> {
        let Some(facts) = self.facts() else {
            return Vec::new();
        };

        // With nothing before it, a disk is compared against *itself* — which is what "no rate yet"
        // means, and not "everything since boot divided by the interval". See this function's
        // documentation.
        let previous = previous.unwrap_or(facts);

        let counter = |facts: &Facts, name: &str| -> Io {
            facts
                .io
                .iter()
                .find(|(device, _)| device == name)
                .map(|(_, io)| *io)
                .unwrap_or_default()
        };

        facts
            .blocks
            .iter()
            .map(|block| {
                let before = counter(previous, &block.name);
                let after = counter(facts, &block.name);

                NasDisk {
                    device: block.name.clone(),
                    model: block.model.clone(),
                    size_bytes: block.size,
                    // The drives' own temperatures are not in this look: reading them needs SMART (which
                    // wants root) or a `drivetemp` module, and an account that exists to read `/proc` has
                    // neither. `None` is what the page draws a dash for, and it is the truth.
                    temperature_c: None,
                    busy_percent: busy_percent(before, after, seconds),
                    read_per_sec: per_sec(after.read.saturating_sub(before.read), seconds),
                    write_per_sec: per_sec(after.written.saturating_sub(before.written), seconds),
                    purpose: purpose_of(block),
                }
            })
            .collect()
    }
}

/// How busy the CPU was between two looks.
///
/// `None` when the counters did not move, which happens when two looks land inside the same tick of the
/// kernel's clock: a usage of "0 or 100%, unknowable" is worse than a dash, and the page draws one.
fn usage(before: &Facts, after: &Facts) -> Option<NasCpu> {
    let (was, now) = (before.cpu?, after.cpu?);

    let total = now.total.saturating_sub(was.total);
    let idle = now.idle.saturating_sub(was.idle);

    if total == 0 {
        return None;
    }

    Some(NasCpu {
        usage_percent: ((total.saturating_sub(idle.min(total)) as f64 / total as f64) * 100.0) as f32,
        // On the CPU reading rather than beside it, because that is what it is: the package's
        // temperature is a fact about the CPU, and the page draws it in the CPU's card.
        temperature_c: after.cpu_temperature,
    })
}

/// What the network did between two looks.
fn throughput(before: &Facts, after: &Facts, seconds: f32) -> Option<NasNetwork> {
    let (before, after) = (before.network?, after.network?);

    Some(NasNetwork {
        received_per_sec: per_sec(after.received.saturating_sub(before.received), seconds),
        sent_per_sec: per_sec(after.sent.saturating_sub(before.sent), seconds),
    })
}

/// A counter difference as a per-second rate.
///
/// Rounded, and never negative. `/proc`'s counters are 64-bit and would take centuries to wrap, but a
/// *rebooted* NAS starts them from zero, and a page that drew negative throughput would be a page
/// nobody could trust about anything else on it.
fn per_sec(delta: u64, seconds: f32) -> u64 {
    if delta == 0 {
        return 0;
    }

    (delta as f64 / seconds.max(0.001) as f64).round().max(0.0) as u64
}

/// How much of the interval a disk spent with work in flight, in `0.0..=100.0` — `iostat`'s `%util`.
fn busy_percent(before: Io, after: Io, seconds: f32) -> f32 {
    let busy = after.busy_ms.saturating_sub(before.busy_ms) as f64;
    let window = seconds.max(0.001) as f64 * 1000.0;

    (busy / window * 100.0).clamp(0.0, 100.0) as f32
}

/// What a disk is for, in the NAS's own words.
///
/// The rule is fnOS's, and it is written down here because it is the one thing on this page that is not
/// in `/proc`: fnOS mounts its pools at `/vol1`, `/vol2` and so on, so a disk with a partition mounted
/// there belongs to 存储空间 1, 存储空间 2 — and the system disk is the one mounted at `/`. Those are the
/// same two words the NAS's own dashboard uses, which is the whole point: a person reading this page
/// should recognise their own box.
///
/// Anything else says where it is mounted, and a disk nothing is mounted from says nothing — a disk
/// somebody put in and has not set up has no purpose yet, and an empty cell is truer than a guess.
fn purpose_of(block: &Block) -> String {
    let Some(mount) = block.mount.as_deref() else {
        return String::new();
    };

    if mount == "/" {
        return String::from("系统安装");
    }

    match mount
        .strip_prefix("/vol")
        .filter(|pool| !pool.is_empty() && pool.chars().all(|c| c.is_ascii_digit()))
    {
        Some(pool) => format!("存储空间 {pool}"),
        None => mount.to_string(),
    }
}

/// The sections of a look, by the marker the command prints before each: a name and the body under it.
///
/// A section runs from its marker to the next one, and its body may be empty — which is what an
/// unreadable file or an absent sensor looks like, and is a reading that is missing rather than an error.
fn sections(text: &str) -> Vec<(&str, &str)> {
    const MARKER: &str = "=== ";

    let mut found: Vec<(&str, &str)> = Vec::new();
    let mut open: Option<(&str, usize)> = None;
    let mut offset = 0usize;

    for line in text.split_inclusive('\n') {
        if let Some(name) = line.trim().strip_prefix(MARKER) {
            if let Some((name, start)) = open.take() {
                let end = offset.min(text.len());
                found.push((name, &text[start.min(end)..end]));
            }

            // The body starts after this line, marker and all.
            open = Some((name.trim(), offset.saturating_add(line.len())));
        }

        offset = offset.saturating_add(line.len());
    }

    if let Some((name, start)) = open {
        found.push((name, &text[start.min(text.len())..]));
    }

    found
}

/// `/proc/uptime`: the seconds since boot, and the seconds spent idle.
fn uptime(text: &str) -> Option<Duration> {
    let seconds: f64 = text.split_whitespace().next()?.parse().ok()?;

    Some(Duration::from_secs_f64(seconds.max(0.0)))
}

/// The aggregate line of `/proc/stat`.
fn cpu_times(text: &str) -> Option<CpuTimes> {
    let line = text.lines().find(|line| line.starts_with("cpu "))?;

    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|field| field.parse().ok())
        .collect();

    if fields.len() < 5 {
        return None;
    }

    // user nice system idle iowait irq softirq steal guest guest_nice — and the first eight are the
    // machine's own time, the last two being guests this kernel merely accounts for.
    let idle = fields.get(3).copied().unwrap_or(0) + fields.get(4).copied().unwrap_or(0);
    let total: u64 = fields.iter().take(8).sum();

    Some(CpuTimes { idle, total })
}

/// `/proc/meminfo`, as the two numbers a page draws.
///
/// `MemAvailable` and not `MemFree`: a Linux box keeps most of its memory in cache, so a dashboard built
/// on `MemFree` reports a machine as full while it is doing exactly what it should. The kernel publishes
/// `MemAvailable` precisely so that this question has an honest answer.
fn memory(text: &str) -> Option<MemoryInfo> {
    let field = |name: &str| -> Option<u64> {
        text.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u64>().ok())
            .map(|kib| kib * 1024)
    };

    Some(MemoryInfo {
        total_bytes: field("MemTotal:")?,
        free_bytes: field("MemAvailable:")?,
    })
}

/// `/proc/net/dev`: every interface's counters added up, except the loopback.
///
/// Loopback is left out because it is not the network: a NAS talking to itself would otherwise show as
/// traffic, and the question this answers is "is something copying a file over the wire".
fn network(text: &str) -> Option<Counters> {
    let mut totals = Counters::default();
    let mut seen = false;

    for line in text.lines() {
        let Some((interface, rest)) = line.split_once(':') else {
            continue;
        };

        let interface = interface.trim();

        if interface.is_empty() || interface == "lo" {
            continue;
        }

        let fields: Vec<&str> = rest.split_whitespace().collect();

        // Receive: bytes packets errs drop fifo frame compressed multicast
        // Transmit: bytes packets errs drop fifo colls carrier compressed
        let (Some(received), Some(sent)) = (
            fields.first().and_then(|value| value.parse::<u64>().ok()),
            fields.get(8).and_then(|value| value.parse::<u64>().ok()),
        ) else {
            continue;
        };

        totals.received += received;
        totals.sent += sent;
        seen = true;
    }

    seen.then_some(totals)
}

/// `/proc/diskstats`, by device name.
fn diskstats(text: &str) -> Vec<(String, Io)> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let name = fields.get(2)?.to_string();

            // major minor name reads_completed reads_merged sectors_read ms_reading writes_completed
            // writes_merged sectors_written ms_writing ios_in_progress ms_doing_io …
            let sectors_read: u64 = fields.get(5)?.parse().ok()?;
            let sectors_written: u64 = fields.get(9)?.parse().ok()?;
            let busy_ms: u64 = fields.get(12)?.parse().ok()?;

            // The kernel counts sectors, and its sectors are 512 bytes whatever the drive's own are.
            Some((
                name,
                Io {
                    read: sectors_read * 512,
                    written: sectors_written * 512,
                    busy_ms,
                },
            ))
        })
        .collect()
}

/// The `hwmon` section: the name of each sensor group, and the first temperature under it.
///
/// The one place this parser guesses, and it guesses by name: a NAS has several sensors and an ordinary
/// login can read them all, so the groups a CPU is called — `coretemp` on Intel, `k10temp` on AMD,
/// `cpu_thermal` on ARM — are preferred, and a machine with none of those falls back to the first group
/// that reported a temperature, which on a small NAS is usually the right one.
fn temperature(text: &str) -> Option<f32> {
    let mut groups: Vec<(&str, Option<f32>)> = Vec::new();

    for line in text.lines() {
        if let Some(name) = line.trim().strip_prefix("-- ") {
            groups.push((name, None));
            continue;
        }

        let Some((_, value)) = groups.last_mut() else {
            continue;
        };

        if value.is_none() {
            if let Ok(millidegrees) = line.trim().parse::<f32>() {
                // Millidegrees, which is what the hwmon interface reports in.
                *value = Some(millidegrees / 1000.0);
            }
        }
    }

    let cpu = groups.iter().find(|(name, value)| {
        value.is_some()
            && (name.contains("coretemp") || name.contains("k10temp") || name.contains("cpu_thermal"))
    });

    cpu.or_else(|| groups.iter().find(|(_, value)| value.is_some()))
        .and_then(|(_, value)| *value)
}

/// One line of `lsblk -P`, as the fields this module wants.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    name: String,
    size: Option<u64>,
    kind: String,
    model: String,
    mount: Option<String>,
}

/// `lsblk -b -P`: the whole disks, each with what is mounted from one of its partitions.
///
/// Two rules, and both are about not being clever:
///
/// * The mount point is printed on the **partition**, not on the disk (`/` is on `sda1`), so a disk's
///   mount point is found by name: `sda1` is a partition of `sda`, `nvme0n1p1` of `nvme0n1`. That is the
///   rule every script uses, and unlike reading the tree it does not depend on how the branches are
///   drawn.
/// * Only `TYPE == disk` is kept. Partitions, LVM volumes and the kernel's own `loop`/`ram` devices are
///   not disks somebody put in the box, and a table that listed them would count one drive's throughput
///   three times under three names.
fn lsblk(text: &str) -> Vec<Block> {
    let rows: Vec<Row> = text
        .lines()
        .filter_map(|line| {
            let pairs = pairs(line);

            let field = |key: &str| -> Option<String> {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| value.clone())
            };

            // The tree's own characters, in case a version of `lsblk` prints them even in pairs mode.
            let name = field("NAME")?;
            let name = name
                .trim_start_matches(['|', '-', '`', ' ', '├', '─', '└'])
                .to_string();

            if name.is_empty() || name == "NAME" {
                return None;
            }

            Some(Row {
                name,
                size: field("SIZE").and_then(|size| size.parse::<u64>().ok()),
                kind: field("TYPE").unwrap_or_default(),
                model: field("MODEL").unwrap_or_default(),
                mount: field("MOUNTPOINT").filter(|mount| !mount.is_empty()),
            })
        })
        .collect();

    rows.iter()
        .filter(|row| row.kind == "disk")
        .map(|disk| Block {
            name: disk.name.clone(),
            size: disk.size,
            model: disk.model.clone(),
            mount: rows
                .iter()
                .filter(|row| row.kind == "part" && row.name.starts_with(&disk.name))
                .find_map(|row| row.mount.clone()),
        })
        .collect()
}

/// The `KEY="value"` pairs one line of `lsblk -P` prints.
///
/// Quoted, so a value with spaces in it ends where its quote ends — which is the whole reason `-P` is
/// asked for: see [`COMMAND`]. A value that is not quoted ends at the next space, which is what an
/// unquoted field means, and a quote that never closes ends the line, because a line that makes no sense
/// from some point on cannot be read past that point.
fn pairs(line: &str) -> Vec<(&str, String)> {
    let mut found = Vec::new();
    let mut rest = line;

    while let Some(at) = rest.find('=') {
        let key = rest[..at].trim();
        let after = &rest[at + 1..];

        let value = match after.strip_prefix('"') {
            Some(quoted) => {
                let Some(end) = quoted.find('"') else {
                    break;
                };

                rest = &quoted[end + 1..];
                quoted[..end].to_string()
            }
            None => {
                let end = after.find(' ').unwrap_or(after.len());

                rest = &after[end..];
                after[..end].to_string()
            }
        };

        if !key.is_empty() {
            found.push((key, value));
        }
    }

    found
}

/// Whether a name is one of the kernel's own devices rather than a disk somebody put in the box.
fn is_virtual(name: &str) -> bool {
    ["loop", "ram", "zram", "dm-", "md"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real look, in the shape [`COMMAND`] prints it: a two-disk fnOS box with an NVMe system drive.
    ///
    /// Written out rather than generated, because the whole point of this module is that the *shape* is
    /// what gets parsed wrong — a sample built from the parser would agree with it about everything.
    const SAMPLE: &str = "\
=== uptime
147212.66 512334.11
=== stat
cpu  123456 789 45678 9876543 12345 0 6789 0 0 0
cpu0 61728 394 22839 4938271 6172 0 3394 0 0 0
=== meminfo
MemTotal:       16333456 kB
MemFree:         1234567 kB
MemAvailable:    9876543 kB
Buffers:          456789 kB
=== net/dev
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 987654321  512345       0    0    0     0          0         0 987654321  512345    0    0    0     0       0          0
  eth0: 8192000000 6543210       0    0    0     0          0         0 1234567890 1234567    0    0    0     0       0          0
=== diskstats
   8       0 sda 654321 12345 12345678 98765 123456 6789 2345678 45678 0 34567 144444
   8       1 sda1 654321 12345 12345678 98765 123456 6789 2345678 45678 0 34567 144444
 259       0 nvme0n1 12 0 1024 5 8 0 2048 9 0 7 14
=== hwmon
-- nvme
38900
-- coretemp
42000
=== lsblk
NAME=\"sda\" SIZE=\"500107862016\" TYPE=\"disk\" MODEL=\"Samsung SSD 870 EVO 500GB\" MOUNTPOINT=\"\"
NAME=\"sda1\" SIZE=\"499963170816\" TYPE=\"part\" MODEL=\"\" MOUNTPOINT=\"/\"
NAME=\"sdb\" SIZE=\"4000787030016\" TYPE=\"disk\" MODEL=\"WDC WD40EFRX-68N32N0\" MOUNTPOINT=\"\"
NAME=\"sdb1\" SIZE=\"4000785441280\" TYPE=\"part\" MODEL=\"\" MOUNTPOINT=\"/vol1\"
NAME=\"nvme0n1\" SIZE=\"2000398934016\" TYPE=\"disk\" MODEL=\"Samsung SSD 980 PRO 2TB\" MOUNTPOINT=\"\"
";

    fn facts() -> Facts {
        Facts::parse(SAMPLE)
    }

    /// One look is read section by section, and each section says what it is.
    #[test]
    fn a_sample_is_read_section_by_section() {
        let facts = facts();

        assert_eq!(facts.uptime, Some(Duration::from_secs_f64(147212.66)));

        let cpu = facts.cpu.unwrap();
        assert_eq!(cpu.total, 123456 + 789 + 45678 + 9876543 + 12345 + 6789);
        assert_eq!(
            cpu.idle,
            9876543 + 12345,
            "idle is idle plus the time spent waiting on I/O, which is what a `top` calls idle"
        );

        let memory = facts.memory.unwrap();
        assert_eq!(memory.total_bytes, 16333456 * 1024);
        assert_eq!(
            memory.free_bytes,
            9876543 * 1024,
            "MemAvailable and not MemFree: a box is not full because it is caching"
        );

        let network = facts.network.unwrap();
        assert_eq!(network.received, 8192000000, "the loopback is not the network");
        assert_eq!(network.sent, 1234567890);

        assert_eq!(
            facts.cpu_temperature,
            Some(42.0),
            "coretemp is the CPU's, even when another sensor is listed first"
        );
    }

    /// The disks are the whole disks, in the order `lsblk` gave them, each carrying the mount point its
    /// partition has — and the partitions are not in the list.
    #[test]
    fn the_disks_are_whole_disks_with_their_partitions_mounts() {
        let facts = facts();

        let names: Vec<&str> = facts.blocks.iter().map(|block| block.name.as_str()).collect();
        assert_eq!(names, ["sda", "sdb", "nvme0n1"]);

        assert_eq!(facts.blocks[0].model, "Samsung SSD 870 EVO 500GB");
        assert_eq!(facts.blocks[0].mount.as_deref(), Some("/"));
        assert_eq!(
            facts.blocks[1].mount.as_deref(),
            Some("/vol1"),
            "the mount point is printed on the partition, and it is the disk's"
        );
        assert_eq!(facts.blocks[2].mount, None, "a disk nothing is mounted from");

        let io: Vec<&str> = facts.io.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            io,
            ["sda", "nvme0n1"],
            "sda1 is a partition of sda, and counting it would double sda's traffic"
        );

        assert_eq!(facts.io[0].1.read, 12345678 * 512);
        assert_eq!(facts.io[0].1.busy_ms, 34567);
    }

    /// The purpose is the NAS's own word for the disk, and a disk nobody has set up says nothing.
    #[test]
    fn a_disk_purpose_is_the_nas_own_word_for_it() {
        let facts = facts();

        let purposes: Vec<String> = facts.blocks.iter().map(purpose_of).collect();

        assert_eq!(purposes, ["系统安装", "存储空间 1", ""]);
    }

    /// Two looks make the rates: the difference between the counters, over the time between the looks.
    #[test]
    fn two_looks_make_the_rates() {
        let mut sampler = Sampler::new();
        let start = Instant::now();

        // The first look: facts, and dashes where the speeds will be.
        let first = sampler.reading(SAMPLE, start);

        assert_eq!(first.status, NasStatus::Online);
        assert!(first.cpu.is_none(), "one look cannot know how busy the CPU is");
        assert!(first.network.is_none());
        assert_eq!(first.disks.len(), 3, "but the disks are facts already");
        assert_eq!(first.disks[0].read_per_sec, 0, "and have no rate yet");
        assert_eq!(first.disks[0].purpose, "系统安装");
        assert_eq!(first.uptime, Some(Duration::from_secs_f64(147212.66)));

        // The second, ten seconds later: the same machine with more counters on it.
        let later = SAMPLE
            .replace("8192000000", "8202472960") // +10 MiB received
            .replace("1234567890", "1236667890") // +2 MiB sent
            .replace(
                "cpu  123456 789 45678 9876543",
                "cpu  173456 789 45678 9881543",
            )
            .replace("12345678 98765", "13345678 98765")
            .replace("0 34567 144444", "0 39567 144444");

        let second = sampler.reading(&later, start + Duration::from_secs(10));

        let network = second.network.unwrap();
        assert_eq!(network.received_per_sec, 10_472_960 / 10);
        assert_eq!(network.sent_per_sec, 2_100_000 / 10);

        let cpu = second.cpu.unwrap();
        assert_eq!(cpu.temperature_c, Some(42.0), "and the CPU is as hot as it was");

        // The two `cpu` lines differ by 55000 ticks, 5000 of which are idle — the idle time is part of
        // the total rather than beside it, which is the one thing about `/proc/stat` worth checking by
        // arithmetic instead of by eye.
        assert!(
            (cpu.usage_percent - (100.0 - 5000.0 / 55000.0 * 100.0)).abs() < 0.01,
            "{cpu:?}"
        );

        assert_eq!(second.disks[0].read_per_sec, 1_000_000 * 512 / 10);
        assert_eq!(
            second.disks[0].busy_percent, 50.0,
            "5000 ms of work in a 10 s window"
        );
        assert_eq!(second.disks[1].read_per_sec, 0, "a disk that moved nothing");
    }

    /// A counter for a device `lsblk` did not call a disk is left out of the table: the join between the
    /// two is what decides, and a line only one of them knows about is not a disk somebody put in.
    #[test]
    fn a_counter_with_no_disk_behind_it_is_left_out() {
        assert!(
            facts().io.iter().all(|(name, _)| name != "sda1"),
            "the partition's counters were filtered out"
        );

        let mut sampler = Sampler::new();
        let start = Instant::now();

        sampler.reading(SAMPLE, start);

        let with_another = format!("{SAMPLE}   8      32 sdc 999999 0 99999999 5 0 0 0 0 0 0 0\n");
        let reading = sampler.reading(&with_another, start + Duration::from_secs(5));

        assert!(
            reading.disks.iter().all(|disk| disk.device != "sdc"),
            "the counters know about sdc, and `lsblk` did not list it"
        );
    }

    /// Forgetting the last look makes the next one a first look: what a backend calls when it is pointed
    /// at a different machine.
    #[test]
    fn forgetting_makes_the_next_look_a_first_one() {
        let mut sampler = Sampler::new();
        let start = Instant::now();

        sampler.reading(SAMPLE, start);
        sampler.forget();

        let after = sampler.reading(SAMPLE, start + Duration::from_secs(10));

        assert!(
            after.network.is_none(),
            "a rate across a forgetting is a rate between two machines"
        );
        assert_eq!(after.disks.len(), 3);
        assert_eq!(after.disks[0].read_per_sec, 0);
    }

    /// A look that came back with half a machine is half a reading and never a panic: a kernel that
    /// moved, a file this account cannot read, a session that closed in the middle of one.
    #[test]
    fn a_partial_or_foreign_look_is_read_for_what_it_has() {
        assert_eq!(Facts::parse(""), Facts::default());
        assert_eq!(
            Facts::parse("something that is not a look at all"),
            Facts::default()
        );

        let no_memory = Facts::parse("=== stat\ncpu  1 2 3 4 5 6 7 8\n");
        assert!(no_memory.cpu.is_some());
        assert!(no_memory.memory.is_none(), "and the meter draws a dash");

        let no_disks = Facts::parse(SAMPLE.split("=== lsblk").next().unwrap());
        assert!(no_disks.memory.is_some());
        assert!(no_disks.blocks.is_empty(), "with no `lsblk` there are no disks");
        assert!(
            no_disks.io.is_empty(),
            "and no counters to attach to disks that are not there"
        );

        // A section from a later version of the command is one this version does not know.
        let ahead = Facts::parse("=== uptime\n10.0 5.0\n=== smart\nwhatever\n");
        assert_eq!(ahead.uptime, Some(Duration::from_secs(10)));

        // And a section with a marker and nothing under it is a section with nothing under it.
        let empty = Facts::parse("=== uptime\n=== stat\n");
        assert_eq!(empty.uptime, None);
        assert_eq!(empty.cpu, None);
    }

    /// `lsblk -P`'s pairs are read by their quotes, and a line that stops making sense is read up to
    /// where it does — a `MOUNTPOINT` with a space in it is why the quoting is asked for in the first
    /// place.
    #[test]
    fn the_pairs_are_read_by_their_quotes() {
        let read =
            pairs(r#"NAME="sdb" SIZE="4000787030016" TYPE="disk" MODEL="WDC WD40EFRX-68N32N0""#);

        assert_eq!(
            read,
            vec![
                ("NAME", String::from("sdb")),
                ("SIZE", String::from("4000787030016")),
                ("TYPE", String::from("disk")),
                ("MODEL", String::from("WDC WD40EFRX-68N32N0")),
            ]
        );

        assert_eq!(
            pairs(r#"MODEL="Samsung SSD 870 EVO" MOUNTPOINT="""#),
            vec![
                ("MODEL", String::from("Samsung SSD 870 EVO")),
                ("MOUNTPOINT", String::new()),
            ],
            "a value with spaces in it, and an empty one"
        );

        assert_eq!(
            pairs("NAME=sdb SIZE=4000"),
            vec![
                ("NAME", String::from("sdb")),
                ("SIZE", String::from("4000")),
            ],
            "an unquoted field ends at its space"
        );

        assert_eq!(
            pairs(r#"NAME="sdb" MODEL="never closed"#),
            vec![("NAME", String::from("sdb"))],
            "a line that stops making sense is read up to where it does"
        );
    }

    /// The command asks for everything in one round trip, and marks every section.
    #[test]
    fn the_command_is_one_round_trip_with_markers() {
        assert!(
            !COMMAND.contains('\n'),
            "one line, so that the session runs it once: {COMMAND}"
        );
        assert!(COMMAND.contains("echo \"=== "), "{COMMAND}");

        for file in ["uptime", "stat", "meminfo", "net/dev", "diskstats"] {
            assert!(COMMAND.contains(file), "{file} is not asked for: {COMMAND}");
        }

        assert!(COMMAND.contains("lsblk -b"), "sizes in bytes, not in whatever G means");
        assert!(COMMAND.contains("hwmon"), "and one temperature it can read");
    }
}
