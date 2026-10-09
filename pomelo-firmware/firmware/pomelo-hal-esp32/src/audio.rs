//! Audio backend (ES8311 / I2S audio sink).
//!
//! Streams PCM audio into the C hardware sink (`hal_audio_write`) on a dedicated worker thread.
//! Whatever the file holds — a WAV's raw samples, an MP3's or FLAC's compressed frames — what
//! reaches the codec is the same thing: signed 16-bit little-endian samples, one or two channels,
//! at the file's own rate. The two ways of getting there are:
//!
//! * **A WAV file** whose samples are already PCM. The only work is narrowing them, which is the
//!   `PcmNormalizer`'s job — and it is real work: an 8-bit or 24-bit or six-channel file used to be
//!   handed to the codec as it was, which is the difference between music and noise.
//! * **An MP3 or FLAC file**, decoded on the way out by `PcmStream`. The decoder is fed a block at a
//!   time rather than read into memory, so a 40 MB FLAC costs the same as a 4 MB one.
//!
//! The two are driven through one interface (`TrackSource`) so that the worker loop — the pause,
//! the stop, the drain, the frame counter — exists once instead of once per format.
//!
//! # The volume
//!
//! `hal_audio_set_volume` is the codec's gain, which makes it the board's and not a page's: this
//! backend reads it back when the board comes up ([`AudioBackend::init`], from
//! `/internal/AppData/MUSIC/music.conf`) and writes it down again on every change, so a board that
//! was turned down stays turned down across a reboot and across a reflash. The file, its format and
//! the reason the volume is in one at all are `pomelo_hal::music_settings`.

use std::ffi::{c_char, CString};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use pomelo_hal::app_data::BOARD_APP_DATA;
use pomelo_hal::decode::PcmStream;
use pomelo_hal::music_settings::{self, MusicSettings};
use pomelo_hal::pcm::PcmNormalizer;
use pomelo_hal::probe::{probe, AudioInfo, AudioKind};
use pomelo_hal::wav::parse_wav_header;
use pomelo_hal::{AudioBackend, AudioMeta, HalError};

mod ffi {
    use std::ffi::c_char;

    extern "C" {
        pub fn hal_audio_init() -> i32;
        pub fn hal_audio_open(sample_rate: u32, channels: u8, bits_per_sample: u8) -> i32;
        pub fn hal_audio_write(data: *const u8, len: u32) -> i32;
        pub fn hal_audio_drain() -> i32;
        pub fn hal_audio_set_volume(volume: u8) -> i32;
        pub fn hal_audio_close() -> i32;

        /// The settings file, on the built-in partition, through the C `stdio` shim — the same pair
        /// `wifi.rs` reads its credentials with. It is not `std::fs`, and the note above that file's
        /// `read_credentials` says why at length: on this board `std::fs` can read a file and cannot
        /// create one, so a volume saved through it would be logged and then lost.
        pub fn hal_storage_read_file(
            path: *const c_char,
            out: *mut c_char,
            capacity: usize,
            out_len: *mut usize,
        ) -> i32;
        pub fn hal_storage_write_file(path: *const c_char, data: *const c_char, len: usize) -> i32;
    }
}

/// The rates the ES8311 takes. Checked here rather than discovered from a failed `open`: a file at
/// 4 kHz would otherwise be reported as "the codec said no", which says nothing about the file.
const CODEC_MIN_RATE: u32 = 8_000;
const CODEC_MAX_RATE: u32 = 96_000;

/// What the codec is always opened with, now that everything is converted to it.
const CODEC_BITS: u8 = 16;

/// The largest settings file this backend will read. One percentage and the table it sits in: the
/// bound is what lets a file too big to be one of these be an error rather than a truncated number.
const SETTINGS_CAPACITY: usize = 256;

/// `ESP_ERR_NOT_FOUND`: the C side's "there is no such file" — the ordinary answer from a board whose
/// volume has never been moved, and not a fault. The same code `wifi.rs` reads its credentials
/// through, spelled out here for the same reason it is spelled out there.
const ESP_ERR_NOT_FOUND: i32 = 0x105;

/// The path as a C string. A NUL inside a path is impossible here and would be a bad argument.
fn c_path(path: &Path) -> Result<CString, HalError> {
    CString::new(path.to_string_lossy().as_ref()).map_err(|_| HalError::InvalidArg)
}

