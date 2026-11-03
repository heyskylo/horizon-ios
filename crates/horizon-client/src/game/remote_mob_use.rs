//! A server's client right-clicking a mob: vanilla's client runs
//! `Player.interactOn` on its own level (`MultiPlayerGameMode.interact`) to
//! learn whether the click ends on the mob or goes on to the held item
//! (`Minecraft.startUseItem` then sends UseItem). The server does the real
//! interaction; this is what the client's copy of the mob answers.
use super::Game;
use horizon_client::{
    first_person::Hand,
    mobs::{Mob, Species, equine, llama},
};

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
        let baby = m.is_baby();
        // Mob.checkAndHandleImportantInteractions: a lead, a named name tag,
        // a spawn egg of the mob's own kind.
        if item == "minecraft:lead"
            || item == "minecraft:name_tag" && stack.is_some_and(|s| s.custom_name().is_some())
            || item.strip_suffix("_spawn_egg") == Some(m.kind.id.as_str())
        {
            return true;
        }
        // AgeableMob.mobInteract: a golden dandelion on a baby that can be age
        // locked is SUCCESS on a client too (the server sets the lock).
        if self.age_lock_takes(m, item) {
            return true;
        }
        // Animal.mobInteract: its food is CONSUME on a client.
        if m.kind.flags.animal && m.kind.food.iter().any(|f| f.as_str() == item) {
            return true;
        }
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
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
            Species::Sheep => item == "minecraft:shears" || item.ends_with("_dye"),
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
            Species::Piglin => {
                !baby
                    && self
                        .res
                        .items()
                        .item_tags
                        .get("minecraft:piglin_loved")
                        .is_some_and(|tag| tag.contains(item))
            }
            _ => false,
        }
    }
    /// `AbstractHorse.handleEating` (`Llama`'s and `Camel`'s too) on this
    /// client's copy: a baby fed something that ages it draws one happy
    /// villager particle. The server draws it on its own level only, so no
    /// packet brings it.
    pub(super) fn remote_feed_particles(&mut self, i: usize, hand: Hand) {
        let Some(m) = self.mobs.get(i).filter(|m| m.alive()) else {
            return;
        };
        let stack = match hand {
            Hand::Main => self.stack_in(self.selected),
            Hand::Off => self.offhand.as_ref(),
        };
        let item = stack.map_or("", |s| &*s.item);
        if !ages_on_food(m, item) {
            return;
        }
        let id = m.id;
        self.particles_around_mob(
            id,
            "happy_villager",
            horizon_client::particles::Burst::HORSE_EATING,
        );
    }
}

/// `AbstractHorse.handleEating` (and the llama and camel variants): this
/// mouthful is the one that ages the baby up, which is the same moment the
/// level draws one happy villager particle.
///
/// `handleEating` gates both on `isBaby() && !isAgeLocked()` — a locked baby
/// does not grow, and the particle goes with the growth.
pub(super) fn ages_on_food(m: &Mob, item: &str) -> bool {
    if !m.is_baby() || m.extra.age_locked || item.is_empty() {
        return false;
    }
    match m.species() {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn mob(kind: &str) -> Option<Mob> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-types.json").exists() {
            return None;
        }
        let data = horizon_client::mobs::MobTypes::load(&root).ok()?;
        let kind = Arc::new(data.types.get(kind)?.clone());
        Some(Mob::new(
            1,
            kind,
            horizon_core::geometry::Vec3::new(0.0, 4.0, 0.0),
            0.0,
        ))
    }

    /// `AbstractHorse.handleEating` draws the particle only while the baby can
    /// still grow: `isBaby() && !isAgeLocked()` (#8590).
    #[test]
    fn a_locked_baby_draws_no_happy_villager() {
        let Some(mut horse) = mob("minecraft:horse") else {
            return;
        };
        horse.extra.age = -24000;
        assert!(ages_on_food(&horse, "minecraft:wheat"));
        horse.extra.age_locked = true;
        assert!(!ages_on_food(&horse, "minecraft:wheat"), "locked");
        horse.extra.age_locked = false;
        horse.extra.age = 0;
        assert!(!ages_on_food(&horse, "minecraft:wheat"), "adult");
        horse.extra.age = -24000;
        assert!(!ages_on_food(&horse, "minecraft:red_mushroom"), "no food");

        if let Some(mut llama) = mob("minecraft:llama") {
            llama.extra.age = -24000;
            assert!(ages_on_food(&llama, "minecraft:hay_block"));
            llama.extra.age_locked = true;
            assert!(!ages_on_food(&llama, "minecraft:hay_block"), "locked llama");
        }
        if let Some(mut camel) = mob("minecraft:camel")
            && camel.kind.food.iter().any(|f| f == "minecraft:cactus")
        {
            camel.extra.age = -24000;
            assert!(ages_on_food(&camel, "minecraft:cactus"));
            camel.extra.age_locked = true;
            assert!(!ages_on_food(&camel, "minecraft:cactus"), "locked camel");
        }
    }
}
