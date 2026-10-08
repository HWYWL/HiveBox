//! Storage interface — the board's filesystems.

use crate::error::HalError;
use crate::types::VolumeInfo;

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
}
