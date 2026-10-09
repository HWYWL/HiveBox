//! Audio playback interface.

use crate::error::HalError;
use crate::types::AudioMeta;

/// PCM audio playback.
pub trait AudioBackend: Send + Sync {
    /// Open the sink and settle on a volume.
    ///
    /// Called once, by [`crate::Board::init`], before the first `play`. What a backend does here is
    /// its own business — the device reads the volume off its own partition — and the one thing it
    /// owes the caller is that afterwards [`Self::volume`] answers with what the board last sounded
    /// like rather than with a default.
    ///
    /// The volume is set from here and not only from [`Self::set_volume`] because it is the board's
    /// and not the page's: a board with something else to say comes up as loud as it was left, with
    /// no player open to ask.
    fn init(&mut self) -> Result<(), HalError>;
    /// Start playback of the file at `path`.
    fn play(&mut self, path: &str) -> Result<AudioMeta, HalError>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    /// Set the volume, as a percentage. Whatever the board does with it afterwards — this one writes
    /// it down — is not the caller's business.
    fn set_volume(&mut self, volume: u8);
    /// How loud the sink is, as a percentage: what [`Self::init`] read back, or what
    /// [`Self::set_volume`] last set.
    ///
    /// A getter rather than a field the caller keeps, because two of them would be one too many:
    /// the thing that knows is the thing that was told.
    fn volume(&self) -> u8;
    fn is_playing(&self) -> bool;
    /// Playback position in seconds.
    fn position_secs(&self) -> f32;
    /// Poll for end-of-stream and internal state transitions.
    fn tick(&mut self);
}
