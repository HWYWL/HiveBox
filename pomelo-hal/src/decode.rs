//! MP3 and FLAC, decoded into the samples the codec takes.
//!
//! The board's own formats are the ones it can hand to the codec untouched: whatever comes out of a
//! WAV file is already PCM. MP3 and FLAC are not — they are compressed, and something has to turn
//! them back into samples before there is anything to write to the speaker. That is all this module
//! is: a thin, stateful adapter around a pure-Rust decoder, producing the same 16-bit interleaved
//! little-endian blocks that the WAV path produces.
//!
//! Why a decoder crate rather than a hardware block or a C library: the ESP32-S3 has neither an MP3
//! nor a FLAC engine, and the alternatives — Espressif's Helix port, `esp-adf`'s decoders — bind
//! the HAL to `esp-idf` and to a component manager, which would leave the desktop simulator, the one
//! place a track can be tried without flashing anything, unable to play the same files. This is
//! pure Rust, so the device and the simulator run the same decoder over the same bytes.
//!
//! Two things are deliberate and worth knowing:
//!
//! * **A damaged frame is skipped, not fatal.** Decoders are fed by network shares, cards pulled out
//!   mid-write and files that were never quite finished; a click is a better answer than silence for
//!   the rest of the track.
//! * **The output shape is decided once.** Whatever the file's channel count, the stream comes out
//!   as one or two channels of 16-bit samples, so the backend opens the codec once and never has to
//!   know what it is playing.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::probe::{AudioInfo, AudioKind};

/// An opened file: the reader, its decoder, and which track of it is being played.
struct Opened {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
}

/// Open a file and build a decoder for its default track.
fn open(path: &Path) -> Result<Opened, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    /* The hint is what the name says and nothing more: a wrong extension makes the probe try a
     * container that does not match, after which it falls back to looking at the bytes anyway. It
     * is passed because it is free, not because it is trusted. */
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }

    let format_options = FormatOptions {
        // Without this, a gapless album's tracks click into each other: the encoder's own padding
        // frames are played as silence.
        enable_gapless: true,
        ..Default::default()
    };

    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &format_options, &MetadataOptions::default())
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| format!("{}: nothing in the file is a playable track", path.display()))?;

    let track_id = track.id;
    let params = track.codec_params.clone();

    let decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("{}: {e}", path.display()))?;

    Ok(Opened {
        format,
        decoder,
        track_id,
    })
}

/// Read a FLAC or MP3 file's shape from its reader.
///
/// Called from [`crate::probe::probe`], which has already decided what the file is; the reader is
/// what turns "it is an MP3" into a sample rate, a length and a channel count. MP3 in particular has
/// no header that holds a duration — the length is either counted from the encoder's own Xing frame
/// or estimated from the bit rate and the file size, and the estimate is marked as such by being
/// zero when even that is not available.
pub fn probe_stream(path: &Path, kind: AudioKind) -> Result<AudioInfo, String> {
    let opened = open(path)?;

    let params = opened
        .format
        .default_track()
        .map(|track| track.codec_params.clone())
        .ok_or_else(|| format!("{}: nothing in the file is a playable track", path.display()))?;

    let sample_rate = params.sample_rate.ok_or_else(|| {
        format!(
            "{}: the {} header does not say its sample rate",
            path.display(),
            kind.label()
        )
    })?;

    let channels = params.channels.map(|c| c.count()).unwrap_or(2) as u16;

    // FLAC says how wide its samples are; MP3 does not have a width, so 16 is not a guess but the
    // width the decoder produces.
    let bits_per_sample = params.bits_per_sample.unwrap_or(16).min(u16::MAX as u32) as u16;

    let duration_secs = match params.n_frames {
        Some(frames) => frames as f32 / sample_rate as f32,
        None => estimate_duration(path),
    };

    Ok(AudioInfo {
        kind,
        sample_rate,
        channels,
        bits_per_sample,
        duration_secs,
    })
}

/// How long a file is when nothing in it says: its first frame's bit rate against its size.
///
/// Reached only by MP3 files with no encoder frame in them — the output of an older CBR encoder, or
/// anything that has been cut down — which is the case where a length is approximate by nature: a
/// file that is not really CBR gets the length its first frame implies.
fn estimate_duration(path: &Path) -> f32 {
    let Ok(metadata) = std::fs::metadata(path) else {
        return 0.0;
    };

    // Enough for the ID3 tag and the first frames of anything a music library holds.
    let mut head = vec![0u8; 64 * 1024];
    let read = match File::open(path).and_then(|mut file| file.read(&mut head)) {
        Ok(read) => read,
        Err(_) => return 0.0,
    };

    let Some((bit_rate, _rate)) = mp3_frame(&head[..read]) else {
        return 0.0;
    };

    (metadata.len() as f64 * 8.0 / bit_rate as f64) as f32
}

