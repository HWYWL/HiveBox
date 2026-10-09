//! The player's data and playback state machine.
//!
//! The playlist, the current track, the status and the calls into the audio backend — and nothing
//! that draws. `rotation_angle` and `title_scroll_offset` are animation state that the two UIs
//! read; nothing here knows what a disc or a title band looks like.

use pomelo_hal::probe::{probe, AudioKind};
use pomelo_hal::{AudioMeta, Board, VolumeKind};
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

/// Which of the player's two screens is showing.
///
/// The list is where the app opens, and that is the point of it: a player that starts a track the
/// moment it is opened is a player that makes a sound nobody asked for. The panel is looked at far
/// more often than it is listened to, and choosing a track is a thing the person does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The tracks that were found, waiting to be chosen.
    Library,
    /// The chosen one: the disc, the progress and the volume.
    NowPlaying,
}

/// Where the player's music is, as the box answered when it was asked.
///
/// The two cases are the two things a screen has to say: a folder to scan, or a slot with nothing
/// in it. There is deliberately no "the folder could not be made" case — a folder that cannot be
/// made is an empty folder for every purpose this player has, and it is reported once, on the
/// console, where somebody can read why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Library {
    /// The card's own folder: `{mount}/music`, made if the card was in the slot and it was not.
    Card { dir: String },
    /// No card: nothing to scan, and nothing to say but "put one in".
    NoCard,
}

pub struct MusicPlayerModel {
    pub playlist: Vec<MusicTrack>,
    pub current_index: usize,
    pub status: PlaybackStatus,
    pub volume: u8,
    pub position_secs: f32,
    pub duration_secs: f32,
    /// Where a finger has dragged the progress bar to, in seconds, while it is still on it.
    ///
    /// `None` is "nobody has hold of the bar", and it is also what keeps [`Self::tick`] from reading
    /// the position back from the backend: while a bar is being dragged the number under it is the
    /// finger's, and one that a frame overwrote would shiver under the hand.
    pub seeking: Option<f32>,
    pub rotation_angle: f32,
    pub title_scroll_offset: f32,
    pub title_scroll_done: bool,
    /// Which screen is showing. See [`Page`].
    pub page: Page,
    pub board: Arc<Board>,
    /// Where the music was looked for: the card's folder, or an empty slot.
    pub library: Library,
    pub last_error: Option<String>,
}

impl MusicPlayerModel {
    /// Create the player using the shared [`Board`]'s audio backend.
    pub fn new(board: Arc<Board>) -> Self {
        /* The volume the board came up at — read off its own partition when it booted, and not a
         * number this app keeps. Two of them would be one too many, and the one that matters is the
         * codec's: a page that opened at 75 while the speaker sat at 30 would be a page that lies. */
        let volume = board.audio().volume();

        let mut state = Self {
            playlist: Vec::new(),
            current_index: 0,
            status: PlaybackStatus::Stopped,
            volume,
            position_secs: 0.0,
            duration_secs: 0.0,
            seeking: None,
            rotation_angle: 0.0,
            title_scroll_offset: 0.0,
            title_scroll_done: true,
            page: Page::Library,
            board,
            // Replaced by the scan below, which is the first thing that asks the slot a question.
            library: Library::NoCard,
            last_error: None,
        };

        state.refresh_playlist();
        state
    }

    /// The folder inside the card that holds the music.
    pub const LIBRARY_DIR: &'static str = "music";

    /// Where the player looks when the card's folder held nothing: the checkout's own music, so the
    /// simulator and a desktop run still play something.
    ///
    /// The board has no part in this list. `/sdcard/music` is where the box's music lives, and a
    /// board whose card has an empty `music` folder is a board with an empty folder — not one that
    /// reads the build directory the firmware happened to be compiled in.
    pub const DESKTOP_DIRS: [&'static str; 4] = [
        "assets/music",
        "./assets/music",
        "../../assets/music",
        "../assets/music",
    ];

    /// Asks the slot for a card, and scans the music on it.
    ///
    /// This is what the app does when it opens, and what a finger does when it asks again. It is
    /// also the only way the player can notice a card that arrived while the box was running: the
    /// slot has no card-detect pin (see `StorageBackend`), so the question is answered by trying to
    /// mount one — and an empty slot is an answer, not a failure.
    pub fn refresh_playlist(&mut self) {
        println!("[MusicPlayer] Looking for the card's music folder...");
        self.library = open_library(&self.board);
        self.playlist.clear();

        let mut seen = std::collections::HashSet::new();

        if let Library::Card { dir } = self.library.clone() {
            self.scan_into(&[dir.as_str()], &mut seen);
        }

        if self.playlist.is_empty() {
            self.scan_into(&Self::DESKTOP_DIRS, &mut seen);
        }

        self.report_scan();
    }

    /// The scan itself, pointed at `dirs` — so a test can point it at a directory it
    /// owns instead of hunting for whatever audio happens to be in the repository.
    ///
    /// [`MusicPlayerModel::library`] is left as it was: a caller naming its own directories is not
    /// answering the question the card answers, so it does not get to answer it.
    pub fn refresh_playlist_in(&mut self, dirs: &[&str]) {
        println!("[MusicPlayer] Scanning for audio files...");
        self.playlist.clear();
        let mut seen = std::collections::HashSet::new();
        self.scan_into(dirs, &mut seen);
        self.report_scan();
    }

