//! The store's decisions, tested without a window.
//!
//! Three of them are pure questions about data: which repository packages are *available*, which
//! installed ones are *upgradable*, and what the three lists therefore contain. The rest are presses,
//! and a press is a request and a file — so they are asked against a scripted client on a board the
//! test drives by hand, the same way the update page's download tests are.
//!
//! The two roots are temporary directories, never `/internal` and `/sdcard`: those are paths *the
//! board* has, and a test that wrote to them would be a test that wrote to the machine running it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use pomelo_hal::sim::SimHttp;
use pomelo_hal::{Board, HttpFault};
use pomelo_widgets::Language;

use app_host::manager::sha256_hex;
use app_host::{Root, Source};
use updater::Manifest;

use crate::{Action, Message, Section, Status, Store, Tone, Words};

/// A package whose metadata says what the test needs and whose module is a valid, empty one.
///
/// The bytes are also a `String`, and that is not an accident: the simulated client answers with a
/// body and not a byte slice, so a fixture that could not be written as text could not be served
/// end-to-end. Everything in this container is ASCII — the header's lengths are small enough to stay
/// under `0x80` — which makes it servable and still a package the installer accepts.
fn package(id: &str, version: &str) -> String {
    let meta = format!(
        r#"{{"id":"{id}","name":"{id}","name_zh":"{id}","version":"{version}","abi":1,"memory_kb":128}}"#
    );
    let wasm = "\0asm\x01\0\0\0";

    let header = app_abi::PackageHeader::new(meta.len() as u32, wasm.len() as u32, 0, 0);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header.encode());
    bytes.extend_from_slice(meta.as_bytes());
    bytes.extend_from_slice(wasm.as_bytes());

    String::from_utf8(bytes).expect("a fixture the simulated client can carry")
}

/// One `apps` entry, as `build_apps.py` writes it and the release script publishes it.
fn entry(id: &str, version: &str, body: &str) -> String {
    format!(
        r##"{{"id":"{id}","name":"{id}","name_zh":"{id}","version":"{version}",
            "file":"hivebox-app-{id}-{version}.app","size":{},"sha256":"{}","abi":1,
            "accent":"#2E9E7B","description":"","glyph":"","permissions":0}}"##,
        body.len(),
        sha256_hex(body.as_bytes())
    )
}

/// A manifest naming one or more packages, as the repository publishes it.
fn manifest(apps: &[(&str, &str, &str)]) -> String {
    let entries: Vec<String> = apps
        .iter()
        .map(|(id, version, body)| entry(id, version, body))
        .collect();

    format!(r#"{{"apps":[{}]}}"#, entries.join(","))
}

/// A publishable manifest that does *not* match the body it names: the failure a truncated release
/// or a mis-built one produces.
fn manifest_with_wrong_hash(id: &str, version: &str, body: &str) -> String {
    format!(
        r#"{{"apps":[{{"id":"{id}","name":"{id}","name_zh":"{id}","version":"{version}",
            "file":"hivebox-app-{id}-{version}.app","size":{},"sha256":"{}"}}]}}"#,
        body.len(),
        "00".repeat(32)
    )
}

