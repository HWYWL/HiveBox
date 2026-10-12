//! Storage interface — the board's filesystems.

use crate::error::HalError;
use crate::types::{FlashLayout, VolumeInfo};

/// The volumes the board has mounted: the built-in partition, and the card in the slot.
///
/// # Why this is a backend and not a path
///
/// The built-in partition is mounted by the firmware before the UI starts, and the card is mounted
/// by this backend, because **a card is not there until someone mounts it**: it is not present at
/// boot, it is not present when the box was sealed, and it can leave while the board is running.
/// What is *in* the filesystem is nobody's business here — the terminal, the music player and the
/// Wi-Fi credentials file all reach it through `std::fs`, over the VFS. This trait answers the one
/// question those cannot: which filesystems exist right now, and how full they are.
///
/// # No card-detect pin
///
/// The reference board wires a microSD slot to SDMMC in 1-bit mode with neither card-detect nor
/// write-protect, so there is nothing to poll: the only way to know whether a card is in the slot is
/// to try to mount one. That is what [`StorageBackend::refresh`] is for, and why a page asks for it
/// when it opens rather than subscribing to frames — a probe that finds nothing still spends the
/// SDMMC driver's timeouts.
pub trait StorageBackend: Send + Sync {
    /// Bring up the host and mount a card if one is in the slot.
    ///
    /// Idempotent and best-effort: an empty slot is the ordinary case, not a failure, so a board with
    /// no card comes up exactly as one with a card does — with a volume fewer.
    fn init(&mut self) -> Result<(), HalError>;

    /// Every volume that is mounted right now: the built-in one first, then the card, if there is one.
    ///
    /// A volume is only ever reported when something is mounted there, so an empty slot is an absent
    /// entry rather than an entry with no size in it. An `Err` is the board having no answer at all,
    /// which is not the same thing as having no card — the caller that draws a slot must be able to
    /// tell those apart.
    fn volumes(&self) -> Result<Vec<VolumeInfo>, HalError>;

    /// Re-probe the slot: mount a card that has appeared, drop one that has gone away.
    ///
    /// Call it when the answer matters — a page opening, a finger on "check again" — and not once a
    /// frame. Finding nothing is not an error: it is what an empty slot looks like.
    fn refresh(&mut self) -> Result<(), HalError>;

    /// The flash chip those filesystems live on, and what every part of it is for.
    ///
    /// The chip and not a filesystem. `volumes` answers "how full is what I can write"; a board whose
    /// built-in volume is 3 MB of a 16 MB chip has 13 MB that answer says nothing about, and the
    /// person looking at a storage page is asking where it went. See [`FlashLayout`].
    ///
    /// Unlike the two above this does not change while the box runs — a partition table is written
    /// once, by whoever flashed the board — so a page may read it once and keep it.
    fn flash(&self) -> Result<FlashLayout, HalError>;

    // -------------------------------------------------------------------------------------------
    // The write side of a filesystem, which on this board `std::fs` does not have.
    // -------------------------------------------------------------------------------------------

    /// Write `data` to `path`, replacing whatever was there, making the directories above it.
    ///
    /// # Why the backend answers this at all
    ///
    /// Everything *inside* a mounted filesystem is `std::fs`'s business, as the note above says — with
    /// one exception, and this is it. On this board `std::fs` reads a file, stats it, lists a
    /// directory and **makes** one, and cannot create a *file*: `File::create`, every `OpenOptions`
    /// spelling, all come back `ENOENT`. The measurement is written out above `read_credentials` in
    /// the ESP32 backend, which is where the Wi-Fi credentials used to be lost to it.
    ///
    /// So a write goes through the backend, whose C side writes with the same `fopen` `main.c` has
    /// always written the partition with. An app that saves something of its own — a log, a chime it
    /// synthesised, a picture off the panel — calls this and not `std::fs`. Reading it back is
    /// `std::fs`'s again, because reading was never the broken half.
    ///
    /// `data` is bytes and not text: a WAV or a PNG has no business being a `String`. The
    /// directories above `path` are made on the way, so the first write to `AppData/CLOCK/` needs no
    /// second call.
    fn write_file(&self, path: &str, data: &[u8]) -> Result<(), HalError>;

    /// Append `data` to `path`, creating the file — and the directories above it — when it is absent.
    ///
    /// The same write as [`StorageBackend::write_file`], except that what the file already holds stays
    /// where it is. This exists for a log and for nothing else: a line an hour added to the end of a
    /// file would otherwise be a read of every line before it, a parse, and a write of all of them
    /// back, which is a cost that grows with the file's age.
    fn append_file(&self, path: &str, data: &[u8]) -> Result<(), HalError>;

    /// Remove `path`.
    ///
    /// Removing something that is not there is `Ok`. The caller asked for the file to be gone and it
    /// is gone; "make sure this is not here" and "delete this" want the same thing from this call,
    /// and neither of them is served by an error that means *already true*.
    fn remove_file(&self, path: &str) -> Result<(), HalError>;

    /// Move `from` to `to`, replacing `to` when there is something there.
    ///
    /// Not a copy and a delete spelled out by the caller: a filesystem can do this without moving the
    /// bytes, and a file manager that renamed a 2 GB video through its own buffers would be a page
    /// that froze on a button press. The C side tries `rename` first and falls back to copying only
    /// for the pairs of mounts where the first cannot work.
    fn rename(&self, from: &str, to: &str) -> Result<(), HalError>;
}
