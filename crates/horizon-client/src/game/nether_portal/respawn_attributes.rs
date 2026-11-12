//! What a client's new player takes from the old one when a `ClientboundRespawnPacket`
//! replaces it (`ClientPacketListener.handleRespawn`): `AttributeMap.assignAllValues` when the
//! packet keeps the attribute modifiers (a level change, through a portal or not: the worn
//! armour's among them), `assignBaseValues` after a death. The new player is made once its
//! position arrives, so what the server sends for it in between waits here, as vanilla's new
//! `LocalPlayer` already holds it.
use super::super::{Game, Result};
use horizon_client::{lan::attributes::apply, motion::PlayerMotion};
use horizon_protocol::packets::play::AttributeSnapshot;
use horizon_world::attributes::Instance;

/// The respawn in progress (`Game::portals`).
#[derive(Default)]
pub(in crate::game) struct RespawnAttributes {
    /// A respawn packet came: whether it keeps the modifiers.
    keep_modifiers: Option<bool>,
    carried: Option<Carried>,
}

struct Carried {
    keep_modifiers: bool,
    instances: Vec<(String, Instance)>,
    /// `UpdateAttributes` of the player's own entity that came before its new player.
    early: Vec<Vec<AttributeSnapshot>>,
}

impl RespawnAttributes {
    /// `KEEP_ATTRIBUTE_MODIFIERS` of the respawn packet being handled.
    pub(in crate::game) fn respawn(&mut self, keep_modifiers: bool) {
        self.keep_modifiers = Some(keep_modifiers);
    }
}

impl Game<'_> {
    /// The old player goes (`handleRespawn`, `handleLogin`): after a respawn its attributes are
    /// kept for the next one; a login starts from nothing.
    pub(in crate::game) fn drop_remote_player(&mut self) {
        let respawn = &mut self.portals.respawn_attributes;
        match (respawn.keep_modifiers.take(), self.player.as_ref()) {
            (Some(keep_modifiers), Some(old)) => {
                respawn.carried = Some(Carried {
                    keep_modifiers,
                    instances: old
                        .attributes
                        .instances()
                        .map(|(_, key, instance)| (key.name().to_owned(), instance.clone()))
                        .collect(),
                    early: Vec::new(),
                });
            }
            // A second respawn before the position: the first one's attributes are still the
            // ones to carry.
            (Some(_), None) => {}
            (None, _) => respawn.carried = None,
        }
        self.player = None;
    }

    /// `ClientPacketListener.handleUpdateAttributes` of the player's own entity.
    pub(in crate::game) fn apply_own_attributes(&mut self, values: Vec<AttributeSnapshot>) {
        if let Some(player) = self.player.as_mut() {
            apply(&mut player.attributes, &values);
        } else if let Some(carried) = self.portals.respawn_attributes.carried.as_mut() {
            carried.early.push(values);
        }
    }

    /// The new player of a respawn takes the old one's attributes, then what the server sent.
    pub(in crate::game) fn take_carried_attributes(
        &mut self,
        player: &mut PlayerMotion,
    ) -> Result<()> {
        let Some(carried) = self.portals.respawn_attributes.carried.take() else {
            return Ok(());
        };
        for (name, old) in &carried.instances {
            let Some(key) = player.attributes.key(name) else {
                continue;
            };
            let Some(handle) = player
                .attributes
                .get_instance(Some(key))
                .map_err(|e| format!("movement attribute: {e:?}"))?
            else {
                continue;
            };
            player
                .attributes
                .edit(&handle, |value, observer| {
                    if carried.keep_modifiers {
                        // `AttributeInstance.replaceFrom`: the base and every modifier.
                        value.replace_from(old, observer)
                    } else {
                        value.set_base_value(old.base_value(), observer)
                    }
                })
                .map_err(|e| format!("movement attribute: {e:?}"))?;
        }
        for values in &carried.early {
            apply(&mut player.attributes, values);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
