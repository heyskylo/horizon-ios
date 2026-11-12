/// The instance a mob effect gives the player; `None` for a creative or
/// spectator player (`immune`) unless it is ambient (the nautilus's Breath).
pub(super) fn mob_effect_instance(
    effect: &'static str,
    ticks: i32,
    amplifier: i32,
    ambient: bool,
    immune: bool,
) -> Option<horizon_client::effects::EffectInstance> {
    if immune && !ambient {
        return None;
    }
    let id = effect.strip_prefix("minecraft:").unwrap_or(effect);
    let mut instance = horizon_client::effects::EffectInstance::new(id, ticks, amplifier);
    instance.ambient = ambient;
    Some(instance)
}
