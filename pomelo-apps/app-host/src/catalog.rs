//! What is installed, found by looking.
//!
//! There is no database. An installed application is a directory with a package in it, and the
//! catalog is the answer to "walk the two directories that may hold one and read what is there".
//! That is deliberate: a list file would be a second place a package exists, and the two would
//! disagree the first time an install was interrupted, a card was pulled, or someone copied a
//! package in by hand — which is a supported way to install one.
//!
//! # Two roots, one winner
//!
//! [`Source::Internal`] is the box's own flash and wins every tie; [`Source::Card`] is the removable
//! volume and is an extension, not a dependency. A package installed on the card disappears when the
//! card does, and the catalog simply stops listing it; the same package on the internal volume is
//! listed with the card out. When both hold the same id, the newer version is the one that runs and
//! an equal version resolves to internal — the same rule the install path uses when it decides where
//! a package goes.
//!
//! # What a catalog is not
//!
//! It does not start anything and it does not validate a wasm module. It reads 32 header bytes, a
//! JSON document of a few hundred, and an icon cache if one was written — nothing that scales with
//! the size of a module, because this runs before the desktop is drawn.

use std::path::{Path, PathBuf};

use crate::meta::{validate_id, Meta};
use crate::package::{Icon, Package, PackageError};
use crate::version::Version;

/// The name of the package file inside an application's directory.
pub const PACKAGE_FILE: &str = "app.hbapp";

/// The name of the icon cache inside an application's directory.
///
/// A cache, not the truth: the icon section of [`PACKAGE_FILE`] is what a package ships, and this is
/// that section written out at install time so that a boot does not read a module to draw a tile.
/// A missing or short cache is a tile with no bitmap, not a broken application.
pub const ICON_FILE: &str = "icon.bin";

/// The file whose presence means "this application has crashed often enough to be stopped".
pub const DISABLED_FILE: &str = "disabled";

/// The previous package, kept across an upgrade so that a bad version can be gone back from.
pub const PREVIOUS_FILE: &str = "app.hbapp.prev";

/// The directory an application keeps its own data in.
pub const DATA_DIR: &str = "data";

/// Which volume an application came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// `/internal/AppData/apps`: the box's own flash, written by the last OTA and untouched by the
    /// next one.
    Internal,
    /// `/sdcard/apps`: removable, larger, and allowed to vanish.
    Card,
}

impl Source {
    /// The sentence the store uses when it says where a package lives.
    pub fn label(self) -> &'static str {
        match self {
            Self::Internal => "内置存储",
            Self::Card => "存储卡",
        }
    }
}

/// One place packages may live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub path: PathBuf,
    pub source: Source,
}

impl Root {
    pub fn new(path: impl Into<PathBuf>, source: Source) -> Self {
        Self {
            path: path.into(),
            source,
        }
    }

    /// The two roots this project uses, internal first.
    pub fn standard() -> Vec<Self> {
        vec![
            Self::new("/internal/AppData/apps", Source::Internal),
            Self::new("/sdcard/apps", Source::Card),
        ]
    }
}

/// An installed application.
#[derive(Clone, Debug)]
pub struct Installed {
    pub meta: Meta,
    pub source: Source,
    /// The application's own directory, which is also the sandbox its files live under.
    pub dir: PathBuf,
    /// The icon cache, when one was written.
    pub icon: Option<Icon>,
    /// Whether the launcher should refuse to start it: it has crashed [`crate::hosted::CRASH_LIMIT`]
    /// times without a successful frame.
    pub disabled: bool,
}

impl Installed {
    /// The package file this application runs from.
    pub fn package_path(&self) -> PathBuf {
        self.dir.join(PACKAGE_FILE)
    }

    /// The directory the application may write to.
    pub fn data_dir(&self) -> PathBuf {
        self.dir.join(DATA_DIR)
    }
}

/// A directory that looked like an application and could not be read as one.
///
/// Kept rather than silently skipped: the store shows these, because a package that was half
/// downloaded or built for a newer system is something a person can act on, and a tile that simply
/// never appears is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Broken {
    pub dir: PathBuf,
    pub id: String,
    pub reason: String,
}

/// Everything installed, across every root.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    roots: Vec<Root>,
    apps: Vec<Installed>,
    broken: Vec<Broken>,
}

impl Catalog {
    /// Walks the two roots this project uses.
    pub fn standard() -> Self {
        Self::scan(Root::standard())
    }