/// Read the settings file under `root`, if there is one.
///
/// `Ok(None)` is a board whose volume has never been moved; an `Err` is a file that is *there* and
/// wrong, which is a different problem and must not be answered as the first — the same split
/// `wifi.rs` makes, so that "it forgot my volume" and "there was nothing to remember" stay two
/// different lines in the log.
fn read_settings(root: &str) -> Result<Option<MusicSettings>, HalError> {
    let path = c_path(&music_settings::path(root))?;

    let mut buffer = vec![0u8; SETTINGS_CAPACITY];
    let mut len = 0usize;

    let code = unsafe {
        ffi::hal_storage_read_file(
            path.as_ptr(),
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len(),
            &mut len,
        )
    };

    if code == ESP_ERR_NOT_FOUND {
        return Ok(None);
    }

    if code != 0 {
        return Err(HalError::Internal(code));
    }

    let text = String::from_utf8_lossy(&buffer[..len]);

    MusicSettings::parse(&text)
        .map(Some)
        .map_err(|error| HalError::Io(error.to_string()))
}

/// Write the settings file, over an existing one. The C side makes the directories above it, so
/// `AppData/MUSIC` does not have to exist first — which is what a board fresh out of the box has.
fn write_settings(settings: &MusicSettings, root: &str) -> Result<(), HalError> {
    let text = settings
        .to_file()
        .map_err(|error| HalError::Io(error.to_string()))?;

    let path = c_path(&music_settings::path(root))?;
    let data = CString::new(text).map_err(|_| HalError::InvalidArg)?;

    let code = unsafe {
        ffi::hal_storage_write_file(path.as_ptr(), data.as_ptr(), data.as_bytes().len())
    };

    if code != 0 {
        return Err(HalError::Internal(code));
    }

    Ok(())
}

struct PlaybackControl {
    is_playing: AtomicBool,
    is_paused: AtomicBool,
    stop_requested: AtomicBool,
    /// Frames (not bytes) written to the codec. Frames are what a position in seconds is made of,
    /// and they survive the conversion: a 24-bit stereo file and a 16-bit mono one both leave one
    /// frame per frame.
    frames_played: AtomicU32,
}

/// A file being played, whichever way its samples have to be arrived at.
enum TrackSource {
    /// Uncompressed: the file's own samples, read and narrowed as they go out.
    Wav {
        file: File,
        normalizer: PcmNormalizer,
    },
    /// Compressed: decoded a block at a time.
    Coded(Box<PcmStream>),
}

impl TrackSource {
    /// How many channels the samples this produces have — what the codec is opened with.
    fn output_channels(&self) -> u8 {
        match self {
            TrackSource::Wav { normalizer, .. } => normalizer.output_channels(),
            TrackSource::Coded(stream) => stream.channels(),
        }
    }

    /// Decode or read the next block into `out`, returning whether there may be more.
    ///
    /// `scratch` is the raw read buffer, which only the WAV path needs: a decoder produces samples,
    /// a file produces bytes.
    fn next_block(&mut self, scratch: &mut [u8], out: &mut Vec<u8>) -> Result<bool, String> {
        match self {
            TrackSource::Wav { file, normalizer } => match file.read(scratch) {
                Ok(0) => Ok(false),
                Ok(n) => {
                    normalizer.push(&scratch[..n], out);
                    Ok(true)
                }
                Err(err) => Err(err.to_string()),
            },
            TrackSource::Coded(stream) => stream.read(out),
        }
    }
}

pub struct EspAudio {
    meta: Option<AudioMeta>,
    control: Option<Arc<PlaybackControl>>,
    volume: u8,
}

impl EspAudio {
    pub fn new() -> Self {
        unsafe {
            let _ = ffi::hal_audio_init();
        }

        /* Opened at the default rather than at silence: this runs before `AudioBackend::init` has
         * read the file, and a codec with no gain is not a state anything here wants to be in even
         * for the length of a boot. */
        let mut audio = Self {
            meta: None,
            control: None,
            volume: music_settings::DEFAULT_VOLUME,
        };
        audio.apply_volume(music_settings::DEFAULT_VOLUME);
        audio
    }

