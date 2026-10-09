//! Strongly-typed data structures shared across HAL backends.

/// How much of `total` is taken by `used`, in `0.0..=100.0`.
///
/// The one place this arithmetic lives. A heap and a volume are both "some of a whole", and two
/// copies of the two guards below — a total of zero, and a used larger than it — would be two
/// chances for the two readings to disagree about what an empty one looks like.
///
/// Zero for a total of zero, and not a division: a thing that reports no size at all is not full,
/// and a bar drawn from a division by it is not a number.
fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }

    (used.min(total) as f64 / total as f64 * 100.0) as f32
}

/// Which of the board's two kinds of storage a [`VolumeInfo`] describes.
///
/// The two are not interchangeable to a person: one is soldered to the board and always there, the
/// other is a card a finger takes out. That difference is what a page has to draw — one is a bar, the
/// other is a bar *or* a line saying there is nothing in the slot — so it is part of the type rather
/// than something the caller infers from a mount point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    /// The built-in partition (LittleFS on the `internal` partition of the flash).
    Internal,
    /// A card in the slot. Also the kind the slot has when it is empty — which is why a volume is
    /// only ever reported when something *is* mounted.
    Removable,
}

/// A mounted filesystem: how big it is and how much of it is left.
///
/// A snapshot, taken when it is asked for. Sizes are exact bytes so that a page can print the number
/// it was given instead of rounding it twice; the percentages are derived ([`VolumeInfo::used_percent`])
/// rather than carried, because two numbers that must agree eventually will not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    pub kind: VolumeKind,
    /// Where it is mounted, exactly as the filesystem spells it (`/internal`, `/sdcard`).
    pub mount_point: String,
    /// The driver's name for the filesystem (`LittleFS`, `FATFS`) — the name of what is doing the
    /// work, not a guess at the on-disk format.
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

impl VolumeInfo {
    /// How many bytes are on it.
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// How full it is, in `0.0..=100.0`.
    ///
    /// Zero for a volume that reports no size at all: an unmounted or unformatted filesystem is not
    /// full, and a division by it is not a number a bar can draw.
    pub fn used_percent(&self) -> f32 {
        percent(self.used_bytes(), self.total_bytes)
    }
}

/// What a region of the board's flash holds.
///
/// The flash is one chip and one address space; what divides it is what a region is *for*. These are
/// the four answers a person reading a storage page needs told apart: what the firmware owns and
/// they cannot touch, what the box can write, and what nothing claims at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashRegionKind {
    /// Reserved or system: the bootloader, the partition table, NVS, the PHY calibration data.
    System,
    /// A firmware image partition — read-only while the box is running.
    Firmware,
    /// A filesystem the box can write: the built-in `internal` partition.
    Data,
    /// Not claimed by any partition.
    Unallocated,
}

/// One region of the flash, in address order.
///
/// A region and not a partition: the bootloader and the space past the last partition are not
/// partitions and have no entry in a partition table, and a map of the chip that left them out would
/// be a map that did not add up to the chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashRegion {
    /// How this machine spells it — a partition's own label (`nvs`, `factory`, `internal`), or the
    /// name the platform gave a region that is not one. Data, not interface text, like a mount point.
    pub label: String,
    pub kind: FlashRegionKind,
    pub size: u32,
}

/// The whole flash chip: how big it is, and what every part of it is for.
///
/// This is why a storage page needs more than [`VolumeInfo`]: the built-in volume is 3 MB of a 16 MB
/// chip, and the other 13 MB are the firmware and the reservations around it. A page that draws only
/// the filesystem answers "how much room is left" and not "where did the flash go".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashLayout {
    pub total_bytes: u32,
    /// In address order, from the first byte of the bootloader to the end of the chip. The regions
    /// tile the whole of it, which is what makes a bar drawn from them add up to the chip.
    pub regions: Vec<FlashRegion>,
}

