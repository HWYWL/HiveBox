//! The page's decisions, tested without a window and without a network.
//!
//! Three of them are pure questions about data the network only supplies: whether a release is *newer*
//! than what is installed ([`Updater::offer`]), which font on the card counts as the installed one
//! ([`installed_font`]), and which words the page has for each state ([`Words`]). Each is asked here
//! with the data written out.
//!
//! The fourth is the download, and it is not pure — there is a request in it, and a file at the end.
//! So it is asked against a *scripted* client on a board the test drives by hand: the ticks are the
//! board's pump, which the page deliberately does not have, and the frame is the page looking. What is
//! asserted is what a person would see: the file on the card, the bar while it is still coming, and
//! the sentence when it does not arrive.

use std::sync::Arc;
use std::time::Instant;

use pomelo_hal::sim::SimHttp;
use pomelo_hal::Board;
use pomelo_widgets::preferences::Language;

use crate::release::{Kind, Manifest, Version};
use crate::{card_font_in, installed_font};
use crate::{DownloadFault, Installed, Message, Status, Updater, Words};

/// What the repository would have answered: the firmware a release ahead of the one the box is running,
/// and a font — a package, which is the shape the project publishes and the one a press has two files in.
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
        "sha256": "9c41",
        "face": {
            "file": "hivebox-face-v2.otf",
            "size": 1910512,
            "sha256": "0f48"
        }
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

/// A directory to put font files in, with the previous run's left-overs gone.
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

