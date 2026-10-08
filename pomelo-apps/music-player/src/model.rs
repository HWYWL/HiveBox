//! The player's data and playback state machine.
//!
//! The playlist, the current track, the status and the calls into the audio backend — and nothing
//! that draws. `rotation_angle` and `title_scroll_offset` are animation state that the two UIs
//! read; nothing here knows what a disc or a title band looks like.

use pomelo_hal::wav::parse_wav_header;
use pomelo_hal::{AudioMeta, Board};
use std::path::Path;
use std::sync::Arc;

/// How fast the disc turns, in degrees per second: 33⅓ rpm, which is what a record does.
///
/// It is a constant here rather than a number of degrees per frame because a frame is not a unit of
/// time — see [`MusicPlayerModel::tick`].
pub const ROTATION_DEGREES_PER_SECOND: f32 = 200.0;

#[derive(Debug, Clone, PartialEq)]
pub struct MusicTrack {
    pub title: String,
    pub path: String,
    pub filename: String,
    pub metadata: Option<AudioMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

pub struct MusicPlayerModel {
    pub playlist: Vec<MusicTrack>,
    pub current_index: usize,
    pub status: PlaybackStatus,
    pub volume: u8,
    pub position_secs: f32,
    pub duration_secs: f32,
    pub rotation_angle: f32,
    pub title_scroll_offset: f32,
    pub title_scroll_done: bool,
    pub show_playlist: bool,
    pub board: Arc<Board>,
    pub last_error: Option<String>,
}

impl MusicPlayerModel {
    /// Create the player using the shared [`Board`]'s audio backend.
    pub fn new(board: Arc<Board>) -> Self {
        let mut state = Self {
            playlist: Vec::new(),
            current_index: 0,
            status: PlaybackStatus::Stopped,
            volume: 75,
            position_secs: 0.0,
            duration_secs: 0.0,
            rotation_angle: 0.0,
            title_scroll_offset: 0.0,
            title_scroll_done: true,
            show_playlist: false,
            board,
            last_error: None,
        };

        state.refresh_playlist();
        state
    }

    /// Where the player looks for tracks: the device's storage first, then wherever
    /// the simulator or a test happens to be running from.
    pub const SEARCH_DIRS: [&'static str; 6] = [
        "/storage/music",
        "/storage",
        "assets/music",
        "./assets/music",
        "../../assets/music",
        "../assets/music",
    ];

    pub fn refresh_playlist(&mut self) {
        self.refresh_playlist_in(&Self::SEARCH_DIRS);
    }