    /// Walks the roots given, in order, and reads what is there.
    pub fn scan(roots: Vec<Root>) -> Self {
        let mut catalog = Self {
            roots,
            apps: Vec::new(),
            broken: Vec::new(),
        };

        catalog.reload();
        catalog
    }

    /// Walks the roots again, replacing what was found.
    pub fn reload(&mut self) {
        self.apps.clear();
        self.broken.clear();

        for root in self.roots.clone() {
            self.scan_root(&root);
        }
    }

    /// Every application that may be started, newest version per id, in the order it was found.
    pub fn apps(&self) -> &[Installed] {
        &self.apps
    }

    /// The directories that looked like applications and were not.
    pub fn broken(&self) -> &[Broken] {
        &self.broken
    }

    pub fn get(&self, id: &str) -> Option<&Installed> {
        self.apps.iter().find(|app| app.meta.id == id)
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// The version installed for an id, if any.
    pub fn version(&self, id: &str) -> Option<Version> {
        self.get(id).and_then(|app| Version::parse(&app.meta.version))
    }

    /// Forgets an application, without touching the disk: the caller has already removed it.
    pub fn remove(&mut self, id: &str) {
        self.apps.retain(|app| app.meta.id != id);
    }

    fn scan_root(&mut self, root: &Root) {
        let Ok(entries) = std::fs::read_dir(&root.path) else {
            // A root that does not exist is a box with no applications on that volume, which is not
            // an error: the internal volume has no `apps` directory until the first install.
            return;
        };

        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();

        dirs.sort();

        for dir in dirs {
            let Some(name) = dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
            else {
                continue;
            };

            // Directories that do not name an application are not the catalog's business: `.trash`,
            // `.staging` and whatever else the installer uses to keep a half-install out of sight.
            if name.starts_with('.') || validate_id(&name).is_err() {
                continue;
            }

            match read_installed(&dir, root.source) {
                Ok(app) if app.meta.id == name => self.insert(app),
                Ok(app) => self.broken.push(Broken {
                    dir,
                    id: name,
                    reason: format!("目录名与包内 id（{}）不一致", app.meta.id),
                }),
                Err(error) => self.broken.push(Broken {
                    dir,
                    id: name,
                    reason: error.to_string(),
                }),
            }
        }
    }

    /// Puts an application in, or replaces the one with its id when this one is newer.
    fn insert(&mut self, app: Installed) {
        match self.apps.iter().position(|existing| existing.meta.id == app.meta.id) {
            None => self.apps.push(app),
            Some(at) => {
                let existing = &self.apps[at];
                let newer = match (Version::parse(&app.meta.version), Version::parse(&existing.meta.version)) {
                    (Some(candidate), Some(current)) => candidate.is_newer_than(&current),
                    _ => false,
                };

                if newer {
                    self.apps[at] = app;
                }
            }
        }
    }
}

fn read_installed(dir: &Path, source: Source) -> Result<Installed, PackageError> {
    let package = dir.join(PACKAGE_FILE);
    let previous = dir.join(PREVIOUS_FILE);

    // A package that was being replaced when the power went is a directory with a `.prev` and no
    // package. Putting the previous one back is the only recovery a person would do by hand, so the
    // catalog does it: an interrupted upgrade is the old version, never nothing.
    if !package.exists() && previous.exists() {
        std::fs::rename(&previous, &package).map_err(|error| PackageError::Io(error.to_string()))?;
    }

    let meta = Package::read_meta(&package)?;
    let icon = read_icon(&dir.join(ICON_FILE));

    Ok(Installed {
        meta,
        source,
        dir: dir.to_path_buf(),
        icon,
        disabled: dir.join(DISABLED_FILE).exists(),
    })
}

fn read_icon(path: &Path) -> Option<Icon> {
    let bytes = std::fs::read(path).ok()?;
    let (width, height, rgb, alpha) = app_abi::icon_parts(&bytes)?;

    let rgb565 = rgb
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();

    Some(Icon::from_parts(
        width,
        height,
        rgb565,
        alpha.to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_abi::PackageHeader;

    fn package_bytes(id: &str, version: &str) -> Vec<u8> {
        let meta = format!(
            r#"{{"id":"{id}","name":"{id}","version":"{version}","abi":1,"memory_kb":128}}"#
        );
        let wasm = b"\0asm\x01\0\0\0";
        let header = PackageHeader::new(meta.len() as u32, wasm.len() as u32, 0, 0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&header.encode());
        bytes.extend_from_slice(meta.as_bytes());
        bytes.extend_from_slice(wasm);
        bytes
    }

    fn install(root: &Path, id: &str, version: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PACKAGE_FILE), package_bytes(id, version)).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hivebox-catalog-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_package_in_a_directory_is_an_installed_application() {
        let work = temp_dir("scan");
        let internal = work.join("internal");
        install(&internal, "calculator", "v1.0.0");

        let catalog = Catalog::scan(vec![Root::new(&internal, Source::Internal)]);

        assert_eq!(catalog.apps().len(), 1);
        assert!(catalog.is_installed("calculator"));
        assert_eq!(catalog.get("calculator").unwrap().source, Source::Internal);
        assert_eq!(
            catalog.get("calculator").unwrap().package_path(),
            internal.join("calculator").join(PACKAGE_FILE)
        );

        std::fs::remove_dir_all(&work).ok();
    }

    /// The card is an extension: with both volumes holding a version, the newer one runs, and an
    /// equal version resolves to the internal volume.
    #[test]
    fn the_newer_version_wins_and_a_tie_goes_to_internal() {
        let work = temp_dir("precedence");
        let internal = work.join("internal");
        let card = work.join("card");

        install(&internal, "clock", "v1.0.0");
        install(&card, "clock", "v1.2.0");

        let catalog = Catalog::scan(vec![
            Root::new(&internal, Source::Internal),
            Root::new(&card, Source::Card),
        ]);

        assert_eq!(catalog.apps().len(), 1);
        assert_eq!(catalog.get("clock").unwrap().meta.version, "v1.2.0");
        assert_eq!(catalog.get("clock").unwrap().source, Source::Card);

        install(&internal, "clock", "v1.2.0");
        let catalog = Catalog::scan(vec![
            Root::new(&internal, Source::Internal),
            Root::new(&card, Source::Card),
        ]);
        assert_eq!(catalog.get("clock").unwrap().source, Source::Internal);

        std::fs::remove_dir_all(&work).ok();
    }

    /// An interrupted upgrade left a `.prev` and no package; the catalog puts the old version back
    /// rather than showing an application that is not there.
    #[test]
    fn an_interrupted_upgrade_recovers_the_previous_package() {
        let work = temp_dir("recover");
        let root = work.join("internal");
        install(&root, "games", "v1.0.0");

        let dir = root.join("games");
        std::fs::rename(dir.join(PACKAGE_FILE), dir.join(PREVIOUS_FILE)).unwrap();

        let catalog = Catalog::scan(vec![Root::new(&root, Source::Internal)]);
        assert_eq!(catalog.get("games").unwrap().meta.version, "v1.0.0");
        assert!(dir.join(PACKAGE_FILE).exists());

        std::fs::remove_dir_all(&work).ok();
    }

    /// A directory that is not a package is reported, not listed as an application.
    #[test]
    fn a_broken_directory_is_reported_and_not_installed() {
        let work = temp_dir("broken");
        let root = work.join("internal");
        let dir = root.join("half");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PACKAGE_FILE), b"not a package").unwrap();

        let catalog = Catalog::scan(vec![Root::new(&root, Source::Internal)]);

        assert!(catalog.apps().is_empty());
        assert_eq!(catalog.broken().len(), 1);
        assert_eq!(catalog.broken()[0].id, "half");

        std::fs::remove_dir_all(&work).ok();
    }

    /// The disabled marker is a file, so that it survives a reboot and can be removed by hand.
    #[test]
    fn a_disabled_application_is_still_installed_but_marked() {
        let work = temp_dir("disabled");
        let root = work.join("internal");
        install(&root, "weather", "v1.0.0");
        std::fs::write(root.join("weather").join(DISABLED_FILE), b"").unwrap();

        let catalog = Catalog::scan(vec![Root::new(&root, Source::Internal)]);
        assert!(catalog.get("weather").unwrap().disabled);

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn a_missing_root_is_an_empty_volume_not_an_error() {
        let work = temp_dir("missing");
        let catalog = Catalog::scan(vec![Root::new(work.join("nope"), Source::Card)]);
        assert!(catalog.apps().is_empty());
        assert!(catalog.broken().is_empty());
        std::fs::remove_dir_all(&work).ok();
    }
}
