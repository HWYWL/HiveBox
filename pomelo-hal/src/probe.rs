//! What an audio file is, before anything plays it.
//!
//! Every backend that plays a track has to answer the same question first — how long is it, what
//! rate and how many channels — and every backend used to answer it by parsing a WAV header, which
//! is a parser that has nothing to say about a FLAC file and used to say "invalid RIFF/WAVE header"
//! about it instead. This module is that question asked once: look at the first bytes, decide what
//! the file is, and hand back its shape.
//!
//! Two kinds of answer live here:
//!
//! * **The header's own numbers**, read straight out of the file — WAV's `fmt ` chunk, FLAC's
//!   STREAMINFO, MP3's first frame header. Cheap, and enough to draw a list.
//! * **What only a decoder can tell**, for MP3 files whose length is not in a header at all. That
//!   part needs the `decode` feature, and a build without it refuses those files by name rather
//!   than pretending the rate is unknown.
//!
//! Note that the file is *identified* by its contents, not by its extension: a `.wav` that holds
//! ADIFF or a `.mp3` that is really a FLAC arrives here and leaves as what it is.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::types::AudioMeta;
use crate::wav::parse_wav_header;

/// The container a file turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioKind {
    /// RIFF/WAVE, uncompressed or float samples.
    Wav,
    /// MPEG audio layers I–III — `.mp3` and its relatives.
    Mp3,
    /// Native FLAC.
    Flac,
}

impl AudioKind {
    /// The extension this kind is usually stored under, for matching filenames.
    pub fn extension(self) -> &'static str {
        match self {
            AudioKind::Wav => "wav",
            AudioKind::Mp3 => "mp3",
            AudioKind::Flac => "flac",
        }
    }

    /// How the format is written in a list or a log.
    pub fn label(self) -> &'static str {
        match self {
            AudioKind::Wav => "WAV",
            AudioKind::Mp3 => "MP3",
            AudioKind::Flac => "FLAC",
        }
    }

    /// Every kind the board can play, for a caller that has to recognise one from a name.
    pub const ALL: [AudioKind; 3] = [AudioKind::Wav, AudioKind::Mp3, AudioKind::Flac];

    /// This kind, if `extension` names it (case-insensitively).
    pub fn from_extension(extension: &str) -> Option<Self> {
        AudioKind::ALL
            .into_iter()
            .find(|kind| extension.eq_ignore_ascii_case(kind.extension()))
    }

    /// Whether a build without the `decode` feature can play this.
    pub fn needs_decoder(self) -> bool {
        !matches!(self, AudioKind::Wav)
    }
}

/// A track's shape, as told by its own header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioInfo {
    pub kind: AudioKind,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub duration_secs: f32,
}

impl AudioInfo {
    /// The same thing in the shape the audio backends take it in.
    pub fn meta(&self) -> AudioMeta {
        AudioMeta {
            sample_rate: self.sample_rate,
            channels: self.channels.min(u8::MAX as u16) as u8,
            bits_per_sample: self.bits_per_sample.min(u8::MAX as u16) as u8,
            duration_secs: self.duration_secs,
        }
    }
}

/// What the first bytes of a file say it is, or `None` if they are not audio.
///
/// Read from the contents rather than the name: extensions are a hint a person types, and the one
/// thing that reliably differs between a file that plays and a file that does not.
pub fn sniff(head: &[u8]) -> Option<AudioKind> {
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return Some(AudioKind::Wav);
    }

    if head.len() >= 4 && &head[0..4] == b"fLaC" {
        return Some(AudioKind::Flac);
    }

    // MPEG audio: either a tag in front of the frames, or a frame sync itself.
    if head.len() >= 3 && &head[0..3] == b"ID3" {
        return Some(AudioKind::Mp3);
    }

    if head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0 {
        return Some(AudioKind::Mp3);
    }

    None
}

/// Read a file's shape: what it is, how fast, how many channels, how long.
///
/// A failure here is a file this board cannot play, and the message says which of the reasons it
/// was — not a WAV at all, a WAV holding a codec the codec cannot take, or a compressed format this
/// build has no decoder for.
pub fn probe(path: &Path) -> Result<AudioInfo, String> {
    let mut head = [0u8; 512];
    let read = {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        file.read(&mut head).map_err(|e| format!("{}: {e}", path.display()))?
    };

    let kind = sniff(&head[..read]).ok_or_else(|| {
        format!(
            "{} is not audio this board knows — the first bytes are neither RIFF/WAVE, FLAC nor MPEG",
            path.display()
        )
    })?;

    match kind {
        AudioKind::Wav => {
            let wav = parse_wav_header(&head[..read])?;

            Ok(AudioInfo {
                kind,
                sample_rate: wav.sample_rate,
                channels: wav.channels,
                bits_per_sample: wav.bits_per_sample,
                duration_secs: wav.duration_secs,
            })
        }
        // FLAC and MP3 both need a reader, so both go through the decoder crate. A build without it
        // still knows *what* the file is, which is what lets the message name it.
        _ => probe_stream(path, kind),
    }
}

/// The header-derived answer for FLAC, and the decoder-derived one for MP3.
///
/// Split out so that the fallback for a build without the `decode` feature is one small function
/// rather than a conditional in the middle of `probe`.
#[cfg(feature = "decode")]
fn probe_stream(path: &Path, kind: AudioKind) -> Result<AudioInfo, String> {
    crate::decode::probe_stream(path, kind)
}

#[cfg(not(feature = "decode"))]
fn probe_stream(path: &Path, kind: AudioKind) -> Result<AudioInfo, String> {
    Err(format!(
        "{} is {}, which this build has no decoder for — rebuild with the `decode` feature",
        path.display(),
        kind.label()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_identified_by_its_bytes() {
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), Some(AudioKind::Wav));
        assert_eq!(sniff(b"fLaC\0\0\0\x22"), Some(AudioKind::Flac));
        assert_eq!(sniff(b"ID3\x04\0\0"), Some(AudioKind::Mp3));
        assert_eq!(sniff(&[0xFF, 0xFB, 0x90, 0x00]), Some(AudioKind::Mp3));

        // A text file, and a RIFF that is not WAVE (an AVI, say).
        assert_eq!(sniff(b"hello world\n"), None);
        assert_eq!(sniff(b"RIFF\0\0\0\0AVI "), None);
        assert_eq!(sniff(b"fL"), None);
    }

    #[test]
    fn an_extension_names_the_kinds_that_have_one() {
        assert_eq!(AudioKind::from_extension("MP3"), Some(AudioKind::Mp3));
        assert_eq!(AudioKind::from_extension("flac"), Some(AudioKind::Flac));
        assert_eq!(AudioKind::from_extension("Wav"), Some(AudioKind::Wav));
        assert_eq!(AudioKind::from_extension("ogg"), None);
        assert_eq!(AudioKind::from_extension("txt"), None);
    }

    #[test]
    fn only_wav_plays_without_a_decoder() {
        assert!(!AudioKind::Wav.needs_decoder());
        assert!(AudioKind::Mp3.needs_decoder());
        assert!(AudioKind::Flac.needs_decoder());
    }
}
