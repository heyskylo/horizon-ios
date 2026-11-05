//! Keep captured level sounds between an emitter's pairing and removal.
use super::{containers::captured_sound_range, entity::EntityState, guest::Guest, hub::Hub};
use crate::{audio::CapturedSound, fast_hash::FxHashSet};
use horizon_protocol::{
    buf::Reader,
    ids::play::clientbound as ids,
    packets::play::{self, Clientbound as C},
};

#[derive(Default)]
pub(super) struct SoundOrder {
    protected: FxHashSet<i32>,
    removed: Vec<(u32, Vec<i32>)>,
}

impl Hub {
    /// Flush the ordered prefix whose emitters need no new pairing. A sound
    /// waiting for AddEntity holds the rest of the queue in order; removals of
    /// those later emitters wait until the remaining sounds have gone out.
    pub fn prepare_captured_sounds(
        &self,
        sounds: &mut Vec<CapturedSound>,
        entities: &[EntityState],
    ) {
        let present: FxHashSet<i32> = if sounds.iter().any(|sound| sound.entity.is_some()) {
            entities
                .iter()
                .map(|e| e.id)
                .chain(
                    self.guests
                        .iter()
                        .filter(|g| self.here(g))
                        .map(|g| g.entity_id),
                )
                .collect()
        } else {
            FxHashSet::default()
        };
        let early = sounds
            .iter()
            .position(|sound| {
                sound.entity.is_some_and(|id| {
                    present.contains(&(id as i32))
                        && self.guests.iter().any(|g| {
                            self.here(g)
                                && g.entity_id as u32 != id
                                && sound.except_entity != Some(g.entity_id as u32)
                                && !g.tracker.is_tracking(id as i32)
                                && super::containers::within_range(
                                    sound.position,
                                    g.position,
                                    captured_sound_range(sound),
                                )
                        })
                })
            })
            .unwrap_or(sounds.len());
        {
            let mut order = self.sound_order.lock().unwrap();
            debug_assert!(
                order.removed.is_empty(),
                "finish the preceding sound batch first"
            );
            order.protected = sounds[early..]
                .iter()
                .filter_map(|s| s.entity.map(|id| id as i32))
                .collect();
        }
        for sound in sounds.drain(..early) {
            self.captured_sound(&sound, None);
        }
    }

    /// Delay only protected ids, keeping every other removal at its usual point.
    pub(super) fn send_ordered_removal(&self, guest: &Guest, ids: &[i32]) {
        let mut now = Vec::new();
        let mut held = Vec::new();
        {
            let mut order = self.sound_order.lock().unwrap();
            for &id in ids {
                if order.protected.contains(&id) {
                    held.push(id);
                } else {
                    now.push(id);
                }
            }
            if !held.is_empty() {
                order.removed.push((guest.conn, held));
            }
        }
        if !now.is_empty() {
            guest.send(&C::RemoveEntities(play::RemoveEntities { ids: now }));
        }
    }

    /// Entity tracking already builds packet bodies. Split its removal batch
    /// without decoding or changing any of its pairing/movement packets.
    pub(super) fn hold_sound_removals(&self, conn: u32, packets: &mut Vec<Vec<u8>>) {
        let mut order = self.sound_order.lock().unwrap();
        if order.protected.is_empty() {
            return;
        }
        packets.retain_mut(|body| {
            let mut reader = Reader::new(body);
            if reader.varint().unwrap() != ids::REMOVE_ENTITIES {
                return true;
            }
            let C::RemoveEntities(mut packet) =
                C::decode(ids::REMOVE_ENTITIES, reader.rest()).unwrap()
            else {
                unreachable!()
            };
            let original = packet.ids.len();
            let mut held = Vec::new();
            packet.ids.retain(|id| {
                if order.protected.contains(id) {
                    held.push(*id);
                    false
                } else {
                    true
                }
            });
            if held.is_empty() {
                return true;
            }
            order.removed.push((conn, held));
            if packet.ids.is_empty() {
                return false;
            }
            debug_assert!(packet.ids.len() < original);
            *body = C::RemoveEntities(packet).encode();
            true
        });
    }

    /// Called after the rest of the captured sounds, before the level changes.
    pub fn finish_captured_sounds(&self) {
        let order = std::mem::take(&mut *self.sound_order.lock().unwrap());
        for (conn, ids) in order.removed {
            if let Some(guest) = self.guest(conn) {
                guest.send(&C::RemoveEntities(play::RemoveEntities { ids }));
            }
        }
    }
}
