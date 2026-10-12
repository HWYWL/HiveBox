//! Paths, as this page spells them.
//!
//! A file manager is mostly string surgery over paths, and all of it is a pure function of two
//! strings — which is why it is here and not in the app: the rules about where "up" goes and what a
//! name may contain are the part that can be tested with no filesystem in sight.

/// The longest a name may be, in bytes. The limit every filesystem this board can mount agrees on.
pub const MAX_NAME: usize = 255;

/// `dir` and `name`, as one path with exactly one slash between them.
///
/// Trailing slashes are trimmed from `dir` first, because a mount point is spelled `/sdcard/` in one
/// place and `/sdcard` in another and a path with `//` in the middle is one a person has to squint
/// at. A `dir` that is *only* slashes - the root - keeps its one, so that joining onto `/` gives
/// `/name` and not `name`.
pub fn join(dir: &str, name: &str) -> String {
    let trimmed = dir.trim_end_matches('/');

    if trimmed.is_empty() {
        format!("/{name}")
    } else {
        format!("{trimmed}/{name}")
    }
}

/// The directory above `dir`, or `None` when `dir` *is* the volume's root.
///
/// `None` and not the root itself, because the two are different answers to a finger: "there is
/// nowhere further up" is what the up button has to know to grey itself out, and handing back the
/// root it is already at would make the button look live and do nothing.
pub fn parent(dir: &str, root: &str) -> Option<String> {
    let dir = dir.trim_end_matches('/');
    let root = root.trim_end_matches('/');

    if dir == root || dir.is_empty() {
        return None;
    }

    match dir.rfind('/') {
        Some(0) => Some(String::from("/")),
        Some(index) => {
            let above = &dir[..index];

            if above.len() < root.len() {
                Some(root.to_string())
            } else {
                Some(above.to_string())
            }
        }
        None => None,
    }
}

/// Whether `dir` is the volume's root, as [`parent`] answers it.
pub fn is_root(dir: &str, root: &str) -> bool {
    parent(dir, root).is_none()
}

/// Whether `name` is a name that can be made in a directory.
///
/// The dot names are refused rather than resolved: `.` and `..` are the two the filesystem already
/// means something by, and a file manager that let a person "create" one would be renaming a
/// directory behind their back. A name with a slash in it is refused for the same kind of reason —
/// it names a path and not an entry, and the entry it would make is not the one that was typed.
pub fn valid_name(name: &str) -> bool {
    let name = name.trim();

    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0')
        && name.len() <= MAX_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_join_leaves_exactly_one_slash() {
        assert_eq!(join("/internal", "notes"), "/internal/notes");
        assert_eq!(join("/internal/", "notes"), "/internal/notes");
        assert_eq!(join("/", "notes"), "/notes");
        assert_eq!(join("/internal/a", "b.txt"), "/internal/a/b.txt");
    }

    #[test]
    fn up_stops_at_the_root_and_nowhere_above_it() {
        assert_eq!(parent("/internal/a/b", "/internal"), Some(String::from("/internal/a")));
        assert_eq!(parent("/internal/a", "/internal"), Some(String::from("/internal")));
        assert_eq!(parent("/internal", "/internal"), None, "the root has no parent");
        assert_eq!(parent("/internal/", "/internal/"), None, "however it is spelled");
        assert!(is_root("/internal", "/internal"));
        assert!(!is_root("/internal/a", "/internal"));
    }

    #[test]
    fn a_name_is_one_entry_and_not_a_path() {
        assert!(valid_name("notes.txt"));
        assert!(valid_name("相册"));

        assert!(!valid_name(""), "an empty name is not a name");
        assert!(!valid_name("   "), "nor is a space");
        assert!(!valid_name("."));
        assert!(!valid_name(".."));
        assert!(!valid_name("a/b"), "a slash names a path, not an entry");
        assert!(!valid_name(&"x".repeat(MAX_NAME + 1)), "and long enough is not a name here");
    }
}
