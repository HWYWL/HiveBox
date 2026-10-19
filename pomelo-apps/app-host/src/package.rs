//! Reading a `.hbapp` file: the container, the metadata inside it, and the icon.
//!
//! Two readers live here and they read different amounts on purpose. [`Package::read`] takes the
//! whole file, because the VM is about to need the module anyway. [`Package::read_meta`] takes the
//! header and the JSON document and stops, because the launcher asks every installed package what it
//! is on every boot — and a boot that read a dozen megabyte-sized modules to draw a dozen tiles would
//! be a boot that spent its time on files nobody had opened.
//!
//! Neither reader executes anything. A package is checked structurally here: magic, format, ABI,
//! section lengths, metadata fields, icon planes. What is *not* checked is whether the module is a
//! wasm module at all — that is `wasmi`'s answer, and it is asked at the moment a person opens the
//! application rather than at install time, so that a module this build of the interpreter cannot
//! compile is a store entry that cannot start rather than an install that fails with no explanation.

use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use app_abi::{
    icon_parts, PackageError as ContainerError, PackageHeader, Sections, MAX_META_BYTES,
    PACKAGE_HEADER_LEN,
};

use crate::meta::{Meta, MetaError};

/// A package, owned: everything the runtime needs to run it and the tile needs to draw it.
#[derive(Clone, Debug)]
pub struct Package {
    pub meta: Meta,
    pub wasm: Vec<u8>,
    pub icon: Option<Icon>,
}

/// The bitmap half of a package's identity: raw RGB565 pixels and an alpha plane.
///
/// Stored as the two planes the panel's own icon path takes rather than as a decoded image, because
/// decoding would mean an image library in the firmware and this is already the format the renderer
/// wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub width: u16,
    pub height: u16,
    rgb565: Vec<u16>,
    alpha: Vec<u8>,
}

impl Icon {
    /// Builds an icon from planes that were already checked against their dimensions.
    pub fn from_parts(width: u16, height: u16, rgb565: Vec<u16>, alpha: Vec<u8>) -> Self {
        debug_assert_eq!(rgb565.len(), width as usize * height as usize);
        debug_assert_eq!(alpha.len(), width as usize * height as usize);

        Self {
            width,
            height,
            rgb565,
            alpha,
        }
    }

    /// The pixels, one `u16` per pixel, in row-major order.
    pub fn rgb565(&self) -> &[u16] {
        &self.rgb565
    }

    /// The alpha plane, one byte per pixel.
    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }
}

impl Package {
    /// Reads a whole package out of bytes in memory.
    pub fn open(bytes: &[u8]) -> Result<Self, PackageError> {
        let header = PackageHeader::parse(bytes).map_err(PackageError::Container)?;
        let sections = Sections::of(bytes, &header);

        let meta = std::str::from_utf8(sections.meta)
            .map_err(|_| PackageError::MetadataNotUtf8)
            .and_then(|text| Meta::parse(text).map_err(PackageError::Meta))?;

        if meta.abi != header.abi_major {
            return Err(PackageError::AbiMismatch {
                meta: meta.abi,
                container: header.abi_major,
            });
        }

        let icon = if sections.icon.is_empty() {
            None
        } else {
            Some(decode_icon(sections.icon)?)
        };

        Ok(Self {
            meta,
            wasm: sections.wasm.to_vec(),
            icon,
        })
    }

    /// Reads a package from disk.
    pub fn read(path: &Path) -> Result<Self, PackageError> {
        let bytes = std::fs::read(path).map_err(|error| PackageError::Io(error.to_string()))?;

        Self::open(&bytes)
    }

    /// Reads only the header and the metadata of a package on disk.
    ///
    /// The catalog's read: a package whose module is megabytes costs its metadata here and nothing
    /// more.
    pub fn read_meta(path: &Path) -> Result<Meta, PackageError> {
        let (mut file, header) = open_head(path)?;

        read_meta_section(&mut file, &header)
    }

