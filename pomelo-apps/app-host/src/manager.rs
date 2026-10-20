//! Installing, upgrading and removing a package.
//!
//! The bytes arrive from the same HTTP backend that fetches a firmware image — [`Sink::File`] into
//! a temporary file, with progress and cancel — and this module's job is everything after the last
//! byte: check it is the thing the manifest named, put it where the catalog will find it, and leave
//! nothing behind if any of that fails.
//!
//! # An install is a rename, not a copy
//!
//! The package is written into a staging directory on the *same volume* as the directory it is
//! going to, verified by reading it back, and then renamed into place. A rename on one filesystem is
//! the one operation that is either done or not: there is no moment at which the catalog can see a
//! half-written `app.hbapp`. The old package is moved aside to [`crate::catalog::PREVIOUS_FILE`]
//! first, so an upgrade that is interrupted recovers as the old version — see the catalog, which
//! does that recovery on the next boot.
//!
//! # What is checked, and against what
//!
//! Size and SHA-256, when the manifest named them, and the container itself either way. Nothing is
//! signed (this project's releases are not), so a package that is the bytes the repository published
//! is the most this can prove; the sandbox is what limits what those bytes may do.
//!
//! [`Sink::File`]: pomelo_hal::Sink

use std::fmt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use app_abi::{encode_icon, MAX_PACKAGE_BYTES};

use crate::catalog::{
    Installed, Root, Source, DATA_DIR, DISABLED_FILE, ICON_FILE, PACKAGE_FILE, PREVIOUS_FILE,
};
use crate::meta::validate_id;
use crate::package::{Icon, Package, PackageError};

/// What the manifest said the file would be.
///
/// Both fields are optional because a manifest in the wild may omit either; the container checks
/// still run. A size the store can compare is worth having even when there is no hash: a truncated
/// download is the failure this project sees most.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expected {
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

impl Expected {
    pub fn new(size: u64, sha256: impl Into<String>) -> Self {
        Self {
            size: Some(size),
            sha256: Some(sha256.into()),
        }
    }
}

