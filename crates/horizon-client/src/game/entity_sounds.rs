//! The entity overload of level sounds, shared by mob and player call sites.
use super::Game;
use horizon_client::sound_engine::{SoundEntity, SoundEntityState};
use horizon_world::nbt::Tag;

fn saved_silent(extra: Option<&Tag>) -> bool {
    horizon_client::save::number(extra.and_then(|tag| tag.get("Silent"))).unwrap_or(0) != 0
}

impl Game<'_> {
    /// `Level.playSound(except, mob, ...)`: the category and actor exclusion
    /// belong to the call site, not to the event's name.
    pub(super) fn play_mob_bound_sound(
        &mut self,
        index: usize,
        event: &str,
        source: usize,
        volume: f32,
        pitch: f32,
        except_actor: bool,
    ) {
        let mob = &self.mobs[index];
        let entity = horizon_client::sound_engine::SoundEntity::new(
            mob.id,
            [mob.position.x, mob.position.y, mob.position.z],
            mob.silent,
        );
        let except = except_actor.then(|| self.native_acting_entity());
        self.audio
            .play_bound(event, source, volume, pitch, entity, except);
    }

    fn sound_player_id(&self) -> u32 {
        self.remote
            .as_ref()
            .map_or_else(|| self.native_acting_entity(), |r| r.entity_id as u32)
    }

    /// The player in place can be a guest while `as_guest` runs. Its entity
    /// id and saved metadata travel with that action, even while the hub is out.
    pub(super) fn play_player_bound_sound(
        &mut self,
        event: &str,
        source: usize,
        volume: f32,
        pitch: f32,
        except_actor: bool,
    ) {
        let id = self.sound_player_id();
        let Some(state) = self.sound_entity_state(id) else {
            return;
        };
        let entity = SoundEntity::new(id, state.position, state.silent);
        entity.update(state);
        self.audio.play_bound(
            event,
            source,
            volume,
            pitch,
            entity,
            except_actor.then_some(id),
        );
    }

    pub(super) fn sound_entity_state(&mut self, id: u32) -> Option<SoundEntityState> {
        if self.remote.is_some() {
            return self.remote_sound_entity_state(id as i32);
        }
        if id == self.sound_player_id()
            && let Some(player) = self.player.as_ref()
        {
            return Some(SoundEntityState {
                position: [player.position.x, player.position.y, player.position.z],
                silent: saved_silent(self.level.player_extra.as_ref()),
                removed: false,
            });
        }
        if let Some(guest) = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|g| g.entity_id as u32 == id && g.placed && g.dimension == self.dim.id())
        }) {
            return Some(SoundEntityState {
                position: guest.position,
                silent: saved_silent(guest.saved.as_ref()),
                removed: false,
            });
        }
        if let Some(t) = self.tridents.iter().find(|t| t.id == id) {
            return Some(SoundEntityState {
                position: [t.position.x, t.position.y, t.position.z],
                silent: false,
                removed: false,
            });
        }
        self.mobs
            .iter()
            .find(|m| m.id == id)
            .map(|m| SoundEntityState {
                position: [m.position.x, m.position.y, m.position.z],
                silent: m.silent,
                removed: m.removed,
            })
    }
}
