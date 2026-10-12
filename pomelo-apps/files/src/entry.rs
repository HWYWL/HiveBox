//! One line in the list: what a directory holds, as values.
//!
//! An [`Entry`] is a snapshot of one name, taken when the directory was listed. It is a plain value
//! and not a `DirEntry` handle, so that the list the page draws is a value the page owns — a file
//! that is renamed or removed under it does not change what is on screen until the next listing,
//! which is the same thing every file manager does and the only thing that can be drawn.

use std::path::Path;

/// What an entry is, as the three things a list draws differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A directory: openable, and something to go into.
    Directory,
    /// A regular file: shown with its size.
    File,
    /// Everything else — a socket, a device, a link this board has no business following.
    Other,
}

impl Kind {
    /// Whether this is something the app opens rather than something it shows.
    pub fn is_dir(self) -> bool {
        self == Kind::Directory
    }
}

/// One name in a directory, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The name alone: `photo.jpg`, and not the path it is at.
    pub name: String,
    /// The whole path, which is what every action on it takes.
    pub path: String,
    /// What it is.
    pub kind: Kind,
    /// How many bytes, for a file. A directory's size is the directories' business and not this
    /// page's, so it is reported as nought rather than as the size of one empty block.
    pub size: u64,
}

impl Entry {
    /// Read one name, or `None` if the filesystem will not say what it is.
    ///
    /// A name that cannot be `stat`ed is *dropped* rather than drawn as a mystery: a file that has
    /// gone away between the listing and this call is not a thing to show, and a row that says it
    /// cannot be read is a row that will not be there when the page is opened again.
    pub fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let name = path.file_name()?.to_string_lossy().into_owned();

        let kind = if meta.is_dir() {
            Kind::Directory
        } else if meta.is_file() {
            Kind::File
        } else {
            Kind::Other
        };

        Some(Self {
            name,
            path: path.to_string_lossy().into_owned(),
            kind,
            size: if kind.is_dir() { 0 } else { meta.len() },
        })
    }

    /// Whether this is a directory.
    pub fn is_dir(&self) -> bool {
        self.kind.is_dir()
    }

    /// The name's extension, for the label beside a file: `jpg`, lowercased.
    pub fn extension(&self) -> Option<String> {
        let (_, extension) = self.name.rsplit_once('.')?;

        if extension.is_empty() {
            None
        } else {
            Some(extension.to_lowercase())
        }
    }
}

/// The entries, ordered the way every file manager orders them: directories first, then by name.
///
/// Directories first because they are the ones a finger goes *into*, and a name-collator that mixed
/// them in with the files would put `2024` between two of its own photographs. The comparison is
/// case-insensitive with a case-sensitive tie-break, so that `Readme` and `readme` are neighbours
/// and their order is stable rather than whatever the filesystem happened to hand back.
pub fn sorted(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });

    entries
}

/// A byte count in the unit that fits it: `512 B`, `8.0 KB`, `1.2 GB`.
///
/// Binary units, and exact below a kilobyte, for the same reason the overview's are: what is on this
/// page is a filesystem's own numbers, and `1.0 KB` for 1024 bytes hides whether a file is empty.
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];

    if count < 1024 {
        return format!("{count} B");
    }

    let mut value = count as f64 / 1024.0;
    let mut unit = 0;

    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: Kind, size: u64) -> Entry {
        Entry {
            name: String::from(name),
            path: format!("/internal/{name}"),
            kind,
            size,
        }
    }

    #[test]
    fn directories_come_first_and_names_are_sorted_without_regard_to_case() {
        let listed = vec![
            entry("zebra.txt", Kind::File, 1),
            entry("Banana", Kind::Directory, 0),
            entry("apple.txt", Kind::File, 1),
            entry("album", Kind::Directory, 0),
            entry("Cherry.txt", Kind::File, 1),
        ];

        let ordered = sorted(listed);
        let names: Vec<&str> = ordered.iter().map(|one| one.name.as_str()).collect();

        assert_eq!(names, ["album", "Banana", "apple.txt", "Cherry.txt", "zebra.txt"]);
    }

    #[test]
    fn a_count_is_shown_in_the_unit_that_fits_it() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn an_extension_is_what_follows_the_last_dot() {
        let photo = entry("holiday.JPG", Kind::File, 10);
        assert_eq!(photo.extension().as_deref(), Some("jpg"));

        let bare = entry("README", Kind::File, 10);
        assert_eq!(bare.extension(), None);

        let dotted = entry("archive.tar.gz", Kind::File, 10);
        assert_eq!(dotted.extension().as_deref(), Some("gz"));

        let trailing = entry("odd.", Kind::File, 10);
        assert_eq!(trailing.extension(), None, "a dot with nothing after it is not an extension");
    }

    #[test]
    fn a_directory_reports_no_size_of_its_own() {
        let directory = entry("album", Kind::Directory, 4096);
        assert!(directory.is_dir());
        assert_eq!(directory.extension(), None);
    }
}