/// A face with `name` in `dir`, and nothing in it. The *name* is all the scan reads, so an empty file
/// is the whole of what these tests are about; what a face has to contain is the baker's business.
fn face(dir: &std::path::Path, name: &str) {
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

/// A package comes back with both halves, and the table is the file the boot reads.
///
/// The boot reads the *file*, so a version without a path would be an answer it could do nothing with
/// — and a path that is not the newest one is the failure this guards: a box drawn with a font older
/// than the page says it has.
#[test]
fn a_package_comes_back_with_both_halves() {
    let dir = scratch("package");

    table(&dir, "hivebox-font-24px-v1.bin");
    table(&dir, "hivebox-font-24px-v2.bin");
    face(&dir, "hivebox-face-v2.otf");

    let font = card_font_in(dir.to_str().unwrap()).expect("a font");

    assert_eq!(font.version.text(), "v2");
    assert!(
        font.table.ends_with("hivebox-font-24px-v2.bin"),
        "{} is the newest table's path",
        font.table
    );

    let face = font.face.expect("the face the table was baked from");
    assert!(face.ends_with("hivebox-face-v2.otf"), "{face} is its face");
    assert!(std::path::Path::new(&font.table).is_file());
    assert!(std::path::Path::new(&face).is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

/// A complete package beats a table on its own that says a newer version.
///
/// Not a preference for packages but a refusal to hand a table to the wrong font: a lone table was
/// baked from the face in the image, so a face that came on a card can draw nothing from it — the
/// renderer refuses glyph numbers that came from another font — and every glyph on the screen would
/// then cost the rasterisation the table existed to avoid. The newest *drawable* font is the question,
/// and an older package is the answer to it.
#[test]
fn a_package_beats_a_newer_table_on_its_own() {
    let dir = scratch("package-wins");

    table(&dir, "hivebox-font-24px-v5.bin");
    table(&dir, "hivebox-font-24px-v2.bin");
    face(&dir, "hivebox-face-v2.otf");

    let font = card_font_in(dir.to_str().unwrap()).expect("a font");

    assert_eq!(font.version.text(), "v2");
    assert!(font.face.is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

/// A face with nothing baked from it is not a font this box draws with, so it is not one this scan
/// offers: every glyph of every screen would be rasterised on the device.
#[test]
fn a_face_with_no_table_is_not_a_font() {
    let dir = scratch("face-alone");

    face(&dir, "hivebox-face-v9.otf");
    let dir = dir.to_str().unwrap();

    assert!(card_font_in(dir).is_none());
    assert!(installed_font(dir).is_none());

    let _ = std::fs::remove_dir_all(std::path::Path::new(dir));
}

/// The boot and the page read the one scan, so they cannot name two different fonts — with a package on
/// the card, which is the case where the two could have been written to disagree.
#[test]
fn the_page_and_the_boot_read_the_same_font() {
    let dir = scratch("agree");

    table(&dir, "hivebox-font-24px-v1.bin");
    table(&dir, "hivebox-font-24px-v3.bin");
    table(&dir, "hivebox-font-24px-v2.bin");
    face(&dir, "hivebox-face-v2.otf");
    let dir = dir.to_str().unwrap();

    assert_eq!(
        installed_font(dir).map(|version| version.text().to_owned()),
        card_font_in(dir).map(|font| font.version.text().to_owned()),
        "the version the page names is the version of the file the boot reads"
    );

    let _ = std::fs::remove_dir_all(std::path::Path::new(dir));
}

/// Pressing a row's button answers about *that* row, and the two rows' answers are not the same one.
///
/// The image's press has one thing to say, because what it waits on — a second app slot — is on the
/// flash whether there is a card in the box or not. The table's press has two, and which is drawn is
/// the slot's answer; the box *with* a card is the one whose press goes on to the card, which is where
/// the download tests below take it up.
#[test]
fn a_press_says_which_row_it_was_for() {
    let mut image = Updater::new(board_with_slot(true));
    image.update(Message::Install(Kind::Firmware));
    assert_eq!(image.status, Status::Unsupported(Kind::Firmware));

    let mut table = Updater::new(board_with_slot(false));
    table.update(Message::Install(Kind::Font));
    assert_eq!(table.status, Status::Unsupported(Kind::Font));
}

/// A board whose slot holds what the caller says it does.
///
/// [`Board::simulated`] has a card in it, and the empty slot is the other board every page has to
/// speak about. The only way to be given one is to build the board: a backend is the composition
/// root's choice and a test is a composition root — see [`pomelo_hal::sim::SimStorage::without_card`].
fn board_with_slot(card: bool) -> Arc<Board> {
    use pomelo_hal::sim::{
        SimAudio, SimHttp, SimImu, SimInput, SimMic, SimNas, SimPower, SimStorage, SimSystem, SimWeb,
        SimWifi,
    };

    let storage = if card {
        SimStorage::new()
    } else {
        SimStorage::without_card()
    };

    Arc::new(Board::from_backends(
        Box::new(SimPower::new()),
        Box::new(SimWifi::new()),
        Box::new(SimAudio::new()),
        Box::new(SimMic::new()),
        Box::new(SimImu::new()),
        Box::new(SimInput::new()),
        Box::new(storage),
        Box::new(SimWeb::new()),
        Box::new(SimSystem::new()),
        Box::new(SimNas::new()),
        Box::new(SimHttp::new()),
    ))
}

/// The glyph table's press asks the slot, and the image's does not.
///
/// Asking is the whole reason the two rows' refusals are allowed to differ: nothing else on this page
/// knows whether there is a card, and the answer is a *mount* rather than a pin — so it is taken at
/// the press, which is the one moment somebody is waiting for it. An image's press has no second
/// question, and asking would spend the driver's timeouts on an answer nothing draws.
#[test]
fn the_glyphs_press_asks_the_slot() {
    let mut with_card = Updater::new(board_with_slot(true));
    let mut empty_slot = Updater::new(board_with_slot(false));

    assert!(!with_card.card, "nothing has asked the slot yet");
    assert!(!empty_slot.card);

    with_card.update(Message::Install(Kind::Firmware));
    assert!(!with_card.card, "a second app slot is not on the card");

    with_card.update(Message::Install(Kind::Font));
    assert!(with_card.card, "and this is the press that needed it");

    empty_slot.update(Message::Install(Kind::Font));
    assert!(!empty_slot.card, "the slot answered, and the answer was no card");
}

/// The refusal a person reads is the one that is true of *their* box: a slot with no card is told
/// what a glyph table needs, and a box that has one is told that the step is not built.
///
/// Two sentences and not one, because a box with no card has nowhere to put a table even once the
/// write exists — and telling that box "this is not wired up yet" would send its owner away to wait
/// for a release that would not fix it.
#[test]
fn a_refusal_says_which_of_the_two_things_is_missing() {
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);

    assert_eq!(
        english.unsupported_line(Kind::Firmware, true),
        english.unsupported
    );
    assert_eq!(
        english.unsupported_line(Kind::Firmware, false),
        english.unsupported,
        "whether there is a card has nothing to do with where an image goes"
    );

    assert_eq!(english.unsupported_line(Kind::Font, true), english.unsupported);
    assert_eq!(english.unsupported_line(Kind::Font, false), english.no_card);
    assert_eq!(chinese.unsupported_line(Kind::Font, true), chinese.unsupported);
    assert_eq!(chinese.unsupported_line(Kind::Font, false), chinese.no_card);

    assert_ne!(
        english.unsupported, english.no_card,
        "the two sentences are the two reasons, so they have to be two sentences"
    );
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

/// Every way a *download* fails has a sentence in both languages too, and the two that are not HTTP
/// faults are the two worth having: they send a person to look somewhere other than the network.
#[test]
fn every_download_fault_has_a_sentence_in_both_languages() {
    use pomelo_hal::HttpFault;

    let faults = [
        DownloadFault::Http(HttpFault::NoNetwork),
        DownloadFault::NotFound,
        DownloadFault::Incomplete,
    ];

    for fault in faults {
        assert!(!Words::of(Language::English).download_fault_line(fault).is_empty());
        assert!(!Words::of(Language::Chinese).download_fault_line(fault).is_empty());
    }
}

// =================================================================================================
// The download, driven by hand
// =================================================================================================

/// The name the fetched table has on the repository, and the name it lands under on the card.
const TABLE_FILE: &str = "hivebox-font-24px-v2.bin";

/// The same, for the other half of a package: the face the table above was baked from.
const FACE_FILE: &str = "hivebox-face-v2.otf";

/// A frame, which is the clock the page reads a request on — see [`Updater::subscription`].
///
/// The same shape the radio page's tests use: a test has no launcher's pump, so the board is ticked by
/// hand and the frame is what makes the page look. Those are the two halves of the real loop.
fn frame(page: &mut Updater) {
    page.update(Message::Tick(Instant::now()));
}

/// Runs the board until the request in flight is over, then lets the page read it.
///
/// Three, hard-coded rather than shared with the simulator, because the number is the *simulator's* and
/// not this page's: what is being tested is that the page reads an answer whenever it lands, and not
/// how long landing takes.
fn settle(board: &Board, page: &mut Updater) {
    for _ in 0..3 {
        board.tick();
    }

    frame(page);
}

/// The body of the table every test below fetches.
fn table_body() -> String {
    "a glyph table, and not a page of HTML".repeat(48)
}

/// The body of the face that comes in front of it, and a different length from the table's on purpose:
/// a page that fetched the same file twice would pass a test where the two were one size.
fn face_body() -> String {
    "an OTF, and not a glyph table either".repeat(24)
}

/// A manifest naming one table of exactly `bytes` bytes, and nothing else: a font that stands on its
/// own, baked from the face the image carries.
///
/// The size is the manifest's and the body is the server's, and they are the same number here on
/// purpose: the page holds one up against the other when the file lands, and a test that served a body
/// of some other length would be running that check without meaning to.
fn manifest_with_table(bytes: usize) -> String {
    format!(r#"{{"font":{{"file":"{TABLE_FILE}","size":{bytes},"sha256":""}}}}"#)
}

/// A manifest naming one *package*: the face, and the table baked from it.
fn manifest_with_package(face: usize, table: usize) -> String {
    format!(
        r#"{{"font":{{"file":"{TABLE_FILE}","size":{table},"sha256":"",
             "face":{{"file":"{FACE_FILE}","size":{face},"sha256":""}}}}}}"#
    )
}

/// A page over a board whose client answers `script`, and the board itself.
///
/// The board comes back because a test has to drive it: a simulated request only moves when somebody
/// ticks, and the page deliberately does not tick — on a board the pump is the launcher's, and a page
/// that pumped itself would be a page doing the platform's job.
///
/// The directory is the page's own and not [`crate::FONT_DIR`]: `/sdcard/fonts` is a path *the board*
/// has, and a test that downloaded into it would be a test that wrote to the machine running it.
fn page_over_script(script: SimHttp, dir: &std::path::Path) -> (Arc<Board>, Updater) {
    let board = Arc::new(Board::simulated().with_http(Box::new(script)));
    let mut page = Updater::new(Arc::clone(&board));
    page.font_dir = dir.to_str().unwrap().to_string();

    (board, page)
}

/// A client that answers the manifest and then the table's own body.
fn serving_the_table(body: &str) -> SimHttp {
    SimHttp::answering(manifest_with_table(body.len())).then(body.to_string())
}

/// A page whose check has come back, holding the manifest and the offer the tests below press on.
fn answered_page(dir: &std::path::Path, script: SimHttp) -> (Arc<Board>, Updater) {
    let (board, mut page) = page_over_script(script, dir);

    settle(&board, &mut page);

    (board, page)
}

/// The table's press fetches the file the manifest named, puts it where the interface reads tables, and
/// says which version just landed.
#[test]
fn the_tables_press_writes_the_table_to_the_card() {
    let dir = scratch("download");
    let body = table_body();
    let (board, mut page) = answered_page(&dir, serving_the_table(&body));

    assert_eq!(page.status, Status::Answered);
    assert!(page.offer(Kind::Font).is_some(), "v2 against nothing");

    page.update(Message::Install(Kind::Font));
    assert_eq!(page.status, Status::Downloading(Kind::Font));
    assert!(page.waiting(), "and this is why the page asks for frames");

    settle(&board, &mut page);

    let path = dir.join(TABLE_FILE);
    assert!(path.is_file(), "the bytes went to the card and not through memory");
    assert_eq!(std::fs::read(&path).unwrap().len(), body.len());
    assert_eq!(page.status, Status::Installed(Kind::Font));
    assert_eq!(
        page.installed.font.as_ref().map(Version::text),
        Some("v2"),
        "the row is re-read, so it names the version that just landed"
    );

    let words = Words::of(Language::English);
    let line = page.prompt(&words).expect("an install is a prompt").line;
    assert!(line.contains("v2"), "{line} names the version that is on the box");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A font that is two files is one press and two requests: the face, and then the table baked from it.
///
/// The order is the point and not an accident — the table is the file the boot reads, so the package is
/// installed when the second file lands and a press that stopped in between leaves the card as it was.
/// What a person sees is one install throughout: one row, one prompt, one bar that goes on moving.
#[test]
fn a_packages_press_fetches_both_halves_and_installs_the_pair() {
    let dir = scratch("package-download");
    let (face, table) = (face_body(), table_body());
    let script = SimHttp::answering(manifest_with_package(face.len(), table.len()))
        .then(face.clone())
        .then(table.clone());
    let (board, mut page) = answered_page(&dir, script);

    page.update(Message::Install(Kind::Font));

    assert_eq!(page.status, Status::Downloading(Kind::Font));
    assert_eq!(
        page.download.as_ref().map(|it| it.file.as_str()),
        Some(FACE_FILE),
        "the half nothing draws with goes first"
    );

    settle(&board, &mut page);

    assert_eq!(
        page.status,
        Status::Downloading(Kind::Font),
        "half a package is not an install, so the press is not over"
    );
    assert_eq!(
        page.download.as_ref().map(|it| it.file.as_str()),
        Some(TABLE_FILE),
        "and this is what it is waiting on now"
    );
    assert_eq!(
        std::fs::read(dir.join(FACE_FILE)).expect("the face landed").len(),
        face.len()
    );

    settle(&board, &mut page);

    assert_eq!(page.status, Status::Installed(Kind::Font));
    assert_eq!(
        std::fs::read(dir.join(TABLE_FILE)).expect("the table landed").len(),
        table.len()
    );
    assert_eq!(
        page.installed.font.as_ref().map(Version::text),
        Some("v2"),
        "the row names the version of the pair, which is the table's own"
    );

    // And the card agrees, read the way the boot reads it: one scan, two files, one font.
    let font = card_font_in(dir.to_str().unwrap()).expect("a package on the card");
    assert_eq!(font.version.text(), "v2");
    assert!(font.face.is_some(), "with the face that arrived beside it");

    let words = Words::of(Language::English);
    let line = page.prompt(&words).expect("an install is a prompt").line;
    assert!(line.contains("v2"), "{line} names the version that is on the box");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A package whose second half does not arrive installs nothing: what landed is a face with no table,
/// which is not a font — every glyph of every screen would be rasterised on the device — so the card is
/// left naming the font the box already had.
///
/// The half that arrived is kept rather than thrown away, and that is the chain's order working: it is
/// a real file that changes nothing until its table is there, and the next press fetches it again.
#[test]
fn a_package_that_stops_at_its_first_half_installs_nothing() {
    let dir = scratch("package-broken");
    table(&dir, "hivebox-font-24px-v1.bin");

    let face = face_body();
    let script = SimHttp::answering(manifest_with_package(face.len(), 64))
        .then(face)
        .then((404u16, "<html>not found</html>"));
    let (board, mut page) = answered_page(&dir, script);

    page.update(Message::Install(Kind::Font));
    settle(&board, &mut page);
    settle(&board, &mut page);

    assert_eq!(
        page.status,
        Status::DownloadFailed(Kind::Font, DownloadFault::NotFound)
    );
    assert!(
        !dir.join(TABLE_FILE).exists(),
        "the error page is not a table, whatever it is called"
    );
    assert!(
        dir.join(FACE_FILE).is_file(),
        "the half that landed is a real file and is not the dangerous one"
    );
    assert_eq!(
        installed_font(dir.to_str().unwrap()).map(|version| version.text().to_owned()),
        Some("v1".to_owned()),
        "and the font the box draws with is the one it had"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The bar reads the request *while it is still out*, and what it reads moves.
///
/// The reading is the whole reason a frame is asked for during a download: a minute of a bar that never
/// changed is indistinguishable from a box that has hung, and the counter is what tells them apart.
#[test]
fn the_bar_moves_while_the_table_is_still_coming() {
    let dir = scratch("progress");
    let body = table_body();
    let (board, mut page) = answered_page(&dir, serving_the_table(&body));

    page.update(Message::Install(Kind::Font));
    let words = Words::of(Language::English);

    let reading = |page: &mut Updater| {
        let prompt = page.prompt(&words).expect("a download is a prompt");

        (prompt.progress, prompt.bytes, prompt.button.to_string())
    };

    board.tick();
    frame(&mut page);
    let (first, bytes, button) = reading(&mut page);

    board.tick();
    frame(&mut page);
    let (second, ..) = reading(&mut page);

    let (first, second) = (first.expect("a length to divide by"), second.expect("the same"));

    assert!(
        first > 0.0 && first < second && second < 1.0,
        "{first} then {second}: a bar part of the way along, and further along than it was"
    );
    assert!(
        bytes.contains(" / "),
        "{bytes} is both numbers, because the count answers what the bar cannot"
    );
    assert_eq!(button, words.cancel, "the one button stops it while it is out");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A download put away early is a download stopped, and it leaves nothing where a table could be read.
#[test]
fn a_download_that_is_put_away_stops_and_leaves_nothing() {
    let dir = scratch("cancel");
    let body = table_body();
    let (board, mut page) = answered_page(&dir, serving_the_table(&body));

    page.update(Message::Install(Kind::Font));
    board.tick();
    page.update(Message::Dismiss);

    assert_eq!(page.status, Status::Answered, "the rows are the subject again");
    assert!(!page.waiting(), "and the frames stop with the request");
    assert!(page.download.is_none());

    // The request is gone and not merely ignored: the ticks that would have finished it land on
    // nothing, and the file that would have been written is not there to be read as the newest table.
    settle(&board, &mut page);

    assert_eq!(page.status, Status::Answered);
    assert!(!dir.join(TABLE_FILE).exists());

    let _ = std::fs::remove_dir_all(&dir);
}

/// An answer that is not the file — a `404`, an HTML page — is not installed, and the bytes it arrived
/// in are thrown away.
///
/// The second half is the part that matters: a *name* is how the interface finds a table, so a page of
/// HTML left where a table should be would be the newest table the box reads.
#[test]
fn an_answer_that_is_not_the_table_is_not_kept() {
    let dir = scratch("not-found");
    let body = table_body();
    let script = SimHttp::answering(manifest_with_table(body.len()))
        .then((404u16, "<html>not found</html>"));
    let (board, mut page) = answered_page(&dir, script);

    page.update(Message::Install(Kind::Font));
    settle(&board, &mut page);

    assert_eq!(
        page.status,
        Status::DownloadFailed(Kind::Font, DownloadFault::NotFound)
    );
    assert!(!dir.join(TABLE_FILE).exists(), "the error page is not a table");
    assert_eq!(page.installed.font, None, "and the row says nothing is installed");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that arrived whole as far as the socket is concerned and short of what the manifest named is
/// the one failure the page checks for itself, and it is not left on the card either.
#[test]
fn a_file_that_is_not_the_size_it_was_named_is_not_kept() {
    let dir = scratch("short");
    let body = table_body();
    let (board, mut page) = page_over_script(
        SimHttp::answering(manifest_with_table(body.len() + 512)).then(body),
        &dir,
    );

    settle(&board, &mut page);
    page.update(Message::Install(Kind::Font));
    settle(&board, &mut page);

    assert_eq!(
        page.status,
        Status::DownloadFailed(Kind::Font, DownloadFault::Incomplete)
    );
    assert!(!dir.join(TABLE_FILE).exists());

    let _ = std::fs::remove_dir_all(&dir);
}
