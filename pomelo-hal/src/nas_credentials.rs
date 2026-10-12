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
//! [[device]]
//! host = "192.168.1.10"
//! port = 22
//! user = "nasstat"
//! password = "hunter2"
//! interval_secs = 5
//! host_key = "ecdsa-sha2-nistp256 AAAAE2VjZHNh…"
//!
//! [[device]]
//! host = "192.168.1.20"
//! port = 22
//! user = "nasstat"
//! password = "hunter2"
//! interval_secs = 5
//! ```
//!
//! # A list, and the shape it had before it was one
//!
//! The file names a **list** of machines, because a board can be pointed at more than one and the app
//! draws them as a list a person picks from. Before it did, it held a single `[nas]` table, and that
//! shape is still *read* as a list of one — see [`NasCredentials::parse_devices`] — so that upgrading
//! the firmware does not cost somebody the machine they had already set up. Everything written from
//! here on is written in the list's shape.
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

    /// The file's text, ready to write — in the single `[nas]` shape the file had before it held a
    /// list. What [`NasCredentials::save`] writes is the list's; this is here so that the older shape
    /// stays readable *and* written, and it is what [`NasCredentials::parse`] is tested against.
    pub fn to_file(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&Document { nas: self.clone() })
    }

    /// Every device the text names, in the order it names them.
    ///
    /// Both shapes are read: the `[[device]]` blocks the file holds now, and the single `[nas]` table it
    /// held before, which is a list of one. Refusing the older shape would cost somebody the machine
    /// they had already set up, which is the same rule the Wi-Fi file follows for its own downgrades.
    ///
    /// A host named twice is *one* device, the later entry winning: two blocks for one machine is what
    /// the web page's form leaves behind when the same box is saved twice, and a board that watched the
    /// same machine twice would be a board whose list grew every time somebody pressed save.
    pub fn parse_devices(text: &str) -> Result<Vec<Self>, toml::de::Error> {
        let file: File = toml::from_str(text)?;

        let mut devices = file.device;
        if let Some(one) = file.nas {
            devices.push(one);
        }

        Ok(unique(devices))
    }

    /// The file's text for `devices`, ready to write: one `[[device]]` block each. See
    /// [`NasCredentials::parse_devices`] for why that shape and what happens to the older one.
    pub fn file_of_devices(devices: &[Self]) -> Result<String, toml::ser::Error> {
        toml::to_string(&FileOf { device: devices })
    }

    /// Every device the file under `root` names, in the order it names them.
    ///
    /// An absent file is an empty list and not a failure — a board that has never been pointed at a
    /// machine. A file that *is* there and unreadable is one, and comes back as `InvalidData` carrying
    /// the reason.
    pub fn load_all(root: impl AsRef<Path>) -> io::Result<Vec<Self>> {
        let text = match fs::read_to_string(path(root)) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        Self::parse_devices(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// The first device the file under `root` names, if it names any. [`NasCredentials::load_all`] is
    /// the whole of it.
    pub fn load(root: impl AsRef<Path>) -> io::Result<Option<Self>> {
        Ok(Self::load_all(root)?.into_iter().next())
    }

    /// Writes `devices` to the file under `root`, making the directories on the way.
    ///
    /// Nothing to remember is nothing to write: an empty list removes the file rather than leaving one
    /// that says nothing, which is the same thing [`NasCredentials::forget`] does and the state a board
    /// with no devices is in.
    pub fn save_all(root: impl AsRef<Path>, devices: &[Self]) -> io::Result<()> {
        if devices.is_empty() {
            return Self::forget(root);
        }

        let path = path(root);
        let text = Self::file_of_devices(devices)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, text)
    }

    /// Writes this one device to the file under `root`, replacing every device that was there.
    pub fn save(&self, root: impl AsRef<Path>) -> io::Result<()> {
        Self::save_all(root, std::slice::from_ref(self))
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
///
/// This is the shape the file had while it held one machine. It is still read (see [`File`]) and still
/// written by [`NasCredentials::to_file`], which is what [`NasCredentials::parse`] is a pair with.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    nas: NasCredentials,
}

/// The whole file as it is written now: one `[[device]]` block per machine.
#[derive(Debug, Serialize)]
struct FileOf<'a> {
    device: &'a [NasCredentials],
}

