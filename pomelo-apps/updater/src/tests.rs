//! The page's decisions, tested without a window and without a network.
//!
//! Three things decide what a person sees: whether a release is *newer* than what is installed
//! ([`Updater::offer`]), which table on the card counts as the installed one ([`installed_font`]), and
//! which words the page has for each state ([`Words`]). Each is a pure question about data the network
//! only supplies, so each is asked here with the data written out.

use std::sync::Arc;

use pomelo_hal::Board;
use pomelo_widgets::preferences::Language;

use crate::release::{Kind, Manifest, Version};
use crate::{installed_font, newest_font_table_in};
use crate::{Installed, Message, Status, Updater, Words};

/// What the repository would have answered: the firmware a release ahead of the one the box is running,
/// and a table.
const MANIFEST: &str = r#"{
    "firmware": {
        "version": "0.7.0",
        "file": "hivebox-firmware-v0.7.0.bin",
        "size": 7222464,
        "sha256": "3f2a"
    },
    "font": {
        "version": "2",
        "file": "hivebox-font-24px-v2.bin",
        "size": 2080400,
        "sha256": "9c41"
    }
}"#;

/// A page with the answer already read and the box already looked at, which is the state every test
/// below is *about* — the request that produces it is a second of TLS and not a decision.
///
/// A page that reached this state on its own would do it through `poll`, which needs a board that
/// answers and a clock that ticks. Neither is what these tests are asking about.
fn app(manifest: &str, firmware: Option<&str>, font: Option<&str>) -> Updater {
    let mut app = Updater::new(Arc::new(Board::simulated()));

    app.manifest = Manifest::parse(manifest);
    app.installed = Installed {
        firmware: firmware.and_then(Version::parse),
        font: font.and_then(Version::parse),
    };

    app
}

/// A directory to put tables in, with the previous run's left-overs gone.
///
/// `temp_dir` rather than a test directory in the tree: the page reads a *path* and does not care which
/// filesystem is under it, and a test that wrote into the repository would leave files behind when it
/// failed.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hivebox-updater-{name}"));

    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");

    dir
}

/// A table with `name` in `dir`, and nothing in it.
fn table(dir: &std::path::Path, name: &str) {
    std::fs::write(dir.join(name), b"").expect("a file");
}

/// The comparison: a release is offered when it is newer, and not when it is the same one written
/// differently or an older one.
#[test]
fn an_offer_is_only_made_for_something_newer() {
    let ahead = app(MANIFEST, Some("v0.6.0"), Some("v1"));
    assert_eq!(ahead.offer(Kind::Firmware).unwrap().version.text(), "v0.7.0");
    assert_eq!(ahead.offer(Kind::Font).unwrap().version.text(), "v2");

    let current = app(MANIFEST, Some("v0.7.0"), Some("v2"));
    assert!(current.offer(Kind::Firmware).is_none());
    assert!(current.offer(Kind::Font).is_none());

    let behind = app(MANIFEST, Some("v0.8.0"), Some("v9"));
    assert!(behind.offer(Kind::Firmware).is_none());
    assert!(behind.offer(Kind::Font).is_none());

    // The same version said two ways is not an update to itself, and this is the comparison a `git
    // describe`'s tail makes possible: the box is running `v0.7.0` three commits on, and the release is
    // `v0.7.0`.
    let described = app(MANIFEST, Some("v0.7.0-3-g1a2b"), None);
    assert!(described.offer(Kind::Firmware).is_none());
}

/// A box that cannot say what it is running is offered the newest thing there is, and a box with no card
/// has nothing to compare a table against.
///
/// The two rows take the same evidence differently on purpose: an unreadable *firmware* version means
/// the comparison could not be made, and the answer to "could not be compared" is the newest one; a
/// missing *table* is a box that has never been given one, which is the very thing an update is for.
#[test]
fn a_version_that_could_not_be_read_is_offered_the_newest_one() {
    let unknown = app(MANIFEST, None, None);

    assert!(unknown.offer(Kind::Firmware).is_some());
    assert!(unknown.offer(Kind::Font).is_some());
}