    /// Whether the box is asking for a card: there is none in the slot.
    pub fn wants_card(&self) -> bool {
        matches!(self.library, Library::NoCard)
    }

    /// The folder the tracks were looked for in, or `None` for an empty slot.
    pub fn library_dir(&self) -> Option<&str> {
        match &self.library {
            Library::Card { dir } => Some(dir.as_str()),
            Library::NoCard => None,
        }
    }

    /// One pass over `dirs`, appending whatever it finds.
    ///
    /// `seen` is the caller's rather than this function's, so that a playlist built out of two
    /// passes — the card's folder, then the fallback — cannot hold the same track twice.
    fn scan_into(&mut self, dirs: &[&str], seen: &mut std::collections::HashSet<String>) {
        for dir in dirs {
            let p = Path::new(dir);
            if p.is_dir() {
                println!("[MusicPlayer] Directory exists: {}", dir);
                if let Ok(entries) = std::fs::read_dir(p) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            /* The extension decides what to *try*, and the bytes decide what it is:
                             * a file named `.wav` that holds ADPCM is refused by `probe`, while an
                             * MP3 or FLAC is recognised even when its name does not say so. */
                            let is_candidate = path
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .map_or(false, |ext| AudioKind::from_extension(ext).is_some());

                            if is_candidate {
                                let filename =
                                    path.file_name().unwrap().to_string_lossy().to_string();
                                if !seen.contains(&filename) {
                                    seen.insert(filename.clone());
                                    let title =
                                        path.file_stem().unwrap().to_string_lossy().to_string();
                                    let path_str = path.to_string_lossy().to_string();

                                    let metadata = match probe(&path) {
                                        Ok(info) => {
                                            println!(
                                                "[MusicPlayer] Loaded track: \"{}\" ({}, {:.1}s, {}Hz, {}ch)",
                                                title,
                                                info.kind.label(),
                                                info.duration_secs,
                                                info.sample_rate,
                                                info.channels
                                            );
                                            Some(info.meta())
                                        }
                                        Err(error) => {
                                            println!(
                                                "[MusicPlayer] Skipping \"{}\": {}",
                                                filename, error
                                            );
                                            None
                                        }
                                    };

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

    /// The tail of every scan: what it found, and the length of whatever is at the current index,
    /// so the progress bar has a duration before anything has been played.
    fn report_scan(&mut self) {
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

        /* A new track opens at its own beginning: a hand that was still on the bar of the last one is
         * not on this one, and its position means nothing here. */
        self.seeking = None;
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

    /// Set the volume, and let the backend keep it.
    ///
    /// Nothing is written down here: the board's audio backend is what remembers it, on the way past
    /// — see `pomelo_hal::music_settings`. This only has to say the number.
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

    /// Move the volume under a finger that is dragging the level: heard, not written down.
    ///
    /// The expensive half of a volume change is the backend remembering it — an erase and a rewrite of
    /// a flash partition, see [`pomelo_hal::AudioBackend::preview_volume`] — and a drag is a stream of
    /// numbers of which only the last is the one anybody meant. So this says the number and leaves it
    /// there;
    /// [`Self::commit_volume`] is what the finger's release says.
    pub fn preview_volume(&mut self, vol: u8) {
        self.volume = vol.min(100);
        self.board.audio().preview_volume(self.volume);
    }

    /// The finger has let go of the level: this is the number the board will come up at next time.
    pub fn commit_volume(&mut self) {
        self.board.audio().set_volume(self.volume);
    }

    /// A finger has taken hold of the progress bar, or moved it: follow it, and tell nobody.
    ///
    /// The backend is not asked to go there until the finger lets go ([`Self::commit_seek`]), because
    /// a seek here is the file opened again further in: once a frame, that is a track made of
    /// restarts. What moves meanwhile is the bar and the number under it, which is the whole of what a
    /// hand can see.
    pub fn scrub(&mut self, position_secs: f32) {
        let position = position_secs.clamp(0.0, self.duration_secs.max(0.0));

        self.position_secs = position;
        self.seeking = Some(position);
    }

    /// The finger has let go of the progress bar: play from where it was left.
    ///
    /// The position stays where the finger put it rather than being read back, so the bar does not
    /// jump home for the frame or two a seek takes to land: the backend's next answer is already the
    /// same place, because the frames it counts from go on from where the needle was put down.
    pub fn commit_seek(&mut self) {
        let Some(position) = self.seeking.take() else {
            return;
        };

        self.position_secs = position;

        match self.board.audio().seek(position) {
            Ok(()) => self.last_error = None,
            Err(error) => self.last_error = Some(error.to_string()),
        }
    }

    /// Open the track at `index`: what a tap on a row in the list means.
    ///
    /// Showing the playing screen and starting the track are one step because they are one intent.
    /// A screen that came up with a stopped disc on it, one more press from making the sound the row
    /// was tapped for, would be asking the same question twice.
    pub fn open_track(&mut self, index: usize) {
        self.page = Page::NowPlaying;
        self.play_track(index);
    }

    /// Leave the playing screen for the list. `true` when there was a screen to leave.
    ///
    /// The answer is what tells the launcher whether this press belonged to the app: from the playing
    /// screen it is the list's, and from the list there is nothing left but leaving the app — the
    /// same bargain `settings` makes with its own back button.
    ///
    /// Playback is deliberately not stopped: choosing the next track while this one plays is the
    /// whole reason to go back to the list, and the row that is playing is the one that says so.
    pub fn go_back(&mut self) -> bool {
        match self.page {
            Page::NowPlaying => {
                self.page = Page::Library;
                true
            }
            Page::Library => false,
        }
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

        /* A finger on the bar is the exception to "the backend owns the position": while one is there
         * the position is what it says, and a poll every frame would drag the number back out from
         * under it. Everything else about the frame still happens — the disc turns, the backend is
         * ticked — and the readout is the same one it will be a frame after the finger leaves. */
        let dragging = self.seeking.is_some();

        if !dragging {
            self.position_secs = self.board.audio().position_secs();
        }

        let still_playing = self.board.audio().is_playing();

        if self.status == PlaybackStatus::Playing {
            self.rotation_angle =
                (self.rotation_angle + elapsed.max(0.0) * ROTATION_DEGREES_PER_SECOND) % 360.0;

            /* Not while a finger is on the bar: a track being dragged has been stopped for the moment
             * of the seek, and "the backend says it is not playing" would otherwise be read as the end
             * of the track and skip to the next one — under the hand, mid-drag. */
            if !dragging
                && !still_playing
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

        /* The bar goes with the track: a drag that was in progress belongs to what was playing. */
        self.seeking = None;
    }

    pub fn is_animating(&self) -> bool {
        self.status == PlaybackStatus::Playing
    }
}

/// The card's music folder, made when the card is in the slot and the folder is not.
///
/// Making it is guarded by the mount point itself, and that guard is the whole of the care here: a
/// directory cannot be made inside a card that is not mounted, and on a desktop `/sdcard` is not a
/// path at the root of the drive — which is the difference between making the board's folder and
/// creating a stray `/sdcard/music` on somebody's machine.
fn open_library(board: &Board) -> Library {
    match card_mount(board) {
        Some(mount) => Library::Card {
            dir: library_dir_in(&mount),
        },
        None => Library::NoCard,
    }
}

/// The music folder inside `mount`, made when the card is really there and the folder is not.
///
/// Split out from [`open_library`] because this is the part with a decision in it and it can be
/// tested without a board: a mount point and a filesystem are all it needs.
fn library_dir_in(mount: &str) -> String {
    let dir = format!(
        "{}/{}",
        mount.trim_end_matches('/'),
        MusicPlayerModel::LIBRARY_DIR
    );

    if !Path::new(&dir).is_dir() && Path::new(mount).is_dir() {
        match std::fs::create_dir_all(&dir) {
            Ok(()) => println!("[MusicPlayer] Created the music folder: {}", dir),
            Err(error) => println!("[MusicPlayer] Could not create {}: {}", dir, error),
        }
    }

    dir
}

/// The card's mount point, or `None` when the slot is empty.
///
/// The question is the storage backend's and not the filesystem's: a volume is only ever reported
/// when something really is mounted there, so "no removable volume" means "no card" rather than "no
/// answer". The probe comes first because a card can arrive while the box is running, and this is
/// the only kind of look that can find it.
fn card_mount(board: &Board) -> Option<String> {
    let mut storage = board.storage();
    let _ = storage.refresh();

    storage
        .volumes()
        .ok()?
        .into_iter()
        .find(|volume| volume.kind == VolumeKind::Removable)
        .map(|volume| volume.mount_point)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of the test's own, standing in for a mounted card.
    fn scratch_mount(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pomelo-music-mount-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch mount point");
        dir
    }

    /// The folder the player reads is made when the card is there and it is not.
    #[test]
    fn the_music_folder_is_made_inside_a_mount_that_exists() {
        let mount = scratch_mount("made");
        let mount = mount.to_str().expect("a utf-8 scratch path");

        let dir = library_dir_in(mount);

        assert!(dir.ends_with("/music"), "got {dir}");
        assert!(
            Path::new(&dir).is_dir(),
            "the folder the player reads from has to exist after it is asked for: {dir}"
        );
    }

    /// And it is *not* made when the card is not: this is the guard that keeps a desktop run from
    /// creating a `/sdcard` at the root of somebody's drive.
    #[test]
    fn nothing_is_created_inside_a_mount_that_is_not_there() {
        let mount = std::env::temp_dir().join(format!("pomelo-music-absent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mount);
        let mount = mount.to_str().expect("a utf-8 scratch path");

        let dir = library_dir_in(mount);

        assert!(dir.ends_with("/music"), "the answer is still a path: {dir}");
        assert!(
            !Path::new(mount).exists(),
            "the mount point itself must not be created: {mount}"
        );
        assert!(!Path::new(&dir).exists(), "nor the folder inside it");
    }
}
