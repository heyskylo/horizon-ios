//! Zombie villagers in the world: zombies turning the villagers they kill
//! into zombie villagers (`Zombie.killedEntity`), the golden apple that starts
//! a cure (`ZombieVillager.mobInteract`), and the cured villager that
//! remembers who saved it (`ZombieVillager.finishConversion`).
use super::{Game, commands::Target};
use horizon_client::{
    advancements as adv,
    gossip::GossipType,
    items::ItemStack,
    lan::playerdata,
    mobs::{Mob, Species, villager, zombie_villager},
};
use horizon_core::random::RandomSource;
use horizon_world::food::Difficulty;

/// The `Zombie` subclasses that convert the villagers they kill.
fn is_zombie(s: Species) -> bool {
    matches!(
        s,
        Species::Zombie
            | Species::Husk
            | Species::Drowned
            | Species::ZombieVillager
            | Species::ZombifiedPiglin
    )
}
/// Whether a zombie turns the villager it killed: always on Hard, half the
/// time on Normal (`Zombie.killedEntity`).
fn infects(difficulty: Difficulty, coin: impl FnOnce() -> bool) -> bool {
    match difficulty {
        Difficulty::Hard => true,
        Difficulty::Normal => !coin(),
        _ => false,
    }
}

impl Game<'_> {
    /// `ZombieVillager.handleEntityEvent(16)`: only the client plays the cure sound.
    pub(super) fn zombie_villager_cure_sound(&mut self, id: u32) {
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            && let Some(horizon_client::mobs::MobEvent::Sound(name, at, volume, pitch)) =
                zombie_villager::cure_sound(m)
        {
            self.audio
                .play_unshared(&format!("minecraft:{name}"), Some(at), volume, pitch);
        }
    }

    /// `Zombie.killedEntity`: a villager killed by a zombie may rise as a
    /// zombie villager keeping its profession, trades and gossip.
    pub(super) fn zombie_killed(&mut self, killer: u32, victim: u32) {
        let Some(k) = self.mobs.iter().position(|m| m.id == killer) else {
            return;
        };
        let Some(v) = self.mobs.iter().position(|m| m.id == victim) else {
            return;
        };
        if !is_zombie(self.mobs[k].species())
            || self.mobs[v].species() != Species::Villager
            || self.mobs[v].alive()
            || self.mobs[v].removed
        {
            return;
        }
        let difficulty = self.world.health_rules.difficulty;
        let random = &mut self.mobs[k].random;
        if !infects(difficulty, || random.next_boolean()) {
            return;
        }
        // The villager is gone without dying: no drops, no death animation.
        let old = self.mobs[v].clone();
        self.mobs[v].removed = true;
        self.mobs[v].health = 0.0;
        let at = [old.position.x, old.position.y, old.position.z];
        let Some(id) = self.spawn_mob("minecraft:zombie_villager", at, old.persistent) else {
            return;
        };
        let ensure = self.mobs.iter().position(|m| m.id == victim);
        if let Some(i) = ensure {
            self.ensure_offers(i);
        }
        let old = ensure.map_or(old, |i| self.mobs[i].clone());
        let Some(z) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        convert_to_zombie_villager(&old, z);
        if !self.mobs[k].silent {
            let at = self.mobs[k].position;
            self.level_event_at(
                1026,
                [
                    at.x.floor() as i32,
                    at.y.floor() as i32,
                    at.z.floor() as i32,
                ],
                0,
            );
        }
    }

    /// `ZombieVillager.mobInteract`: a golden apple on a weakened zombie
    /// villager starts the cure and is eaten; without weakness it does
    /// nothing. Returns whether the click was used.
    pub(super) fn interact_zombie_villager(&mut self, index: usize, item: &str) -> bool {
        let m = &self.mobs[index];
        if m.species() != Species::ZombieVillager || item != "minecraft:golden_apple" {
            return false;
        }
        if !m.has_effect("minecraft:weakness") {
            return true;
        }
        self.consume_held();
        // `player.getUUID()`: the acting player, a server's guest included.
        let starter = Some(self.acting_player_uuid());
        let time = zombie_villager::WAIT_MIN
            + self.mobs[index]
                .random
                .next_int_bound(zombie_villager::WAIT_SPREAD)
                .unwrap_or(0);
        let mut events = Vec::new();
        zombie_villager::start_converting(&mut self.mobs[index], starter, time, &mut events);
        let _ = self.handle_mob_events(events);
        self.swing_arm();
        true
    }

    /// `ZombieVillager.finishConversion`: the villager returns with its
    /// data, trades, experience and gossip, and the curing player earns the
    /// village's trust.
    pub(super) fn cure_zombie_villager(&mut self, id: u32) {
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        if self.mobs[i].removed || zombie_villager::cure(&self.mobs[i]).is_none() {
            return;
        }
        let old = self.mobs[i].clone();
        self.mobs[i].removed = true;
        let at = [old.position.x, old.position.y, old.position.z];
        let Some(vid) = self.spawn_mob("minecraft:villager", at, old.persistent) else {
            return;
        };
        let Some(v) = self.mobs.iter_mut().find(|m| m.id == vid) else {
            return;
        };
        let drops = convert_to_villager(&old, v);
        for stack in drops {
            self.spawn_at_location(old.position, 0.0, stack);
        }
        // `level.getPlayerByUUID(conversionStarter)`: the player who fed the
        // apple, if still in this level (the host or a server's guest).
        let starter = zombie_villager::cure(&old).and_then(|c| c.starter);
        let curer = starter.and_then(|uuid| Some((uuid, self.player_here(uuid)?)));
        // `Villager.onReputationEventFrom(ZOMBIE_VILLAGER_CURED)`.
        let Some(v) = self.mobs.iter_mut().find(|m| m.id == vid) else {
            return;
        };
        if let Some((uuid, _)) = curer {
            let g = &mut villager::ext_mut(v).gossips;
            g.add(uuid, GossipType::MajorPositive, 20);
            g.add(uuid, GossipType::MinorPositive, 25);
        }
        let cured = v.clone();
        let villager_view = self.mob_entity(&cured);
        let zombie_view = self.mob_entity(&old);
        if let Some((_, who)) = curer {
            self.for_player(who, |g| {
                g.trigger_cured_zombie_villager(&zombie_view, &villager_view)
            });
        }
        // The nausea comes after the advancement criterion (finishConversion).
        if let Some(v) = self.mobs.iter_mut().find(|m| m.id == vid) {
            v.add_effect("minecraft:nausea", 200, 0);
        }
        if !old.silent {
            self.level_event_at(
                1027,
                [
                    old.position.x.floor() as i32,
                    old.position.y.floor() as i32,
                    old.position.z.floor() as i32,
                ],
                0,
            );
        }
    }

    /// `ServerLevel.getPlayerByUUID`: the player with `uuid` in this level.
    fn player_here(&self, uuid: [i32; 4]) -> Option<Target> {
        match self.target_by_uuid(playerdata::ints_uuid(uuid))? {
            Target::Player => Some(Target::Player),
            Target::Guest(conn) => {
                let g = self.lan.as_ref()?.hub.guest(conn)?;
                (g.dimension == self.dim.id()).then_some(Target::Guest(conn))
            }
            _ => None,
        }
    }

    /// `CuredZombieVillagerTrigger`.
    fn trigger_cured_zombie_villager(
        &mut self,
        zombie: &adv::EntityView,
        villager: &adv::EntityView,
    ) {
        let me = self.player_entity_for("minecraft:cured_zombie_villager");
        let (zombie, villager) = (zombie.clone(), villager.clone());
        self.trigger("minecraft:cured_zombie_villager", move |c, w| {
            let origin = me.position;
            let ctx = |e| adv::Entities {
                this: Some(e),
                origin,
                tool: None,
                block_pos: None,
            };
            adv::conditions(&c["player"], &ctx(&me), w)
                && adv::conditions(&c["zombie"], &ctx(&zombie), w)
                && adv::conditions(&c["villager"], &ctx(&villager), w)
        });
    }
}

