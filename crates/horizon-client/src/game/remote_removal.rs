//! Removing entities a server dropped (`RemoveEntities`), out of remote.rs,
//! which may not grow.
use super::super::Game;

impl Game<'_> {
    pub(super) fn remove_entity(&mut self, id: i32) {
        self.remove_entities(&[id]);
    }

    /// `ClientboundRemoveEntitiesPacket`: a packet's ids go together, the big
    /// entity lists filtered once (#13007), each id's own cleanup in order.
    pub(super) fn remove_entities(&mut self, ids: &[i32]) {
        let removed: horizon_client::fast_hash::FxHashSet<u32> =
            ids.iter().map(|&id| id as u32).collect();
        let mut vehicles = horizon_client::fast_hash::FxHashSet::default();
        let mut include = |id| {
            if removed.contains(&id) {
                vehicles.insert(id);
            }
        };
        if let Some(r) = &self.remote {
            for &id in r.passenger_vehicles.values() {
                include(id);
            }
        }
        // Shared/server riding paths can also set these links without a
        // SetPassengers entry. Preserve their cleanup, scanning once per packet.
        for m in &self.mobs {
            if let Some(id) = m.extra.mount {
                include(id);
            }
            if m.extra.rider.is_some() {
                include(m.id);
            }
        }
        for id in self
            .text_displays
            .values()
            .filter_map(|d| d.vehicle)
            .chain(self.armor_stands.iter().filter_map(|a| a.vehicle))
            .chain(self.interactions.values().filter_map(|i| i.vehicle))
            .chain(
                self.boats
                    .iter()
                    .filter(|b| b.paddle_controller)
                    .map(|b| b.id),
            )
            .chain(self.riding)
        {
            include(id);
        }
        // Snapshot before removing passenger keys: a removed passenger can
        // precede its vehicle, whose other seat state still needs clearing.
        for &id in ids {
            self.detach_remote_tracking_entity(id);
            self.audio.remove_sound_entity(id as u32);
            // Entity.setRemoved detaches every passenger, even when no empty
            // SetPassengers packet preceded the vehicle's removal. Otherwise
            // mobs and other players keep their passenger pose after it is gone.
            if vehicles.contains(&(id as u32)) {
                self.remote_set_passengers(id, &[]);
            }
            // Before the crystals go: the dragon's beam index follows them.
            self.remove_remote_dragon(id);
            self.crystals.retain(|c| c.id != id as u32);
            self.remove_remote_object(id as u32);
            self.remove_remote_small_projectiles(id as u32);
            self.persist.uuids.remove(&(id as u32));
            if let Some(r) = self.remote.as_mut() {
                r.glowing.remove(&id);
                r.entities.remove(&id);
                r.passenger_vehicles.remove(&(id as u32));
                r.lerps.remove(&id);
                r.cart_lerps.remove(&id);
                r.stepped.remove(&id);
                r.head_lerps.remove(&id);
                r.pending_items.remove(&id);
            }
        }
        let keep = |id: u32| {
            if ids.len() > 1 {
                !removed.contains(&id)
            } else {
                ids.first().is_none_or(|&removed| id != removed as u32)
            }
        };
        self.mobs.retain(|m| keep(m.id));
        self.orbs.retain(|o| keep(o.id));
        self.dropped.retain(|d| keep(d.id));
        self.falling.retain(|f| keep(f.id));
        self.tnts.retain(|t| keep(t.id));
        if let Some(r) = self.remote.as_mut() {
            r.passenger_vehicles.retain(|_, v| keep(*v));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn remove_entities_detaches_only_removed_vehicles() {
        use crate::game::{
            Audio, Game, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
        };
        use horizon_protocol::packets::play::{self, Clientbound as C};
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
        options.execution_role = ExecutionRole::Client;
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
        let (client, _server) = horizon_protocol::connection::Pipe::pair();
        game.connect_local(client, "PassengerRemovalTest").unwrap();
        for (id, name) in [
            (40, "minecart"),
            (41, "minecart"),
            (42, "armor_stand"),
            (43, "zombie"),
            (44, "falling_block"),
            (45, "falling_block"),
            (46, "falling_block"),
        ] {
            let kind = game
                .remote
                .as_ref()
                .unwrap()
                .builtin
                .get("minecraft:entity_type")
                .unwrap()
                .id(&format!("minecraft:{name}"))
                .unwrap();
            game.handle_packet(C::AddEntity(play::AddEntity {
                id,
                uuid: id as u128,
                kind,
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
                data: 0,
            }))
            .unwrap();
        }
        game.remote_set_passengers(40, &[42]);
        game.remote_set_passengers(41, &[43]);
        assert_eq!(game.armor_stands[0].vehicle, Some(40));
        assert_eq!(game.mobs[0].extra.mount, Some(41));
        game.handle_packet(C::RemoveEntities(play::RemoveEntities {
            ids: vec![44, 40, 46],
        }))
        .unwrap();
        let r = game.remote.as_ref().unwrap();
        assert_eq!(r.entities[&42].vehicle, None);
        assert_eq!(game.armor_stands[0].vehicle, None);
        assert_eq!(r.entities[&43].vehicle, Some(41));
        assert_eq!(game.mobs[0].extra.mount, Some(41));
        assert!(game.mobs[0].extra.passenger);
        assert_eq!(r.passenger_vehicles.get(&42), None);
        assert_eq!(r.passenger_vehicles.get(&43), Some(&41));
        assert_eq!(game.falling.iter().map(|e| e.id).collect::<Vec<_>>(), [45]);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn remove_entities_filters_mixed_entity_lists() {
        use crate::game::{
            Audio, Game, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
        };
        use horizon_protocol::packets::play::{self, Clientbound as C};

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
        options.execution_role = ExecutionRole::Client;
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
        let (client, _server) = horizon_protocol::connection::Pipe::pair();
        game.connect_local(client, "RemovalTest").unwrap();
        for (id, kind) in [
            (40, "minecraft:falling_block"),
            (41, "minecraft:falling_block"),
            (42, "minecraft:falling_block"),
            (43, "minecraft:tnt"),
            (44, "minecraft:tnt"),
            (45, "minecraft:experience_orb"),
            (46, "minecraft:experience_orb"),
            (47, "minecraft:zombie"),
            (48, "minecraft:zombie"),
        ] {
            if matches!(kind, "minecraft:falling_block" | "minecraft:tnt") {
                assert!(game.spawn_remote_projectile(
                    id,
                    kind,
                    [0.0, 64.0, 0.0],
                    (0.0, 0.0),
                    [0.0; 3],
                    0
                ));
            } else {
                game.spawn_entity(id, kind, [0.0, 64.0, 0.0], [0.0; 3]);
            }
        }
        for id in [49, 50] {
            game.dropped.push(horizon_client::entities::ItemEntity::new(
                id,
                horizon_client::items::ItemStack::new("minecraft:sand", 1),
                horizon_core::geometry::Vec3::new(0.0, 64.0, 0.0),
                horizon_core::geometry::Vec3::ZERO,
                0.0,
            ));
        }
        game.handle_packet(C::RemoveEntities(play::RemoveEntities {
            ids: vec![40, 42, 43, 45, 47, 49],
        }))
        .unwrap();
        assert_eq!(game.falling.iter().map(|e| e.id).collect::<Vec<_>>(), [41]);
        assert_eq!(game.tnts.iter().map(|e| e.id).collect::<Vec<_>>(), [44]);
        assert_eq!(game.orbs.iter().map(|e| e.id).collect::<Vec<_>>(), [46]);
        assert_eq!(game.mobs.iter().map(|e| e.id).collect::<Vec<_>>(), [48]);
        assert_eq!(game.dropped.iter().map(|e| e.id).collect::<Vec<_>>(), [50]);
    }
}
