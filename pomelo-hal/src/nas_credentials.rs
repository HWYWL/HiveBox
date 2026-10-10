//! The NAS this board watches, and how to log into it.
//!
//! # The same file as Wi-Fi, one directory over
//!
//! `/internal/AppData/NAS/nas.conf`, written and read exactly like `/internal/AppData/WIFI/wifi.conf`
//! — same TOML, same `serde`, same `load`/`save`/`forget`, same rule that an unknown key is ignored
//! rather than refused. The two files sit under [`crate::app_data`] together for the same reason they
//! are the same shape: both are "how this box reaches a machine that is not this box", and a person
//! who has edited one has edited the other.
//!
//! ```text
//! # /internal/AppData/NAS/nas.conf
//! [nas]
//! host = "192.168.1.10"
//! port = 22
//! user = "nasstat"
//! password = "hunter2"
//! interval_secs = 5
//! host_key = "ecdsa-sha2-nistp256 AAAAE2VjZHNh…"
//! ```
//!
//! # In the clear, and more deliberately than the Wi-Fi password
//!
//! The password is written as text, like the Wi-Fi one and for the same reason: neither
//! `CONFIG_NVS_ENCRYPTION` nor `CONFIG_FLASH_ENCRYPTION_ENABLED` is set, so a mangled copy would be
//! mangles on top of a readable flash. What is *different* is the blast radius, and that is why this
//! module says it out loud: a Wi-Fi password gets a machine onto a network, and this one gets somebody
//! a login on the NAS. **Use an account that exists only for this** — no administrator, nothing in
//! `sudoers`, no access to anything the numbers on the panel do not need. The commands this board runs
//! are all readings of `/proc` and `/sys`, which an ordinary login can already read.
//!
//! [`NasCredentials::host_key`] is the other half of taking that seriously, and it is why the field is
//! here rather than in the backend's memory: the first look records the key the server offered,
//! somebody looks at the panel once and decides it is the NAS, and from then on a *different* key is a
//! fault the panel reports rather than a session the board opens anyway.
//!
//! # On the board, `std::fs` is not the way in
//!
//! [`NasCredentials::load`] and [`NasCredentials::save`] are the desktop and test path. The device is
//! the other way round: `std::fs` cannot create a file on this board's partition, which is what
//! `pomelo-hal-esp32`'s `read_settings`/`write_settings` say at length for the music player's file.
//! The parser and the writer *are* shared, so what the board writes and what a desktop writes are the
//! same bytes — and the side that knows how to put them on this platform's flash is the backend, which
//! is why an app asks [`NasBackend`][crate::traits::NasBackend] rather than reading this file itself.
//!
//! # Unknown keys are ignored
//!
//! Like the Wi-Fi file: a file written by a later firmware has to stay readable by an earlier one, or
//! a downgrade costs a person their NAS.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The file's name, inside the app's NAS directory.
pub const FILE: &str = "nas.conf";

/// The directory the NAS file lives in, inside an app-data root.
pub const DIRECTORY: &str = "NAS";

/// The app-data root on the board, and the one on a desktop. See [`crate::app_data`].
pub use crate::app_data::{BOARD_APP_DATA, DESKTOP_APP_DATA};

/// Where the file is, under `root`.
pub fn path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(DIRECTORY).join(FILE)
}

/// The `[nas]` section of the board's NAS file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NasCredentials {
    /// The machine's address: a name or a dotted quad, as `getaddrinfo` will take either.
    ///
    /// **No default.** A file that names no machine is not a NAS file, and a `String` would happily
    /// default to `""` and then resolve nothing at all without saying so.
    pub host: String,

    /// The port the SSH server is on. 22 unless somebody moved it, which is why this has a default
    /// and the host does not.
    #[serde(default = "ssh_port")]
    pub port: u16,

    /// The account to log in as. No default, for the reason the host has none: a login is two halves
    /// and the file that has one of them is a file somebody stopped in the middle of writing.
    pub user: String,

    /// The password, in the clear. See the note at the top of the module.
    #[serde(default)]
    pub password: String,

    /// How often to look, in seconds.
    ///
    /// Five by default: slow enough that a NAS's own disks are not the thing being measured, and fast
    /// enough that a person watching a copy of a file sees the numbers move. The other end of the
    /// scale is not a preference either — an SSH look is a key exchange when the session has to be
    /// rebuilt, so a panel asking twice a second would spend more time logging in than reading.
    #[serde(default = "interval")]
    pub interval_secs: u64,

    /// The host key this board saw the first time it looked, as the server offered it.
    ///
    /// Empty until a look has happened, and written by the *app* rather than by the backend: the
    /// backend is the side that can see the key (see [`crate::types::NasReading::host_key`]) and the
    /// app is the side that holds the file. A key here and a different one from the server is
    /// [`NasFault::HostKeyChanged`][crate::types::NasFault::HostKeyChanged], which stops the reading
    /// rather than replacing what was remembered.
    #[serde(default)]
    pub host_key: Option<String>,
}