    /// The scan itself, pointed at `dirs` — so a test can point it at a directory it
    /// owns instead of hunting for whatever audio happens to be in the repository.
    pub fn refresh_playlist_in(&mut self, dirs: &[&str]) {
        println!("[MusicPlayer] Scanning for audio files...");
        self.playlist.clear();

        let mut seen = std::collections::HashSet::new();

        for dir in dirs {
            let p = Path::new(dir);
            if p.is_dir() {
                println!("[MusicPlayer] Directory exists: {}", dir);
                if let Ok(entries) = std::fs::read_dir(p) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(ext) = path.extension() {
                                if ext.to_string_lossy().eq_ignore_ascii_case("wav") {
                                    let filename =
                                        path.file_name().unwrap().to_string_lossy().to_string();
                                    if !seen.contains(&filename) {
                                        seen.insert(filename.clone());
                                        let title =
                                            path.file_stem().unwrap().to_string_lossy().to_string();
                                        let path_str = path.to_string_lossy().to_string();
                                        let mut metadata = None;
                                        if let Ok(mut f) = std::fs::File::open(&path_str) {
                                            use std::io::Read;
                                            let mut header_buf = [0u8; 512];
                                            if let Ok(n) = f.read(&mut header_buf) {
                                                if let Ok(meta) = parse_wav_header(&header_buf[..n])
                                                {
                                                    println!("[MusicPlayer] Loaded track: \"{}\" ({:.1}s, {}Hz)", title, meta.duration_secs, meta.sample_rate);
                                                    metadata = Some(AudioMeta {
                                                        sample_rate: meta.sample_rate,
                                                        channels: meta.channels as u8,
                                                        bits_per_sample: meta.bits_per_sample as u8,
                                                        duration_secs: meta.duration_secs,
                                                    });
                                                }
                                            }
                                        }
                                        self.playlist.push(MusicTrack {
                                            title,
                                            path: path_str,
                                            filename,
                                            metadata,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        println!(
            "[MusicPlayer] Playlist scan complete: {} track(s) discovered.",
            self.playlist.len()
        );

        if let Some(track) = self.playlist.get(self.current_index) {
            if let Some(meta) = track.metadata {
                self.duration_secs = meta.duration_secs;
            }
        }
    }

    pub fn current_track(&self) -> Option<&MusicTrack> {
        self.playlist.get(self.current_index)
    }

    pub fn play_track(&mut self, index: usize) {
        if self.playlist.is_empty() {
            return;
        }

        self.current_index = index % self.playlist.len();
        self.rotation_angle = 0.0;
        self.title_scroll_offset = 0.0;
        self.title_scroll_done = true;
        let track_path = self.playlist[self.current_index].path.clone();

        let result = self.board.audio().play(&track_path);
        match result {
            Ok(meta) => {
                self.duration_secs = meta.duration_secs;
                self.position_secs = 0.0;
                self.status = PlaybackStatus::Playing;
                self.last_error = None;
                if let Some(t) = self.playlist.get_mut(self.current_index) {
                    t.metadata = Some(meta);
                }
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                self.status = PlaybackStatus::Stopped;
            }
        }
    }

    pub fn toggle_play_pause(&mut self) {
        match self.status {
            PlaybackStatus::Playing => {
                self.board.audio().pause();
                self.status = PlaybackStatus::Paused;
            }
            PlaybackStatus::Paused => {
                self.board.audio().resume();
                self.status = PlaybackStatus::Playing;
            }
            PlaybackStatus::Stopped => {
                if !self.playlist.is_empty() {
                    self.play_track(self.current_index);
                }
            }
        }
    }

    pub fn next_track(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        let next_idx = (self.current_index + 1) % self.playlist.len();
        self.play_track(next_idx);
    }

    pub fn prev_track(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        let prev_idx = if self.current_index == 0 {
            self.playlist.len() - 1
        } else {
            self.current_index - 1
        };
        self.play_track(prev_idx);
    }

    pub fn set_volume(&mut self, vol: u8) {
        self.volume = vol.min(100);
        self.board.audio().set_volume(self.volume);
    }

    pub fn volume_up(&mut self) {
        self.set_volume(self.volume.saturating_add(10).min(100));
    }

    pub fn volume_down(&mut self) {
        self.set_volume(self.volume.saturating_sub(10));
    }

    pub fn toggle_playlist_view(&mut self) {
        self.show_playlist = !self.show_playlist;
    }

    /// One frame of playback: `elapsed` seconds since the frame before this one.
    ///
    /// The *position* is not advanced here — the audio backend owns it, and this polls it — but the
    /// disc is turned by `elapsed`, because a frame is not a unit of time. The app is handed one
    /// frame per drawn frame by the platform, and that rate is the platform's: a window draws
    /// hundreds of times a second, the panel as fast as it can paint a damaged strip. The original
    /// counted 2.5° per frame, which is only a speed if you know the frame rate.
    pub fn tick(&mut self, elapsed: f32) {
        self.board.audio().tick();
        let position = self.board.audio().position_secs();
        self.position_secs = position;
        let still_playing = self.board.audio().is_playing();

        if self.status == PlaybackStatus::Playing {
            self.rotation_angle =
                (self.rotation_angle + elapsed.max(0.0) * ROTATION_DEGREES_PER_SECOND) % 360.0;

            if !still_playing
                && self.position_secs >= self.duration_secs
                && self.duration_secs > 0.0
            {
                // Auto advance to next track
                self.next_track();
            }
        }
    }

    /// Stop audio output (used when the app is killed).
    pub fn stop_audio(&mut self) {
        self.board.audio().stop();
        self.status = PlaybackStatus::Stopped;
    }

    pub fn is_animating(&self) -> bool {
        self.status == PlaybackStatus::Playing
    }
}
