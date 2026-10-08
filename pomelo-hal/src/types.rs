//! Strongly-typed data structures shared across HAL backends.

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
        if self.total_bytes == 0 {
            return 0.0;
        }

        (self.used_bytes() as f64 / self.total_bytes as f64 * 100.0) as f32
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
}