/// The bit rate and sample rate in an MPEG audio frame header, looked for in the file's first bytes.
///
/// There is no header in an MP3 that holds a duration, which is why this exists at all: a decoder
/// that has read the encoder's own frame knows the length, and one that has not is left with a
/// frame's bit rate and the size of the file on disk.
///
/// Only layer III is recognised — that is what `.mp3` means, and the layers above it are numbered
/// on different tables, where guessing would produce a length that is simply wrong.
fn mp3_frame(head: &[u8]) -> Option<(u32, u32)> {
    /// MPEG-1 layer III, kbps by index.
    const MPEG1_LAYER3: [u32; 16] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
    /// MPEG-2 and 2.5 layer III, kbps by index — half the rates of MPEG-1, halved again for 2.5.
    const MPEG2_LAYER3: [u32; 16] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0];

    let mut cursor = 0usize;

    // An ID3v2 tag sits in front of the frames, and its size is written in seven-bit bytes.
    if head.len() >= 10 && &head[0..3] == b"ID3" {
        let size = ((head[6] as usize & 0x7F) << 21)
            | ((head[7] as usize & 0x7F) << 14)
            | ((head[8] as usize & 0x7F) << 7)
            | (head[9] as usize & 0x7F);

        cursor = 10 + size;
    }

    while cursor + 4 <= head.len() {
        if head[cursor] == 0xFF && (head[cursor + 1] & 0xE0) == 0xE0 {
            let flags = head[cursor + 1];
            let fields = head[cursor + 2];

            let version = (flags >> 3) & 0x03;
            let layer = (flags >> 1) & 0x03;
            let bit_rate_index = (fields >> 4) & 0x0F;
            let rate_index = (fields >> 2) & 0x03;

            /* Version 1 and layer 0 are reserved values; a bit rate index of 0 means "free format"
             * and 15 is invalid; a rate index of 3 is invalid. Anything else is a frame header. */
            if version != 1
                && layer == 1
                && bit_rate_index != 0
                && bit_rate_index != 15
                && rate_index != 3
            {
                let table = if version == 3 { MPEG1_LAYER3 } else { MPEG2_LAYER3 };
                let bit_rate = table[bit_rate_index as usize] * 1000;

                let rate = match version {
                    3 => [44100u32, 48000, 32000],
                    2 => [22050, 24000, 16000],
                    _ => [11025, 12000, 8000],
                }[rate_index as usize];

                return Some((bit_rate, rate));
            }
        }

        cursor += 1;
    }

    None
}

/// A compressed file being decoded, block by block.
///
/// The backend drives this: open the codec from [`PcmStream::sample_rate`] and
/// [`PcmStream::channels`], then call [`PcmStream::read`] until it says the stream has ended. What
/// comes out is always 16-bit interleaved little-endian, one or two channels — the same shape the
/// WAV path produces, so nothing downstream has to ask what the source was.
pub struct PcmStream {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    /// The decoder's own output buffer, rebuilt only when the format changes.
    buffer: Option<SampleBuffer<i16>>,
    /// The spec `buffer` was built for, so a mid-stream format change is noticed.
    spec: Option<SignalSpec>,
    sample_rate: u32,
    /// What the file holds and what comes out: one channel stays one, anything else becomes two.
    source_channels: u8,
    output_channels: u8,
    /// Frames decoded so far — the position, counted rather than estimated, which is the only way a
    /// variable-bit-rate file can report one that does not drift.
    frames: u64,
    finished: bool,
}

impl PcmStream {
    /// Open a file for decoding, or say why it cannot be played.
    pub fn open(path: &Path) -> Result<Self, String> {
        let opened = open(path)?;

        let params = opened
            .format
            .default_track()
            .map(|track| track.codec_params.clone())
            .ok_or_else(|| format!("{}: nothing in the file is a playable track", path.display()))?;

        let sample_rate = params.sample_rate.ok_or_else(|| {
            format!("{}: the header does not say its sample rate", path.display())
        })?;

        let source_channels = params.channels.map(|c| c.count()).unwrap_or(2) as u8;

        Ok(Self {
            format: opened.format,
            decoder: opened.decoder,
            track_id: opened.track_id,
            buffer: None,
            spec: None,
            sample_rate,
            source_channels,
            output_channels: if source_channels == 1 { 1 } else { 2 },
            frames: 0,
            finished: false,
        })
    }

    /// The rate to open the codec with.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// How many channels the samples this produces have.
    pub fn channels(&self) -> u8 {
        self.output_channels
    }

    /// How many channels the file itself holds, which may be more than come out.
    pub fn source_channels(&self) -> u8 {
        self.source_channels
    }