impl FlashLayout {
    /// The bytes some partition claims: everything but what nothing does.
    ///
    /// Derived rather than carried: a total that disagreed with the parts drawn beside it would be a
    /// second opinion on the same sum, and two numbers that must agree eventually will not.
    pub fn allocated_bytes(&self) -> u32 {
        self.regions
            .iter()
            .filter(|region| region.kind != FlashRegionKind::Unallocated)
            .map(|region| region.size)
            .sum()
    }
}

/// Wi-Fi scan progress reported by [`crate::traits::WifiBackend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanState {
    /// No scan has been started, or the previous results were consumed.
    #[default]
    Idle,
    /// A scan is currently in progress.
    Scanning,
    /// The scan completed and results are ready.
    Done,
    /// The scan failed.
    Error,
}

/// High-level Wi-Fi connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiState {
    #[default]
    Disconnected,
    Scanning,
    Connecting,
    Connected,
}

/// A discovered Wi-Fi access point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApInfo {
    pub ssid: String,
    pub rssi: i8,
    pub secure: bool,
    pub channel: u8,
}

impl ApInfo {
    pub fn new(ssid: impl Into<String>, rssi: i8, secure: bool, channel: u8) -> Self {
        Self {
            ssid: ssid.into(),
            rssi,
            secure,
            channel,
        }
    }

    /// Signal quality mapped to `0..=4` bars, matching the status-bar icon.
    pub fn signal_bars(&self) -> u8 {
        signal_bars(self.rssi)
    }
}

/// Signal strength mapped to `0..=4` bars.
///
/// The one place the scale is written: a network's row and the status bar both ask a device or a
/// connection for its bars, so a scale with two owners cannot drift.
pub fn signal_bars(rssi: i8) -> u8 {
    match rssi {
        r if r >= -55 => 4,
        r if r >= -65 => 3,
        r if r >= -75 => 2,
        r if r >= -85 => 1,
        _ => 0,
    }
}

/// Current Wi-Fi status snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiStatus {
    pub state: WifiState,
    pub ssid: String,
    pub ip: String,
    pub netmask: String,
    pub gateway: String,
    pub rssi: i8,
}

