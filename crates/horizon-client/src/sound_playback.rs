//! Client sound interception, bridged from the render-owned engine to trait mods.
use crate::sound_engine::SoundPlayback;

/// A client sound is about to start or update (`sound.starting`). Change
/// `volume`/`pitch` or cancel before subtitles and backend writes. Parameters
/// are the instance's base values, before the chosen file's random samples,
/// category/master gain and ordinary pitch clamps. Edits are transient:
/// tickable updates and loop replays start from current unmodified values.
/// Changed values must be finite, volume nonnegative and pitch positive;
/// invalid edits cancel that operation (an update stops the sound). No Host,
/// world borrow or reverb/DSP surface is supplied. Runs on the game thread.
/// API @API:sound_play@.
#[inline(never)]
pub fn native_sound_play(
    sound: &SoundPlayback<'_>,
    volume: &mut f32,
    pitch: &mut f32,
    cancel: &mut bool,
) {
    crate::native_client_hooks::sound_play(sound, volume, pitch, cancel);
}
