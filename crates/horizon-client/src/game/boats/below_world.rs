//! `Entity.checkBelowWorld` for boats and minecarts: one more than 64 blocks
//! under its level's floor is discarded (`onBelowWorld`) before it moves.
//! `AbstractBoat.tick` reaches it through `Entity.baseTick`, after the fire;
//! `AbstractMinecart.tick` calls it itself, after its hurt and damage wear off.
//! A discard is a destroying removal: the riders get off where they are and a
//! chest boat or container minecart spills its slots
//! (`AbstractChestBoat.remove`, `AbstractMinecartContainer.remove`), but no
//! ENTITY_DIE and no item of the vehicle itself.
use super::super::{minecarts::cart_at, native_ticks::TickRound};
use super::Game;

/// `Entity.checkBelowWorld`: `getY() < level.getMinY() - 64`.
pub(super) fn below_world(y: f64, min_y: i32) -> bool {
    y < f64::from(min_y - 64)
}

impl Game<'_> {
    /// `Entity.checkBelowWorld` for the boat at `i`: true when it fell out of
    /// the world and is gone.
    pub(in crate::game) fn boat_below_world(&mut self, i: usize) -> bool {
        if !below_world(self.boats[i].position.y, self.world.bottom_y()) {
            return false;
        }
        self.eject_boat_mobs(i, true);
        let id = self.boats[i].id;
        self.release_vehicle_container(id);
        let b = self.boats.remove(i);
        self.drop_vehicle_contents(b.position.components(), &b.items);
        self.boat_removed(&b);
        true
    }

    /// The minecart `id` at or after `slot`, unless `AbstractMinecart.tick`'s
    /// `checkBelowWorld` discards it: then its tick is over and the round's
    /// post hook runs for it.
    pub(in crate::game) fn cart_in_world(
        &mut self,
        id: u32,
        slot: usize,
        round: &TickRound,
        kind: &str,
    ) -> Option<usize> {
        let i = cart_at(&self.carts, id, slot)?;
        if !below_world(self.carts[i].position.y, self.world.bottom_y()) {
            return Some(i);
        }
        self.eject_cart_mobs(i);
        self.release_vehicle_container(id);
        let c = self.carts.remove(i);
        self.drop_vehicle_contents(c.position.components(), &c.items);
        self.native_tick_post_one(round, id, kind);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::below_world;

    /// The line is 64 under the floor and the vehicle must be below it:
    /// -128 in the overworld (floor -64), -64 in the Nether and the End.
    #[test]
    fn a_vehicle_falls_out_of_the_world_64_blocks_under_the_floor() {
        assert!(!below_world(-128.0, -64));
        assert!(below_world(-128.001, -64));
        assert!(!below_world(-100.0, -64));
        assert!(!below_world(-64.0, 0));
        assert!(below_world(-64.5, 0));
    }

    /// A tick discards the boat and the chest minecart under -128 in the
    /// overworld, before they move, and spills the cart's slots; the two on
    /// the line or above it stay, whatever their fall this tick.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_tick_discards_the_vehicles_under_the_world() {
        use horizon_client::chunk::Chunk;
        use horizon_core::position::ChunkPos;
        let mut g = crate::game::native_machines::tests::server_game();
        g.build_registries();
        g.set_allow_commands(true);
        g.world
            .chunks
            .insert((0, 0), std::sync::Arc::new(Chunk::empty(0, 0)));
        g.block_ticks.add_empty_chunk(ChunkPos::new(0, 0)).unwrap();
        assert!(g.run_command("forceload add 0 0"));
        for summon in [
            "summon oak_boat 0.5 -128 0.5",
            "summon oak_boat 4.5 -128.5 4.5",
            "summon minecart 6.5 -127 6.5",
            "summon chest_minecart 2.5 -129 2.5 {Items:[{Slot:0b,id:\"minecraft:diamond\",count:3}]}",
        ] {
            assert!(g.run_command(summon), "{summon}");
        }
        g.sync_tick_chunks();
        g.tick_boats();
        g.tick_carts();
        let boats: Vec<f64> = g.boats.iter().map(|b| b.position.x).collect();
        assert_eq!(boats, [0.5]);
        let carts: Vec<f64> = g.carts.iter().map(|c| c.position.x).collect();
        assert_eq!(carts, [6.5]);
        let spilled: u32 = g
            .dropped
            .iter()
            .filter(|i| &*i.stack.item == "minecraft:diamond")
            .map(|i| i.stack.count)
            .sum();
        assert_eq!(spilled, 3);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn saved_deep_end_boat_loads_then_is_discarded_and_extreme_scans_terminate() {
        use super::super::Boat;
        use horizon_client::{chunk::Chunk, motion::PlayerMotion};
        use horizon_core::geometry::Vec3;
        use horizon_world::nbt::Tag;
        let mut g = crate::game::native_machines::tests::server_game();
        g.world.shape.min_y = 0;
        let doubles = |v: &[f64]| Tag::List(v.iter().map(|d| Tag::Double(d.to_bits())).collect());
        let tag = Tag::compound([
            (
                "Pos",
                doubles(&[-77.75688596047263, -2146769274.8915758, -55.6734678353711]),
            ),
            ("Motion", doubles(&[-2e-323, -13104.999899947774, -2e-323])),
        ]);
        g.load_boat(tag, None, "minecraft:oak_boat");
        assert_eq!(g.boats.len(), 1);
        assert_eq!(g.boats[0].position.y, -20_000_000.0);
        assert_eq!(g.boats[0].velocity, Vec3::new(-2e-323, 0.0, -2e-323));
        g.player = Some(PlayerMotion::new(g.motion, Vec3::new(-77.75, 64.0, -55.67)).unwrap());
        g.world
            .chunks
            .insert((-5, -4), std::sync::Arc::new(Chunk::empty(-5, -4)));
        g.update_chunk_tickets();
        assert!(g.entity_ticking_area()(-78, -56));
        g.tick_boats();
        assert!(g.boats.is_empty());
        for y in [f64::from(i32::MIN) - 1.0, f64::from(i32::MAX) + 1.0] {
            g.boats
                .push(Boat::new(2, "oak".into(), Vec3::new(0.5, y, 0.5), 0.0));
            assert!(g.boat_bubble_columns(0).is_empty());
            assert_eq!(g.boat_ground_friction(g.boats[0].bounds()), 0.0);
            g.boats.clear();
        }
    }
}
