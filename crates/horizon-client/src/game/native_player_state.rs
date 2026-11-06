//! A player's state for server mods (docs/native-mods.md, "Server hooks for
//! plugin ports: player state"): what a mod that keeps players in step
//! across servers (a HuskSync port) reads from a player and writes back.
//! Only the server's own players (the hub's guests, as
//! `native_server_players` lists them). Inside a player's turn (their hook,
//! their command) only that player is reached, outside one (`native_tick`,
//! `native_server_tick`, `native_menu_clicked`) everyone on.
use super::{Game, commands::Target};
use horizon_client::{entities::Experience, lan::playerdata};
use horizon_world::nbt::Tag;

/// Where a player's state is while a helper reads it.
#[derive(Clone, Copy)]
enum Place {
    /// Swapped in: the acting player of the turn.
    Acting,
    /// Their record in the hub, at this index.
    Guest(usize),
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The player `uuid` as a helper finds them: the acting player inside a
    /// turn (nobody else then), a placed guest outside one.
    fn native_place(&self, uuid: u128) -> Option<Place> {
        if self.lan_acting.is_some() {
            return (self.lan_acting_uuid == Some(uuid) && self.player.is_some())
                .then_some(Place::Acting);
        }
        let host = self.lan.as_ref()?;
        host.hub
            .guests
            .iter()
            .position(|g| g.uuid == uuid && g.placed && !g.released)
            .map(Place::Guest)
    }

    /// The player `uuid`'s compound as the server would save it now
    /// (`players/data/<uuid>.dat`, vanilla's `ServerPlayer.saveWithoutId`):
    /// inventory, health, food, experience, effects, attributes, position,
    /// and every key Horizon keeps without modelling it. What a mod stores to
    /// give the player back elsewhere. None inside a player's turn (their
    /// live state is swapped out of their record: call from
    /// `native_server_tick`), and for a player not on. API
    /// @API:player_compound@.
    pub(super) fn native_player_compound(&self, uuid: u128) -> Option<Tag> {
        match self.native_place(uuid)? {
            Place::Acting => None,
            Place::Guest(i) => Some(playerdata::player_tag(&self.lan.as_ref()?.hub.guests[i])),
        }
    }

    /// Runs `f` as the player `uuid`, in their level, as `/effect` reaches a
    /// player (`for_player`): None, and nothing run, for a player not
    /// reached (`native_place`) or dead.
    fn native_player_turn<T>(&mut self, uuid: u128, f: impl FnOnce(&mut Self) -> T) -> Option<T> {
        match self.native_place(uuid)? {
            Place::Acting => {
                if self.player.as_ref()?.health.dead_or_dying() {
                    return None;
                }
                Some(f(self))
            }
            Place::Guest(i) => {
                let g = &self.lan.as_ref()?.hub.guests[i];
                if g.dead {
                    return None;
                }
                let conn = g.conn;
                self.for_player(Target::Guest(conn), f)
            }
        }
    }

    /// The player `uuid`'s food level (0 to 20), saturation and exhaustion,
    /// as vanilla's `FoodData` holds them. None for a player not reached.
    /// API @API:player_status@.
    pub(super) fn native_player_food(&self, uuid: u128) -> Option<(i32, f32, f32)> {
        match self.native_place(uuid)? {
            Place::Acting => {
                let food = &self.player.as_ref()?.health.food;
                Some((food.food, food.saturation, food.exhaustion))
            }
            Place::Guest(i) => {
                let g = &self.lan.as_ref()?.hub.guests[i];
                let exhaustion = match (&g.motion, &g.loaded_body) {
                    (Some(m), _) => m.health.food.exhaustion,
                    (None, Some(body)) => body.food_exhaustion,
                    (None, None) => 0.0,
                };
                Some((g.food, g.saturation, exhaustion))
            }
        }
    }

    /// Sets what is given of the player `uuid`'s health, food (level 0 to 20,
    /// saturation, exhaustion) and experience (level, progress 0 to 1 toward
    /// the next, total points), as Bukkit's `setHealth`, `setFoodLevel`,
    /// `setSaturation`, `setExhaustion`, `setLevel`, `setExp` and
    /// `setTotalExperience`; None leaves it. Health is capped at their
    /// maximum; 0 or less kills them as `/kill` does, the death message
    /// "<player> died" (Bukkit's `setHealth(0)`). Food and experience come
    /// before health. Their client gets `SetHealth` and `SetExperience` with
    /// the tick's sync. The mod's food is not heard as a change of the
    /// player's (`native_food_level_changed`), except inside the player's
    /// own turn, where it counts with what the turn did. False, and nothing
    /// set, for a player not reached, a dead one, a value that is not a
    /// number, or a progress outside 0 to 1 or a negative level or total.
    /// API @API:player_status@.
    pub(super) fn native_set_player_status(
        &mut self,
        uuid: u128,
        health: Option<f32>,
        food: Option<(i32, f32, f32)>,
        xp: Option<(i32, f32, i32)>,
    ) -> bool {
        let bad_food = food.is_some_and(|(_, s, e)| !s.is_finite() || !e.is_finite());
        let bad_xp = xp.is_some_and(|(l, p, t)| l < 0 || t < 0 || !(0.0..=1.0).contains(&p));
        if health.is_some_and(|h| !h.is_finite()) || bad_food || bad_xp {
            return false;
        }
        let food = food.map(|(level, saturation, exhaustion)| {
            // FoodData.addExhaustion's cap.
            (
                level.clamp(0, 20),
                saturation.max(0.0),
                exhaustion.clamp(0.0, 40.0),
            )
        });
        self.native_player_turn(uuid, |game| {
            if let (Some((level, saturation, exhaustion)), Some(p)) = (food, game.player.as_mut()) {
                p.health.food.food = level;
                p.health.food.saturation = saturation;
                p.health.food.exhaustion = exhaustion;
                // The record too, so the turn's end doesn't take it for the
                // player's own eating or hunger.
                if let (Some(conn), Some(host)) = (game.lan_acting, game.lan.as_mut())
                    && let Some(g) = host.hub.guest_mut(conn)
                {
                    g.food = level;
                    g.saturation = saturation;
                }
            }
            if let Some((level, progress, total)) = xp {
                game.experience = Experience {
                    level,
                    progress,
                    total,
                };
            }
            if let (Some(health), Some(p)) = (health, game.player.as_mut()) {
                if health > 0.0 {
                    let _ = p.set_health(health, false);
                    let _ = p.clamp_health_to_max();
                } else {
                    // LivingEntity.die(damageSources().generic()).
                    let _ = p.set_health(0.0, false);
                    game.ui.death_cause = Some("death.attack.generic".into());
                }
            }
        })
        .is_some()
    }
}