impl WifiStatus {
    /// Signal quality of the connection, in `0..=4` bars — the same scale as
    /// [`ApInfo::signal_bars`].
    ///
    /// Zero when there is no connection: `rssi` only means something while one exists, and a status
    /// that was never filled in has `rssi` 0, which would otherwise read as a perfect signal.
    pub fn signal_bars(&self) -> u8 {
        if self.state != WifiState::Connected {
            return 0;
        }

        signal_bars(self.rssi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_bar_scale_is_monotone() {
        // The thresholds, from best to none, and one sample either side of each.
        let samples = [
            (-40, 4),
            (-55, 4),
            (-56, 3),
            (-65, 3),
            (-66, 2),
            (-75, 2),
            (-76, 1),
            (-85, 1),
            (-86, 0),
            (-127, 0),
        ];

        for (rssi, bars) in samples {
            assert_eq!(signal_bars(rssi), bars, "{rssi} dBm");
        }
    }

    #[test]
    fn a_status_without_a_connection_has_no_bars() {
        // `rssi` is 0 in a default status, which on the bare scale is a perfect signal.
        assert_eq!(WifiStatus::default().signal_bars(), 0);

        let connected = WifiStatus {
            state: WifiState::Connected,
            rssi: -60,
            ..WifiStatus::default()
        };

        assert_eq!(connected.signal_bars(), 3);
    }

    /// The two derived numbers a usage bar is drawn from.
    #[test]
    fn a_volume_reports_what_is_used_of_it() {
        let volume = |total: u64, free: u64| VolumeInfo {
            kind: VolumeKind::Internal,
            mount_point: "/internal".to_string(),
            filesystem: "LittleFS".to_string(),
            total_bytes: total,
            free_bytes: free,
        };

        let half = volume(1000, 500);
        assert_eq!(half.used_bytes(), 500);
        assert_eq!(half.used_percent(), 50.0);

        let empty = volume(1000, 1000);
        assert_eq!(empty.used_percent(), 0.0);

        let full = volume(1000, 0);
        assert_eq!(full.used_percent(), 100.0);

        // A filesystem that cannot say how big it is must not be drawn as full — nor divide by zero.
        let unknown = volume(0, 0);
        assert_eq!(unknown.used_bytes(), 0);
        assert_eq!(unknown.used_percent(), 0.0);
    }

    /// The one number a flash map is headed with: what is claimed, against the chip it is claimed
    /// out of. Unallocated is the part that is *not* — the whole point of drawing it.
    #[test]
    fn a_flash_layout_says_how_much_of_the_chip_is_claimed() {
        let region = |kind: FlashRegionKind, size: u32| FlashRegion {
            label: String::from("region"),
            kind,
            size,
        };

        let layout = FlashLayout {
            total_bytes: 16 * 1024 * 1024,
            regions: vec![
                region(FlashRegionKind::System, 0x9000),
                region(FlashRegionKind::System, 0x6000),
                region(FlashRegionKind::System, 0x1000),
                region(FlashRegionKind::Firmware, 0xC0_0000),
                region(FlashRegionKind::Data, 0x30_0000),
                region(FlashRegionKind::Unallocated, 0xF_0000),
            ],
        };

        assert_eq!(layout.allocated_bytes(), 16 * 1024 * 1024 - 0xF_0000);

        // The regions are the chip: a map built from them has to add up to it, or the bar the page
        // draws is a bar with a hole in it.
        let sum: u32 = layout.regions.iter().map(|region| region.size).sum();
        assert_eq!(sum, layout.total_bytes);
    }

    /// The address a person types, and the three cases in which there is none.
    #[test]
    fn a_page_has_an_address_only_when_there_is_a_network_to_reach_it_on() {
        let connected = WifiStatus {
            state: WifiState::Connected,
            ssid: String::from("hive"),
            ip: String::from("192.168.1.42"),
            ..WifiStatus::default()
        };

        let running = WebStatus {
            running: true,
            port: WebStatus::DEFAULT_PORT,
        };

        // Port 80 is not spelled: it is what typing the address without one means.
        assert_eq!(running.url(&connected).as_deref(), Some("http://192.168.1.42"));

        let elsewhere = WebStatus { port: 8080, ..running };
        assert_eq!(elsewhere.url(&connected).as_deref(), Some("http://192.168.1.42:8080"));

        // Stopped, or not on a network: the same answer, because both are "do not tell someone to
        // open this".
        assert_eq!(WebStatus::default().url(&connected), None);
        assert_eq!(running.url(&WifiStatus::default()), None);

        // A connected status that has not been filled in yet has no address either.
        let no_address = WifiStatus {
            state: WifiState::Connected,
            ..WifiStatus::default()
        };
        assert_eq!(running.url(&no_address), None);
    }
}

/// Whether the management page is being served, and where.
///
/// `running` and `port` are one value rather than two questions because the answer to "is it up" is
/// only usable together with "on what": a page drawn from two calls is a page that can say a server
/// is up on the port of the one that was stopped a moment ago.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WebStatus {
    pub running: bool,
    /// The port it is up on. Zero while it is down, which is also what a backend that has never
    /// been asked to start reports.
    pub port: u16,
}

impl WebStatus {
    /// The port a browser reaches when the address says nothing about one.
    ///
    /// The default because it is the one address a person can read out loud and type: an URL with
    /// `:8080` in it is a URL that gets mistyped onto a phone keyboard.
    pub const DEFAULT_PORT: u16 = 80;

