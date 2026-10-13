//! Who else is on the air, and which channel is worth using.
//!
//! # A channel number is not a bucket
//!
//! 2.4 GHz channels are 5 MHz apart and a network's signal is about 20 MHz wide, so a network on
//! channel 6 is *heard* on 4, 5, 6, 7 and 8 at once. Counting exact matches would leave a channel with
//! one loud neighbour looking empty — and an empty-looking channel is exactly the one a person would
//! then move onto. So every network is counted through an [`overlap`] factor and weighted by how loud
//! it is, and the sum is what the chart draws and what the recommendation is made from.
//!
//! These numbers are a **model**, and the page says so: the factors are the textbook shape of a 20 MHz
//! OFDM mask rather than a measurement taken with this box's radio. What the page claims from them is
//! the *ordering* — which channel is the least crowded of the ones that can be used — and an ordering
//! is what survives this approximation. It would not survive the other one: "count the networks on each
//! channel" gets the ordering wrong whenever the loudest network is not on the most crowded channel,
//! which in a city is most of the time.
//!
//! # Only 1 to 13, and only 2.4 GHz
//!
//! The chart is the thirteen usable channels of the 2.4 GHz band, because that is the band this box's
//! radio can be on and the band where "which channel" has an answer. Networks outside it — 5 GHz, and
//! the 14th channel some countries allow — are counted and listed but kept out of the arithmetic: a
//! network on 36 takes up none of channel 1, and adding it to the pile would make the page quieter
//! about the noise that is really there.

use std::cmp::Ordering;

use pomelo_hal::ApInfo;

/// The first channel this page charts.
pub const FIRST: u8 = 1;

/// The last channel this page charts.
///
/// Thirteen rather than fourteen: 14 is a Japanese-only channel nothing here will meet, and a
/// fourteenth column that is always empty is a fourteenth of the chart spent saying nothing.
pub const LAST: u8 = 13;

/// The channels that do not overlap one another at all.
///
/// This is the whole of the standard advice about 2.4 GHz, and the reason the recommendation is drawn
/// from these three rather than from the quietest channel on the chart: in a quiet enough building any
/// channel works, and in a busy one a channel that overlaps two is worse than a busier-looking channel
/// that overlaps none.
pub const CLEAN: [u8; 3] = [1, 6, 11];

/// How many rows the signal map has, which is also how many bands of signal it tells apart.
pub const LEVELS: usize = 7;

/// How wide one band of the signal map is, in dBm.
const BAND_DBM: i32 = 10;

/// The signal the map's first row is for: everything at least this strong.
const LEVEL_CEILING: i32 = -40;

/// The channels of the band, in order.
pub fn channels() -> impl Iterator<Item = u8> {
    FIRST..=LAST
}

/// Whether `channel` is one this page charts.
pub fn in_band(channel: u8) -> bool {
    (FIRST..=LAST).contains(&channel)
}

/// How much of `candidate` a network on channel `ap` takes up, `0.0..=1.0`.
///
/// The shape of a 20 MHz mask sampled at the 5 MHz channel spacing, and the reason the recommendation
/// is not simply "the channel with no networks on it".
pub fn overlap(ap: u8, candidate: u8) -> f32 {
    match ap.abs_diff(candidate) {
        0 => 1.0,
        1 => 0.75,
        2 => 0.5,
        3 => 0.25,
        4 => 0.1,
        _ => 0.0,
    }
}

/// How loud a signal is, `0.0..=1.0`, for weighting what it takes up.
///
/// Linear in dBm across the window a 2.4 GHz network is ever seen in: nothing at -95 (−95 dBm is the
/// floor of a scan that has stopped finding things), everything at -35, half at -65. A scale rather
/// than a measurement, because the comparison this page makes is between two networks on one chart,
/// and any monotone scale gets that comparison right.
pub fn weight(rssi: i8) -> f32 {
    ((f32::from(rssi) + 95.0) / 60.0).clamp(0.0, 1.0)
}

/// Which row of the signal map a signal belongs in: `0` is the strongest, [`LEVELS`] − 1 is the rest.
///
/// The rows are [`BAND_DBM`] wide and the first holds everything at [`LEVEL_CEILING`] or better, so a
/// row's label is the *best* signal that lands in it: row 1 is the networks between -41 and -50 dBm.
pub fn level_of(rssi: i8) -> usize {
    ((LEVEL_CEILING - i32::from(rssi)) / BAND_DBM).clamp(0, LEVELS as i32 - 1) as usize
}

