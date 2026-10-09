//! Zero-dependency RIFF WAV parser, shared by the native and simulator audio
//! backends (and re-exported for applications that want to display metadata).
//!
//! A WAV file is a container, not a format: the `fmt ` chunk says what the samples are, and the
//! `data` chunk holds them as they were written. Almost everything that has ever gone wrong with
//! playback on this board came from ignoring that first part — a file that says "ADPCM" or "µ-law"
//! is not a stream of signed 16-bit samples, and sending it to the codec as one produces noise
//! rather than an error. So the format tag is read, checked, and carried out of here on the
//! metadata, and a file in a shape that cannot be played is refused with the name of the shape.

use crate::pcm::PcmSource;

/// Parsed metadata for a RIFF/WAVE stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavMetadata {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub byte_rate: u32,
    /// The `fmt ` format tag: 1 for PCM, 3 for IEEE float, 0xFFFE for the extensible form of either.
    /// Kept because the samples' meaning depends on it, not for display.
    pub audio_format: u16,
    pub float: bool,
    pub data_offset: usize,
    pub data_len: usize,
    pub duration_secs: f32,
}

impl WavMetadata {
    pub fn format_duration(&self) -> String {
        format_time(self.duration_secs)
    }

    /// The shape of the samples in the `data` chunk, for the converter that narrows them to what
    /// the codec takes.
    pub fn source(&self) -> PcmSource {
        PcmSource {
            bits_per_sample: self.bits_per_sample,
            channels: self.channels,
            float: self.float,
        }
    }
}

/// Format a duration in seconds as `MM:SS`.
pub fn format_time(seconds: f32) -> String {
    let total_secs = seconds.max(0.0) as u32;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

/// The name of a `fmt ` format tag, for error messages that say what the file actually is.
fn format_name(tag: u16) -> &'static str {
    match tag {
        0x0002 => "ADPCM",
        0x0006 => "A-law",
        0x0007 => "µ-law",
        0x0011 => "IMA ADPCM",
        0x0031 | 0x0055 => "MPEG audio",
        0x0161 | 0x0162 | 0x0163 => "Windows Media audio",
        0x2000 => "AC-3",
        0x674F | 0x6771 => "Vorbis",
        _ => "an unrecognised codec",
    }
}

