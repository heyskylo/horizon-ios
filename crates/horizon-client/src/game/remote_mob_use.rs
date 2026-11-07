//! A server's client right-clicking a mob: vanilla's client runs
//! `Player.interactOn` on its own level (`MultiPlayerGameMode.interact`) to
//! learn whether the click ends on the mob or goes on to the held item
//! (`Minecraft.startUseItem` then sends UseItem). The server does the real
//! interaction; this is what the client's copy of the mob answers.
use super::Game;
use horizon_client::actions::Action;
use horizon_client::{
    first_person::Hand,
    mobs::{Mob, Species, armadillo, equine, llama, nautilus},
};

impl Game<'_> {
    /// Whether mob `i` takes a right click with `hand`'s item on this
    /// client (`Mob.interact`'s result consumes the action).
    pub(super) fn remote_mob_takes(&mut self, i: usize, hand: Hand) -> bool {
        self.remote_mob_answers(i, hand, true)
    }

    /// The same answer, changing nothing: an allay's and a dyed sheep's answer is the
    /// interaction itself, and the controller's guide asks every frame.
    pub(super) fn remote_mob_would_take(&mut self, i: usize, hand: Hand) -> bool {
        self.remote_mob_answers(i, hand, false)
    }

    fn remote_mob_answers(&mut self, i: usize, hand: Hand, act: bool) -> bool {
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
        // Armadillo.mobInteract: a rolled-up armadillo fails before
        // AgeableMob/Animal.mobInteract, so food and age lock are not taken.
        // A brush still reaches brushOffScute below.
        if m.species() == Species::Armadillo && armadillo::client_rejects_food(m, item) {
            return false;
        }
        // AgeableMob.mobInteract: a golden dandelion on a baby that can be age
        // locked is SUCCESS on a client too (the server sets the lock).
        if self.age_lock_takes(m, item) {
            return true;
        }
        // Animal.mobInteract: its food is CONSUME on a client. A nautilus
        // overrides mobInteract, and an untamed one only takes its taming
        // items, so it answers in its own arm below.
        if m.kind.flags.animal
            && !matches!(m.species(), Species::Nautilus | Species::ZombieNautilus)
            && m.kind.food.iter().any(|f| f.as_str() == item)
        {
            return true;
        }
        let sneaking = self.keys.held(Action::Sneak);
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
            Species::Allay if !act => self.allay_would_take(i, hand, stack.cloned()),
            Species::Allay => self.predict_allay_interaction(i, hand, stack.cloned()),
            Species::Cow | Species::Goat => item == "minecraft:bucket" && !baby,
            Species::Mooshroom => {
                !baby
                    && (matches!(
                        item,
                        "minecraft:bucket" | "minecraft:bowl" | "minecraft:shears"
                    ) || m.extra.variant == 1
                        && horizon_client::flowers::stew_effect(item).is_some())
            }
            Species::Sheep if !act => {
                item == "minecraft:shears" || self.sheep_would_take_dye(i, hand)
            }
            Species::Sheep => item == "minecraft:shears" || self.interact_sheep_dye(i, hand, true),
            // SnowGolem.mobInteract: shears take the pumpkin off.
            Species::SnowGolem => item == "minecraft:shears" && !m.extra.sheared,
            // AbstractNautilus.mobInteract: an untamed adult only answers its
            // taming items, a tame one is fed, mounted or opens its screen.
            Species::Nautilus | Species::ZombieNautilus => {
                let taming: Vec<String> = self
                    .res
                    .items()
                    .item_tags
                    .get("minecraft:nautilus_taming_items")
                    .map(|t| t.iter().cloned().collect())
                    .unwrap_or_default();
                nautilus_takes(nautilus::is_food(m, item, &taming), m.extra.tame, baby)
            }
            // IronGolem.mobInteract.
            Species::IronGolem => iron_golem_takes(item, m.health, m.attrs.max_health),
            // CopperGolem.mobInteract.
            Species::CopperGolem => {
                copper_golem_takes(item, m.extra.main_hand.is_some(), self.golem_shearable(i))
            }
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
            // Armadillo.brushOffScute fails on a baby, so its brush falls
            // through to FAIL (scared) or Mob.mobInteract's PASS.
            Species::Armadillo => item == "minecraft:brush" && !baby,
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
    /// Whether that answer is `InteractionResult.SUCCESS`, the only result that
    /// swings the hand: `CONSUME` ends the click without one. An adult animal's
    /// food (`Animal.mobInteract`), a zombie villager's golden apple and a
    /// sheep's shears (the server alone checks `readyForShearing`) answer
    /// CONSUME on a client, and a horse's or panda's food SUCCESS_SERVER
    /// (#9437). A tame wolf, cat or parrot sitting down is SUCCESS.
    pub(super) fn remote_mob_swings(&self, i: usize, hand: Hand) -> bool {
        let Some(m) = self.mobs.get(i).filter(|m| m.alive()) else {
            return false;
        };
        let stack = match hand {
            Hand::Main => self.stack_in(self.selected),
            Hand::Off => self.offhand.as_ref(),
        };
        let item = stack.map_or("", |s| &*s.item);
        mob_swings(m, item)
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

/// `AbstractNautilus.mobInteract` on a client: its food is answered while the
/// nautilus takes it (taming items for an untamed adult, its food once tamed),
/// and a tame adult answers every click because that is the mount.
fn nautilus_takes(food: bool, tame: bool, baby: bool) -> bool {
    food || (tame && !baby)
}

/// `IronGolem.mobInteract`: an iron ingot heals 25 and only answers SUCCESS when
/// that changed the health, so a full golem leaves the click to the item.
fn iron_golem_takes(item: &str, health: f32, max_health: f32) -> bool {
    item == "minecraft:iron_ingot" && health < max_health
}

/// `CopperGolem.mobInteract` on a client: an empty hand makes the golem throw
/// what it holds, and shears answer only when `readyForShearing`. Honeycomb and
/// axes return `PASS` here because the waxing and scraping run on the server.
fn copper_golem_takes(item: &str, holding: bool, shearable: bool) -> bool {
    (item.is_empty() && holding) || (item == "minecraft:shears" && shearable)
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

/// [`Game::remote_mob_swings`] for mob `m` and the held `item`.
fn mob_swings(m: &Mob, item: &str) -> bool {
    let food = m.kind.food.iter().any(|f| f.as_str() == item);
    let hurt = m.health < m.attrs.max_health;
    match m.species() {
        // AbstractHorse.fedFood (Camel's, Llama's too): SUCCESS_SERVER,
        // which swings on the server's side only.
        s if food
            && (equine::is_equine(s)
                || llama::is_llama(s)
                || matches!(s, Species::Camel | Species::CamelHusk)) =>
        {
            return false;
        }
        // Panda.mobInteract: its bamboo answers SUCCESS_SERVER.
        Species::Panda if food => return false,
        // Cat.mobInteract: food tames an untamed cat, and heals a hurt
        // tame one: SUCCESS before Animal.mobInteract is reached.
        Species::Cat if food && (!m.extra.tame || hurt) => return true,
        // Wolf.mobInteract: food heals a hurt tame wolf (SUCCESS).
        Species::Wolf if food && m.extra.tame && hurt => return true,
        _ => {}
    }
    // Animal.mobInteract: a baby's food is SUCCESS, since it ages up; an
    // adult's is CONSUME, as only the server knows whether it can breed.
    if m.kind.flags.animal
        && !matches!(m.species(), Species::Nautilus | Species::ZombieNautilus)
        && food
    {
        return m.is_baby() && !m.extra.age_locked;
    }
    // A tame wolf, cat or parrot sits or stands up: SUCCESS
    // (TamableAnimal's subclasses), so it swings.
    let consumes = match m.species() {
        // ZombieVillager.mobInteract: a golden apple cures it on the server.
        Species::ZombieVillager => item == "minecraft:golden_apple",
        // Sheep.mobInteract: shears, sheared or not.
        Species::Sheep => item == "minecraft:shears",
        _ => false,
    };
    !consumes
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

    /// Tamables' and horses' own `mobInteract` answer before `Animal.mobInteract`'s
    /// CONSUME (#9450): sitting down and healing are SUCCESS, a horse's food
    /// SUCCESS_SERVER.
    #[test]
    fn only_a_client_success_swings_the_hand() {
        let Some(mut wolf) = mob("minecraft:wolf") else {
            return;
        };
        wolf.extra.tame = true;
        assert!(
            mob_swings(&wolf, ""),
            "a tame wolf sits: SUCCESS.withoutItem"
        );
        assert!(
            !mob_swings(&wolf, "minecraft:beef"),
            "full health: Animal's CONSUME"
        );
        wolf.health = wolf.attrs.max_health - 1.0;
        assert!(
            mob_swings(&wolf, "minecraft:beef"),
            "hurt: Wolf.feed is SUCCESS"
        );
        let mut cat = mob("minecraft:cat").unwrap();
        assert!(mob_swings(&cat, "minecraft:cod"), "taming: SUCCESS");
        cat.extra.tame = true;
        assert!(
            !mob_swings(&cat, "minecraft:cod"),
            "tame, full health: CONSUME"
        );
        assert!(mob_swings(&cat, ""), "a tame cat sits: SUCCESS");
        let mut parrot = mob("minecraft:parrot").unwrap();
        parrot.extra.tame = true;
        assert!(mob_swings(&parrot, ""), "a tame parrot sits: SUCCESS");
        let horse = mob("minecraft:horse").unwrap();
        assert!(
            !mob_swings(&horse, "minecraft:apple"),
            "AbstractHorse.fedFood: SUCCESS_SERVER"
        );
        let cow = mob("minecraft:cow").unwrap();
        assert!(!mob_swings(&cow, "minecraft:wheat"), "adult: CONSUME");
        let sheep = mob("minecraft:sheep").unwrap();
        assert!(!mob_swings(&sheep, "minecraft:shears"), "Sheep: CONSUME");
    }

    /// `AbstractNautilus.mobInteract` (#8595): an untamed adult takes its taming
    /// items only, and a tame adult is mounted by any click.
    #[test]
    fn a_nautilus_answers_its_food_and_a_tame_one_is_ridden() {
        assert!(
            !nautilus_takes(false, false, false),
            "untamed, taming item missing"
        );
        assert!(nautilus_takes(true, false, false), "a taming item");
        assert!(
            nautilus_takes(false, true, false),
            "a tame adult is mounted"
        );
        assert!(!nautilus_takes(false, true, true), "a baby is not mounted");

        // `AbstractNautilus.isFood`: taming items while untamed, its food after.
        let Some(mut m) = mob("minecraft:nautilus") else {
            return;
        };
        let taming = vec!["minecraft:pufferfish".to_string()];
        m.extra.age = 0;
        m.extra.tame = false;
        assert!(nautilus::is_food(&m, "minecraft:pufferfish", &taming));
        assert!(
            !nautilus::is_food(&m, "minecraft:cod", &taming),
            "cod tames nothing"
        );
        m.extra.tame = true;
        assert!(
            nautilus::is_food(&m, "minecraft:cod", &taming),
            "a tame one eats cod"
        );
    }

    /// `IronGolem.mobInteract` and `CopperGolem.mobInteract` (#8630): the click
    /// has to end on the golem, or the client goes on to use the item and turns
    /// to the off hand.
    #[test]
    fn the_golems_answer_the_click() {
        assert!(iron_golem_takes("minecraft:iron_ingot", 50.0, 100.0));
        assert!(
            !iron_golem_takes("minecraft:iron_ingot", 100.0, 100.0),
            "full"
        );
        assert!(
            !iron_golem_takes("minecraft:stick", 50.0, 100.0),
            "not ingot"
        );

        assert!(copper_golem_takes("", true, false), "takes its item back");
        assert!(!copper_golem_takes("", false, false), "nothing to take");
        assert!(
            copper_golem_takes("minecraft:shears", false, true),
            "shears"
        );
        assert!(
            !copper_golem_takes("minecraft:shears", false, false),
            "no flower"
        );
        assert!(
            !copper_golem_takes("minecraft:honeycomb", false, true),
            "waxing is server-only"
        );
        assert!(
            !copper_golem_takes("minecraft:iron_axe", false, true),
            "scraping too"
        );
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
