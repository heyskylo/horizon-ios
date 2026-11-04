//! A server's client right-clicking a mob: vanilla's client runs
//! `Player.interactOn` on its own level (`MultiPlayerGameMode.interact`) to
//! learn whether the click ends on the mob or goes on to the held item
//! (`Minecraft.startUseItem` then sends UseItem). The server does the real
//! interaction; this is what the client's copy of the mob answers.
use super::Game;
use horizon_client::{
    first_person::Hand,
    mobs::{Species, equine, llama, render::DYE_NAMES},
};

/// The dye color index (0..16) from a dye item identifier, if it is a dye item.
fn dye_color(item: &str) -> Option<u8> {
    let name = item
        .strip_prefix("minecraft:")
        .unwrap_or(item)
        .strip_suffix("_dye")?;
    DYE_NAMES.iter().position(|&d| d == name).map(|c| c as u8)
}

/// Whether mob `m` takes a right click with `item` on a client.
pub(super) fn remote_mob_takes_item(
    m: &horizon_client::mobs::Mob,
    item: &str,
    named_tag: bool,
    age_lock_takes: bool,
    sneaking: bool,
    piglin_loved: bool,
) -> bool {
    let baby = m.is_baby();
    // Mob.checkAndHandleImportantInteractions: a lead, a named name tag,
    // a spawn egg of the mob's own kind.
    if item == "minecraft:lead"
        || named_tag
        || item.strip_suffix("_spawn_egg") == Some(m.kind.id.as_str())
    {
        return true;
    }
    // AgeableMob.mobInteract: a golden dandelion on a baby that can be age
    // locked is SUCCESS on a client too (the server sets the lock).
    if age_lock_takes {
        return true;
    }
    // Animal.mobInteract: its food is SUCCESS on a client for a baby that can
    // age up and CONSUME otherwise, whether or not the server's animal can
    // breed (the client doesn't know an adult's breeding cooldown).
    if m.kind.flags.animal && m.kind.food.iter().any(|f| f.as_str() == item) {
        return true;
    }
    match m.species() {
        // Villager / WanderingTrader.mobInteract: trading, or a baby's head shake.
        Species::Villager | Species::WanderingTrader => item != "minecraft:villager_spawn_egg",
        // AbstractHorse, Camel: ridden (or their inventory opened); a baby only eats.
        s if equine::is_equine(s)
            || llama::is_llama(s)
            || matches!(s, Species::Camel | Species::CamelHusk) =>
        {
            !baby
        }
        // Pig / Strider.mobInteract: a saddled one is ridden.
        Species::Pig | Species::Strider => m.extra.saddled && !sneaking,
        // HappyGhast.mobInteract: a harnessed adult is ridden.
        Species::HappyGhast => !baby && m.extra.body.is_some(),
        // TamableAnimal (wolf, cat, parrot) on a client: CONSUME once tame.
        Species::Wolf | Species::Cat | Species::Parrot => m.extra.tame,
        // Allay.mobInteract: gives an item or takes it back.
        Species::Allay => !item.is_empty() || m.extra.main_hand.is_some(),
        Species::Cow | Species::Goat => item == "minecraft:bucket" && !baby,
        Species::Mooshroom => {
            !baby
                && (matches!(
                    item,
                    "minecraft:bucket" | "minecraft:bowl" | "minecraft:shears"
                ) || m.extra.variant == 1
                    && horizon_client::flowers::stew_effect(item).is_some())
        }
        // Sheep.mobInteract: shears are CONSUME on a client, sheared or not.
        // DyeItem.interactLivingEntity: a dye takes an unsheared sheep of
        // another colour, and passes otherwise.
        Species::Sheep => {
            item == "minecraft:shears"
                || !m.extra.sheared && dye_color(item).is_some_and(|c| c != m.extra.color)
        }
        // SnowGolem.mobInteract: shears take the pumpkin off.
        Species::SnowGolem => item == "minecraft:shears" && !m.extra.sheared,
        // Bucketable.bucketMobPickup.
        Species::Cod
        | Species::Salmon
        | Species::TropicalFish
        | Species::Pufferfish
        | Species::Axolotl => item == "minecraft:water_bucket",
        // Tadpole.mobInteract: frog food (not while age locked), else the bucket.
        Species::Tadpole => {
            item == "minecraft:water_bucket"
                || !m.extra.age_locked && m.kind.food.iter().any(|f| f.as_str() == item)
        }
        Species::Armadillo => item == "minecraft:brush",
        // ZombieVillager.mobInteract: CONSUME for a golden apple.
        Species::ZombieVillager => item == "minecraft:golden_apple",
        // Creeper.mobInteract: CREEPER_IGNITERS.
        Species::Creeper => {
            matches!(item, "minecraft:flint_and_steel" | "minecraft:fire_charge")
        }
        // PiglinAi.mobInteract: an adult admires what it loves.
        Species::Piglin => !baby && piglin_loved,
        _ => false,
    }
}

