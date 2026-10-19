//! The version rules the store compares two packages with.
//!
//! The rules are the release manifest's, and they are restated here rather than imported because
//! this crate runs on the board beside a page that already has its own copy: the updater reads a
//! version off a *file name* on a repository, and this reads one out of a package's metadata. The
//! behaviour is identical — leading `v`, dot-separated numbers, missing fields are zero — and the
//! tests below are the same cases the updater's are, so a change to either without the other fails
//! one of them.

use std::cmp::Ordering;

/// A version with the numbers it compares by and the text it is shown as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    text: String,
    numbers: Vec<u64>,
}

impl Version {
    /// Reads the leading version out of a string: `v1.2.0`, `1.2`, `v3`.
    ///
    /// Stops at the first character that is neither a digit nor a dot, which is what makes a
    /// `git describe`'s tail — `-3-g1a2b` — a part of the build rather than of the version.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let rest = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let head = rest[..end].trim_end_matches('.');

        if head.is_empty() {
            return None;
        }

        let numbers: Vec<u64> = head
            .split('.')
            .map(|part| part.parse().ok())
            .collect::<Option<_>>()?;

        Some(Self {
            text: format!("v{head}"),
            numbers,
        })
    }

    /// What to put on the screen.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether this one is a later version than `other`, field by field with missing fields zero.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Greater
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let fields = self.numbers.len().max(other.numbers.len());

        for at in 0..fields {
            let mine = self.numbers.get(at).copied().unwrap_or(0);
            let theirs = other.numbers.get(at).copied().unwrap_or(0);

            match mine.cmp(&theirs) {
                Ordering::Equal => continue,
                ordering => return ordering,
            }
        }

        Ordering::Equal
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_read_out_of_the_shapes_this_project_writes() {
        assert_eq!(Version::parse("v1.2.0").unwrap().text(), "v1.2.0");
        assert_eq!(Version::parse("1.2.0").unwrap().text(), "v1.2.0");
        assert_eq!(Version::parse("v1").unwrap().text(), "v1");
        assert_eq!(Version::parse("v0.6.0-3-g1a2b").unwrap().text(), "v0.6.0");
        assert_eq!(Version::parse("nightly"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn a_comparison_is_field_by_field_and_pads_with_zeros() {
        let older = Version::parse("v0.6.0").unwrap();
        let newer = Version::parse("v0.7.0").unwrap();
        let patch = Version::parse("v0.6.1").unwrap();

        assert!(newer.is_newer_than(&older));
        assert!(patch.is_newer_than(&older));
        assert!(!older.is_newer_than(&patch));
        assert!(!older.is_newer_than(&older));

        let one = Version::parse("v1").unwrap();
        let two = Version::parse("v1.0.0").unwrap();
        assert!(!one.is_newer_than(&two));
        assert!(!two.is_newer_than(&one));
    }
}