/// One channel of the chart: what is on it, what is heard on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    /// The channel number.
    pub channel: u8,
    /// How many networks are *on* this channel, exactly.
    pub aps: usize,
    /// Everything heard here: each in-band network's loudness times its [`overlap`] with this channel.
    pub pressure: f32,
    /// The strongest network on this channel, if there is one. Neighbours do not count.
    pub strongest: Option<i8>,
}

/// The band as the last scan found it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spectrum {
    bins: Vec<Bin>,
    others: Vec<ApInfo>,
}

impl Spectrum {
    /// Read a scan.
    pub fn of(aps: &[ApInfo]) -> Self {
        let here: Vec<&ApInfo> = aps.iter().filter(|ap| in_band(ap.channel)).collect();

        let bins = channels()
            .map(|channel| {
                let mut pressure = 0.0;
                let mut aps = 0;
                let mut strongest = None;

                for ap in &here {
                    pressure += overlap(ap.channel, channel) * weight(ap.rssi);

                    if ap.channel == channel {
                        aps += 1;
                        strongest = Some(strongest.map_or(ap.rssi, |best: i8| best.max(ap.rssi)));
                    }
                }

                Bin {
                    channel,
                    aps,
                    pressure,
                    strongest,
                }
            })
            .collect();

        Self {
            bins,
            others: aps
                .iter()
                .filter(|ap| !in_band(ap.channel))
                .cloned()
                .collect(),
        }
    }

    /// The thirteen columns, in channel order.
    pub fn bins(&self) -> &[Bin] {
        &self.bins
    }

    /// The networks the chart has no column for: 5 GHz, and anything else out of band.
    pub fn others(&self) -> &[ApInfo] {
        &self.others
    }

    /// How many networks the scan found, charted or not.
    pub fn networks(&self) -> usize {
        self.bins.iter().map(|bin| bin.aps).sum::<usize>() + self.others.len()
    }

    /// The column for `channel`, for a page that has a channel in mind.
    pub fn bin(&self, channel: u8) -> Option<&Bin> {
        self.bins.iter().find(|bin| bin.channel == channel)
    }

    /// The most pressure any one channel is under.
    pub fn peak(&self) -> f32 {
        self.bins
            .iter()
            .map(|bin| bin.pressure)
            .fold(0.0, f32::max)
    }

    /// Whether anything at all is on the band.
    pub fn crowded(&self) -> bool {
        self.peak() > 0.0
    }

    /// How full a column is drawn, `0.0..=1.0`.
    ///
    /// Capped, and the cap is the point: the height is an absolute scale rather than a share of the
    /// busiest channel, so a quiet building draws short columns instead of drawing its quietest channel
    /// as a full one. Two loud networks on one channel would otherwise add up to twice a full column.
    pub fn level(&self, bin: &Bin) -> f32 {
        bin.pressure.min(1.0)
    }

    /// The channel under the most pressure, if any is.
    ///
    /// Ties go to the lower channel number, so the answer is a fact about the air rather than about the
    /// order the driver happened to report the scan in.
    pub fn busiest(&self) -> Option<&Bin> {
        self.bins
            .iter()
            .filter(|bin| bin.pressure > 0.0)
            .max_by(|a, b| {
                a.pressure
                    .partial_cmp(&b.pressure)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| b.channel.cmp(&a.channel))
            })
    }

    /// Every in-band network, loudest first — the order a person reads a list in.
    pub fn loudest(&self, aps: &[ApInfo]) -> Vec<ApInfo> {
        let mut sorted: Vec<ApInfo> = aps.iter().filter(|ap| in_band(ap.channel)).cloned().collect();
        sorted.sort_by(|a, b| b.rssi.cmp(&a.rssi).then_with(|| a.ssid.cmp(&b.ssid)));

        sorted
    }

    /// The channel to use: the least crowded of the three that do not overlap.
    ///
    /// Deliberately not the least crowded channel on the chart. A channel outside 1, 6 and 11 shares
    /// spectrum with both of its neighbours, so the quietest of them can be worse to *be* on than a
    /// middle one that looks busier — and this number is advice rather than a reading. Ties go to the
    /// lower channel number, which is also why an empty band answers 1.
    pub fn recommended(&self) -> u8 {
        CLEAN
            .iter()
            .copied()
            .min_by(|a, b| {
                let left = self.bin(*a).map_or(0.0, |bin| bin.pressure);
                let right = self.bin(*b).map_or(0.0, |bin| bin.pressure);

                left.partial_cmp(&right)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| a.cmp(b))
            })
            .unwrap_or(FIRST)
    }
}

#[cfg(test)]
mod tests;