/// Parse the header of a RIFF/WAVE buffer, returning its format metadata.
///
/// The buffer need only reach past the `data` chunk's own eight bytes; the samples themselves are
/// not looked at, which is what lets a 512-byte read of a 40 MB file say how long it is.
pub fn parse_wav_header(bytes: &[u8]) -> Result<WavMetadata, String> {
    if bytes.len() < 44 {
        return Err("File too small to be a valid WAV (< 44 bytes)".to_string());
    }

    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Invalid RIFF/WAVE header".to_string());
    }

    let mut cursor = 12;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut byte_rate = 0u32;
    let mut audio_format = 0u16;
    let mut float = false;
    let mut data_offset = 0usize;
    let mut data_len = 0usize;
    let mut found_fmt = false;
    let mut found_data = false;

    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;

        if chunk_id == b"fmt " {
            if chunk_size < 14 || cursor + chunk_size > bytes.len() {
                return Err("Malformed fmt chunk in WAV".to_string());
            }

            audio_format = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
            channels = u16::from_le_bytes(bytes[cursor + 2..cursor + 4].try_into().unwrap());
            sample_rate = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap());
            byte_rate = u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap());
            let block_align = u16::from_le_bytes(bytes[cursor + 12..cursor + 14].try_into().unwrap());

            if chunk_size >= 16 {
                bits_per_sample =
                    u16::from_le_bytes(bytes[cursor + 14..cursor + 16].try_into().unwrap());
            }

            /* The extensible form puts the real tag in the first two bytes of the SubFormat GUID,
             * which sits 24 bytes into the chunk. Reachable only when the chunk is long enough to
             * hold it — a truncated header falls back to reading it as plain PCM below. */
            if audio_format == 0xFFFE && chunk_size >= 40 {
                audio_format = u16::from_le_bytes(bytes[cursor + 24..cursor + 26].try_into().unwrap());
            }

            match audio_format {
                1 => float = false,
                3 => float = true,
                other => {
                    return Err(format!(
                        "WAV holds {} audio (format tag {}), which the board cannot play — this is a \
                         compressed WAV, not PCM",
                        format_name(other),
                        other
                    ));
                }
            }

            if channels == 0 {
                return Err("WAV declares zero channels".to_string());
            }

            /* A `fmt ` chunk of the original 14-byte size carries no width, and some encoders leave
             * it out entirely. The block alignment still says how big a frame is, so the width is
             * taken from there rather than assumed to be 16. */
            if bits_per_sample == 0 {
                if block_align > 0 && block_align % channels == 0 {
                    bits_per_sample = (block_align / channels) * 8;
                } else {
                    return Err("WAV does not say how wide its samples are".to_string());
                }
            }

            found_fmt = true;
            cursor += chunk_size;
        } else if chunk_id == b"data" {
            data_offset = cursor;
            data_len = chunk_size;
            found_data = true;
            break; // Typically data is the main payload
        } else {
            // Skip unknown chunk (e.g. LIST, JUNK, ID3)
            cursor += chunk_size;
        }

        /* RIFF pads every chunk to an even length, and the pad byte is not counted in the size.
         * Without this, a LIST chunk of odd length shifts every chunk after it by one byte — the
         * reason some files' headers read as garbage. */
        if chunk_size % 2 == 1 {
            cursor += 1;
        }
    }

    if !found_fmt {
        return Err("Missing 'fmt ' chunk in WAV file".to_string());
    }

    if !found_data {
        return Err("Missing 'data' chunk in WAV file".to_string());
    }

    if byte_rate == 0 {
        byte_rate = sample_rate * (channels as u32) * ((bits_per_sample as u32) / 8);
    }

    let duration_secs = if byte_rate > 0 {
        data_len as f32 / byte_rate as f32
    } else {
        0.0
    };

    Ok(WavMetadata {
        channels,
        sample_rate,
        bits_per_sample,
        byte_rate,
        audio_format,
        float,
        data_offset,
        data_len,
        duration_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV header of the shape an encoder writes: fmt, an odd-length LIST to be padded, then data.
    fn header(format: u16, channels: u16, rate: u32, bits: u16, data_len: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36u32 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");

        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&format.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * channels as u32 * (bits as u32 / 8)).to_le_bytes());
        bytes.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());

        // An odd-sized LIST with its pad byte: the chunk that follows must still be found.
        bytes.extend_from_slice(b"LIST");
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(b"INFOx");
        bytes.push(0);

        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_a_pcm_header_and_walks_past_an_odd_chunk() {
        let meta = parse_wav_header(&header(1, 2, 44100, 16, 44100 * 4)).unwrap();
        assert_eq!(meta.channels, 2);
        assert_eq!(meta.sample_rate, 44100);
        assert_eq!(meta.bits_per_sample, 16);
        assert!(!meta.float);
        assert_eq!(meta.audio_format, 1);
        assert_eq!(meta.source().output_channels(), 2);
        assert!((meta.duration_secs - 1.0).abs() < 0.001, "{}", meta.duration_secs);
        assert_eq!(&meta.format_duration(), "00:01");
    }

    /// The whole point of the strict pass: a compressed WAV is refused by name instead of played.
    #[test]
    fn a_compressed_wav_is_refused_with_its_own_name() {
        let error = parse_wav_header(&header(0x0011, 1, 22050, 4, 1000)).unwrap_err();
        assert!(error.contains("IMA ADPCM"), "{error}");
        assert!(error.contains("format tag 17"), "{error}");

        assert!(parse_wav_header(&header(7, 1, 8000, 8, 8000)).unwrap_err().contains("µ-law"));
        assert!(parse_wav_header(&header(0x55, 2, 44100, 0, 4096)).unwrap_err().contains("MPEG audio"));
    }

    /// Extensible is a wrapper: what matters is the codec inside it.
    #[test]
    fn extensible_headers_are_read_through_to_their_subformat() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&0xFFFEu16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&(48000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        bytes.extend_from_slice(&24u16.to_le_bytes()); // valid bits
        bytes.extend_from_slice(&3u32.to_le_bytes()); // channel mask
        bytes.extend_from_slice(&3u16.to_le_bytes()); // SubFormat: IEEE float
        bytes.extend_from_slice(&[0u8; 14]);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4096u32.to_le_bytes());

        let meta = parse_wav_header(&bytes).unwrap();
        assert_eq!(meta.audio_format, 3);
        assert!(meta.float);
        assert_eq!(meta.bits_per_sample, 24);
    }

    #[test]
    fn a_header_without_a_width_falls_back_to_the_block_alignment() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&14u32.to_le_bytes()); // the original, width-less size
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&44100u32.to_le_bytes());
        bytes.extend_from_slice(&176400u32.to_le_bytes());
        bytes.extend_from_slice(&6u16.to_le_bytes()); // block align: 2 channels of 24-bit
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 4]); // past the minimum header length

        let meta = parse_wav_header(&bytes).unwrap();
        assert_eq!(meta.bits_per_sample, 24);
    }

    #[test]
    fn a_file_that_is_not_a_wav_is_refused() {
        let mut bytes = vec![0u8; 64];
        bytes[0..4].copy_from_slice(b"fLaC");
        assert!(parse_wav_header(&bytes).unwrap_err().contains("RIFF"));

        assert!(parse_wav_header(&[0u8; 20]).unwrap_err().contains("too small"));
    }
}