/// Why an install did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The bytes are larger than a package may be.
    TooLarge(u64),
    /// The file is not the size the manifest named.
    SizeMismatch { expected: u64, actual: u64 },
    /// The file is not the bytes the manifest named.
    ShaMismatch { expected: String, actual: String },
    /// The container or its metadata is not readable.
    Package(PackageError),
    /// The id in the package is not one this project will make a directory for.
    BadId(String),
    /// The package is newer than this host's ABI.
    UnsupportedAbi(u16),
    /// No volume that could hold it exists.
    NoVolume,
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(size) => write!(f, "应用包太大（{size} 字节）"),
            Self::SizeMismatch { expected, actual } => {
                write!(f, "下载不完整：{actual} / {expected} 字节")
            }
            Self::ShaMismatch { .. } => write!(f, "文件校验失败，已丢弃"),
            Self::Package(error) => write!(f, "{error}"),
            Self::BadId(id) => write!(f, "应用 id 不合法: {id}"),
            Self::UnsupportedAbi(abi) => write!(f, "应用需要更新的系统（ABI {abi}）"),
            Self::NoVolume => write!(f, "没有可写入的存储"),
            Self::Io(error) => write!(f, "写入失败: {error}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Installs `bytes` into one of `roots`.
///
/// `prefer` is where the store asked for it to go; the first root of that source that exists wins,
/// and when it does not exist the other volume is used rather than failing — a card that was pulled
/// between the press and the install should still install to flash.
pub fn install(
    bytes: &[u8],
    expected: &Expected,
    roots: &[Root],
    prefer: Option<Source>,
) -> Result<Installed, InstallError> {
    verify(bytes, expected)?;

    let package = Package::open(bytes).map_err(InstallError::Package)?;
    validate_id(&package.meta.id).map_err(|_| InstallError::BadId(package.meta.id.clone()))?;

    let root = choose_root(roots, prefer, bytes.len() as u64).ok_or(InstallError::NoVolume)?;
    let staging = staging_dir(&root);

    let io = |error: std::io::Error| InstallError::Io(error.to_string());

    std::fs::create_dir_all(&staging).map_err(io)?;

    let staged_package = staging.join(PACKAGE_FILE);
    std::fs::write(&staged_package, bytes).map_err(io)?;

    // Read back what was written: the copy that matters is the one on the filesystem, and a card
    // that lied about a write is the failure a size check one layer up cannot see.
    let read_back = match std::fs::read(&staged_package) {
        Ok(read_back) => read_back,
        Err(error) => return Err(fail(&staging, InstallError::Io(error.to_string()))),
    };

    if read_back != bytes {
        return Err(fail(
            &staging,
            InstallError::Io("写入后读回的内容不一致".to_string()),
        ));
    }

    install_into(&root, package.meta, package.icon, &staged_package)
}

/// Installs a package that is already a file on disk.
///
/// The store's path, and the reason it exists: a download lands in a *file* — [`Sink::File`] writes
/// the bytes as they arrive and they are never in RAM — so reading the package back into a `Vec`
/// just to hand it to [`install`] would be a two-megabyte copy of a file that is already exactly
/// where it needs to be. This verifies the file, reads its metadata and icon, and renames it into
/// place; the module is never read, let alone held.
///
/// The file is expected to be inside one of the roots' `.staging` directories (the store downloads
/// there), which makes the move a rename on one filesystem. A file from anywhere else is copied and
/// then removed, and either way the source does not survive a successful install: a `.part` file
/// left behind after the package is installed would be a second copy of the same application.
///
/// [`Sink::File`]: pomelo_hal::Sink
pub fn install_file(
    path: &Path,
    expected: &Expected,
    roots: &[Root],
    prefer: Option<Source>,
) -> Result<Installed, InstallError> {
    let length = std::fs::metadata(path)
        .map_err(|error| InstallError::Io(error.to_string()))?
        .len();

    if length > MAX_PACKAGE_BYTES as u64 {
        return Err(InstallError::TooLarge(length));
    }

    if let Some(size) = expected.size {
        if size != length {
            return Err(InstallError::SizeMismatch {
                expected: size,
                actual: length,
            });
        }
    }

    let root = choose_root(roots, prefer, length).ok_or(InstallError::NoVolume)?;
    let staging = staging_dir(&root);

    let io = |error: std::io::Error| InstallError::Io(error.to_string());

    std::fs::create_dir_all(&staging).map_err(io)?;

    let staged_package = staging.join(PACKAGE_FILE);

    if std::fs::rename(path, &staged_package).is_err() {
        std::fs::copy(path, &staged_package)
            .map_err(|error| fail(&staging, InstallError::Io(error.to_string())))?;

        std::fs::remove_file(path).ok();
    }

    // The hash is computed on the file the filesystem now holds, not on the bytes that were
    // downloaded: a card that lied about a write is exactly the failure this step can see.
    if let Some(expected) = expected.sha256.as_deref().filter(|hash| !hash.is_empty()) {
        let actual = match sha256_file(&staged_package) {
            Ok(actual) => actual,
            Err(error) => return Err(fail(&staging, error)),
        };

        if !actual.eq_ignore_ascii_case(expected) {
            return Err(fail(
                &staging,
                InstallError::ShaMismatch {
                    expected: expected.to_string(),
                    actual,
                },
            ));
        }
    }

    let (meta, icon) = match Package::read_meta_and_icon(&staged_package) {
        Ok(package) => package,
        Err(error) => return Err(fail(&staging, InstallError::Package(error))),
    };

    if validate_id(&meta.id).is_err() {
        return Err(fail(&staging, InstallError::BadId(meta.id.clone())));
    }

    install_into(&root, meta, icon, &staged_package)
}

/// The root an install of `size` bytes would use, without installing anything.
///
/// The store asks this *before* the download, because the download's temporary file has to be on
/// the same volume as the install: it is a rename that makes an install atomic, and a rename across
/// filesystems is a copy.
pub fn root_for(roots: &[Root], prefer: Option<Source>, size: u64) -> Option<Root> {
    choose_root(roots, prefer, size)
}

/// Checks the bytes against what the manifest said, before anything is written.
pub fn verify(bytes: &[u8], expected: &Expected) -> Result<(), InstallError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(InstallError::TooLarge(bytes.len() as u64));
    }

    if let Some(size) = expected.size {
        if size != bytes.len() as u64 {
            return Err(InstallError::SizeMismatch {
                expected: size,
                actual: bytes.len() as u64,
            });
        }
    }

    if let Some(expected) = expected.sha256.as_deref().filter(|hash| !hash.is_empty()) {
        let actual = sha256_hex(bytes);

        if !actual.eq_ignore_ascii_case(expected) {
            return Err(InstallError::ShaMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    Ok(())
}

/// The lowercase hex SHA-256 of a byte slice, as a manifest writes it.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The lowercase hex SHA-256 of a file, read in chunks.
///
/// The file-sized half of [`sha256_hex`], for the install path: a package is verified *after* it has
/// landed on the filesystem, and reading it back into a `Vec` to hash it would defeat the reason
/// [`install_file`] exists.
///
/// # The read buffer is on the heap, and it is 8 KB
///
/// It was `[0u8; 64 * 1024]` on the stack, and that array alone was the store's crash. The UI task
/// this board's whole panel runs on has a 64 KB stack (`CONFIG_ESP_MAIN_TASK_STACK_SIZE`), and
/// `sha256_file` is reached from the store's `update()`, so the compiler reserved the 64 KB at the
/// top of *that* frame whether or not the hash was ever taken: opening the store cost 66,512 bytes
/// of stack on entry, the frame ran off the end of the stack into the Wi-Fi driver's own `.bss`
/// (`s_fragment`, `if_ctrl` sit just below it), and the box rebooted — sometimes in the panel's own
/// `fclose`, sometimes in `ppRxFragmentProc` a few milliseconds later, which is two faces of one
/// overflow and why it looked like a radio bug.
///
/// A `Vec` is where a buffer that large belongs: PSRAM has megabytes to spare and a stack this size
/// does not. `#[inline(never)]` is the other half of the same guard — a frame inlined into the
/// store's `update()` would put the allocation back where it cannot fit.
#[inline(never)]
pub fn sha256_file(path: &Path) -> Result<String, InstallError> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|error| InstallError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| InstallError::Io(error.to_string()))?;

        if read == 0 {
            break;
        }

        hasher.update(&buffer[..read]);
    }

    Ok(hex(&hasher.finalize()))
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);

    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }

    out
}

