//! The four things a file manager does to a filesystem, on the board's terms.
//!
//! # Which half is whose
//!
//! Reading was never broken, so listing, `stat` and reading a file are `std::fs`'s — and so is
//! *making* a directory, which the board's `std::fs` can do. What it cannot do is create a **file**:
//! `File::create` and every `OpenOptions` spelling come back `ENOENT`, which is the measurement
//! written above `read_credentials` in the ESP32 backend. So every write — deleting, renaming, and
//! the copy a paste makes — goes through [`StorageBackend`], whose C side uses the `fopen` the
//! firmware has always written its own partition with.
//!
//! Each of these answers a `Result<(), String>` and not a HAL error, because the only caller is the
//! page and the only thing the page can do with a failure is put its words on a line for a person to
//! read: a `HalError` spent on a row would be a diagnostic code where a sentence belongs.
//!
//! [`StorageBackend`]: pomelo_hal::StorageBackend

use pomelo_hal::Board;

/// Make `path` and every directory above it.
///
/// `std::fs`'s job, and not the backend's, because making a directory is the one write the board's
/// `std::fs` *can* do. The path it is handed is the user's own name under the directory that is up,
/// which the caller has already checked with [`crate::path::valid_name`].
pub fn make_dir(path: &str) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| error.to_string())
}

/// Remove `path`: a whole tree when it is a directory, one file when it is not.
///
/// A directory goes through `std::fs`, which unlinks what is under it; a file goes through the
/// backend, because removing a file is on the write side of the board's `std::fs`. The two are not
/// one call for that reason and not for a preference.
pub fn remove(board: &Board, path: &str, is_dir: bool) -> Result<(), String> {
    if is_dir {
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())
    } else {
        board
            .storage()
            .remove_file(path)
            .map_err(|error| error.to_string())
    }
}

/// Move `from` to `to`, letting the filesystem do it rather than the page.
///
/// Through the backend's [`rename`](pomelo_hal::StorageBackend::rename), which tries `rename(2)`
/// first and only copies for the pairs of mounts where that cannot work: a file manager that renamed
/// a large file through its own buffers would be a page that froze on the button press.
pub fn rename(board: &Board, from: &str, to: &str) -> Result<(), String> {
    board
        .storage()
        .rename(from, to)
        .map_err(|error| error.to_string())
}

/// Copy `from` to `to`, a tree at a time.
///
/// The recursion is here and not in the backend because the backend copies one file when `rename`
/// cannot; a *copy* is a walk, and a walk is the caller's. What each leaf goes through is
/// [`StorageBackend::write_file`], so a copied file is created the one way this board can create
/// one.
///
/// [`StorageBackend::write_file`]: pomelo_hal::StorageBackend::write_file
pub fn copy(board: &Board, from: &str, to: &str) -> Result<(), String> {
    let source = std::fs::metadata(from).map_err(|error| error.to_string())?;

    if source.is_dir() {
        std::fs::create_dir_all(to).map_err(|error| error.to_string())?;

        let entries = std::fs::read_dir(from).map_err(|error| error.to_string())?;

        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            copy(
                board,
                &format!("{from}/{name}"),
                &format!("{to}/{name}"),
            )?;
        }

        Ok(())
    } else {
        let data = std::fs::read(from).map_err(|error| error.to_string())?;

        board
            .storage()
            .write_file(to, &data)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    /// A directory of its own under the system's temp directory, so that a test writes nowhere a
    /// person would notice and two tests never share a tree.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("pomelo-files-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch directory");
        root
    }

    #[test]
    fn a_copy_writes_the_bytes_through_the_backend() {
        let board = Board::simulated();
        let root = scratch("copy");
        let source = root.join("source.txt");
        std::fs::write(&source, b"hello").expect("the source is writable on a desktop");

        let to = root.join("copy.txt");
        copy(&board, source.to_str().unwrap(), to.to_str().unwrap()).expect("a copy");

        assert_eq!(std::fs::read(&to).unwrap(), b"hello");
    }

    #[test]
    fn a_copy_of_a_tree_comes_out_a_tree() {
        let board = Board::simulated();
        let root = scratch("tree");
        let from = root.join("from");
        std::fs::create_dir_all(from.join("inner")).unwrap();
        std::fs::write(from.join("top.txt"), b"top").unwrap();
        std::fs::write(from.join("inner/deep.txt"), b"deep").unwrap();

        let to = root.join("to");
        copy(&board, from.to_str().unwrap(), to.to_str().unwrap()).expect("a copy");

        assert_eq!(std::fs::read(to.join("top.txt")).unwrap(), b"top");
        assert_eq!(std::fs::read(to.join("inner/deep.txt")).unwrap(), b"deep");
    }

    #[test]
    fn a_directory_is_made_with_everything_above_it() {
        let root = scratch("mkdir");
        let made = root.join("a/b/c");

        make_dir(made.to_str().unwrap()).expect("three deep");

        assert!(made.is_dir());
    }

    #[test]
    fn removing_a_directory_takes_what_is_under_it() {
        let board = Board::simulated();
        let root = scratch("remove");
        let tree = root.join("tree");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("file.txt"), b"x").unwrap();

        remove(&board, tree.to_str().unwrap(), true).expect("a removal");

        assert!(!tree.exists());
    }

    #[test]
    fn removing_a_file_that_is_not_there_is_not_a_failure() {
        let board = Board::simulated();
        let root = scratch("absent");
        let nowhere = root.join("nothing.txt");

        remove(&board, nowhere.to_str().unwrap(), false).expect("already gone is gone");
    }
}