    /// Reads the metadata and the icon of a package on disk, and not the module.
    ///
    /// The installer's read. A download lands as a *file*, and the install path must verify and
    /// place that file without ever holding the module: a package is up to two megabytes of code
    /// and the box has better uses for its RAM than a copy of it. The icon is read because it is
    /// small (27 KB), written beside the package as the catalog's cache, and the one part of a
    /// package that a boot draws without opening the module.
    pub fn read_meta_and_icon(path: &Path) -> Result<(Meta, Option<Icon>), PackageError> {
        let (mut file, header) = open_head(path)?;
        let meta = read_meta_section(&mut file, &header)?;

        if header.icon_len == 0 {
            return Ok((meta, None));
        }

        // The file is positioned just past the metadata, and the module is next: skip it rather
        // than read it.
        file.seek(SeekFrom::Current(i64::from(header.wasm_len)))
            .map_err(|error| PackageError::Io(error.to_string()))?;

        let mut icon_bytes = vec![0u8; header.icon_len as usize];
        file.read_exact(&mut icon_bytes)
            .map_err(|error| PackageError::Io(error.to_string()))?;

        Ok((meta, Some(decode_icon(&icon_bytes)?)))
    }
}

/// Opens a package and reads its header, checking the file is exactly as long as the header says.
fn open_head(path: &Path) -> Result<(std::fs::File, PackageHeader), PackageError> {
    let mut file = std::fs::File::open(path).map_err(|error| PackageError::Io(error.to_string()))?;

    let mut header_bytes = [0u8; PACKAGE_HEADER_LEN];

    file.read_exact(&mut header_bytes)
        .map_err(|error| PackageError::Io(error.to_string()))?;

    let header = PackageHeader::decode(&header_bytes).map_err(PackageError::Container)?;

    // The file's own length, not the header's: a half-downloaded package is a file whose metadata
    // may be intact and whose module is not, and it must not be offered as installed.
    let length = file
        .metadata()
        .map_err(|error| PackageError::Io(error.to_string()))?
        .len();

    if length != header.total_len() {
        return Err(PackageError::Container(ContainerError::LengthMismatch {
            declared: header.total_len(),
            actual: length,
        }));
    }

    Ok((file, header))
}

/// Reads the metadata section, leaving the file positioned at the module.
fn read_meta_section(
    file: &mut std::fs::File,
    header: &PackageHeader,
) -> Result<Meta, PackageError> {
    if header.meta_len as usize > MAX_META_BYTES {
        return Err(PackageError::Container(ContainerError::MetadataTooLarge));
    }

    let mut meta_bytes = vec![0u8; header.meta_len as usize];
    file.read_exact(&mut meta_bytes)
        .map_err(|error| PackageError::Io(error.to_string()))?;

    let meta = std::str::from_utf8(&meta_bytes)
        .map_err(|_| PackageError::MetadataNotUtf8)
        .and_then(|text| Meta::parse(text).map_err(PackageError::Meta))?;

    if meta.abi != header.abi_major {
        return Err(PackageError::AbiMismatch {
            meta: meta.abi,
            container: header.abi_major,
        });
    }

    Ok(meta)
}

fn decode_icon(section: &[u8]) -> Result<Icon, PackageError> {
    let (width, height, rgb, alpha) = icon_parts(section).ok_or(PackageError::BadIcon)?;

    let rgb565 = rgb
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();

    Ok(Icon {
        width,
        height,
        rgb565,
        alpha: alpha.to_vec(),
    })
}

/// Why a package could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageError {
    Container(ContainerError),
    Meta(MetaError),
    MetadataNotUtf8,
    AbiMismatch { meta: u16, container: u16 },
    BadIcon,
    Io(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => write!(f, "{}", error.sentence()),
            Self::Meta(error) => write!(f, "{error}"),
            Self::MetadataNotUtf8 => write!(f, "应用包的说明不是 UTF-8"),
            Self::AbiMismatch { meta, container } => {
                write!(f, "应用包两处 ABI 不一致（{meta} / {container}）")
            }
            Self::BadIcon => write!(f, "应用包的图标数据不完整"),
            Self::Io(error) => write!(f, "读取应用包失败: {error}"),
        }
    }
}

