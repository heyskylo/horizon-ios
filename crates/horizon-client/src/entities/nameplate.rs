//! The nameplate attributes carried by armor stands and mobs.
use super::ArmorStand;
use crate::items::ItemStack;
use crate::mobs::Mob;

/// The two synced attributes that control name tags and below-name lines.
/// `None` means the server has not sent a sync for that attribute yet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NameplateDistances {
    /// `minecraft:name_tag_distance` from the server's attribute updates.
    pub name_tag: Option<f64>,
    /// `minecraft:below_name_distance` from the server's attribute updates.
    pub below_name: Option<f64>,
}

impl NameplateDistances {
    /// `LivingEntity.getAttributeValue(Attributes.NAME_TAG_DISTANCE)`: the
    /// default is 64 and vanilla's maximum is 512.
    pub fn name_tag_distance(&self) -> f64 {
        name_tag_distance_of(self.name_tag)
    }

    /// `LivingEntity.getAttributeValue(Attributes.BELOW_NAME_DISTANCE)`: the
    /// default is 10 and vanilla's maximum is 512.
    pub fn below_name_distance(&self) -> f64 {
        below_name_distance_of(self.below_name)
    }
}

impl Mob {
    /// Stores both distances, from a model's own live attributes.
    pub fn set_nameplate(&mut self, distances: NameplateDistances) {
        let attributes = self.extra.attributes.get_or_insert_with(Default::default);
        attributes.name_tag_distance = distances.name_tag;
        attributes.below_name_distance = distances.below_name;
    }

    /// Stores `NAME_TAG_DISTANCE` from a server attribute update.
    pub fn set_name_tag_distance(&mut self, value: f64) {
        self.extra
            .attributes
            .get_or_insert_with(Default::default)
            .name_tag_distance = Some(value);
    }

    /// Stores `BELOW_NAME_DISTANCE` from a server attribute update.
    pub fn set_below_name_distance(&mut self, value: f64) {
        self.extra
            .attributes
            .get_or_insert_with(Default::default)
            .below_name_distance = Some(value);
    }

    /// `LivingEntity.getAttributeValue(Attributes.NAME_TAG_DISTANCE)`.
    pub fn name_tag_distance(&self) -> f64 {
        name_tag_distance_of(
            self.extra
                .attributes
                .as_ref()
                .and_then(|attributes| attributes.name_tag_distance),
        )
    }

    /// `LivingEntity.getAttributeValue(Attributes.BELOW_NAME_DISTANCE)`.
    pub fn below_name_distance(&self) -> f64 {
        below_name_distance_of(
            self.extra
                .attributes
                .as_ref()
                .and_then(|attributes| attributes.below_name_distance),
        )
    }
}

impl ArmorStand {
    /// `NameTagItem.interactLivingEntity`: a custom name handles the use;
    /// only a living target changes. Stack consumption belongs to the caller.
    pub fn name_with_tag(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:name_tag" {
            return false;
        }
        let Some(name) = stack.data().and_then(|data| data.custom_name.as_ref()) else {
            return false;
        };
        if self.health > 0.0 {
            self.custom_name = Some(crate::component::name_text(name));
        }
        true
    }

    /// `LivingEntity.getAttributeValue(Attributes.NAME_TAG_DISTANCE)`.
    pub fn name_tag_distance(&self) -> f64 {
        self.nameplate.name_tag_distance()
    }

    /// `LivingEntity.getAttributeValue(Attributes.BELOW_NAME_DISTANCE)`.
    pub fn below_name_distance(&self) -> f64 {
        self.nameplate.below_name_distance()
    }
}

/// The vanilla default (64) and maximum (512) for `NAME_TAG_DISTANCE`.
pub fn name_tag_distance_of(value: Option<f64>) -> f64 {
    value
        .filter(|v| v.is_finite())
        .unwrap_or(64.0)
        .clamp(0.0, 512.0)
}

/// The vanilla default (10) and maximum (512) for `BELOW_NAME_DISTANCE`.
pub fn below_name_distance_of(value: Option<f64>) -> f64 {
    value
        .filter(|v| v.is_finite())
        .unwrap_or(10.0)
        .clamp(0.0, 512.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;

    #[test]
    fn nameplate_distances_use_vanilla_defaults_and_clamp() {
        assert_eq!(name_tag_distance_of(None), 64.0);
        assert_eq!(name_tag_distance_of(Some(5.0)), 5.0);
        assert_eq!(name_tag_distance_of(Some(600.0)), 512.0);
        assert_eq!(name_tag_distance_of(Some(-4.0)), 0.0);
        assert_eq!(below_name_distance_of(None), 10.0);
        assert_eq!(below_name_distance_of(Some(2.0)), 2.0);
        assert_eq!(below_name_distance_of(Some(-1.0)), 0.0);
    }

    #[test]
    fn armor_stand_nameplate_values_override_defaults_and_clamp() {
        let mut stand = ArmorStand::new(1, Vec3::new(0.5, 64.0, 0.5), 0.0);
        assert_eq!(stand.name_tag_distance(), 64.0);
        assert_eq!(stand.below_name_distance(), 10.0);
        stand.nameplate = NameplateDistances {
            name_tag: Some(5.0),
            below_name: Some(2.0),
        };
        assert_eq!(
            (stand.name_tag_distance(), stand.below_name_distance()),
            (5.0, 2.0)
        );
        stand.nameplate.name_tag = Some(600.0);
        assert_eq!(stand.name_tag_distance(), 512.0);
    }
}