    /// The address to open, or `None` while there is nothing that could be opened.
    ///
    /// The rule lives here rather than in the app because it is not a preference: a server that is
    /// down serves nothing and a box that is not on a network has no address a browser on that
    /// network can reach, so an URL built without asking is a link that fails in the one place
    /// (someone else's phone) where the person cannot see why.
    pub fn url(&self, wifi: &WifiStatus) -> Option<String> {
        if !self.running || wifi.state != WifiState::Connected || wifi.ip.is_empty() {
            return None;
        }

        if self.port == Self::DEFAULT_PORT {
            Some(format!("http://{}", wifi.ip))
        } else {
            Some(format!("http://{}:{}", wifi.ip, self.port))
        }
    }
}

/// Metadata describing an audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioMeta {
    pub sample_rate: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
    pub duration_secs: f32,
}

/// A simple three-component vector used for IMU readings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

/// The heap the firmware runs on: what there is, and what is left of it.
///
/// The two counts and not a percentage. A percentage is a share of *something*, and which something
/// is the reader's question to answer: the same bytes are a comfortable margin on a chip with 8 MB
/// of PSRAM beside them, and nearly nothing on one without. So the backend reports what it measured
/// and [`MemoryInfo::used_percent`] is the arithmetic a caller does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryInfo {
    /// Every byte the allocator was given.
    pub total_bytes: u64,
    /// What is still free. Fragmented space is free and counted: the allocator can hand it out, even
    /// when no single allocation is small enough to take it.
    pub free_bytes: u64,
}

impl MemoryInfo {
    /// How many bytes are handed out.
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// How full the heap is, in `0.0..=100.0`.
    pub fn used_percent(&self) -> f32 {
        percent(self.used_bytes(), self.total_bytes)
    }
}

/// The chip the firmware is running on.
///
/// The three facts a "what is this machine" card has room for, and all three are the silicon's own
/// account of itself rather than a board's marketing name: `ESP32-S3` is what the chip answers when
/// it is asked, and no part of this stack can answer for the board it is soldered to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipInfo {
    /// The part number as the chip reports it.
    pub model: String,
    /// How many cores it has.
    pub cores: u8,
    /// Its silicon revision.
    pub revision: u16,
}

/// The firmware that is running, as it names itself.
///
/// Read out of the image's own description — the same strings the build stamped into it — and not
/// from a literal in whichever page is drawing them. A version written down twice is a version that
/// is wrong once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareInfo {
    /// The project's name.
    pub name: String,
    /// The version string built into the image.
    pub version: String,
}

/// An abstract user input action (physical button, gesture, or navigation command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    /// Back navigation: return to previous page/screen, close modal, or keep app in background.
    Back,
    /// Exit / Kill: terminate the current app and release its memory.
    Exit,
}

/// A hardware or system event emitted by the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemEvent {
    /// Battery or power supply status changed.
    BatteryChanged {
        percent: u8,
        charging: bool,
        voltage_mv: u32,
    },
    /// Wi-Fi connection status or signal changed.
    WifiStatusChanged(WifiStatus),
    /// User input action.
    InputAction(InputAction),
    /// A second has passed.
    ///
    /// The only event here that nothing *happened* to produce, and it exists for the one thing a
    /// readout cannot get any other way: a clock that moves. A page showing how long the board has
    /// been up has no hardware interrupt to wait on, and this platform has no timer subscription to
    /// ask for — `iced::time::every` needs an async runtime the board has not got — so the pulse
    /// comes from the thread that is already awake every second anyway.
    ///
    /// No payload: what a reader wants at the tick is whatever it is showing, and a struct carrying
    /// every reading would be this event deciding which ones matter. The board itself is the thing
    /// to ask, and [`crate::Board`] is where it is.
    ///
    /// Once a second, out of the thread that already waits a second between hardware interrupts —
    /// see `firmware/pomelo-hal-esp32/src/event.rs`. A consumer with nothing that needs a clock
    /// drops it, which is every consumer but one.
    Tick,
}