    /// How many frames have been decoded, for a position readout.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// How far into the track playback has reached, in seconds.
    pub fn position_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        self.frames as f32 / self.sample_rate as f32
    }

    /// Whether the stream has ended or failed.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Decode the next block of packets into 16-bit samples, appended to `out`.
    ///
    /// Returns `false` when the file has ended. A frame that fails to decode is skipped rather than
    /// reported: one damaged packet in the middle of an album is a click, and stopping the track
    /// there would be a worse answer to the same problem.
    pub fn read(&mut self, out: &mut Vec<u8>) -> Result<bool, String> {
        if self.finished {
            return Ok(false);
        }

        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => {
                    self.finished = true;
                    return Ok(false);
                }
                // A chained stream (an MP3 followed by another, a concatenated FLAC): the decoder
                // would have to be rebuilt for the new one. Treated as the end of the track.
                Err(SymphoniaError::ResetRequired) => {
                    self.finished = true;
                    return Ok(false);
                }
                Err(err) => {
                    self.finished = true;
                    return Err(err.to_string());
                }
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let capacity = decoded.capacity() as u64;

                    if self.spec != Some(spec) {
                        self.buffer = Some(SampleBuffer::<i16>::new(capacity, spec));
                        self.spec = Some(spec);
                    }

                    let buffer = self
                        .buffer
                        .as_mut()
                        .expect("a buffer was just built for this spec");
                    buffer.copy_interleaved_ref(decoded);

                    let samples = buffer.samples();
                    let channels = spec.channels.count().max(1);
                    self.frames += (samples.len() / channels) as u64;

                    push_kept(samples, channels, self.output_channels as usize, out);

                    if !out.is_empty() {
                        return Ok(true);
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => {
                    self.finished = true;
                    return Ok(false);
                }
                Err(err) => {
                    self.finished = true;
                    return Err(err.to_string());
                }
            }
        }
    }
}

/// Append interleaved samples to `out`, keeping at most `kept` channels.
///
/// The channel count is not always reduced (a stereo file stays stereo), so the common case is a
/// straight copy; the trimmed case walks frames, because dropping the third channel of each frame
/// is not the same as dropping every third sample of the stream.
fn push_kept(samples: &[i16], channels: usize, kept: usize, out: &mut Vec<u8>) {
    if kept == 0 || samples.is_empty() {
        return;
    }

    if channels <= kept {
        out.reserve(samples.len() * 2);

        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }

        return;
    }

    out.reserve(samples.len() / channels * kept * 2);

    for frame in samples.chunks_exact(channels) {
        for sample in &frame[..kept] {
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_samples_are_copied_whole() {
        let mut out = Vec::new();
        push_kept(&[1, 2, 3, 4], 2, 2, &mut out);
        assert_eq!(out.len(), 8);
        assert_eq!(i16::from_le_bytes([out[0], out[1]]), 1);
        assert_eq!(i16::from_le_bytes([out[6], out[7]]), 4);
    }

    /// Six channels of a film mix: the first two of each frame, in order — not the first two of the
    /// stream, which would be one frame's left and right.
    #[test]
    fn extra_channels_are_dropped_per_frame() {
        let mut out = Vec::new();
        push_kept(&[10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 25], 6, 2, &mut out);

        let values: Vec<i16> = out
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        assert_eq!(values, vec![10, 11, 20, 21]);
    }

    #[test]
    fn a_mono_stream_is_left_as_one_channel() {
        let mut out = Vec::new();
        push_kept(&[7, 8], 1, 1, &mut out);
        assert_eq!(out.len(), 4);
    }

    /// An MPEG-1 layer III frame header says 128 kbps at 44.1 kHz.
    #[test]
    fn a_frame_header_gives_the_bit_rate_and_the_rate() {
        assert_eq!(mp3_frame(&[0xFF, 0xFB, 0x90, 0x00]), Some((128_000, 44_100)));

        // The same frame behind an ID3v2 tag, which is exactly where the frames are not.
        let mut tagged = b"ID3\x03\x00\x00\x00\x00\x00\x0A".to_vec();
        tagged.extend_from_slice(&[0u8; 10]);
        tagged.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        assert_eq!(tagged.len(), 24, "tag + body + frame");
        assert_eq!(&tagged[20..24], &[0xFF, 0xFB, 0x90, 0x00], "frame position");
        assert_eq!(mp3_frame(&tagged), Some((128_000, 44_100)));

        // MPEG-2 at 22.05 kHz is numbered on the other table: index 1 is 8 kbps, not 32.
        assert_eq!(mp3_frame(&[0xFF, 0xF3, 0x10, 0x00]), Some((8_000, 22_050)));

        // Noise, and a frame that is not layer III: no answer rather than a guessed one.
        assert_eq!(mp3_frame(&[0u8; 32]), None);
        assert_eq!(mp3_frame(&[0xFF, 0xFD, 0x90, 0x00]), None);
    }

    /// A file with no encoder frame in it is measured by its first frame and its size.
    #[test]
    fn a_duration_can_be_estimated_from_the_first_frame() {
        let path = std::env::temp_dir().join("pomelo-decode-duration-test.mp3");

        let mut bytes = vec![0xFF, 0xFB, 0x90, 0x00];
        bytes.resize(16_000, 0);
        std::fs::write(&path, &bytes).unwrap();

        // 128 kbps of 16 kB is exactly one second.
        let seconds = estimate_duration(&path);
        assert!((seconds - 1.0).abs() < 0.01, "{seconds}");

        // A file with no frame header at all: no length rather than a wrong one.
        std::fs::write(&path, vec![0u8; 4096]).unwrap();
        assert_eq!(estimate_duration(&path), 0.0);

        let _ = std::fs::remove_file(&path);
    }
}