/// The directory a package is written to before it is renamed into place.
///
/// One per install and under `.staging`, so that a process that dies part way leaves something the
/// catalog does not scan and the next boot's [`clean_up`] deletes.
fn staging_dir(root: &Root) -> PathBuf {
    root.path.join(STAGING_DIR).join(format!("incoming-{}", stamp()))
}

/// Deletes a staging directory and hands the error back, for a `return Err` on a failure path.
fn fail(staging: &Path, error: InstallError) -> InstallError {
    std::fs::remove_dir_all(staging).ok();

    // The shared `.staging` goes too, but only when it is empty: it is one directory for every
    // install on this volume.
    if let Some(shared) = staging.parent() {
        std::fs::remove_dir(shared).ok();
    }

    error
}

/// The directory the installer keeps half-finished work in; never scanned, always cleaned.
pub const STAGING_DIR: &str = ".staging";

fn choose_root(roots: &[Root], prefer: Option<Source>, size: u64) -> Option<Root> {
    let usable = |root: &&Root| root.path.is_dir();

    // A package bigger than the small-package threshold goes to the card by default: the internal
    // volume is 3.88 MB shared by every application's data, and the card is where the fonts already
    // live for the same reason. The store can override either way.
    let wanted = prefer.unwrap_or(if size > SMALL_PACKAGE_BYTES {
        Source::Card
    } else {
        Source::Internal
    });

    roots
        .iter()
        .filter(usable)
        .find(|root| root.source == wanted)
        .or_else(|| roots.iter().find(usable))
        .cloned()
}

/// Above this, a package is installed to the card unless the person chose otherwise.
pub const SMALL_PACKAGE_BYTES: u64 = 256 * 1024;