    /// Put the codec at `volume`, and the field with it.
    ///
    /// The one place the two are moved together, so that no path can do one without the other:
    /// `new` and `init` put a remembered number back, `play` re-applies the field because
    /// `hal_audio_open` resets the codec, and `set_volume` is the field plus a note of it on disk.
    fn apply_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        unsafe {
            let _ = ffi::hal_audio_set_volume(self.volume);
        }
    }

    /// Play an arbitrary `Read` stream (network stream, memory buffer, TTS),
    /// pushing raw PCM blocks to the hardware audio sink.
    pub fn play_stream<R: Read + Send + 'static>(
        &mut self,
        mut stream: R,
        sample_rate: u32,
        channels: u8,
        bits_per_sample: u8,
    ) -> Result<(), HalError> {
        self.stop();

        let ret = unsafe { ffi::hal_audio_open(sample_rate, channels, bits_per_sample) };
        if ret != 0 {
            return Err(HalError::Internal(ret));
        }
        // `hal_audio_open` resets the codec to its own gain, so the volume goes back on here.
        self.apply_volume(self.volume);

        let control = Arc::new(PlaybackControl {
            is_playing: AtomicBool::new(true),
            is_paused: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            frames_played: AtomicU32::new(0),
        });

        let control_clone = Arc::clone(&control);
        self.control = Some(control);

        let _ = std::thread::Builder::new()
            .name("audio_stream".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    if control_clone.stop_requested.load(Ordering::Relaxed) {
                        break;
                    }

                    if control_clone.is_paused.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(15));
                        continue;
                    }

                    match stream.read(&mut buf) {
                        Ok(0) => {
                            // End of stream: drain DMA pipeline before closing
                            unsafe {
                                ffi::hal_audio_drain();
                            }
                            break;
                        }
                        Ok(n) => {
                            let ret = unsafe { ffi::hal_audio_write(buf.as_ptr(), n as u32) };
                            if ret != 0 {
                                std::thread::sleep(std::time::Duration::from_millis(2));
                            }
                        }
                        Err(_) => break,
                    }
                }

                unsafe {
                    ffi::hal_audio_close();
                }
                control_clone.is_playing.store(false, Ordering::Relaxed);
            });

        Ok(())
    }

    /// Open a file for playback: its own samples if it holds them, a decoder if it does not.
    fn open_track(path: &str, info: &AudioInfo) -> Result<TrackSource, HalError> {
        match info.kind {
            AudioKind::Wav => {
                let mut file = File::open(path)
                    .map_err(|e| HalError::Io(format!("failed to open '{path}': {e}")))?;

                /* Read again rather than carried along from the probe: the header is 512 bytes and
                 * the probe has already closed its file, and this is the one place that needs the
                 * data offset rather than the format. */
                let mut header = [0u8; 512];
                let read = file
                    .read(&mut header)
                    .map_err(|e| HalError::Io(e.to_string()))?;
                let wav = parse_wav_header(&header[..read]).map_err(HalError::Io)?;

                file.seek(SeekFrom::Start(wav.data_offset as u64))
                    .map_err(|e| HalError::Io(e.to_string()))?;

                let normalizer = PcmNormalizer::new(wav.source()).ok_or_else(|| {
                    HalError::Io(format!(
                        "'{path}' holds {} samples of {} bits, which this build cannot convert",
                        wav.channels, wav.bits_per_sample
                    ))
                })?;

                Ok(TrackSource::Wav { file, normalizer })
            }
            AudioKind::Mp3 | AudioKind::Flac => {
                let stream = PcmStream::open(Path::new(path)).map_err(HalError::Io)?;
                Ok(TrackSource::Coded(Box::new(stream)))
            }
        }
    }

    /// Drive `source` into the codec until it ends, the caller stops it, or reading fails.
    ///
    /// The codec is already open when this is called and is closed when it returns.
    fn run_worker(source: TrackSource, control: Arc<PlaybackControl>) {
        let channels = source.output_channels().max(1) as usize;
        let bytes_per_frame = channels * 2;

        let mut source = source;
        let mut scratch = vec![0u8; 8192];
        let mut pcm: Vec<u8> = Vec::with_capacity(32 * 1024);
        let mut frames: u64 = 0;

        loop {
            if control.stop_requested.load(Ordering::Relaxed) {
                break;
            }

            if control.is_paused.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(15));
                continue;
            }

            pcm.clear();

            match source.next_block(&mut scratch, &mut pcm) {
                Ok(true) => {}
                Ok(false) => {
                    // End of the track: let the DMA pipeline finish rather than cutting the tail.
                    unsafe {
                        ffi::hal_audio_drain();
                    }
                    break;
                }
                Err(_) => break,
            }

            if pcm.is_empty() {
                continue;
            }

            let ret = unsafe { ffi::hal_audio_write(pcm.as_ptr(), pcm.len() as u32) };
            if ret != 0 {
                // The sink is full or busy; come back to it rather than dropping the block.
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }

            frames += (pcm.len() / bytes_per_frame) as u64;
            control.frames_played.store(frames as u32, Ordering::Relaxed);
        }

        unsafe {
            ffi::hal_audio_close();
        }
        control.is_playing.store(false, Ordering::Relaxed);
    }
}

