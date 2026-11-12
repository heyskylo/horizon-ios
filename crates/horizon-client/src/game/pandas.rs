//! Pandas on the game side: the food they find on the ground and pick up
//! (`Mob.aiStep`, `Panda.pickUpItem`), the bamboo a player gives them
//! (`Panda.mobInteract`), the start a sneeze gives the grown pandas about,
//! and the crumbs and the sneeze a window draws.
use super::{
    Game,
    pickups::{Collector, Taken},
};
use horizon_client::{
    items::ItemStack,
    mobs::{Species, panda},
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::geometry::Vec3;

impl Game<'_> {
    /// `Panda.canPickUpAndEat`'s tag.
    fn panda_eats_from_ground(&self, item: &str) -> bool {
        self.res
            .items()
            .item_tags
            .get("minecraft:panda_eats_from_ground")
            .is_some_and(|tag| tag.contains(item))
    }
    /// A panda takes what lies at its feet when its mouth is free, and
    /// looks for food within 6 and 8 blocks for `PandaSitGoal`.
    pub(super) fn tick_pandas(&mut self) {
        let griefing = self.ui.rules.bool("mob_griefing");
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            if m.species() != Species::Panda || !m.alive() || !self.creatures.ticked.contains(&m.id)
            {
                continue;
            }
            let id = m.id;
            let wants = |g: &Self, e: &horizon_client::entities::ItemEntity| {
                e.pickup_delay == 0 && g.panda_eats_from_ground(&e.stack.item)
            };
            // Mob.aiStep: the items within the box grown by (1, 0, 1).
            if griefing && m.can_pick_up_loot() && m.extra.main_hand.is_none() {
                let reach = m.bounds().inflate(Vec3::new(1.0, 0.0, 1.0));
                let taken = self
                    .dropped
                    .iter()
                    .position(|e| wants(self, e) && reach.intersects(e.bounds()));
                if let Some(j) = taken {
                    let before = self.dropped[j].clone();
                    // `Panda.pickUpItem`'s `onItemPickup` comes first: the
                    // advancement of whoever threw it.
                    self.mob_item_pickup_trigger(i, &before);
                    let e = self.remove_dropped(j);
                    self.add_pickup(Taken::Item(Box::new(e.clone())), Collector::Mob(id));
                    panda::hold(&mut self.mobs[i], &e.stack.item, e.stack.count, true);
                }
            }
            let m = &self.mobs[i];
            let area = |r: f64| m.bounds().inflate(Vec3::new(r, r, r));
            let (near, wide) = (area(6.0), area(8.0));
            let food_near = self
                .dropped
                .iter()
                .any(|e| wants(self, e) && near.intersects(e.bounds()));
            let food_at = self
                .dropped
                .iter()
                .find(|e| wants(self, e) && wide.intersects(e.bounds()))
                .map(|e| [e.position.x, e.position.y, e.position.z]);
            panda::set_food(&mut self.mobs[i], food_near, food_at);
        }
    }
    /// `Panda.mobInteract` for a click with `item`; whether the click is taken.
    pub(super) fn interact_panda(&mut self, index: usize, item: &str) -> bool {
        let creative = self.creative;
        let m = &mut self.mobs[index];
        if panda::scared(m) {
            return false;
        }
        if panda::get_up(m) {
            return true;
        }
        // A baby holding a golden_dandelion goes on to `AgeableMob.mobInteract`,
        // which `interact_mob` answered before this; anything else that is not
        // bamboo passes.
        if !m.kind.food.iter().any(|f| f == item) {
            return false;
        }
        if m.ai.target.is_some() {
            panda::got_bamboo(m);
        }
        let at = [m.position.x, m.position.y, m.position.z];
        let id = m.id;
        let mut dropped = None;
        let mut loved = false;
        if m.is_baby() {
            // Panda.mobInteract: a baby an age lock holds back passes.
            if !horizon_client::mobs::ai::can_age_up(m) {
                return false;
            }
            // ageUp((int) (-getAge() / 20 * 0.1F), true).
            let seconds = ((-m.extra.age / 20) as f32 * 0.1) as i32;
            horizon_client::mobs::ai::age_up(m, seconds, true);
        } else if m.extra.age == 0 && m.extra.in_love <= 0 {
            m.extra.in_love = 600;
            m.extra.love_player = true;
            loved = true;
        } else {
            if panda::is_sitting(m) || m.fluids.in_water {
                return false;
            }
            panda::try_to_sit(m);
            panda::start_eating(m);
            // What it held falls, unless the player has infinite materials.
            if let Some(held) = m.extra.main_hand.clone()
                && !creative
            {
                dropped = Some(ItemStack::new(&held, panda::held_count(m)));
            }
            panda::hold(m, item, 1, false);
        }
        if loved {
            self.love_cause.insert(id, self.lan_acting);
            let kind = self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map(super::native_hooks::mob_kind)
                .unwrap_or_default();
            self.native_entity_enter_love_mode(id, &kind, self.lan_acting_uuid);
        }
        self.consume_held();
        if let Some(stack) = dropped {
            self.spawn_item(stack, at, [0.0, 0.2, 0.0], 10);
        }
        true
    }
    /// `Panda.afterSneeze`: the grown pandas within 10 blocks of a cub jump.
    pub(super) fn panda_sneezed(&mut self, id: u32) {
        let Some(area) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.bounds().inflate(Vec3::new(10.0, 10.0, 10.0)))
        else {
            return;
        };
        let mut mobs = std::mem::take(&mut self.mobs);
        let w = self.mob_world_base(&[]);
        for m in &mut mobs {
            if m.species() == Species::Panda
                && !m.is_baby()
                && m.on_ground
                && !m.fluids.in_water
                && panda::can_perform_action(m, panda::scared(m))
                && area.intersects(m.bounds())
            {
                // LivingEntity.jumpFromGround.
                m.needs_sync = true;
                let power = f64::from(m.jump_power(&w));
                if power > 1.0e-5 {
                    m.velocity.y = power.max(m.velocity.y);
                }
            }
        }
        self.mobs = mobs;
    }
    /// What a window draws of its pandas: the crumbs of a bite
    /// (`addEatingParticles`) and the sneeze's particle.
    pub(super) fn panda_effects(&mut self) {
        let mut bites = Vec::new();
        let mut sneezes = Vec::new();
        for m in self
            .mobs
            .iter_mut()
            .filter(|m| m.species() == Species::Panda)
        {
            let (crumbs, sneeze) = panda::effects(m);
            if let (Some(crumbs), Some(item)) = (crumbs, m.extra.main_hand.clone()) {
                bites.push((item, crumbs));
            }
            sneezes.extend(sneeze);
        }
        for (item, crumbs) in bites {
            let options = self.item_particle(&ItemStack::new(&item, 1));
            for (at, velocity) in crumbs {
                self.particles.add(&options, at, velocity);
            }
        }
        let options = ParticleOptions::simple(ParticleType::Sneeze);
        for (at, velocity) in sneezes {
            self.particles.add(&options, at, velocity);
        }
    }
}

