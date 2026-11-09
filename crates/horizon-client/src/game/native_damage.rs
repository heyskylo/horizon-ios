//! Who a hit names, for `native_living_incoming_damage_v3` (docs/native-mods.md, "Server hooks
//! for plugin ports"): the hit arrives through `health::HURT_SERVER` with entity ids only, and
//! no `Game` to look a player up in, so the server's players are noted by entity id as each
//! guest's turn starts. Kept out of `native_hooks.rs`, whose lines may not hold macros.
use horizon_client::health::{Hurting, knockback_attacker};
use horizon_client::mobs::Who;
use std::{cell::RefCell, collections::HashMap};

/// More players than a server holds at once: past it, the notes start over (an entity id
/// is never reused, so a left or respawned player's note is only stale).
const MAX_NOTED: usize = 4096;

thread_local! {
    /// The server's players' UUIDs by entity id, noted on the thread that ticks the world.
    static PLAYERS: RefCell<HashMap<i32, u128>> = RefCell::new(HashMap::new());
}

/// Notes that the player with entity id `entity` is `uuid` (a guest whose turn starts), while
/// a native mod is loaded: a hit on them or by them is named by it from then on.
pub(super) fn note_player(entity: i32, uuid: u128) {
    if horizon_client::mod_api::mods().is_empty() {
        return;
    }
    PLAYERS.with(|players| {
        let mut players = players.borrow_mut();
        if players.len() >= MAX_NOTED && !players.contains_key(&entity) {
            players.clear();
        }
        players.insert(entity, uuid);
    });
}

/// The UUID of the player with entity id `entity`, if one was noted.
fn player_uuid(entity: i32) -> Option<u128> {
    PLAYERS.with(|players| players.borrow().get(&entity).copied())
}

/// The victim's UUID (a player's), the hit's attacker (the causing entity), its UUID and the
/// direct entity, each none when not known.
pub(super) struct Parties {
    pub victim_uuid: Option<u128>,
    pub attacker: Option<u32>,
    /// A player attacker's UUID.
    pub attacker_uuid: Option<u128>,
    pub direct: Option<u32>,
}

/// Who `hurting` names. The causing entity is the source's, or a mob's attacker; a player
/// attacker (`Who::Player`) and a source that names none fall back to the synchronous attack in
/// progress (`health::knockback_attacker`), as `native_living_knockback_v2` does. An attacker
/// that is the victim itself (thorns inside the victim's own attack) is none.
pub(super) fn parties(hurting: &Hurting<'_>) -> Parties {
    let scope = knockback_attacker();
    let (victim, victim_uuid, causing, direct) = match hurting {
        Hurting::Player(entity, source) => {
            (*entity, player_uuid(*entity), source.causing, source.direct)
        }
        Hurting::Mob(mob, damage) => {
            let causing = match damage.attacker {
                Some(Who::Mob(id)) => id as i32,
                Some(Who::Player) => scope,
                None => 0,
            };
            let direct = if super::creatures::melee(damage.kind) {
                causing
            } else {
                0
            };
            // A mob's UUID is the game's (`persist.uuids`), out of reach here.
            (mob.id as i32, None, causing, direct)
        }
        Hurting::Dragon(entity, source) | Hurting::ArmorStand(entity, source) => {
            (*entity, None, source.causing, source.direct)
        }
    };
    let causing = if causing != 0 { causing } else { scope };
    let known = |id: i32| (id > 0 && id != victim).then_some(id as u32);
    let attacker = known(causing);
    Parties {
        victim_uuid,
        attacker,
        attacker_uuid: attacker.and_then(|a| player_uuid(a as i32)),
        direct: known(direct),
    }
}
