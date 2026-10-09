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
    /// Move the volume *without* writing it down: what a finger dragging the level says, once a
    /// frame, while it is still deciding.
    ///
    /// Written down is expensive — the device erases and rewrites a flash partition — so a drag
    /// only sounds different and [`Self::set_volume`] at the end of the drag is what the next boot
    /// hears. The default is `set_volume` itself, which is right for a backend that keeps nothing:
    /// there is nothing to defer, and one write per frame costs it nothing.
    fn preview_volume(&mut self, volume: u8) {
        self.set_volume(volume);
    }
    fn is_playing(&self) -> bool;
    /// Playback position in seconds.
    fn position_secs(&self) -> f32;
    /// Move playback to `position_secs` from the start of the track, leaving the sink doing whatever
    /// it was doing: a paused track stays paused, a playing one keeps playing, and one that had
    /// already run out plays again from where the needle was put — a bar that moves over a silent
    /// speaker is a bar that lied.
    ///
    /// A *stopped* track is not one of those: stopping closes it ([`Self::stop`]), and what is closed
    /// has nothing to seek in, so the call is quietly nothing. So is a position past the end, which a
    /// backend clamps rather than refuses — a finger that has run out of bar is not an error — and so
    /// is a seek before anything has ever played, which is the same case wearing a different hat.
    fn seek(&mut self, position_secs: f32) -> Result<(), HalError>;
    /// Poll for end-of-stream and internal state transitions.
    fn tick(&mut self);
}
