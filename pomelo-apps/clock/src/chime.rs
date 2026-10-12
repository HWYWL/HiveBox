//! The sound this app makes, synthesised rather than shipped.
//!
//! # Why there is no `chime.wav` in the repository
//!
//! A file would be a second thing to keep in step with the code that decides when to play it, and it
//! would have to be *put* on the board — the assets this project ships are baked into the image or
//! embedded in the firmware, and a sound is neither. Half a kilobyte of sine wave is cheaper to
//! compute than to find: the app writes the bytes once, into a directory of its own, on the first
//! alarm it ever raises, and plays that file from then on.
//!
//! # What the sound is
//!
//! A struck bell, near enough: two partials an octave and a fifth apart, a decay with a time constant
//! of a quarter of a second, and a five-millisecond attack — which is not decoration, because a wave
//! that starts at full amplitude starts with a *click*, and the click is the part that sounds cheap.

/// The rate the chime is written at.
///
/// Sixteen kilohertz rather than the forty-four the desktop would use: this is a bell, whose highest
/// partial is under two kilohertz, and the smaller number halves both the bytes written to the board's
/// flash and the work the codec does per second.
pub const SAMPLE_RATE: u32 = 16_000;

/// How long the chime is.
pub const SECONDS: f32 = 0.9;

/// The low partial, and the one a fifth above it.
const FUNDAMENTAL_HZ: f32 = 880.0;
const FIFTH_HZ: f32 = 1_320.0;

/// How much of the second partial is in the mix.
const FIFTH_MIX: f32 = 0.4;

/// The decay's time constant, and the attack's length, in seconds.
const DECAY_SECS: f32 = 0.26;
const ATTACK_SECS: f32 = 0.005;

/// How loud the chime is written, as a fraction of full scale.
///
/// A third, which leaves the *page* the rest: this plays over whatever the board was already doing,
/// and a bell at full scale is a bell that clips the moment anything else is in the mix with it.
const PEAK: f32 = 0.33;

/// The chime as 16-bit mono PCM samples.
///
/// A pure function of nothing, which is what makes it testable: two calls are the same samples, and
/// the shape of the envelope is an assertion rather than a matter of taste.
pub fn samples() -> Vec<i16> {
    let count = (SAMPLE_RATE as f32 * SECONDS).round() as usize;
    let full_scale = i16::MAX as f32;
    let mut out = Vec::with_capacity(count);

    for index in 0..count {
        let t = index as f32 / SAMPLE_RATE as f32;

        let decay = (-t / DECAY_SECS).exp();
        // Ramps *up* to one over the first few milliseconds and stays there; the `.min(1.0)` is the
        // "stays there".
        let attack = (t / ATTACK_SECS).min(1.0);

        let fundamental = (std::f32::consts::TAU * FUNDAMENTAL_HZ * t).sin();
        let fifth = (std::f32::consts::TAU * FIFTH_HZ * t).sin() * FIFTH_MIX;

        let value = (fundamental + fifth) * decay * attack * PEAK * full_scale;

        out.push(value.clamp(-full_scale, full_scale) as i16);
    }

    out
}

/// The chime as a complete WAV file: the canonical 44-byte header, then the samples little-endian.
///
/// Written out rather than pulled from a crate, for the same reason the calendar in [`crate::time`]
/// is: this is a header of thirteen fields, every one of which a player checks, and the two that a
/// player is most likely to refuse are the ones a hand-written file gets wrong — the two lengths in
/// it, which must agree with each other and with what follows.
pub fn wav() -> Vec<u8> {
    let samples = samples();
    let data_bytes = (samples.len() * 2) as u32;

    let block_align = 2u16; // one channel, sixteen bits
    let byte_rate = SAMPLE_RATE * block_align as u32;

    let mut out = Vec::with_capacity(44 + data_bytes as usize);

    // "RIFF", the size of everything after this field, and "WAVE".
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // "fmt ", its length, then the format: PCM, mono, this rate, this byte rate, this block alignment,
    // sixteen bits a sample.
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());

    // "data" and how much of it there is.
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());

    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header, field by field, including the two lengths that have to agree.
    #[test]
    fn the_header_says_what_the_bytes_are() {
        let file = wav();

        assert_eq!(&file[0..4], b"RIFF");
        assert_eq!(&file[4..8], &(36 + file.len() as u32 - 44).to_le_bytes());
        assert_eq!(&file[8..12], b"WAVE");
        assert_eq!(&file[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(file[16..20].try_into().unwrap()), 16);

        assert_eq!(u16::from_le_bytes(file[20..22].try_into().unwrap()), 1, "PCM");
        assert_eq!(u16::from_le_bytes(file[22..24].try_into().unwrap()), 1, "mono");
        assert_eq!(
            u32::from_le_bytes(file[24..28].try_into().unwrap()),
            SAMPLE_RATE
        );
        assert_eq!(
            u32::from_le_bytes(file[28..32].try_into().unwrap()),
            SAMPLE_RATE * 2,
            "byte rate: samples are two bytes each"
        );
        assert_eq!(u16::from_le_bytes(file[32..34].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(file[34..36].try_into().unwrap()), 16);

        assert_eq!(&file[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(file[40..44].try_into().unwrap()),
            file.len() as u32 - 44
        );
    }

    /// The file is the header and the samples and nothing else — the property a player discovers as
    /// "the sound stopped early" rather than as an error.
    #[test]
    fn the_file_is_a_header_and_the_samples() {
        let file = wav();
        let samples = samples();

        assert_eq!(file.len(), 44 + samples.len() * 2);
        assert_eq!(samples.len(), (SAMPLE_RATE as f32 * SECONDS).round() as usize);

        // The first sample is written little-endian at the end of the header.
        let first = i16::from_le_bytes(file[44..46].try_into().unwrap());
        assert_eq!(first, samples[0]);
    }

    /// A bell: it starts quiet, it rings, and it has rung itself out by the end.
    #[test]
    fn the_shape_is_a_struck_bell() {
        let samples = samples();

        let first = samples[0].unsigned_abs();
        let loudest = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();

        assert!(first < 64, "the attack starts at nothing: {first}");

        let tenth = samples.len() / 10;
        let head = rms(&samples[..tenth]);
        let tail = rms(&samples[samples.len() - tenth..]);

        assert!(
            tail < head / 4.0,
            "the tail ({tail}) should be well under the head ({head})"
        );

        // And it is audible rather than a rounding error: a third of full scale, give or take the
        // partials lining up.
        assert!(
            (8_000..=i16::MAX as u16).contains(&loudest),
            "the peak is {loudest}"
        );
    }

    /// The peak is the sum of two partials, so a mix that made the second one too loud would clip
    /// rather than sound brighter. Clipping is measurable, and this is where it would show.
    #[test]
    fn nothing_clips() {
        let samples = samples();

        let clipped = samples
            .iter()
            .filter(|sample| sample.unsigned_abs() >= i16::MAX as u16)
            .count();

        assert_eq!(clipped, 0, "samples at full scale are samples that clipped");
    }

    /// Synthesis is a pure function, so the file the app writes is the same file every time — which is
    /// what makes "write it once" a decision rather than a hope.
    #[test]
    fn the_same_sound_every_time() {
        assert_eq!(wav(), wav());
    }

    /// The root mean square of a run of samples, as a loudness an assertion can compare.
    fn rms(samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }

        let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();

        (sum / samples.len() as f64).sqrt() as f32
    }
}