impl std::error::Error for PackageError {}

#[cfg(test)]
mod tests {
    use super::*;
    use app_abi::{encode_icon, PackageHeader};

    pub(crate) fn build(meta: &str, wasm: &[u8], icon: &[u8]) -> Vec<u8> {
        let header = PackageHeader::new(meta.len() as u32, wasm.len() as u32, icon.len() as u32, 0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&header.encode());
        bytes.extend_from_slice(meta.as_bytes());
        bytes.extend_from_slice(wasm);
        bytes.extend_from_slice(icon);
        bytes
    }

    const META: &str = r##"{"id":"calculator","name":"Calculator","name_zh":"计算器",
        "version":"v1.0.0","abi":1,"memory_kb":256,"accent":"#F69E22"}"##;

    fn wasm() -> Vec<u8> {
        b"\0asm\x01\0\0\0".to_vec()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hivebox-app-host-{name}-{}-{}",
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
    fn a_package_is_its_metadata_and_its_module() {
        let bytes = build(META, &wasm(), b"");
        let package = Package::open(&bytes).expect("a package this crate built");

        assert_eq!(package.meta.id, "calculator");
        assert_eq!(package.wasm, wasm());
        assert!(package.icon.is_none());
    }

    #[test]
    fn an_icon_is_read_into_the_two_planes_the_tile_takes() {
        let rgb = [0x00u8, 0xF8, 0xE0, 0x07]; // red, green
        let alpha = [0xFF, 0x80];
        let icon = encode_icon(2, 1, &rgb, &alpha).unwrap();

        let package = Package::open(&build(META, &wasm(), &icon)).unwrap();
        let icon = package.icon.expect("the icon in the package");

        assert_eq!((icon.width, icon.height), (2, 1));
        assert_eq!(icon.rgb565(), &[0xF800, 0x07E0]);
        assert_eq!(icon.alpha(), &[0xFF, 0x80]);
    }

    #[test]
    fn metadata_alone_is_read_from_disk_without_the_module() {
        let dir = temp_dir("meta");
        let path = dir.join("app.hbapp");
        std::fs::write(&path, build(META, &wasm(), b"")).unwrap();

        let meta = Package::read_meta(&path).expect("the metadata of a package on disk");
        assert_eq!(meta.id, "calculator");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A package whose file is shorter than its header says is not installed, however readable its
    /// metadata is: the module behind it is the half that would be executed.
    #[test]
    fn a_truncated_package_on_disk_is_refused() {
        let dir = temp_dir("truncated");
        let path = dir.join("app.hbapp");
        let mut bytes = build(META, &wasm(), b"");
        bytes.truncate(bytes.len() - 2);
        std::fs::write(&path, bytes).unwrap();

        assert!(matches!(
            Package::read_meta(&path),
            Err(PackageError::Container(ContainerError::LengthMismatch { .. }))
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The two places a package states its ABI must agree, or the metadata's memory budget and
    /// permissions would be describing a different module than the one in the file.
    #[test]
    fn the_metadata_may_not_disagree_with_the_container_about_the_abi() {
        let meta = r#"{"id":"x","name":"x","version":"v1","abi":2}"#;
        assert!(matches!(
            Package::open(&build(meta, &wasm(), b"")),
            Err(PackageError::AbiMismatch {
                meta: 2,
                container: 1
            })
        ));
    }

    #[test]
    fn a_missing_or_broken_metadata_document_is_a_refusal() {
        assert!(matches!(
            Package::open(&build("not json", &wasm(), b"")),
            Err(PackageError::Meta(_))
        ));
    }
}