/// Two roots in a scratch directory, and the directory itself for the cache.
fn scratch(name: &str) -> (PathBuf, Vec<Root>) {
    let dir = std::env::temp_dir().join(format!(
        "hivebox-store-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let internal = dir.join("internal");
    let card = dir.join("card");
    std::fs::create_dir_all(&internal).unwrap();
    std::fs::create_dir_all(&card).unwrap();

    (
        dir.clone(),
        vec![
            Root::new(internal, Source::Internal),
            Root::new(card, Source::Card),
        ],
    )
}

/// Puts a package where the catalog reads it, as an install would.
fn install_on_disk(root: &Root, id: &str, body: &str) {
    let dir = root.path.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(app_host::catalog::PACKAGE_FILE), body).unwrap();
}

/// A page over a scripted client.
fn page_over(script: SimHttp, roots: Vec<Root>, cache: &Path) -> (Arc<Board>, Store) {
    let board = Arc::new(Board::simulated().with_http(Box::new(script)));
    let page = Store::with_roots(Arc::clone(&board), roots, cache.to_path_buf());

    (board, page)
}

/// Runs the board until the request in flight is over, then lets the page read it.
fn settle(board: &Board, page: &mut Store) {
    for _ in 0..3 {
        board.tick();
    }

    page.update(Message::Tick(Instant::now()));
}

/// A page that has read the manifest and knows the roots, with no request in flight.
fn answered(roots: Vec<Root>, cache: &Path, document: &str) -> Store {
    let mut page = Store::with_roots(
        Arc::new(Board::simulated().with_http(Box::new(SimHttp::new()))),
        roots,
        cache.to_path_buf(),
    );

    page.manifest = Manifest::parse(document);
    page.status = Status::Answered;

    page
}

/// A release is an offer only when it is newer than what is installed.
#[test]
fn an_upgrade_is_offered_only_for_something_newer() {
    let (work, roots) = scratch("upgrade-offer");
    install_on_disk(&roots[0], "clock", &package("clock", "v0.1.0"));

    let body = package("clock", "v0.2.0");
    let document = manifest(&[("clock", "v0.2.0", &body)]);
    let page = answered(roots, &work.join("catalog.json"), &document);

    let installed = page.catalog.get("clock").expect("the installed clock");
    assert_eq!(
        page.upgrade_for(installed).map(|release| release.version.text()),
        Some("v0.2.0")
    );

    // The same version said two ways is not an update.
    let document = manifest(&[("clock", "v0.1", &body)]);
    let same = answered(page.roots, &work.join("catalog.json"), &document);
    assert!(same.updates().is_empty());

    std::fs::remove_dir_all(&work).ok();
}

/// A package already installed is not in the "available" list, and one that is not installed is.
#[test]
fn available_is_the_manifest_minus_what_is_installed() {
    let (work, roots) = scratch("available");
    install_on_disk(&roots[0], "clock", &package("clock", "v0.1.0"));

    let document = manifest(&[
        ("clock", "v0.1.0", &package("clock", "v0.1.0")),
        ("weather", "v0.1.0", &package("weather", "v0.1.0")),
    ]);
    let page = answered(roots, &work.join("catalog.json"), &document);

    let available: Vec<&str> = page
        .available()
        .iter()
        .map(|release| release.id.as_str())
        .collect();

    assert_eq!(available, ["weather"]);

    std::fs::remove_dir_all(&work).ok();
}

/// The whole press: a request, a `.part` file, a verified install, and the catalog knowing about it.
#[test]
fn an_install_downloads_verifies_and_appears_without_a_restart() {
    let (work, roots) = scratch("install");
    let body = package("calculator", "v0.1.0");
    let document = manifest(&[("calculator", "v0.1.0", &body)]);

    let script = SimHttp::answering(document).then(body.clone());
    let (board, mut page) = page_over(script, roots.clone(), &work.join("catalog.json"));

    settle(&board, &mut page);
    assert_eq!(page.status, Status::Answered, "the manifest was read");

    page.update(Message::Install("calculator".to_string()));
    assert!(matches!(page.status, Status::Downloading(_)));

    settle(&board, &mut page);

    assert!(page.catalog.is_installed("calculator"));
    assert!(roots[0]
        .path
        .join("calculator")
        .join(app_host::catalog::PACKAGE_FILE)
        .exists());
    assert!(
        !roots[0].path.join(app_host::manager::STAGING_DIR).exists(),
        "the staging directory is gone with the install"
    );

    let prompt = page
        .prompt(&Words::of(Language::Chinese))
        .expect("the install's report");

    assert_eq!(prompt.tone, Tone::Done);
    assert!(prompt.line.contains("v0.1.0"));

    // The grid is told once, and only once: the flag is the launcher's cue to rescan its tiles.
    assert!(page.take_changed(), "an install changes what the grid shows");
    assert!(!page.take_changed(), "and says so once");

    std::fs::remove_dir_all(&work).ok();
}

/// A file that is not the bytes the manifest named is refused, and nothing is left installed.
#[test]
fn a_hash_that_does_not_match_leaves_nothing() {
    let (work, roots) = scratch("hash");
    let body = package("weather", "v0.1.0");
    let document = manifest_with_wrong_hash("weather", "v0.1.0", &body);

    let script = SimHttp::answering(document).then(body.clone());
    let (board, mut page) = page_over(script, roots.clone(), &work.join("catalog.json"));

    settle(&board, &mut page);
    page.update(Message::Install("weather".to_string()));
    settle(&board, &mut page);

    assert!(!page.catalog.is_installed("weather"));
    assert!(!roots[0].path.join("weather").exists());
    assert!(!roots[0].path.join(app_host::manager::STAGING_DIR).exists());

    let prompt = page.prompt(&Words::of(Language::Chinese)).unwrap();
    assert_eq!(prompt.tone, Tone::Failed);

    std::fs::remove_dir_all(&work).ok();
}

/// Putting the prompt away mid-download cancels the request and deletes the half-written file.
#[test]
fn a_cancelled_download_leaves_no_part_file() {
    let (work, roots) = scratch("cancel");
    let body = package("games", "v0.1.0");
    let document = manifest(&[("games", "v0.1.0", &body)]);

    let script = SimHttp::answering(document).then(body.clone());
    let (board, mut page) = page_over(script, roots.clone(), &work.join("catalog.json"));

    settle(&board, &mut page);
    page.update(Message::Install("games".to_string()));
    board.tick();
    page.update(Message::Dismiss);

    assert_eq!(page.status, Status::Answered);
    assert!(!page.waiting());

    settle(&board, &mut page);

    assert!(!page.catalog.is_installed("games"));
    assert!(!roots[0].path.join("games").exists());

    std::fs::remove_dir_all(&work).ok();
}

/// A removal is a question first: the directory stays until the second button is pressed.
#[test]
fn an_uninstall_asks_and_then_removes_the_directory() {
    let (work, roots) = scratch("uninstall");
    install_on_disk(&roots[0], "power-log", &package("power-log", "v0.1.0"));

    let page = answered(roots.clone(), &work.join("catalog.json"), "{}");
    let mut page = page;

    page.update(Message::Uninstall("power-log".to_string()));
    assert!(page.catalog.is_installed("power-log"));

    let prompt = page.prompt(&Words::of(Language::Chinese)).unwrap();
    assert_eq!(prompt.confirm, Some("卸载"));
    assert_eq!(prompt.action, Action::Uninstall("power-log".to_string()));

    page.update(Message::Confirm);

    assert!(!page.catalog.is_installed("power-log"));
    assert!(!roots[0].path.join("power-log").exists());
    assert_eq!(
        page.prompt(&Words::of(Language::Chinese)).map(|prompt| prompt.tone),
        Some(Tone::Done)
    );

    std::fs::remove_dir_all(&work).ok();
}

/// The prompt's first button is "no": it closes the question and changes nothing.
#[test]
fn a_removal_that_is_put_away_removes_nothing() {
    let (work, roots) = scratch("uninstall-no");
    install_on_disk(&roots[0], "terminal", &package("terminal", "v0.1.0"));

    let mut page = answered(roots.clone(), &work.join("catalog.json"), "{}");

    page.update(Message::Uninstall("terminal".to_string()));
    page.update(Message::Dismiss);

    assert!(page.catalog.is_installed("terminal"));
    assert!(page.prompt(&Words::of(Language::Chinese)).is_none());
    assert!(roots[0].path.join("terminal").exists());

    std::fs::remove_dir_all(&work).ok();
}

/// A cached manifest is what an offline box opens on: the request fails, the list does not empty.
#[test]
fn an_offline_check_keeps_the_cached_manifest() {
    let (work, roots) = scratch("offline");
    let document = manifest(&[("weather", "v0.1.0", &package("weather", "v0.1.0"))]);

    let cache = work.join("catalog.json");
    std::fs::write(&cache, &document).unwrap();

    let script = SimHttp::offline();
    let (board, mut page) = page_over(script, roots.clone(), &cache);

    assert_eq!(
        page.available().len(),
        1,
        "the cache is read before the request is even answered"
    );

    settle(&board, &mut page);

    assert!(matches!(page.status, Status::Failed(HttpFault::NoNetwork)));
    assert_eq!(page.available().len(), 1, "the cache survives the failure");

    std::fs::remove_dir_all(&work).ok();
}

/// A 404 is not a package: the body lands in the file and is thrown away with it.
#[test]
fn a_missing_file_installs_nothing() {
    let (work, roots) = scratch("not-found");
    let body = package("files", "v0.1.0");
    let document = manifest(&[("files", "v0.1.0", &body)]);

    let script = SimHttp::answering(document).then((404u16, "not here"));
    let (board, mut page) = page_over(script, roots.clone(), &work.join("catalog.json"));

    settle(&board, &mut page);
    page.update(Message::Install("files".to_string()));
    settle(&board, &mut page);

    assert!(!page.catalog.is_installed("files"));
    assert!(!roots[0].path.join("files").exists());
    assert!(!roots[0].path.join(app_host::manager::STAGING_DIR).exists());

    let prompt = page.prompt(&Words::of(Language::Chinese)).unwrap();
    assert_eq!(prompt.tone, Tone::Failed);
    assert_eq!(prompt.line, "仓库里没有这个文件");

    std::fs::remove_dir_all(&work).ok();
}

/// The three tabs are the three lists, and switching one changes only what is drawn.
#[test]
fn the_sections_are_the_three_lists() {
    let (work, roots) = scratch("sections");
    install_on_disk(&roots[0], "clock", &package("clock", "v0.1.0"));

    let document = manifest(&[
        ("clock", "v0.2.0", &package("clock", "v0.2.0")),
        ("weather", "v0.1.0", &package("weather", "v0.1.0")),
    ]);

    let mut page = answered(roots, &work.join("catalog.json"), &document);

    page.update(Message::Section(Section::Available));
    assert_eq!(page.section, Section::Available);
    assert_eq!(page.available().len(), 1);
    assert_eq!(page.updates().len(), 1);
    assert_eq!(page.catalog.apps().len(), 1);

    std::fs::remove_dir_all(&work).ok();
}

/// The words the page draws are in both languages, and no language is the other's copy.
#[test]
fn the_words_are_written_in_both_languages() {
    let chinese = Words::of(Language::Chinese);
    let english = Words::of(Language::English);

    for (zh, en) in [
        (chinese.title, english.title),
        (chinese.subtitle, english.subtitle),
        (chinese.installed, english.installed),
        (chinese.available, english.available),
        (chinese.updates, english.updates),
        (chinese.install, english.install),
        (chinese.uninstall, english.uninstall),
        (chinese.empty_installed, english.empty_installed),
        (chinese.unreadable, english.unreadable),
    ] {
        assert_ne!(zh, en, "{zh} is not translated");
    }

    // A fault is a sentence in the same two languages, and not a `{:?}` of the enum.
    assert_eq!(chinese.fault(HttpFault::NoNetwork), "没有网络");
    assert_eq!(english.fault(HttpFault::NoNetwork), "No network");
}

/// A package's permissions are shown as the words a person reads, and a package that asks for
/// nothing has no line about not asking.
#[test]
fn permissions_are_named_in_the_page_s_language() {
    let chinese = Words::of(Language::Chinese);
    let english = Words::of(Language::English);

    let bits = app_abi::perm::NETWORK | app_abi::perm::AUDIO;

    assert_eq!(chinese.permissions(bits), "网络 · 音频");
    assert_eq!(english.permissions(bits), "Network · Audio");

    assert_eq!(chinese.permissions(0), "");
    assert_eq!(
        english.permissions(app_abi::perm::ALL),
        "Network · System · Audio · SSH · Wi-Fi · Cloud · Files"
    );
}
