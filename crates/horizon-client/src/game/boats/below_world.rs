use super::super::Game;

impl Game<'_> {
    /// `Entity.checkBelowWorld` / `onBelowWorld`, reached by `AbstractBoat.tick`
    /// through `Entity.baseTick`: discard below the dimension floor minus 64.
    /// `DISCARDED` destroys the container and lead, without a hull or death event.
    pub(super) fn boat_below_world(&mut self, i: usize) -> bool {
        if self.remote.is_some()
            || self.boats[i].position.y >= f64::from(self.world.shape.min_y) - 64.0
        {
            return false;
        }
        self.eject_boat_mobs(i, true);
        self.release_vehicle_container(self.boats[i].id);
        let b = self.boats.remove(i);
        self.drop_vehicle_contents([b.position.x, b.position.y, b.position.z], &b.items);
        self.boat_removed(&b);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::Boat;
    use crate::game::native_machines::tests::server_game;
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::geometry::Vec3;
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn falling_boats_are_discarded_on_the_tick_after_crossing_the_void_line() {
        let mut g = server_game();
        g.player = Some(PlayerMotion::new(g.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
        g.update_chunk_tickets();
        for min_y in [-64, 0] {
            g.world.shape.min_y = min_y;
            let line = f64::from(min_y) - 64.0;
            let mut b = Boat::new(1, "oak".into(), Vec3::new(0.5, line, 0.5), 0.0);
            b.velocity.y = -0.5;
            g.boats.push(b);
            g.tick_boats();
            assert_eq!(g.boats.len(), 1, "the threshold itself survives baseTick");
            assert!(g.boats[0].position.y < line);
            g.tick_boats();
            assert!(g.boats.is_empty());
            assert!(g.dropped.is_empty(), "discard has no boat item drop");
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn saved_deep_end_boat_loads_then_is_discarded_and_extreme_scans_terminate() {
        let mut g = server_game();
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
