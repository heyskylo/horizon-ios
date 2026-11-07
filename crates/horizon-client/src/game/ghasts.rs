//! Happy ghasts on the game side: putting a harness on (and shearing it
//! off), riding, getting off on top, and the ghastling a dried ghast block
//! hatches.
use super::Game;
use horizon_client::actions::Action;
use horizon_client::{
    items::ItemStack,
    mobs::{MobEvent, happy_ghast},
};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    /// Client `EntityGetter.getEntityCollisions` for the player: the
    /// entities that `canBeCollidedWith` it, solid happy ghasts, living
    /// shulkers (`Shulker.canBeCollidedWith`) and boats
    /// (`AbstractBoat.canBeCollidedWith`), except the player's own vehicle.
    pub(super) fn player_entity_collisions(&self) -> Vec<Aabb> {
        let Some(p) = &self.player else {
            return Vec::new();
        };
        self.mobs
            .iter()
            .filter(|m| {
                (m.species() == horizon_client::mobs::Species::HappyGhast
                    && Some(m.id) != self.riding
                    && happy_ghast::collides_with_player(m, p.position.y))
                    || (m.species() == horizon_client::mobs::Species::Shulker && m.alive())
            })
            .map(|m| m.bounds())
            .chain(
                self.boats
                    .iter()
                    .filter(|b| Some(b.id) != self.riding)
                    .map(super::boats::Boat::bounds),
            )
            .collect()
    }
    /// `EntityGetter.getEntityCollisions` for a dropped item: the entities whose
    /// `canBeCollidedWith(item)` is true. A boat always, a living shulker, and a happy
    /// ghast only while it is held still (`isOnStillTimeout`): a player on its back keeps
    /// it still, so an item dropped there stays on top. A minecart keeps `Entity`'s false
    /// (`AbstractMinecart` overrides only `canCollideWith`), so an item falls through it.
    pub(super) fn item_entity_collisions(&self) -> Vec<Aabb> {
        self.mobs
            .iter()
            .filter(|m| {
                (m.species() == horizon_client::mobs::Species::HappyGhast
                    && m.alive()
                    && !m.is_baby()
                    && happy_ghast::on_still_timeout(m))
                    || (m.species() == horizon_client::mobs::Species::Shulker && m.alive())
            })
            .map(|m| m.bounds())
            .chain(self.boats.iter().map(super::boats::Boat::bounds))
            .collect()
    }
    /// `HappyGhast.mobInteract`. Returns whether the click was used.
    pub(super) fn interact_happy_ghast(&mut self, index: usize, item: &str) -> bool {
        let m = &self.mobs[index];
        if !m.alive() || Some(m.id) == self.riding {
            return false;
        }
        if m.is_baby() {
            // Animal.mobInteract: food grows it up.
            return false;
        }
        let at = m.center();
        let sneaking = self.keys.held(Action::Sneak);
        let harnessed = m.extra.body.is_some();
        // Equippable.equipOnInteract: a harness goes on a bare adult.
        if item.ends_with("_harness") && item.starts_with("minecraft:") && !harnessed {
            self.mobs[index].extra.body = Some(item.into());
            self.audio
                .play("minecraft:entity.happy_ghast.equip", Some(at), 1.0, 1.0);
            self.consume_held();
            return true;
        }
        // Equippable.canBeSheared: shears take the harness off again.
        if item == "minecraft:shears"
            && harnessed
            && !sneaking
            && let Some(harness) = self.mobs[index].extra.body.take()
        {
            self.audio
                .play("minecraft:entity.happy_ghast.unequip", Some(at), 1.0, 1.0);
            self.spawn_item(ItemStack::new(&harness, 1), at, [0.0, 0.2, 0.0], 10);
            self.damage_held_tool();
            return true;
        }
        if harnessed && !sneaking {
            // doPlayerRide.
            let id = self.mobs[index].id;
            if !self.native_may_board(id) {
                return true;
            }
            self.break_leash(id, true, false);
            let was_empty = self.mobs[index].rider.is_none();
            // addPassenger: the rider is not standing on top, so no still
            // timeout.
            self.mobs[index].extra.counter = 0;
            self.yaw = self.mobs[index].yaw;
            self.mount.reset_jump();
            if was_empty {
                let down = happy_ghast::goggles(true, &self.mobs[index]);
                let _ = self.handle_mob_events(vec![down]);
            }
            self.start_riding(id);
            return true;
        }
        false
    }
    /// Getting off lands on top of it.
    pub(super) fn dismount_happy_ghast(&mut self, id: u32) {
        // removePassenger: held still for half a second, and with no one
        // aboard the ghast forgets its home.
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.extra.counter = 10;
            m.ai.clear_home();
        }
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let top = Vec3::new(
            m.position.x,
            m.position.y + f64::from(m.height()),
            m.position.z,
        );
        let up: MobEvent = happy_ghast::goggles(false, m);
        if let Some(p) = self.player.as_mut() {
            p.position = top;
            p.velocity = Vec3::ZERO;
        }
        let _ = self.handle_mob_events(vec![up]);
    }
    /// `DriedGhastBlock.spawnGhastling`: a ghastling facing the way the block did.
    pub(super) fn spawn_ghastling(&mut self, pos: [i32; 3], state: BlockStateId) {
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        ];
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing").cloned());
        let yaw = match facing.as_deref() {
            Some("north") => 180.0,
            Some("east") => 270.0,
            Some("west") => 90.0,
            _ => 0.0,
        };
        if let Some(id) = self.spawn_mob("minecraft:happy_ghast", at, true)
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            // DriedGhastBlock.spawnGhastling sets the baby age before
            // addFreshEntity, so ageBoundaryReached runs the ghastling setup
            // (`babyGhastSetup`) while the mob is still being hatched: its
            // goals, navigation and brain are a ghastling's before its first
            // tick, not an adult's until the next aiStep.
            horizon_client::mobs::finalize::set_age(m, -24000);
            m.yaw = yaw;
            m.body_yaw = yaw;
            m.head_yaw = yaw;
            self.audio.play_bound(
                "minecraft:entity.ghastling.spawn",
                4,
                1.0,
                1.0,
                horizon_client::sound_engine::SoundEntity::new(
                    m.id,
                    [m.position.x, m.position.y, m.position.z],
                    m.silent,
                ),
                None,
            );
        }
    }
}

