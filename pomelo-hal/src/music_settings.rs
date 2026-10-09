//! The music player's settings, as a file on the board.
//!
//! # What is in here
//!
//! The volume, and so far only the volume: the one thing a finger sets on the player that has to
//! outlive the power going off. Everything else a track needs is in the track's own file.
//!
//! # Why it is a file and not a field in RAM
//!
//! Because it is a *setting*, and a setting that is forgotten is not one. The volume is also the
//! board's, not the player's: `hal_audio_set_volume` is the codec's gain, so it is set once at boot
//! from here and not only when the music page happens to be open.
//!
//! # The format
//!
//! TOML, through `serde`, in the same shape as [`crate::wifi_credentials`] and for the same
//! reasons: the struct below *is* the schema, adding a setting is adding a field, and unknown keys
//! are **ignored and not refused**, so a file a later version of this firmware wrote stays readable
//! by an earlier one.
//!
//! ```text
//! # /internal/AppData/MUSIC/music.conf
//! [music]
//! volume = 75
//! ```
//!
//! # On the board, not through `std::fs`
//!
//! [`MusicSettings::load`] and [`MusicSettings::save`] are the desktop and test path, and the device
//! does not take it: `std::fs` cannot create a file on this board's partition, which is the whole of
//! what `pomelo-hal-esp32`'s `read_settings`/`write_settings` say at length. The parser and the
//! writer below are shared by both, so what is written on the board and what is written on a desktop
//! are the same bytes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file's name, inside the app's music directory.
pub const FILE: &str = "music.conf";

/// The directory the music player's files live in, inside an app-data root.
pub const DIRECTORY: &str = "MUSIC";

/// What a board that has never had its volume moved comes up at, as a percentage.
///
/// 75 and not 100: the codec's gain on top of an already full-scale sample is where this speaker
/// starts to clip, and a board that came up at its loudest is the one thing a person reaches for
/// first. It is also the number `EspAudio` opened with before any of this was remembered, so a board
/// upgrading into this file sounds exactly as it did the day before.
pub const DEFAULT_VOLUME: u8 = 75;

/// Where the file is, under `root`.
pub fn path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(DIRECTORY).join(FILE)
}

/// The `[music]` section of the board's music file.
///
/// The `derive` is the schema: the reader, the writer and the check that the file matches all follow
/// from the fields here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicSettings {
    /// How loud the speaker is, as a percentage.
    ///
    /// A file that says nothing about it is a board whose volume was never moved, which is
    /// [`DEFAULT_VOLUME`] — and a value that is not a percentage at all is an error on the line it
    /// is on, the way every other typed field here is. Silently clamping 120 to 100 would hide a file
    /// written by something that means something else by the number.
    #[serde(default = "default_volume")]
    pub volume: u8,
}

/// The default for `volume`, as `serde` demands it: a function, because an attribute holds a path
/// and not an expression.
fn default_volume() -> u8 {
    DEFAULT_VOLUME
}

impl Default for MusicSettings {
    fn default() -> Self {
        Self {
            volume: DEFAULT_VOLUME,
        }
    }
}

/// The whole file: one table.
///
/// It exists so that a table this version does not know about is a field this struct does not have —
/// and `serde` ignores those, which is the whole of what keeps a downgrade working.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    music: MusicSettings,
}

impl MusicSettings {
    /// Reads the file's text.
    ///
    /// A missing `[music]`, a value of the wrong type — both are an error here, on the line they are
    /// on. That is what a typed file buys: the failure happens at the file, named, instead of at a
    /// speaker that came up quiet and cannot say why.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Document>(text).map(|document| document.music)
    }

    /// The file's text, ready to write.
    ///
    /// Fallible in principle — `serde` can fail to write a type it has no representation for — and
    /// infallible in fact for one plain field. The signature says "in principle" rather than
    /// promising what it cannot promise.
    pub fn to_file(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&Document { music: *self })
    }

    /// Reads the file under `root`, if there is one.
    ///
    /// `Ok(None)` for a board whose volume has never been moved, which is not a failure. A file that
    /// is *there* and unreadable is one, and comes back as `InvalidData` carrying the reason — so
    /// "nothing was ever saved" and "what was saved is corrupt" stay two different screens.
    pub fn load(root: impl AsRef<Path>) -> io::Result<Option<Self>> {
        let text = match fs::read_to_string(path(root)) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };

        Self::parse(&text)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Writes the file under `root`, making the directories on the way.
    pub fn save(&self, root: impl AsRef<Path>) -> io::Result<()> {
        let path = path(root);
        let text = self
            .to_file()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, text)
    }
}

// No error type of our own, for the same reason `wifi_credentials` has none: `toml::de::Error`
// already says what is wrong and on which line, and wrapping it to say the same thing in this
// crate's words would cost the line number to say it.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_is_a_file_read() {
        let written = MusicSettings { volume: 35 };

        assert_eq!(
            MusicSettings::parse(&written.to_file().unwrap()).unwrap(),
            written,
            "the writer and the reader are the same `derive`, which is the point of it"
        );
    }

    #[test]
    fn a_file_that_says_nothing_is_the_volume_the_board_has_always_come_up_at() {
        let without = MusicSettings::parse("[music]\n").unwrap();

        assert_eq!(
            without.volume, DEFAULT_VOLUME,
            "a file from before the key existed belongs to a board nobody had turned down"
        );
        assert_eq!(
            MusicSettings::default().volume,
            DEFAULT_VOLUME,
            "and so does no file at all"
        );
    }

    #[test]
    fn a_table_a_later_version_wrote_is_ignored() {
        let read = MusicSettings::parse("[music]\nvolume = 40\nshuffle = true\n\n[eq]\nbass = 3\n")
            .unwrap();

        assert_eq!(
            read,
            MusicSettings { volume: 40 },
            "an unknown key or table is a field this struct does not have, which `serde` ignores"
        );
    }

    #[test]
    fn a_value_that_is_not_a_percentage_is_an_error_and_not_a_default() {
        let error = MusicSettings::parse("[music]\nvolume = \"loud\"\n").unwrap_err();

        assert!(
            error.to_string().contains("invalid type"),
            "`loud` is a string and not a number, and a typed file says so instead of guessing: \
             {error}"
        );
        assert!(
            MusicSettings::parse("[music]\nvolume = 300\n").is_err(),
            "and 300 is out of range for a percentage rather than being quietly pinned to 100"
        );
    }

    /// A directory of this test's own, under the system's temp directory.
    ///
    /// Named after the test, so that two of them running at once do not share a file — and so that a
    /// failure leaves its evidence behind to be looked at.
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join("pomelo-hal-music-settings")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn nothing_saved_is_not_a_failure() {
        let root = temp_root("nothing");

        assert_eq!(
            MusicSettings::load(&root).unwrap(),
            None,
            "a board whose volume has never been moved, which is not an error"
        );
    }

    #[test]
    fn a_saved_volume_comes_back() {
        let root = temp_root("round-trip");
        let written = MusicSettings { volume: 20 };

        written.save(&root).unwrap();

        assert_eq!(
            MusicSettings::load(&root).unwrap(),
            Some(written),
            "through the filesystem, not only through the parser"
        );
        assert!(
            path(&root).ends_with("MUSIC/music.conf"),
            "and it went where the constant says: {:?}",
            path(&root)
        );
    }

    #[test]
    fn a_file_that_is_there_and_wrong_says_so() {
        let root = temp_root("malformed");
        let file = path(&root);

        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "[music]\nnot a pair\n").unwrap();

        let error = MusicSettings::load(&root).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("line 2"),
            "and the message points at the line: {error}"
        );
    }
}