/// Moves a verified package from its staging file into the directory the catalog reads.
fn install_into(
    root: &Root,
    meta: crate::meta::Meta,
    icon: Option<Icon>,
    staged_package: &Path,
) -> Result<Installed, InstallError> {
    let id = &meta.id;
    let dir = root.path.join(id);
    let package_path = dir.join(PACKAGE_FILE);
    let previous = dir.join(PREVIOUS_FILE);

    let io = |error: std::io::Error| InstallError::Io(error.to_string());

    std::fs::create_dir_all(dir.join(DATA_DIR)).map_err(io)?;

    // The old package is kept, not deleted: an upgrade that does not work is a version a person can
    // go back to, and the catalog restores it if this process dies before the new one lands.
    if package_path.exists() {
        if previous.exists() {
            std::fs::remove_file(&previous).ok();
        }

        std::fs::rename(&package_path, &previous).map_err(io)?;
    }

    std::fs::rename(staged_package, &package_path).map_err(|error| {
        // Put the old package back before reporting the failure: a rename that failed has not
        // changed what is on the disk, and the application must still start.
        if previous.exists() {
            std::fs::rename(&previous, &package_path).ok();
        }

        InstallError::Io(error.to_string())
    })?;

    // The icon cache is a convenience for the catalog's boot read, not part of the package: a
    // package that installed without one still starts and still has its icon section inside it.
    let _ = write_icon(&dir.join(ICON_FILE), icon.as_ref());

    // The staging directory is this install's own; the shared `.staging` is removed only when it is
    // empty, because another install may be using it at the same time.
    if let Some(staging) = staged_package.parent() {
        std::fs::remove_dir_all(staging).ok();

        if let Some(shared) = staging.parent() {
            std::fs::remove_dir(shared).ok();
        }
    }

    Ok(Installed {
        meta,
        source: root.source,
        dir,
        icon,
        disabled: false,
    })
}

fn write_icon(path: &Path, icon: Option<&Icon>) -> Result<(), InstallError> {
    let Some(icon) = icon else {
        std::fs::remove_file(path).ok();
        return Ok(());
    };

    let mut rgb = Vec::with_capacity(icon.rgb565().len() * 2);

    for pixel in icon.rgb565() {
        rgb.extend_from_slice(&pixel.to_le_bytes());
    }

    let section = encode_icon(icon.width, icon.height, &rgb, icon.alpha())
        .ok_or_else(|| InstallError::Io("图标数据不完整".to_string()))?;

    std::fs::write(path, section).map_err(|error| InstallError::Io(error.to_string()))
}

/// Removes an installed application and everything it wrote.
///
/// The directory is renamed into `.trash` first and then deleted. The rename is what makes the
/// uninstall atomic from the catalog's point of view: the application stops existing in one step,
/// and a deletion that is interrupted by a power cut leaves garbage in a directory the catalog does
/// not scan rather than half an application.
pub fn uninstall(root: &Root, id: &str) -> Result<(), InstallError> {
    validate_id(id).map_err(|_| InstallError::BadId(id.to_string()))?;

    let dir = root.path.join(id);

    if !dir.exists() {
        return Ok(());
    }

    let trash_root = root.path.join(".trash");
    let trash = trash_root.join(format!("{id}-{}", stamp()));

    let io = |error: std::io::Error| InstallError::Io(error.to_string());

    std::fs::create_dir_all(&trash_root).map_err(io)?;
    std::fs::rename(&dir, &trash).map_err(io)?;

    // A deletion that fails leaves the package in `.trash`, which no scan reads; the boot-time
    // cleanup will try again. Reporting it would be reporting that an application is still
    // installed, which it is not.
    std::fs::remove_dir_all(&trash).ok();

    Ok(())
}

/// Marks an application as stopped, or clears the mark.
pub fn set_disabled(root: &Root, id: &str, disabled: bool) -> Result<(), InstallError> {
    validate_id(id).map_err(|_| InstallError::BadId(id.to_string()))?;

    let marker = root.path.join(id).join(DISABLED_FILE);

    if disabled {
        std::fs::write(marker, b"").map_err(|error| InstallError::Io(error.to_string()))
    } else {
        std::fs::remove_file(marker).ok();
        Ok(())
    }
}

/// Goes back to the package an upgrade replaced, when there is one.
pub fn rollback(root: &Root, id: &str) -> Result<bool, InstallError> {
    validate_id(id).map_err(|_| InstallError::BadId(id.to_string()))?;

    let dir = root.path.join(id);
    let package = dir.join(PACKAGE_FILE);
    let previous = dir.join(PREVIOUS_FILE);

    if !previous.exists() {
        return Ok(false);
    }

    let rejected = dir.join("app.hbapp.bad");

    if rejected.exists() {
        std::fs::remove_file(&rejected).ok();
    }

    if package.exists() {
        std::fs::rename(&package, &rejected)
            .map_err(|error| InstallError::Io(error.to_string()))?;
    }

    std::fs::rename(&previous, &package).map_err(|error| InstallError::Io(error.to_string()))?;

    Ok(true)
}