impl Default for EspAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for EspAudio {
    /// Put the speaker at the volume this board was last left at.
    ///
    /// Read once, here, and never again: what `set_volume` writes is the truth from then on, and a
    /// filesystem read behind a getter is a cost nobody would see coming.
    fn init(&mut self) -> Result<(), HalError> {
        match read_settings(BOARD_APP_DATA) {
            Ok(Some(saved)) => {
                self.apply_volume(saved.volume);
                eprintln!("[audio] volume {}", self.volume);
            }
            /* Said out loud rather than passed over: "the board forgot how loud it was" and "nobody
             * ever moved the volume" look the same from the panel, and this line is the difference —
             * the same argument `wifi.rs` makes for its own version of this match. */
            Ok(None) => eprintln!("[audio] no remembered volume, staying at {}", self.volume),
            Err(error) => eprintln!(
                "[audio] the settings file is unreadable, staying at {}: {error}",
                self.volume
            ),
        }

        Ok(())
    }

    fn play(&mut self, path: &str) -> Result<AudioMeta, HalError> {
        self.stop();

        /* What the file is, from its own bytes rather than its name: a `.wav` holding µ-law and a
         * `.mp3` that is really a FLAC are both refused or accepted on what is actually in them. */
        let info = probe(Path::new(path)).map_err(HalError::Io)?;

        if !(CODEC_MIN_RATE..=CODEC_MAX_RATE).contains(&info.sample_rate) {
            return Err(HalError::Io(format!(
                "'{path}' is {} Hz, which the speaker's codec does not take ({}-{} Hz)",
                info.sample_rate, CODEC_MIN_RATE, CODEC_MAX_RATE
            )));
        }

        let source = Self::open_track(path, &info)?;
        let channels = source.output_channels().max(1) as u8;

        let ret = unsafe { ffi::hal_audio_open(info.sample_rate, channels, CODEC_BITS) };
        if ret != 0 {
            return Err(HalError::Internal(ret));
        }

        // `hal_audio_open` resets the codec to its own gain, so the volume goes back on here.
        self.apply_volume(self.volume);

        let control = Arc::new(PlaybackControl {
            is_playing: AtomicBool::new(true),
            is_paused: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            frames_played: AtomicU32::new(0),
        });

        let control_clone = Arc::clone(&control);

        let _ = std::thread::Builder::new()
            .name("audio_worker".into())
            .spawn(move || Self::run_worker(source, control_clone));

        let audio_meta = info.meta();

        self.meta = Some(audio_meta);
        self.control = Some(control);

        Ok(audio_meta)
    }

    fn pause(&mut self) {
        if let Some(ref c) = self.control {
            c.is_paused.store(true, Ordering::Relaxed);
        }
    }

    fn resume(&mut self) {
        if let Some(ref c) = self.control {
            c.is_paused.store(false, Ordering::Relaxed);
        }
    }

    fn stop(&mut self) {
        if let Some(ref c) = self.control {
            c.stop_requested.store(true, Ordering::Relaxed);
            c.is_playing.store(false, Ordering::Relaxed);
        }
        unsafe {
            ffi::hal_audio_close();
        }
    }

    fn set_volume(&mut self, volume: u8) {
        self.apply_volume(volume);

        /* Written down on every change. A volume is turned one step at a time, so this is a handful
         * of small writes rather than a stream of them — and a board that came back at the wrong
         * volume because the power went before it was saved would be worse than the write costs. */
        let settings = MusicSettings { volume: self.volume };
        if let Err(error) = write_settings(&settings, BOARD_APP_DATA) {
            eprintln!("[audio] could not remember the volume: {error}");
        }
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    #[inline]
    fn is_playing(&self) -> bool {
        if let Some(ref c) = self.control {
            c.is_playing.load(Ordering::Relaxed) && !c.is_paused.load(Ordering::Relaxed)
        } else {
            false
        }
    }

    #[inline]
    fn position_secs(&self) -> f32 {
        if let (Some(meta), Some(c)) = (&self.meta, &self.control) {
            if meta.sample_rate > 0 {
                let frames = c.frames_played.load(Ordering::Relaxed);
                let position = frames as f32 / meta.sample_rate as f32;

                /* A track whose length could not be worked out reports its position as it is rather
                 * than clamped to zero — an MP3 with no encoder frame is long, not empty. */
                return if meta.duration_secs > 0.0 {
                    position.min(meta.duration_secs)
                } else {
                    position
                };
            }
        }
        0.0
    }

    #[inline]
    fn tick(&mut self) {
        // No-op! State is updated atomically by the audio streaming worker thread
        // upon reaching EOF and completing DMA drain.
    }
}
