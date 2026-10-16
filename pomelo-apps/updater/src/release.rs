//! What the repository publishes, and how this page reads two versions off it.
//!
//! One document, at a URL that does not change: [`MANIFEST_URL`]. It names the firmware image and the
//! glyph table, and the page's whole job is reading a version out of each and holding it up against the
//! one on the box.
//!
//! # A table can bring the face it was baked from
//!
//! The image's member names one file and always does; the *font's* names one file, or two. A table is
//! nothing on its own — its glyph numbers are the numbers of the face it was baked from — so a table
//! that covers more than the image's subset has to arrive with that face, and the manifest says so the
//! only way it can: a `"face"` member beside the `"file"`, naming the half that is not the table. See
//! [`Release::face`].
//!
//! The version is the *table's*, read off the table's own name, and the face carries none of its own
//! for the same reason it is not a release: the two are one font, and a face whose name disagreed with
//! the table's would be a number nothing on this page could compare. The publisher is what keeps them
//! in step — it refuses a directory that is not exactly one pair, in one version.
//!
//! # The version is in the file name
//!
//! `hivebox-firmware-v0.7.0.bin`, `hivebox-font-24px-v1.bin`: a file name that says what is inside it is
//! a file a person can read off a directory listing, and it is what makes the check a *comparison*
//! rather than a download — the page decides whether it wants the file before it asks for a byte of it.
//! [`Version::from_file_name`] is that read, and [`Manifest::parse`] prefers it to any separate
//! `"version"` field, so that the name on the repository's own page and the version this box compares
//! against cannot disagree.
//!
//! # The firmware's own version is a `git describe`
//!
//! The image is stamped with the tag it was built from and how far past it the build went —
//! `v0.6.0-3-g1a2b` — where a release file is simply `v0.7.0`. [`Version::parse`] reads the leading
//! numbers of either and stops at the first thing that is not one of them, which is what makes a nightly
//! build comparable to a release: three commits past `v0.6.0` is *not* newer than `v0.7.0`, and the
//! comparison is a comparison of the tags rather than of the strings.

use crate::json;

/// Where the manifest lives.
///
/// The raw file on the mirror of this project's own repository, and not an API endpoint: a raw file is
/// one request with no token, no rate limit and no JSON envelope around the JSON, which is a smaller
/// thing to parse on a board that has no JSON library. The branch rather than a tag, because the manifest
/// is the *pointer* to the newest files and a pointer that only moved at release time would be a pointer
/// that was always one release behind.
///
/// `main`, which is what the branch is actually called on both the repository and its mirror. A branch
/// name that does not exist is a 404 and not a fallback, so this one string has to match something
/// outside this repository — it is the only thing on this page that a rename elsewhere can break.
pub const MANIFEST_URL: &str = "https://gitee.com/hwyhy/HiveBox/raw/main/bin/manifest.json";

/// Which of the two things this page updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The image this box is running. Written to the flash, and the box reboots into it.
    Firmware,
    /// The 24 px glyph table. Written to the card, and read by the interface from there.
    Font,
}

impl Kind {
    /// The manifest's member for it: `"firmware"`, `"font"`.
    pub fn member(self) -> &'static str {
        match self {
            Kind::Firmware => "firmware",
            Kind::Font => "font",
        }
    }

}

/// A version, as a repository writes one and this page compares two of.
///
/// The numbers, and the text they were written as — both, because they answer different questions: the
/// numbers decide which of two is newer, and the text is what the page puts on the screen. A version
/// that only kept its text could not be compared, and one that only kept its numbers could not be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    text: String,
    numbers: Vec<u64>,
}