/// Deletes what an interrupted install or uninstall left behind.
///
/// Called once at boot, before the catalog is read: `.staging` holds files whose package never
/// landed, and `.trash` holds directories whose deletion did not finish. Neither is an application.
pub fn clean_up(roots: &[Root]) {
    for root in roots {
        let _ = std::fs::remove_dir_all(root.path.join(STAGING_DIR));
        let _ = std::fs::remove_dir_all(root.path.join(".trash"));
    }
}

/// A timestamp that only has to be unique within one volume's staging directory.
fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

/// Where an application's own files live, for a caller that only has its root and id.
pub fn data_dir(root: &Root, id: &str) -> PathBuf {
    root.path.join(id).join(DATA_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_abi::PackageHeader;

    fn package_bytes(id: &str, version: &str, icon: Option<Vec<u8>>) -> Vec<u8> {
        let meta = format!(
            r#"{{"id":"{id}","name":"{id}","version":"{version}","abi":1,"memory_kb":128}}"#
        );
        let wasm = b"\0asm\x01\0\0\0";
        let icon = icon.unwrap_or_default();

        let header = PackageHeader::new(meta.len() as u32, wasm.len() as u32, icon.len() as u32, 0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&header.encode());
        bytes.extend_from_slice(meta.as_bytes());
        bytes.extend_from_slice(wasm);
        bytes.extend_from_slice(&icon);
        bytes
    }

    fn temp_root(name: &str) -> (PathBuf, Root) {
        let work = std::env::temp_dir().join(format!(
            "hivebox-install-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = Root::new(work.join("apps"), Source::Internal);
        std::fs::create_dir_all(&root.path).unwrap();
        (work, root)
    }

    #[test]
    fn an_install_lands_as_a_directory_the_catalog_reads() {
        let (work, root) = temp_root("land");
        let bytes = package_bytes("calculator", "v1.0.0", None);
        let expected = Expected::new(bytes.len() as u64, sha256_hex(&bytes));

        let installed = install(&bytes, &expected, std::slice::from_ref(&root), None)
            .expect("a package that matches its manifest");

        assert_eq!(installed.meta.id, "calculator");
        assert!(root.path.join("calculator").join(PACKAGE_FILE).exists());
        assert!(root.path.join("calculator").join(DATA_DIR).is_dir());
        assert!(!root.path.join(".staging").exists());

        std::fs::remove_dir_all(&work).ok();
    }

    /// The two failures a network produces, and the guarantee that neither leaves a package behind:
    /// the right bytes with the wrong hash, and fewer bytes than were promised.
    #[test]
    fn a_file_that_is_not_what_the_manifest_named_is_refused_before_writing() {
        let (work, root) = temp_root("reject");
        let bytes = package_bytes("clock", "v1.0.0", None);

        let wrong_hash = Expected::new(bytes.len() as u64, "00".repeat(32));
        assert!(matches!(
            install(&bytes, &wrong_hash, std::slice::from_ref(&root), None),
            Err(InstallError::ShaMismatch { .. })
        ));

        let wrong_size = Expected::new(bytes.len() as u64 + 10, sha256_hex(&bytes));
        assert!(matches!(
            install(&bytes, &wrong_size, std::slice::from_ref(&root), None),
            Err(InstallError::SizeMismatch { .. })
        ));

        assert!(!root.path.join("clock").exists());

        std::fs::remove_dir_all(&work).ok();
    }

    /// An upgrade keeps the old package as `.prev`, and a rollback swaps it back.
    #[test]
    fn an_upgrade_keeps_the_previous_version_for_a_rollback() {
        let (work, root) = temp_root("upgrade");
        let first = package_bytes("games", "v1.0.0", None);
        let second = package_bytes("games", "v2.0.0", None);

        install(&first, &Expected::default(), std::slice::from_ref(&root), None).unwrap();
        install(&second, &Expected::default(), std::slice::from_ref(&root), None).unwrap();

        let previous = root.path.join("games").join(PREVIOUS_FILE);
        assert!(previous.exists());

        let installed = crate::package::Package::read_meta(
            &root.path.join("games").join(PACKAGE_FILE),
        )
        .unwrap();
        assert_eq!(installed.version, "v2.0.0");

        assert!(rollback(&root, "games").unwrap());
        let rolled_back = crate::package::Package::read_meta(
            &root.path.join("games").join(PACKAGE_FILE),
        )
        .unwrap();
        assert_eq!(rolled_back.version, "v1.0.0");

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn an_uninstall_leaves_no_directory_and_no_trash() {
        let (work, root) = temp_root("uninstall");
        let bytes = package_bytes("weather", "v1.0.0", None);
        install(&bytes, &Expected::default(), std::slice::from_ref(&root), None).unwrap();

        uninstall(&root, "weather").unwrap();

        assert!(!root.path.join("weather").exists());
        assert!(!root.path.join(".trash").exists() || !root.path.join(".trash").read_dir().unwrap().any(|e| e.is_ok()));

        std::fs::remove_dir_all(&work).ok();
    }

    /// The store's path: the bytes are already a file, so the install verifies and renames it rather
    /// than reading it back into memory. What must survive the press is the application directory,
    /// and what must not is the `.part` file it came from.
    #[test]
    fn a_downloaded_file_is_installed_and_the_part_file_does_not_survive_it() {
        let (work, root) = temp_root("file");
        let bytes = package_bytes("power-log", "v1.0.0", None);
        let download = work.join("power-log-v1.0.0.part");
        std::fs::write(&download, &bytes).unwrap();

        let expected = Expected::new(bytes.len() as u64, sha256_hex(&bytes));
        let installed =
            install_file(&download, &expected, std::slice::from_ref(&root), None).unwrap();

        assert_eq!(installed.meta.id, "power-log");
        assert_eq!(installed.meta.version, "v1.0.0");
        assert!(root.path.join("power-log").join(PACKAGE_FILE).exists());
        assert!(root.path.join("power-log").join(DATA_DIR).is_dir());
        assert!(!download.exists(), "the downloaded .part is consumed by the install");
        assert!(!root.path.join(STAGING_DIR).exists());

        std::fs::remove_dir_all(&work).ok();
    }

    /// A file that is not what the manifest named, and one whose container does not hold together,
    /// are both refused before an application directory exists — and neither leaves staging behind.
    #[test]
    fn a_file_install_that_fails_leaves_no_application_and_no_staging() {
        let (work, root) = temp_root("file-reject");
        let bytes = package_bytes("weather", "v1.0.0", None);
        let download = work.join("weather.part");
        std::fs::write(&download, &bytes).unwrap();

        let wrong_hash = Expected::new(bytes.len() as u64, "00".repeat(32));
        assert!(matches!(
            install_file(&download, &wrong_hash, std::slice::from_ref(&root), None),
            Err(InstallError::ShaMismatch { .. })
        ));
        assert!(!root.path.join("weather").exists());
        assert!(!root.path.join(STAGING_DIR).exists());

        // The module's section is the half a truncated file loses; the metadata may still be whole.
        std::fs::write(&download, &bytes[..bytes.len() - 1]).unwrap();
        assert!(matches!(
            install_file(&download, &Expected::default(), std::slice::from_ref(&root), None),
            Err(InstallError::Package(PackageError::Container(
                app_abi::PackageError::LengthMismatch { .. }
            )))
        ));
        assert!(!root.path.join("weather").exists());
        assert!(!root.path.join(STAGING_DIR).exists());

        std::fs::remove_dir_all(&work).ok();
    }

    /// A bigger package picks the card by default, and the store can ask for internal explicitly.
    #[test]
    fn the_volume_is_chosen_by_size_unless_the_caller_says_otherwise() {
        let work = std::env::temp_dir().join(format!(
            "hivebox-install-volume-{}-{}",
            std::process::id(),
            stamp()
        ));

        let internal = Root::new(work.join("internal"), Source::Internal);
        let card = Root::new(work.join("card"), Source::Card);
        std::fs::create_dir_all(&internal.path).unwrap();
        std::fs::create_dir_all(&card.path).unwrap();

        let root = choose_root(&[internal.clone(), card.clone()], None, 1024).unwrap();
        assert_eq!(root.source, Source::Internal);

        let root = choose_root(
            &[internal.clone(), card.clone()],
            None,
            SMALL_PACKAGE_BYTES + 1,
        )
        .unwrap();
        assert_eq!(root.source, Source::Card);

        let root = choose_root(
            &[internal.clone(), card.clone()],
            Some(Source::Internal),
            SMALL_PACKAGE_BYTES + 1,
        )
        .unwrap();
        assert_eq!(root.source, Source::Internal);

        std::fs::remove_dir_all(&work).ok();
    }
}
