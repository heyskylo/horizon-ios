//! `ClientPacketListener.handleUpdateAttributes` for entity attributes that
//! affect the client-side nameplate.
use super::{Game, snapshot_value};
use horizon_client::entities::nameplate::NameplateDistances;
use horizon_protocol::packets::play;

impl Game<'_> {
    /// Computes one synced attribute using the catalog's vanilla range.
    fn remote_attribute_value(
        &self,
        name: &str,
        values: &[play::AttributeSnapshot],
    ) -> Option<f64> {
        let attribute = self.motion.attributes.id(name)?;
        let snapshot = values
            .iter()
            .find(|snapshot| snapshot.attribute == attribute.registry_id())?;
        Some(
            self.motion
                .attributes
                .attribute(attribute)
                .definition
                .sanitize(snapshot_value(snapshot)),
        )
    }

    /// The two attributes read by `LivingEntityRenderer` and
    /// `EntityRenderer.extractNameTags`.
    pub(super) fn remote_nameplate_values(
        &self,
        values: &[play::AttributeSnapshot],
    ) -> NameplateDistances {
        NameplateDistances {
            name_tag: self.remote_attribute_value("minecraft:name_tag_distance", values),
            below_name: self.remote_attribute_value("minecraft:below_name_distance", values),
        }
    }

    /// `ClientPacketListener.handleUpdateAttributes` for a mob. The same
    /// packet can target a player, mob or armor stand.
    pub(super) fn remote_mob_attributes(&mut self, id: i32, values: &[play::AttributeSnapshot]) {
        let max_health = self.remote_attribute_value("minecraft:max_health", values);
        let scale = self.remote_attribute_value("minecraft:scale", values);
        let speed = self.remote_attribute_value("minecraft:movement_speed", values);
        let jump = self.remote_attribute_value("minecraft:jump_strength", values);
        let nameplate = self.remote_nameplate_values(values);
        let Some(m) = self.mob_index.get_mut(&mut self.mobs, id as u32) else {
            if let Some(a) = self.armor_stands.iter_mut().find(|a| a.id == id as u32) {
                if let Some(value) = nameplate.name_tag {
                    a.nameplate.name_tag = Some(value);
                }
                if let Some(value) = nameplate.below_name {
                    a.nameplate.below_name = Some(value);
                }
            }
            return;
        };
        // A horse its rider's client moves goes at its own speed and jumps
        // its own height (`getSpeed`, `getCustomJump`).
        if horizon_client::mobs::equine::moved_by_rider(m.species()) {
            if let Some(speed) = speed {
                horizon_client::mobs::finalize::set_base(
                    m,
                    horizon_client::mobs::finalize::Attr::MovementSpeed,
                    speed,
                );
            }
            if let Some(jump) = jump {
                m.extra.vector[0] = jump;
            }
        }
        if let Some(max_health) = max_health {
            horizon_client::mobs::finalize::set_base(
                m,
                horizon_client::mobs::finalize::Attr::MaxHealth,
                max_health,
            );
        }
        if let Some(scale) = scale {
            m.extra.model_scale = scale as f32;
        }
        if let Some(value) = nameplate.name_tag {
            m.set_name_tag_distance(value);
        }
        if let Some(value) = nameplate.below_name {
            m.set_below_name_distance(value);
        }
    }
}