/// The whole file as it is read: the list, and the one table the shape before it used.
///
/// Both fields optional, because either shape is a file this has to be able to read — and an empty one
/// is a file that names nothing, which is not an error.
#[derive(Debug, Deserialize)]
struct File {
    #[serde(default)]
    device: Vec<NasCredentials>,
    #[serde(default)]
    nas: Option<NasCredentials>,
}

/// `devices` with each host kept once: in the order it was first named, holding what it was last given.
///
/// See [`NasCredentials::parse_devices`], which is the only caller.
fn unique(devices: Vec<NasCredentials>) -> Vec<NasCredentials> {
    let mut kept: Vec<NasCredentials> = Vec::new();

    for device in devices {
        match kept.iter_mut().find(|held| held.host == device.host) {
            Some(held) => *held = device,
            None => kept.push(device),
        }
    }

    kept
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

    /// A list of machines is written and read back as one, in the order it was written.
    #[test]
    fn a_list_of_devices_survives_a_round_trip() {
        let devices = vec![
            credentials(),
            NasCredentials {
                host: String::from("192.168.1.20"),
                ..credentials()
            },
        ];

        let text = NasCredentials::file_of_devices(&devices).unwrap();

        assert!(text.contains("[[device]]"), "{text}");
        assert_eq!(NasCredentials::parse_devices(&text).unwrap(), devices);
    }

    /// The shape the file had before it held a list is a list of one, so that upgrading the firmware
    /// does not cost somebody the machine they had already set up.
    #[test]
    fn the_one_table_shape_reads_as_a_list_of_one() {
        let read = NasCredentials::parse_devices(
            "[nas]\nhost = \"nas.local\"\nuser = \"nasstat\"\npassword = \"hunter2\"\n",
        )
        .unwrap();

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].host, "nas.local");
        assert_eq!(read[0].port, 22, "with the defaults the older shape relied on");
    }

    /// One machine named twice is one device, and the later entry is the one that was meant: the web
    /// page's form appends, so a box saved twice is two blocks and the second is the correction.
    #[test]
    fn a_host_named_twice_is_one_device_and_the_later_one_wins() {
        let read = NasCredentials::parse_devices(
            "[[device]]\nhost = \"nas.local\"\nuser = \"old\"\npassword = \"old\"\n\
             [[device]]\nhost = \"other.local\"\nuser = \"nasstat\"\n\
             [[device]]\nhost = \"nas.local\"\nuser = \"new\"\npassword = \"new\"\n",
        )
        .unwrap();

        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].host, "nas.local", "in the place it was first named");
        assert_eq!(read[0].user, "new", "holding what it was last given");
        assert_eq!(read[1].host, "other.local");
    }

    /// Through the filesystem — and nothing to remember is no file rather than an empty one.
    #[test]
    fn a_list_of_devices_is_saved_and_loaded() {
        let root = std::env::temp_dir()
            .join("pomelo-hal-nas-credentials")
            .join("list-round-trip");
        let _ = fs::remove_dir_all(&root);

        let devices = vec![
            credentials(),
            NasCredentials {
                host: String::from("192.168.1.20"),
                ..credentials()
            },
        ];

        NasCredentials::save_all(&root, &devices).unwrap();

        assert_eq!(NasCredentials::load_all(&root).unwrap(), devices);
        assert_eq!(
            NasCredentials::load(&root).unwrap(),
            Some(credentials()),
            "and the one-device reader takes the first of them"
        );

        NasCredentials::save_all(&root, &[]).unwrap();

        assert!(!path(&root).exists(), "nothing to remember is no file");
        assert!(NasCredentials::load_all(&root).unwrap().is_empty());
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