/// A manifest that names nothing leaves both rows with nothing to offer, whatever is installed.
#[test]
fn a_manifest_that_names_nothing_offers_nothing() {
    let empty = app(r#"{"error": "not found"}"#, Some("v0.6.0"), Some("v1"));

    assert!(empty.offer(Kind::Firmware).is_none());
    assert!(empty.offer(Kind::Font).is_none());
}

/// The newest table on the card is the installed one, and the version comes off the file's name.
#[test]
fn the_newest_table_on_the_card_is_the_one_that_counts() {
    let dir = scratch("newest");

    table(&dir, "hivebox-font-24px-v1.bin");
    table(&dir, "hivebox-font-24px-v3.bin");
    table(&dir, "hivebox-font-24px-v2.bin");

    let installed = installed_font(dir.to_str().unwrap()).expect("a table");

    assert_eq!(installed.text(), "v3");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Anything in the directory that is not a table is skipped, and a directory that is not there is a box
/// with no card rather than a fault.
#[test]
fn anything_that_is_not_a_table_is_skipped() {
    let dir = scratch("skipped");

    table(&dir, "README.md");
    table(&dir, "glyphs.bin");
    table(&dir, "hivebox-font-24px-v2.bin");

    assert_eq!(
        installed_font(dir.to_str().unwrap()).unwrap().text(),
        "v2",
        "the only name with a version in it"
    );

    let _ = std::fs::remove_dir_all(&dir);

    assert!(installed_font("/no/such/hivebox/fonts").is_none());
}

/// A directory with nothing readable in it is a box with no table, which is a state and not a fault.
#[test]
fn a_card_with_no_tables_has_no_installed_version() {
    let dir = scratch("empty");

    assert!(installed_font(dir.to_str().unwrap()).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

/// The newest table comes back with the path it is at, and not only its version.
///
/// The boot reads the *file*, so a version without a path would be an answer it could do nothing with —
/// and a path that is not the newest one is the failure this guards: a box drawn with an older table
/// than the page says it has.
#[test]
fn the_newest_table_comes_back_with_its_path() {
    let dir = scratch("path");

    table(&dir, "hivebox-font-24px-v1.bin");
    table(&dir, "hivebox-font-24px-v2.bin");

    let (version, path) = newest_font_table_in(dir.to_str().unwrap()).expect("a table");

    assert_eq!(version.text(), "v2");
    assert!(
        path.ends_with("hivebox-font-24px-v2.bin"),
        "{path} is the newest table's path"
    );
    assert!(std::path::Path::new(&path).is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

/// The boot and the page read the one scan, so they cannot name two different tables.
#[test]
fn the_page_and_the_boot_read_the_same_table() {
    let dir = scratch("agree");

    table(&dir, "hivebox-font-24px-v1.bin");
    table(&dir, "hivebox-font-24px-v3.bin");
    table(&dir, "hivebox-font-24px-v2.bin");
    let dir = dir.to_str().unwrap();

    assert_eq!(
        installed_font(dir).map(|version| version.text().to_owned()),
        newest_font_table_in(dir).map(|(version, _)| version.text().to_owned()),
        "the version the page names is the version of the file the boot reads"
    );

    let _ = std::fs::remove_dir_all(std::path::Path::new(dir));
}

/// Pressing a row's button says what this image can do about it, which today is nothing — and says it
/// per row, because a new firmware and a new table need different things of a person.
#[test]
fn a_press_says_which_row_it_was_for() {
    let mut page = app(MANIFEST, Some("v0.6.0"), Some("v1"));

    page.update(Message::Install(Kind::Firmware));
    assert_eq!(page.status, Status::Unsupported(Kind::Firmware));

    page.update(Message::Install(Kind::Font));
    assert_eq!(page.status, Status::Unsupported(Kind::Font));
}

/// Both rows have a name in both languages, and the two languages are told apart by the words
/// themselves rather than by a literal compared against a title.
#[test]
fn both_rows_are_named_in_both_languages() {
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);

    assert_eq!(english.name(Kind::Firmware), "Firmware");
    assert_eq!(english.name(Kind::Font), "Glyph table");
    assert_eq!(chinese.name(Kind::Firmware), "固件");
    assert_eq!(chinese.name(Kind::Font), "字库");

    assert!(!english.chinese);
    assert!(chinese.chinese);
}

/// A version that is installed is drawn as one, a table that is not installed says so, and a firmware
/// version that could not be read says *that* — three different sentences, because they are three
/// different states.
#[test]
fn what_is_installed_is_drawn_differently_from_what_could_not_be_read() {
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);
    let version = Version::parse("v0.7.0").unwrap();

    assert_eq!(english.installed_line(Some(&version), Kind::Firmware), "Installed v0.7.0");
    assert_eq!(chinese.installed_line(Some(&version), Kind::Font), "当前 v0.7.0");
    assert_eq!(english.installed_line(None, Kind::Font), "not installed");
    assert_eq!(chinese.installed_line(None, Kind::Font), "未安装");
    assert_eq!(english.installed_line(None, Kind::Firmware), "unknown");
}

/// A fault is a sentence in the page's own language, and every fault in the list has one.
#[test]
fn every_fault_has_a_sentence_in_both_languages() {
    use pomelo_hal::HttpFault;

    let faults = [
        HttpFault::NoNetwork,
        HttpFault::ClockNotSet,
        HttpFault::Unreachable,
        HttpFault::Timeout,
        HttpFault::Tls,
        HttpFault::TooLarge,
        HttpFault::Storage,
        HttpFault::Unsupported,
    ];

    for fault in faults {
        assert!(!Words::of(Language::English).fault_line(fault).is_empty());
        assert!(!Words::of(Language::Chinese).fault_line(fault).is_empty());
    }
}
