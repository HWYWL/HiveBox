//! Turning whatever a file holds into the one shape the codec takes.
//!
//! The board's speaker is one device with one opinion: it is handed a sample rate, a channel count
//! and a width, and everything written after that has to match. Files, on the other hand, are
//! whatever whoever made them felt like — 8 bits or 24, unsigned or float, six channels of a film
//! mix — and between the two sits this: a converter that is told the source's shape once and then
//! turns each block into signed 16-bit little-endian samples with one or two channels.
//!
//! Three decisions are worth stating, because they are the cases that used to go wrong by being
//! passed through instead:
//!
//! * **8-bit is unsigned.** RIFF says so and nothing else does, so the offset is applied here
//!   rather than left to the codec, which would hear it as a half-rectified signal.
//! * **Wider is not better.** 24- and 32-bit samples, integers or floats, are narrowed to 16 bits:
//!   the codec wants 16, and a file that says 24 otherwise arrives at it as noise.
//! * **More than two channels is two.** The first two are kept and the rest dropped — the board has
//!   one speaker, and a film's centre channel is not a thing it can play anyway.
//!
//! The stream is cut into blocks by whoever reads the file, so a block boundary can land inside a
//! sample or inside a frame. What does not complete is carried to the next call rather than dropped:
//! dropping the tail of every 4 KB block is a click every 4 KB.

/// The shape a file declares, before anything is converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmSource {
    pub bits_per_sample: u16,
    pub channels: u16,
    /// IEEE float samples rather than integers. Only 32-bit in practice.
    pub float: bool,
}

impl PcmSource {
    /// Whether the samples are in a shape this converter knows how to read.
    ///
    /// Stated rather than assumed: a decoder that guessed at an unknown width would produce sounds
    /// that are not wrong loudly but wrong quietly, which is the kind nobody reports.
    pub fn is_supported(&self) -> bool {
        matches!(self.bits_per_sample, 8 | 16 | 24 | 32) && self.channels > 0
    }

    /// How many output channels a source of this shape produces: one stays one, anything else
    /// becomes two.
    pub fn output_channels(&self) -> u8 {
        if self.channels == 1 {
            1
        } else {
            2
        }
    }
}

/// Converts blocks of a stream into 16-bit samples for the codec.
///
/// Stateful only for the incomplete tail of the previous block (see the note at the top), which is
/// why a converter belongs to one playback and not to the backend.
pub struct PcmNormalizer {
    source: PcmSource,
    /// Bytes of one input frame, all channels at the source's width.
    frame_bytes: usize,
    out_channels: u8,
    /// Bytes that did not complete a frame, kept for the next call.
    leftover: Vec<u8>,
}

impl PcmNormalizer {
    /// A converter for a source of this shape, or `None` if the shape is not one it can read.
    pub fn new(source: PcmSource) -> Option<Self> {
        if !source.is_supported() {
            return None;
        }

        let frame_bytes = (source.channels as usize) * (source.bits_per_sample as usize) / 8;

        Some(Self {
            source,
            frame_bytes,
            out_channels: source.output_channels(),
            leftover: Vec::new(),
        })
    }

    /// The number of channels the codec is to be opened with.
    pub fn output_channels(&self) -> u8 {
        self.out_channels
    }

    /// Converts as much of `input` as completes whole frames, appending the result to `out`.
    ///
    /// `out` is appended to rather than returned because the caller sends it and reuses the buffer;
    /// a fresh `Vec` per 4 KB block is a page of heap churn per second of music.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) {
        let mut joined;
        let bytes: &[u8] = if self.leftover.is_empty() {
            input
        } else {
            joined = std::mem::take(&mut self.leftover);
            joined.extend_from_slice(input);
            &joined
        };

        let complete = bytes.len() / self.frame_bytes;
        let used = complete * self.frame_bytes;

        let kept = self.out_channels as usize;
        out.reserve(complete * kept * 2);

        for frame in bytes[..used].chunks_exact(self.frame_bytes) {
            for sample in frame.chunks_exact(self.source.bits_per_sample as usize / 8).take(kept) {
                let value = self.sample(sample);
                out.extend_from_slice(&value.to_le_bytes());
            }
        }

        self.leftover.clear();
        self.leftover.extend_from_slice(&bytes[used..]);