impl Game<'_> {
    /// Whether mob `i` takes a right click with `hand`'s item on this
    /// client (`Mob.interact`'s result consumes the action).
    pub(super) fn remote_mob_takes(&self, i: usize, hand: Hand) -> bool {
        let Some(m) = self.mobs.get(i).filter(|m| m.alive()) else {
            return false;
        };
        let stack = match hand {
            Hand::Main => self.stack_in(self.selected),
            Hand::Off => self.offhand.as_ref(),
        };
        let item = stack.map_or("", |s| &*s.item);
        let named_tag =
            item == "minecraft:name_tag" && stack.is_some_and(|s| s.custom_name().is_some());
        let age_lock_takes = self.age_lock_takes(m, item);
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        let piglin_loved = !m.is_baby()
            && self
                .res
                .items()
                .item_tags
                .get("minecraft:piglin_loved")
                .is_some_and(|tag| tag.contains(item));
        remote_mob_takes_item(m, item, named_tag, age_lock_takes, sneaking, piglin_loved)
    }
    /// `AbstractHorse.handleEating` (`Llama`'s and `Camel`'s too) on this
    /// client's copy: a baby fed something that ages it draws one happy
    /// villager particle. The server draws it on its own level only, so no
    /// packet brings it.
    pub(super) fn remote_feed_particles(&mut self, i: usize, hand: Hand) {
        let Some(m) = self.mobs.get(i).filter(|m| m.alive() && m.is_baby()) else {
            return;
        };
        let stack = match hand {
            Hand::Main => self.stack_in(self.selected),
            Hand::Off => self.offhand.as_ref(),
        };
        let item = stack.map_or("", |s| &*s.item);
        let ages = match m.species() {
            // Llama.handleEating: wheat 10, hay 90.
            s if llama::is_llama(s) => matches!(item, "minecraft:wheat" | "minecraft:hay_block"),
            // Camel.handleEating: its food ages it by 10.
            Species::Camel => m.kind.food.iter().any(|f| f.as_str() == item),
            // AbstractHorse.handleEating: every food but the red mushroom.
            Species::Horse | Species::Donkey | Species::Mule => matches!(
                item,
                "minecraft:wheat"
                    | "minecraft:sugar"
                    | "minecraft:hay_block"
                    | "minecraft:apple"
                    | "minecraft:carrot"
                    | "minecraft:golden_carrot"
                    | "minecraft:golden_apple"
                    | "minecraft:enchanted_golden_apple"
            ),
            _ => false,
        };
        if ages {
            let id = m.id;
            self.particles_around_mob(
                id,
                "happy_villager",
                horizon_client::particles::Burst::HORSE_EATING,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::mobs::{Anim, Brain, Flags, Mob, MobType, Part};
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    fn test_mob(species: Species, id: &str, is_animal: bool, food: &[&str]) -> Mob {
        let kind = Arc::new(MobType {
            id: id.into(),
            species,
            width: 0.9,
            height: 1.4,
            eye_height: 1.3,
            category: "creature".into(),
            max_health: 10.0,
            speed: 0.2,
            flying_speed: 0.4,
            attack_damage: 0.0,
            attack_knockback: 0.0,
            follow_range: 16.0,
            step_height: 0.6,
            armor: 0.0,
            armor_toughness: 0.0,
            knockback_resistance: 0.0,
            jump_strength: 0.42,
            attributes: Arc::default(),
            gravity: 0.08,
            safe_fall_distance: 3.0,
            fall_damage_multiplier: 1.0,
            water_efficiency: 0.0,
            water_slowdown: 0.8,
            air_speed: 0.02,
            baby_scale: 0.5,
            head_rot_speed: 10.0,
            max_head_x_rot: 40.0,
            max_head_y_rot: 75.0,
            ambient_interval: 120,
            flags: Flags {
                animal: is_animal,
                ..Default::default()
            },
            model: Arc::new(Part {
                name: "root".into(),
                pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                polygons: Vec::new(),
                children: Vec::new(),
            }),
            layers: Default::default(),
            texture: String::new(),
            brain: Brain::Passive,
            anim: Anim::Rabbit,
            sounds: [""; 3],
            step_sound: None,
            food: food.iter().map(|f| (*f).to_string()).collect(),
            tempt: Arc::default(),
        });
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    fn takes(m: &Mob, item: &str) -> bool {
        remote_mob_takes_item(m, item, false, false, false, false)
    }

    /// `Sheep.mobInteract` answers CONSUME for shears on a client whether or not the
    /// sheep is sheared: only the server checks `readyForShearing`.
    #[test]
    fn shears_take_a_sheep_sheared_or_not() {
        let mut sheep = test_mob(
            Species::Sheep,
            "minecraft:sheep",
            true,
            &["minecraft:wheat"],
        );
        for sheared in [false, true] {
            sheep.extra.sheared = sheared;
            assert!(takes(&sheep, "minecraft:shears"), "sheared {sheared}");
        }
    }

    #[test]
    fn same_color_sheep_does_not_take_matching_dye() {
        let mut sheep = test_mob(
            Species::Sheep,
            "minecraft:sheep",
            true,
            &["minecraft:wheat"],
        );
        sheep.extra.color = 0;
        assert!(!takes(&sheep, "minecraft:white_dye"));
        assert!(takes(&sheep, "minecraft:red_dye"));

        sheep.extra.color = 14;
        assert!(!takes(&sheep, "minecraft:red_dye"));
        assert!(takes(&sheep, "minecraft:white_dye"));

        // DyeItem.interactLivingEntity: a sheared sheep passes any dye.
        sheep.extra.sheared = true;
        assert!(!takes(&sheep, "minecraft:white_dye"));
    }

    /// `Animal.mobInteract` on a client: food is SUCCESS for a baby that can age up and
    /// CONSUME otherwise, so it takes the click in love, cooling down or age locked.
    #[test]
    fn an_animal_takes_its_food_on_a_client_whatever_its_age() {
        let mut cow = test_mob(Species::Cow, "minecraft:cow", true, &["minecraft:wheat"]);
        for (age, in_love, age_locked) in [
            (0, 0, false),
            (6000, 0, false),
            (0, 600, false),
            (-24000, 0, false),
            (-24000, 0, true),
        ] {
            cow.extra.age = age;
            cow.extra.in_love = in_love;
            cow.extra.age_locked = age_locked;
            assert!(
                takes(&cow, "minecraft:wheat"),
                "{age} {in_love} {age_locked}"
            );
        }
        assert!(!takes(&cow, "minecraft:stick"));
    }
}