impl NasCredentials {
    /// How often to look, as a duration.
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs.max(1))
    }

    /// Reads the file's text.
    ///
    /// A missing `[nas]`, a missing `host`, a value of the wrong type — each is an error here, on the
    /// line it is on. That is what a typed file buys: the failure happens at the file, named, instead
    /// of three screens away at a password that will not log in.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Document>(text).map(|document| document.nas)
    }

    /// The file's text, ready to write.
    pub fn to_file(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&Document { nas: self.clone() })
    }

    /// Reads the file under `root`, if there is one.
    ///
    /// `Ok(None)` for a board that has never been pointed at a NAS, which is not a failure. A file that
    /// is *there* and unreadable is one, and comes back as `InvalidData` carrying the reason.
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

    /// Removes the file under `root`. Not an error if there was nothing to remove.
    pub fn forget(root: impl AsRef<Path>) -> io::Result<()> {
        match fs::remove_file(path(root)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// The whole file: one table, so that a table this version does not know about is a field this struct
/// does not have — and `serde` ignores those, which is the whole of what keeps a downgrade working.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    nas: NasCredentials,
}

/// The default port: the one SSH is on unless somebody moved it.
fn ssh_port() -> u16 {
    22
}

/// The default interval, in seconds. See [`NasCredentials::interval_secs`].
fn interval() -> u64 {
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> NasCredentials {
        NasCredentials {
            host: "192.168.1.10".into(),
            port: 22,
            user: "nasstat".into(),
            password: "hunter2".into(),
            interval_secs: 5,
            host_key: None,
        }
    }

    /// A file written is a file read, which is the whole of what the `derive` is for.
    #[test]
    fn a_file_written_is_a_file_read() {
        let written = credentials();

        assert_eq!(
            NasCredentials::parse(&written.to_file().unwrap()).unwrap(),
            written
        );
    }

    /// A password with spaces, quotes or backslashes survives, because a password nobody can spell is
    /// worse than one everybody can read.
    #[test]
    fn a_password_with_spaces_or_quotes_survives() {
        for password in ["  padded  ", "\"\"", "a \"quoted\" word", "pass\\word"] {
            let written = NasCredentials {
                password: password.into(),
                ..credentials()
            };

            assert_eq!(
                NasCredentials::parse(&written.to_file().unwrap()).unwrap(),
                written,
                "{password:?} came back changed"
            );
        }
    }

    /// The port and the interval have defaults, because a file that names a machine and an account is
    /// a file somebody meant to write; the host and the user do not, because half a login is not one.
    #[test]
    fn the_machine_and_the_login_are_required_and_the_rest_has_defaults() {
        let read = NasCredentials::parse("[nas]\nhost = \"nas.local\"\nuser = \"nasstat\"\n").unwrap();

        assert_eq!(read.port, 22, "SSH is on 22 unless somebody moved it");
        assert_eq!(read.interval_secs, 5);
        assert_eq!(read.password, "");
        assert_eq!(read.host_key, None, "and nothing has looked yet");

        assert!(
            NasCredentials::parse("[nas]\nuser = \"nasstat\"\n").is_err(),
            "a file with no machine in it is not a NAS file"
        );
        assert!(
            NasCredentials::parse("[nas]\nhost = \"nas.local\"\n").is_err(),
            "and neither is one with no account"
        );
    }

    /// A key this version does not know is a field this version does not have, and a downgrade costs
    /// the radio before it costs the NAS.
    #[test]
    fn a_value_a_later_version_wrote_is_ignored() {
        let read = NasCredentials::parse(
            "[nas]\n\
             host = \"nas.local\"\n\
             user = \"nasstat\"\n\
             key_file = \"/internal/keys/nas\"\n\
             \n\
             [snmp]\n\
             community = \"public\"\n",
        )
        .unwrap();

        assert_eq!(read.host, "nas.local");
        assert_eq!(read.user, "nasstat");
    }

    /// Through the filesystem, not only through the parser — and it went where the constant says.
    #[test]
    fn a_saved_nas_comes_back() {
        let root = std::env::temp_dir()
            .join("pomelo-hal-nas-credentials")
            .join("round-trip");
        let _ = fs::remove_dir_all(&root);

        credentials().save(&root).unwrap();

        assert_eq!(NasCredentials::load(&root).unwrap(), Some(credentials()));
        assert!(path(&root).ends_with("NAS/nas.conf"), "{:?}", path(&root));

        NasCredentials::forget(&root).unwrap();
        assert_eq!(NasCredentials::load(&root).unwrap(), None);
    }

    /// A file that is there and wrong says so, on the line it is on.
    #[test]
    fn a_file_that_is_there_and_wrong_says_so() {
        let root = std::env::temp_dir()
            .join("pomelo-hal-nas-credentials")
            .join("malformed");
        let file = path(&root);
        let _ = fs::remove_dir_all(&root);

        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "[nas]\nnot a pair\n").unwrap();

        let error = NasCredentials::load(&root).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 2"), "{error}");
    }
}