/// `Mob.convertTo` (`ConversionType.SINGLE`, keeping equipment and the
/// pick-up-loot flag): what any converted mob carries over.
fn carry_over(from: &Mob, to: &mut Mob) {
    to.position = from.position;
    to.previous = from.position;
    to.velocity = from.velocity;
    to.yaw = from.yaw;
    to.previous_yaw = from.yaw;
    to.body_yaw = from.body_yaw;
    to.previous_body_yaw = from.body_yaw;
    to.head_yaw = from.head_yaw;
    to.previous_head_yaw = from.head_yaw;
    to.head_pitch = from.head_pitch;
    to.previous_head_pitch = from.head_pitch;
    to.fall_distance = from.fall_distance;
    to.hurt_time = from.hurt_time;
    to.on_ground = from.on_ground;
    to.fire_ticks = from.fire_ticks;
    to.persistent = from.persistent;
    to.custom_name = from.custom_name.clone();
    to.custom_name_text = from.custom_name_text.clone();
    to.name_visible = from.name_visible;
    to.invisible = from.invisible;
    to.extra.pick_up_loot = from.extra.pick_up_loot;
    // ConversionType.convertCommon: the flags a converted mob keeps.
    to.extra.left_handed = from.extra.left_handed;
    to.no_ai = from.no_ai;
    to.invulnerable_flag = from.invulnerable_flag;
    to.invulnerable_time = from.invulnerable_time;
    to.no_gravity = from.no_gravity;
    to.silent = from.silent;
    to.extra.set_main_hand(from.extra.main_hand.clone());
    if let Some(stack) = from.extra.main_hand_stack() {
        to.extra.set_main_hand_stack(Some(stack.clone()));
    }
    to.extra.off_hand = from.extra.off_hand.clone();
    to.extra.head = from.extra.head.clone();
    to.extra.chest = from.extra.chest.clone();
    to.extra.legs = from.extra.legs.clone();
    to.extra.feet = from.extra.feet.clone();
    to.extra.effects = from.extra.effects.clone();
    to.extra.effect_fallbacks = from.extra.effect_fallbacks.clone();
    if to
        .extra
        .effects
        .iter()
        .any(|e| e.0 == "minecraft:health_boost")
    {
        horizon_client::mobs::finalize::health_boost_changed(to);
    }
    // VillagerData and the trades, experience and gossip.
    to.extra.variant = from.extra.variant;
    to.extra.variant2 = from.extra.variant2;
    to.extra.variant3 = from.extra.variant3.max(1);
    to.extra.trade = from.extra.trade.clone();
    if let Some(g) = villager::ext(from).map(|e| e.gossips.clone()) {
        villager::ext_mut(to).gossips = g;
    }
}
/// A villager rises as a zombie villager (never as an adult-age baby: a baby
/// stays a baby for good).
pub(super) fn convert_to_zombie_villager(from: &Mob, to: &mut Mob) {
    carry_over(from, to);
    to.extra.age = if from.is_baby() { i32::MIN / 2 } else { 0 };
}
/// A cured zombie villager becomes a villager again (the caller adds the
/// 10 second nausea after the cure criterion).
/// `convertTo(.., keepEquipment = false)` leaves the villager bare, then
/// `ZombieVillager.finishConversion` runs `Mob.dropPreservedEquipment` with
/// "no `PREVENT_ARMOR_CHANGE`": kept (cursed) stacks go to the villager's
/// slot `EquipmentSlot.getIndex() + 300` (its inventory), picked-up gear
/// (drop chance > 1) is returned for the caller to drop, the rest vanishes.
pub(super) fn convert_to_villager(from: &Mob, to: &mut Mob) -> Vec<ItemStack> {
    use horizon_client::mobs::{drops, finalize};
    carry_over(from, to);
    let mut drops_out = Vec::new();
    // `EquipmentSlot.VALUES` order: main hand, off hand, feet, legs, chest, head.
    for (slot, name) in drops::SLOTS.iter().take(6).enumerate() {
        let Some(stack) = finalize::item_stack(from, slot) else {
            continue;
        };
        drops::clear_slot(to, name);
        if stack.enchantment_level("minecraft:binding_curse") > 0 {
            // `getIndex() + 300`: hands 0..1, then feet, legs, chest, head 0..3.
            let index = if slot < 2 { slot } else { slot - 2 };
            let inventory = &mut villager::ext_mut(to).inventory;
            if index < inventory.len() {
                inventory[index] = stack;
            } else {
                inventory.push(stack);
            }
        } else if drops::drop_chance(from, name) > 1.0 {
            drops_out.push(stack);
        }
    }
    to.extra.age = if from.is_baby() { -24000 } else { 0 };
    drops_out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zombies_convert_by_difficulty() {
        assert!(infects(Difficulty::Hard, || unreachable!()));
        assert!(!infects(Difficulty::Easy, || false));
        assert!(!infects(Difficulty::Peaceful, || false));
        // Normal: a coin toss that spares the villager when it is true.
        assert!(infects(Difficulty::Normal, || false));
        assert!(!infects(Difficulty::Normal, || true));
    }
    #[test]
    fn every_zombie_kind_infects() {
        for s in [
            Species::Zombie,
            Species::Husk,
            Species::Drowned,
            Species::ZombieVillager,
            Species::ZombifiedPiglin,
        ] {
            assert!(is_zombie(s));
        }
        assert!(!is_zombie(Species::Skeleton) && !is_zombie(Species::Villager));
    }

    /// `ConversionType.convertCommon` copies Silent, NoAI, LeftHanded,
    /// NoGravity and Invulnerable onto the mob that replaces the old one.
    #[test]
    fn infection_and_the_cure_keep_the_common_flags() {
        use horizon_client::mobs::{Anim, Brain, MobType, Part};
        use horizon_core::geometry::Vec3;
        use std::sync::Arc;
        let mob = |id, species, name: &str| {
            let kind = MobType {
                id: name.into(),
                species,
                width: 0.6,
                height: 1.95,
                eye_height: 1.74,
                category: "monster".into(),
                max_health: 20.0,
                speed: 0.23,
                flying_speed: 0.4,
                attack_damage: 3.0,
                attack_knockback: 0.0,
                follow_range: 35.0,
                step_height: 0.6,
                armor: 0.0,
                armor_toughness: 0.0,
                knockback_resistance: 0.0,
                jump_strength: 0.42,
                attributes: Arc::default(),
                item_stack_limits: Arc::default(),
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
                ambient_interval: 80,
                flags: Default::default(),
                model: Arc::new(Part {
                    name: "root".into(),
                    pose: [0.0; 9],
                    polygons: Vec::new(),
                    children: Vec::new(),
                }),
                layers: Default::default(),
                texture: String::new(),
                brain: Brain::Passive,
                anim: Anim::Humanoid { zombie_arms: true },
                sounds: ["entity.villager.ambient"; 3],
                step_sound: None,
                food: Arc::default(),
                tempt: Arc::default(),
            };
            Mob::new(id, Arc::new(kind), Vec3::ZERO, 0.0)
        };
        let mut villager = mob(1, Species::Villager, "minecraft:villager");
        villager.silent = true;
        villager.no_ai = true;
        villager.no_gravity = true;
        villager.invulnerable_flag = true;
        villager.extra.left_handed = true;
        let mut zombie = mob(2, Species::ZombieVillager, "minecraft:zombie_villager");
        assert!(!zombie.silent && !zombie.no_ai && !zombie.extra.left_handed);
        convert_to_zombie_villager(&villager, &mut zombie);
        assert!(zombie.silent);
        assert!(zombie.no_ai);
        assert!(zombie.no_gravity);
        assert!(zombie.invulnerable_flag);
        assert!(zombie.extra.left_handed);
        let mut cured = mob(3, Species::Villager, "minecraft:villager");
        let drops = convert_to_villager(&zombie, &mut cured);
        assert!(drops.is_empty());
        assert!(cured.silent);
        assert!(cured.no_ai);
        assert!(cured.no_gravity);
        assert!(cured.invulnerable_flag);
        assert!(cured.extra.left_handed);
        // The cure keeps no equipment: picked-up gear drops, cursed gear goes
        // to the inventory, the rest vanishes.
        use horizon_client::mobs::finalize::{
            FEET, HEAD, MAINHAND, item_stack, set_drop_chance, set_item, set_item_stack,
        };
        let mut gear = mob(4, Species::ZombieVillager, "minecraft:zombie_villager");
        set_item(&mut gear, MAINHAND, "minecraft:iron_sword");
        set_drop_chance(&mut gear, MAINHAND, 2.0);
        set_item(&mut gear, HEAD, "minecraft:iron_helmet");
        let mut boots = ItemStack::new("minecraft:iron_boots", 1);
        boots.set_crafting_enchantments(vec![("minecraft:binding_curse".into(), 1)]);
        set_item_stack(&mut gear, FEET, boots);
        let mut bare = mob(5, Species::Villager, "minecraft:villager");
        let drops = convert_to_villager(&gear, &mut bare);
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].item.as_ref(), "minecraft:iron_sword");
        for slot in 0..=5 {
            assert!(item_stack(&bare, slot).is_none(), "slot {slot}");
        }
        let kept = &villager::ext(&bare).unwrap().inventory;
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].item.as_ref(), "minecraft:iron_boots");
    }
}
