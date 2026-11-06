//! Callbacks into the executable for native trait mods' client-library hooks.
use crate::items::ItemStack;
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Client-library hooks, installed before any menu, entity or recipe is used.
pub struct NativeClientHooks {
    pub menu_slot_rules: fn(&str, usize, &ItemStack, &mut bool, &mut u32),
    pub sound_play: crate::sound_engine::PlaybackHook,
    pub living_heal: fn(u32, &mut f32, &mut bool),
    pub player_chat: fn(u128, &str, &str, &mut Vec<horizon_client_core::text::Text>, &mut bool),
    pub living_breathe: fn(u32, &str, &mut bool, &mut i32, &mut i32),
    pub living_drown: fn(u32, &str, &mut bool, &mut f32, i32, &mut bool),
    pub living_fall: fn(u32, &str, &mut f64, &mut f32, &mut bool),
    pub living_jump: fn(u32, &str),
    pub register_goals: fn(&mut crate::mobs::Mob),
    pub ingredient: fn(&str, &Map<String, Value>, &mut Vec<String>),
    pub ingredient_test: fn(&str, &Map<String, Value>, &ItemStack, &mut bool),
}

static HOOKS: OnceLock<NativeClientHooks> = OnceLock::new();

/// Install the process-wide callbacks once; a second installation keeps the first.
pub fn install(hooks: NativeClientHooks) -> Result<(), NativeClientHooks> {
    HOOKS.set(hooks)
}

pub(crate) fn menu_slot_rules(
    menu: &str,
    slot: usize,
    stack: &ItemStack,
    may_place: &mut bool,
    max_stack: &mut u32,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.menu_slot_rules)(menu, slot, stack, may_place, max_stack);
    }
}

pub(crate) fn player_chat(
    player: u128,
    name: &str,
    message: &str,
    replies: &mut Vec<horizon_client_core::text::Text>,
    cancel: &mut bool,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.player_chat)(player, name, message, replies, cancel);
    }
}

pub(crate) fn living_heal(entity: u32, amount: &mut f32, cancel: &mut bool) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_heal)(entity, amount, cancel);
    }
}

pub(crate) fn living_breathe(
    entity: u32,
    kind: &str,
    can_breathe: &mut bool,
    consume: &mut i32,
    refill: &mut i32,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_breathe)(entity, kind, can_breathe, consume, refill);
    }
}

pub(crate) fn living_drown(
    entity: u32,
    kind: &str,
    is_drowning: &mut bool,
    damage_amount: &mut f32,
    bubble_count: i32,
    cancel: &mut bool,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_drown)(
            entity,
            kind,
            is_drowning,
            damage_amount,
            bubble_count,
            cancel,
        );
    }
}

pub(crate) fn living_fall(
    entity: u32,
    kind: &str,
    distance: &mut f64,
    multiplier: &mut f32,
    cancel: &mut bool,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_fall)(entity, kind, distance, multiplier, cancel);
    }
}

pub(crate) fn living_jump(entity: u32, kind: &str) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_jump)(entity, kind);
    }
}

pub(crate) fn register_goals(mob: &mut crate::mobs::Mob) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.register_goals)(mob);
    }
}

pub(crate) fn ingredient(kind: &str, data: &Map<String, Value>, items: &mut Vec<String>) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.ingredient)(kind, data, items);
    }
}

pub(crate) fn ingredient_test(
    kind: &str,
    data: &Map<String, Value>,
    stack: &ItemStack,
    accept: &mut bool,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.ingredient_test)(kind, data, stack, accept);
    }
}

pub(crate) fn sound_play(
    sound: &crate::sound_engine::SoundPlayback<'_>,
    volume: &mut f32,
    pitch: &mut f32,
    cancel: &mut bool,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.sound_play)(sound, volume, pitch, cancel);
    }
}