impl Version {
    /// Reads the leading version out of a string: `v0.7.0`, `0.7.0`, `v1`.
    ///
    /// Stops at the first character that is neither a digit nor a dot, so a `git describe`'s tail —
    /// `-3-g1a2b`, how far past the tag the build went — is dropped rather than parsed. `None` when
    /// there is no version at the front at all, which is a repository that named a file something this
    /// page cannot compare.
    pub fn parse(text: &str) -> Option<Self> {
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

    /// Reads the version a release file's name carries: `hivebox-font-24px-v1.bin` is `v1`.
    ///
    /// The *last* `v`-then-a-digit in the name, because the name is a description before it is a
    /// version: `24px` in the font's is a size and not a number of anything to compare, so the read has
    /// to prefer the `v` that marks the version rather than the first number it comes across.
    pub fn from_file_name(name: &str) -> Option<Self> {
        let bytes = name.as_bytes();
        let mut found = None;

        for at in 0..bytes.len().saturating_sub(1) {
            if matches!(bytes[at], b'v' | b'V') && bytes[at + 1].is_ascii_digit() {
                found = Some(at);
            }
        }

        Self::parse(&name[found?..])
    }

    /// What to put on the screen: `v0.7.0`.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether this one is a later version than `other`.
    ///
    /// Field by field, and a field one of them does not have counts as zero: `v1` and `v1.0.0` are the
    /// same version written two ways, and a comparison that said otherwise would offer a person an
    /// update to what they already have.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        let fields = self.numbers.len().max(other.numbers.len());

        for at in 0..fields {
            let mine = self.numbers.get(at).copied().unwrap_or(0);
            let theirs = other.numbers.get(at).copied().unwrap_or(0);

            if mine != theirs {
                return mine > theirs;
            }
        }

        false
    }
}

/// One file the repository publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub kind: Kind,
    pub version: Version,
    /// The file's name on the repository, which is the version's own spelling and what the page shows
    /// when it says what it would download.
    pub file: String,
    /// How many bytes it is. `0` for a manifest that did not say, which is a size the page does not
    /// draw rather than one it draws as zero.
    pub size: u64,
    /// The file's SHA-256, lowercase hex. Empty for a manifest that did not say.
    pub sha256: String,
    /// The face this release's table was baked from, when the manifest named one: the second half of a
    /// font package, and `None` for every release that is one file — the image, and a table baked from
    /// the subset the image itself carries. See this module's header.
    pub face: Option<Face>,
}

/// The other half of a font package: the face a table's glyph numbers are the numbers of.
///
/// A [`Release`]'s file that is not a table, and not a release of its own: it has no version and no
/// kind, because it is nothing without the table beside it — one font, published as two files, and the
/// version they share is read off the table's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    /// Its name on the repository, which is also the name it takes on the card.
    pub file: String,
    /// How many bytes it is. `0` for a manifest that did not say.
    pub size: u64,
    /// Its SHA-256, lowercase hex. Empty for a manifest that did not say.
    pub sha256: String,
}

/// What the repository has, as its own manifest said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub firmware: Option<Release>,
    pub font: Option<Release>,
}

impl Manifest {
    /// Reads a manifest.
    ///
    /// Never fails, and that is deliberate: a document that is not JSON, one that is JSON but not this
    /// project's manifest, and one that is this project's manifest with a file missing are three things
    /// a person does the same thing about — press the button again later — and the page draws one line
    /// for all three. The release it *did* find is still a release, so a manifest naming only the
    /// firmware is a page that still offers a firmware update.
    pub fn parse(document: &str) -> Self {
        Self {
            firmware: Release::read(document, Kind::Firmware),
            font: Release::read(document, Kind::Font),
        }
    }

    /// What the repository has of one kind.
    pub fn of(&self, kind: Kind) -> Option<&Release> {
        match kind {
            Kind::Firmware => self.firmware.as_ref(),
            Kind::Font => self.font.as_ref(),
        }
    }

    /// Whether it named anything at all.
    pub fn is_empty(&self) -> bool {
        self.firmware.is_none() && self.font.is_none()
    }
}

impl Release {
    /// Where this file is, on the repository.
    pub fn url(&self) -> String {
        url_of(&self.file)
    }

    /// One member of the manifest, if it is there and complete enough to download.
    fn read(document: &str, kind: Kind) -> Option<Self> {
        let object = json::member(document, kind.member())?;
        let file = json::string(object, "file")?.to_string();

        // The file name first, and the manifest's own `"version"` only as a fallback: see this module's
        // header. A release whose name carries no version is still a release if the manifest said the
        // version separately, and one that says it in neither place is not a release this page can hold
        // up against anything.
        let version = Version::from_file_name(&file)
            .or_else(|| json::string(object, "version").and_then(Version::parse))?;

        Some(Self {
            kind,
            version,
            file,
            size: json::number(object, "size").unwrap_or(0),
            sha256: json::string(object, "sha256").unwrap_or_default().to_string(),
            // Only the font's member can carry one: the image is one file by definition, and a member
            // beside *it* would be a manifest describing something no press on this page asks for.
            face: match kind {
                Kind::Font => Face::read(object),
                Kind::Firmware => None,
            },
        })
    }
}