#[cfg(test)]
mod item_collision_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    /// An item dropped onto a minecart falls through it to the floor, and one
    /// dropped onto a boat rests on the boat (`canBeCollidedWith`).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_dropped_item_rests_on_a_boat_but_falls_through_a_minecart() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::env::temp_dir().join(format!("horizon-item-cart-unit-{}", std::process::id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap()
            .0;
        let mut chunk = Chunk::empty(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone);
            }
        }
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("summon minecart 4.5 200 4.5"));
        assert!(game.run_command("summon oak_boat 10.5 200 10.5"));
        for (x, z) in [(4.5, 4.5), (10.5, 10.5)] {
            assert!(game.run_command(&format!(
                "summon item {x} 201.5 {z} {{Item:{{id:\"minecraft:stick\",count:1}},Motion:[0.0d,0.0d,0.0d]}}"
            )));
        }
        for _ in 0..60 {
            game.tick_world(true).unwrap();
        }
        let rest = |x: f64| {
            game.dropped
                .iter()
                .find(|d| (d.position.x - x).abs() < 0.5)
                .map(|d| d.position.y)
                .unwrap()
        };
        assert_eq!(rest(4.5), 200.0, "through the minecart to the stone");
        let boat_top = game.boats[0].bounds().max_position().y;
        assert_eq!(rest(10.5), boat_top, "on the boat");
    }
}