#[cfg(test)]
mod pickup_tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    use horizon_client::advancements as adv;
    use horizon_client::motion::PlayerMotion;
    use horizon_world::lazy_json::LazyJson;

    const ADV: &str = "horizon:test/panda_pickup";
    const TRIGGER: &str = "minecraft:thrown_item_picked_up_by_entity";
    const THROWER: [i32; 4] = [413, 0, 0, 1];

    /// An advancement on the pickup trigger, as a data pack would add one: no
    /// vanilla advancement tests it, so the tree needs one of its own.
    fn install_pickup_advancement(game: &mut Game<'_>) {
        let mut tree = std::sync::Arc::new(adv::Tree::default());
        let one = std::sync::Arc::get_mut(&mut tree).unwrap();
        one.list.push(adv::Advancement {
            id: ADV.into(),
            parent: None,
            display: None,
            criteria: vec![adv::Criterion {
                name: "pickup".into(),
                trigger: TRIGGER.into(),
                conditions: LazyJson::new(&serde_json::json!({})),
            }],
            requirements: vec![vec!["pickup".into()]],
            rewards: Default::default(),
        });
        one.by_id.insert(ADV.into(), 0);
        one.by_trigger.insert(TRIGGER.into(), vec![(0, 0)]);
        game.adv.tree = tree;
    }

    /// `Panda.pickUpItem` calls `onItemPickup`, which fires
    /// `CriteriaTriggers.THROWN_ITEM_PICKED_UP_BY_ENTITY` for whoever threw the
    /// bamboo, as the fox's pickup does (`LivingEntity.onItemPickup`).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_panda_that_takes_a_thrown_item_credits_the_thrower() {
        let mut game = server_game();
        game.options.dedicated = None;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 100.0, 4.5)).unwrap());
        game.level.player_uuid = Some(THROWER);
        game.level.health = 20.0;
        game.creative = false;
        // A world of its own under the temporary folder, never the cwd.
        game.world_root =
            std::env::temp_dir().join(format!("horizon-panda-pickup-world-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&game.world_root);
        game.build_registries();
        game.set_allow_commands(true);
        install_pickup_advancement(&mut game);
        // `DistanceManager` keeps a chunk entity-ticking only while its whole
        // 5x5 neighbourhood is resident.
        for cx in -2..=2 {
            for cz in -2..=2 {
                game.world.chunks.insert(
                    (cx, cz),
                    std::sync::Arc::new(crate::chunk::Chunk::empty(cx, cz)),
                );
            }
        }
        for command in [
            "forceload add 0 0",
            "tick freeze",
            "fill 0 99 0 15 99 15 stone",
            "summon minecraft:panda 4.5 100 6.5 {UUID:[I;9,9,9,9],PersistenceRequired:1b,CanPickUpLoot:1b}",
            "summon minecraft:item 4.5 100 6.5 {Item:{id:\"minecraft:bamboo\",count:1},Thrower:[I;413,0,0,1],PickupDelay:0s}",
        ] {
            assert!(game.run_command(command), "{command}");
        }
        let id = game.mob_by_uuid([9, 9, 9, 9]).unwrap();
        let holding = |g: &Game<'_>| {
            g.mobs
                .iter()
                .any(|m| m.id == id && m.extra.main_hand.is_some())
        };
        for _ in 0..20 {
            if holding(&game) {
                break;
            }
            assert!(game.run_command("tick step 1"));
            game.tick_server(false).unwrap();
        }
        assert!(holding(&game), "the panda took the bamboo");
        assert!(
            game.adv.progress.has(ADV, "pickup"),
            "the thrower's advancement: {:?}",
            game.adv.progress.criteria
        );
        let _ = std::fs::remove_dir_all(&game.world_root);
    }
}