impl Face {
    /// Where this file is, on the repository. The same directory as the manifest and the table beside
    /// it — see [`url_of`].
    pub fn url(&self) -> String {
        url_of(&self.file)
    }

    /// The `"face"` member of a font's object, when it is there and complete enough to download.
    ///
    /// A face with no file name is not one: there would be nothing to fetch and no name to put on the
    /// card, so a manifest that named half a package is read as one that named none — the table stands
    /// on its own, which is what a lone `"file"` means everywhere else here.
    fn read(object: &str) -> Option<Self> {
        let face = json::member(object, "face")?;
        let file = json::string(face, "file")?.to_string();

        Some(Self {
            file,
            size: json::number(face, "size").unwrap_or(0),
            sha256: json::string(face, "sha256").unwrap_or_default().to_string(),
        })
    }
}

/// Where a file the manifest names is, on the repository.
///
/// Derived from [`MANIFEST_URL`] rather than written down, because the manifest and the files it names
/// are one directory: a second address here would be a second string to keep in step with the branch
/// name, and the branch name is the one thing on this page a rename elsewhere can break — see
/// [`MANIFEST_URL`]. Deriving it means the two cannot be on different branches, which is a failure that
/// would look like a manifest offering a file that is not there.
///
/// The name is the file's own and never a path: a manifest naming `"card/…"` would be fetched from
/// there and *written* under the directory the card's tables live in, because the two halves of a
/// download — where it comes from and where it goes — are one name on this page, deliberately. What the
/// page puts on the card is a file in the place the interface scans, or it is not a font.
fn url_of(file: &str) -> String {
    let dir = MANIFEST_URL
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or(MANIFEST_URL);

    format!("{dir}/{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest this project publishes, as the build script writes it: a merged image, and a font
    /// that is two files — a table, and the face it was baked from.
    const MANIFEST: &str = r#"{
        "generated": "2026-10-16T09:00:00Z",
        "firmware": {
            "version": "0.7.0",
            "file": "hivebox-firmware-v0.7.0.bin",
            "size": 7222464,
            "sha256": "3f2a"
        },
        "font": {
            "version": "3",
            "file": "hivebox-font-24px-v3.bin",
            "size": 2268084,
            "sha256": "1cfb",
            "face": {
                "file": "hivebox-face-v3.otf",
                "size": 1910512,
                "sha256": "0f48"
            }
        }
    }"#;

    /// Both releases are read, and the version comes out of the file name rather than out of the field,
    /// because the two are the same answer written twice and the name is the one a person can see.
    #[test]
    fn a_manifest_names_both_releases_with_the_version_in_the_file_name() {
        let manifest = Manifest::parse(MANIFEST);
        let firmware = manifest.of(Kind::Firmware).expect("the firmware");
        let font = manifest.of(Kind::Font).expect("the font");

        assert_eq!(firmware.file, "hivebox-firmware-v0.7.0.bin");
        assert_eq!(firmware.version.text(), "v0.7.0");
        assert_eq!(firmware.size, 7_222_464);
        assert_eq!(font.version.text(), "v3");
        assert_eq!(font.size, 2_268_084);
        assert!(!manifest.is_empty());
    }

    /// The font's second half is read, and both halves are addressed out of the manifest's own
    /// directory: a face somewhere else on the repository would be a file the page fetches from a
    /// directory no release of this project has ever been published in.
    #[test]
    fn a_font_names_the_face_beside_the_table() {
        let font = Manifest::parse(MANIFEST).font.expect("the font");
        let face = font.face.as_ref().expect("the face the table was baked from");

        assert_eq!(face.file, "hivebox-face-v3.otf");
        assert_eq!(face.size, 1_910_512);
        assert_eq!(face.sha256, "0f48");

        let directory = |url: &str| url.rsplit_once('/').expect("a path").0.to_string();
        assert_eq!(
            directory(&face.url()),
            directory(&font.url()),
            "one directory for the manifest and both halves of the font"
        );
        assert!(
            face.url().ends_with("/hivebox-face-v3.otf"),
            "{} is the face under its own name",
            face.url()
        );
    }

    /// A face is read only when the manifest names it *whole*: a table on its own has none, half a face
    /// member is none, and the image's member never has one.
    ///
    /// The half-named case is the one worth being strict about. A `"face"` with no file in it has no
    /// name to fetch and no name to put on the card, so it is read as a manifest that named no package
    /// at all — the table stands on its own, which is what a lone `"file"` means everywhere else here.
    #[test]
    fn a_face_is_read_only_when_the_manifest_names_it_whole() {
        let alone = r#"{"font": {"file": "hivebox-font-24px-v1.bin"}}"#;
        assert!(Manifest::parse(alone).font.unwrap().face.is_none());

        let half = r#"{"font": {"file": "hivebox-font-24px-v3.bin", "face": {"size": 12}}}"#;
        assert!(Manifest::parse(half).font.unwrap().face.is_none());

        let image = r#"{"firmware": {"file": "hivebox-firmware-v0.7.0.bin",
                                     "face": {"file": "hivebox-face-v3.otf"}}}"#;
        assert!(
            Manifest::parse(image).firmware.unwrap().face.is_none(),
            "an image is one file, whatever a manifest says beside it"
        );
    }

    /// A version is read from each of the three shapes a version is written in on this project: a
    /// release tag, a `git describe`'s tag-and-distance, and a file's name.
    #[test]
    fn a_version_is_read_out_of_a_tag_a_describe_and_a_file_name() {
        assert_eq!(Version::parse("v0.7.0").unwrap().text(), "v0.7.0");
        assert_eq!(Version::parse("0.7.0").unwrap().text(), "v0.7.0");
        assert_eq!(Version::parse("v1").unwrap().text(), "v1");

        // The tail a describe carries says how far *past* the tag the build went, and it is not a part
        // of the version: `v0.6.0-3-g1a2b` is the `v0.6.0` release, three commits on.
        let described = Version::parse("v0.6.0-3-g1a2b").unwrap();
        assert_eq!(described.text(), "v0.6.0");
        assert!(!described.is_newer_than(&Version::parse("v0.6.0").unwrap()));

        // The font's name has a size in it before it has a version, and the size is not the version.
        assert_eq!(
            Version::from_file_name("hivebox-font-24px-v1.bin")
                .unwrap()
                .text(),
            "v1"
        );
        assert_eq!(
            Version::from_file_name("hivebox-firmware-v0.7.0.bin")
                .unwrap()
                .text(),
            "v0.7.0"
        );
        assert_eq!(Version::from_file_name("hivebox-firmware.bin"), None);
        assert_eq!(Version::parse("nightly"), None);
        assert_eq!(Version::parse(""), None);
    }

    /// Newer is newer field by field, and a version written with fewer fields is the same version.
    #[test]
    fn a_comparison_is_field_by_field_and_pads_with_zeros() {
        let older = Version::parse("v0.6.0").unwrap();
        let newer = Version::parse("v0.7.0").unwrap();
        let patch = Version::parse("v0.6.1").unwrap();
        let major = Version::parse("v1").unwrap();

        assert!(newer.is_newer_than(&older));
        assert!(patch.is_newer_than(&older));
        assert!(!older.is_newer_than(&patch));
        assert!(!older.is_newer_than(&older), "and it is not newer than itself");
        assert!(major.is_newer_than(&Version::parse("v0.9.9").unwrap()));

        // The same version written two ways is not an update.
        let one = Version::parse("v1").unwrap();
        let two = Version::parse("v1.0.0").unwrap();
        assert!(!one.is_newer_than(&two));
        assert!(!two.is_newer_than(&one));
    }

    /// A document that is not a manifest is an empty one, and half a manifest is still half a manifest.
    #[test]
    fn a_document_that_is_not_a_manifest_names_nothing() {
        assert!(Manifest::parse("").is_empty());
        assert!(Manifest::parse("<html>a captive portal</html>").is_empty());
        assert!(Manifest::parse(r#"{"error": "not found"}"#).is_empty());

        let only_firmware = r#"{"firmware": {"file": "hivebox-firmware-v0.7.0.bin"}}"#;
        let manifest = Manifest::parse(only_firmware);

        assert!(manifest.font.is_none());
        assert!(manifest.firmware.is_some());
        assert!(!manifest.is_empty());
    }

    /// A release whose name carries no version is read from the field beside it, and one that names a
    /// file with neither is not a release at all.
    #[test]
    fn a_release_needs_a_version_from_one_place_or_the_other() {
        let named = r#"{"font": {"file": "glyphs.bin", "version": "2"}}"#;
        assert_eq!(
            Manifest::parse(named).font.unwrap().version.text(),
            "v2",
            "the file name had nothing to compare, so the field is what did"
        );

        let unnamed = r#"{"font": {"file": "glyphs.bin"}}"#;
        assert!(Manifest::parse(unnamed).font.is_none());
    }
}
