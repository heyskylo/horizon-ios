//! The death message a tamed mob sends its owner (`TamableAnimal.die`), as a native mod may change it.

/// A tamed mob's death message, as `TamableAnimal.die` sends it to its owner: asked once per such death, after
/// the message is built and only when it would be sent (the owner is online and `show_death_messages` is on).
/// `entity` is the mob's ID, `kind` its type (`minecraft:wolf`). `message` holds the text Horizon would send:
/// replace it, or set it to `None` to tell the owner nothing. Every mod is asked in order and may change it again.
/// Server-side and on the host. Pending API @API:living_death_message@.
#[inline(never)]
pub fn native_living_death_message(
    entity: u32,
    kind: &str,
    message: &mut Option<horizon_client_core::text::Text>,
) {
    crate::native_client_hooks::living_death_message(entity, kind, message);
}