        // A frame longer than a block can never complete; keeping it would grow forever. Two
        // channels of 32-bit is eight bytes, so this is a limit no real file reaches.
        if self.leftover.len() > self.frame_bytes {
            self.leftover.clear();
        }
    }

    /// The tail that never completed a frame, for the end of a stream.
    pub fn leftover_len(&self) -> usize {
        self.leftover.len()
    }

    /// One sample of the source's shape, as the 16-bit value the codec takes.
    fn sample(&self, bytes: &[u8]) -> i16 {
        if self.source.float {
            let value = match bytes.len() {
                4 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                8 => f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]) as f32,
                _ => 0.0,
            };

            /* Float audio lives in -1.0..=1.0; anything past it is clipped rather than wrapped, which
             * is what a limiter does and what a cast to i16 would not. */
            let scaled = (value.clamp(-1.0, 1.0) * 32767.0).round();

            return scaled as i16;
        }

        match bytes.len() {
            // Unsigned, centred on 128 — the one width RIFF stores unsigned.
            1 => ((bytes[0] as i16) - 128) << 8,
            2 => i16::from_le_bytes([bytes[0], bytes[1]]),
            // Narrowed by the high bits, with the low byte's top bit as a round-half-up. Widened to
            // 64 bits for the rounding so that a sample at full scale does not wrap into the
            // opposite sign on its way out.
            3 => {
                let raw = ((bytes[2] as i32) << 16) | ((bytes[1] as i32) << 8) | bytes[0] as i32;
                let signed = (raw << 8) >> 8;
                ((i64::from(signed) + 0x80) >> 8).clamp(-32768, 32767) as i16
            }
            4 => {
                let raw = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                ((i64::from(raw) + 0x8000) >> 16).clamp(-32768, 32767) as i16
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(source: PcmSource, input: &[u8]) -> Vec<u8> {
        let mut normalizer = PcmNormalizer::new(source).expect("a supported source");
        let mut out = Vec::new();
        normalizer.push(input, &mut out);
        out
    }

    fn samples(bytes: &[u8]) -> Vec<i16> {
        bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
    }

    /// The one width RIFF stores unsigned: silence is 128, not 0.
    #[test]
    fn eight_bit_audio_is_unsigned_and_centred() {
        let out = convert(PcmSource { bits_per_sample: 8, channels: 1, float: false }, &[128, 255, 0]);
        assert_eq!(samples(&out), vec![0, 32512, -32768]);
    }

    /// 24-bit is narrowed by its high bits, and both ends of the range survive the trip.
    #[test]
    fn twenty_four_bit_audio_is_narrowed_to_sixteen() {
        let out = convert(
            PcmSource { bits_per_sample: 24, channels: 1, float: false },
            &[0x00, 0x00, 0x00, 0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x80],
        );
        assert_eq!(samples(&out), vec![0, 32767, -32768]);
    }

    #[test]
    fn thirty_two_bit_floats_are_scaled_and_clipped() {
        let out = convert(
            PcmSource { bits_per_sample: 32, channels: 1, float: true },
            &[
                0.0f32.to_le_bytes(),
                1.0f32.to_le_bytes(),
                (-1.0f32).to_le_bytes(),
                4.0f32.to_le_bytes(),
            ]
            .concat(),
        );
        assert_eq!(samples(&out), vec![0, 32767, -32767, 32767]);
    }

    /// A block boundary in the middle of a frame must not swallow the rest of it.
    #[test]
    fn a_frame_split_across_blocks_is_rejoined() {
        let source = PcmSource { bits_per_sample: 16, channels: 2, float: false };
        let mut normalizer = PcmNormalizer::new(source).unwrap();

        let mut out = Vec::new();
        normalizer.push(&[0x01, 0x00, 0x02], &mut out); // half of the second sample
        assert!(out.is_empty(), "an incomplete frame is not a sample yet");

        normalizer.push(&[0x00, 0x03, 0x00, 0x04, 0x00], &mut out);
        assert_eq!(samples(&out), vec![1, 2, 3, 4]);
        assert_eq!(normalizer.leftover_len(), 0);
    }

    /// Six channels of a film mix become the two the board can play, in order.
    #[test]
    fn more_than_two_channels_keep_the_first_two() {
        let source = PcmSource { bits_per_sample: 16, channels: 6, float: false };
        let mut input = Vec::new();
        for value in 1..=6i16 {
            input.extend_from_slice(&value.to_le_bytes());
        }

        let out = convert(source, &input);
        assert_eq!(samples(&out), vec![1, 2]);
    }

    /// Mono stays mono: the codec is opened with one channel, not with two copies.
    #[test]
    fn a_mono_source_stays_mono() {
        let source = PcmSource { bits_per_sample: 16, channels: 1, float: false };
        assert_eq!(source.output_channels(), 1);

        let normalizer = PcmNormalizer::new(source).unwrap();
        assert_eq!(normalizer.output_channels(), 1);
    }

    /// A width the converter has no rule for is refused rather than guessed at.
    #[test]
    fn an_unknown_width_is_not_supported() {
        for bits in [12u16, 20, 64, 0] {
            let source = PcmSource { bits_per_sample: bits, channels: 2, float: false };
            assert!(!source.is_supported(), "{bits} bits");
            assert!(PcmNormalizer::new(source).is_none(), "{bits} bits");
        }
    }
}
