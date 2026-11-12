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
    pub player_chat_v2:
        fn(u128, &str, u8, &str, &mut Vec<horizon_client_core::text::Text>, &mut bool),
    pub chat_decorate: fn(u128, &str, &str, &mut Option<horizon_client_core::text::Text>),
    pub living_breathe: fn(u32, &str, &mut bool, &mut i32, &mut i32),
    pub living_drown: fn(u32, &str, &mut bool, &mut f32, i32, &mut bool),
    pub living_fall: fn(u32, &str, &mut f64, &mut f32, &mut bool),
    pub living_jump: fn(u32, &str),
    pub register_goals: fn(&mut crate::mobs::Mob),
    pub player_pre_login: fn(
        u128,
        &str,
        std::net::SocketAddr,
        &mut Vec<std::sync::mpsc::Receiver<Result<(), horizon_client_core::text::Text>>>,
    ),
    pub player_profile: fn(u128, &str, &mut Vec<horizon_protocol::packets::login::Property>),
    pub ingredient: fn(&str, &Map<String, Value>, &mut Vec<String>),
    pub ingredient_test: fn(&str, &Map<String, Value>, &ItemStack, &mut bool),
    pub log_line: fn(&str, &str, &str, &str),
    pub living_death_message: fn(u32, &str, &mut Option<horizon_client_core::text::Text>),
    pub handshake: fn(
        std::net::SocketAddr,
        &mut String,
        i32,
        &mut bool,
        &mut Option<horizon_client_core::text::Text>,
    ),
}

static HOOKS: OnceLock<NativeClientHooks> = OnceLock::new();

/// Install the process-wide callbacks once; a second installation keeps the first.
#[allow(
    clippy::result_large_err,
    reason = "the rejected table is handed back whole; it is installed once at start"
)]
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

pub(crate) fn player_chat_v2(
    player: u128,
    name: &str,
    level: u8,
    message: &str,
    replies: &mut Vec<horizon_client_core::text::Text>,
    cancel: &mut bool,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.player_chat_v2)(player, name, level, message, replies, cancel);
    }
}

pub(crate) fn chat_decorate(
    player: u128,
    name: &str,
    message: &str,
    decorated: &mut Option<horizon_client_core::text::Text>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.chat_decorate)(player, name, message, decorated);
    }
}

pub(crate) fn living_heal(entity: u32, amount: &mut f32, cancel: &mut bool) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_heal)(entity, amount, cancel);
    }
}

pub(crate) fn living_death_message(
    entity: u32,
    kind: &str,
    message: &mut Option<horizon_client_core::text::Text>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_death_message)(entity, kind, message);
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

pub(crate) fn player_pre_login(
    player: u128,
    name: &str,
    address: std::net::SocketAddr,
    defer: &mut Vec<std::sync::mpsc::Receiver<Result<(), horizon_client_core::text::Text>>>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.player_pre_login)(player, name, address, defer);
    }
}

pub(crate) fn player_profile(
    player: u128,
    name: &str,
    properties: &mut Vec<horizon_protocol::packets::login::Property>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.player_profile)(player, name, properties);
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

/// A message was written to the log (`logs/latest.log`), on the log's own writer thread, never
/// the server's or the render thread, so a slow mod holds up no tick (the file is written first,
/// and the lines a mod logs here come back to it later). `level` is `INFO`, `WARN` or `ERROR`,
/// `thread` the thread it names (`Server thread`, `User Authenticator #1`...), `message` the text
/// as logged and `line` what the file got, `[HH:mm:ss] [thread/LEVEL]: message`, a multi-line
/// message's further lines after newlines. Every message the file gets, from the client or a
/// dedicated server, from the first lines on; lines the flood limit leaves out are not heard.
/// There is no `Game`: keep what you need and act on it in another hook. A mod that panics
/// here is not called again. API @API:log_line@.
#[inline(never)]
pub fn native_log_line(level: &str, thread: &str, message: &str, line: &str) {
    log_line(level, thread, message, line);
}

pub(crate) fn log_line(level: &str, thread: &str, message: &str, line: &str) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.log_line)(level, thread, message, line);
    }
}

pub(crate) fn handshake(
    address: std::net::SocketAddr,
    host: &mut String,
    intent: i32,
    cancel: &mut bool,
    reason: &mut Option<horizon_client_core::text::Text>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.handshake)(address, host, intent, cancel, reason);
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
