//! Entity and level handles for native mods (docs/native-mods.md, "Entity
//! and level handles", #7387): `native_entity(id)` turns an event's entity id
//! into an `EntityRef`, and each `native_entity_*` helper reads the entity a
//! handle names. On a server the entities are those of the level in place
//! (the one a hook runs in) and the server's players, whatever their level.
//! On a client (API @API:client_entity_refs@) they are the entities the
//! server shows this window, read from what it synced: positions, rotations,
//! types, UUIDs, custom names and equipment, never saved data or tags. With
//! no world open, or for a handle of an earlier open (on a client, of an
//! earlier client level), they return `None`.
use super::{Dim, Game, commands::Target};
use horizon_client::mod_api::{EntityRef, LevelRef};

impl Game<'_> {
    /// What a handle names now, or None for a stale or foreign one.
    pub(super) fn native_entity_target(&self, entity: EntityRef) -> Option<Target> {
        // Client handles preserve the signed VarInt entity ID's bits (NPC
        // plugins also use negative IDs). Server-generated IDs stay checked.
        let id = if self.remote.is_some() {
            entity.id() as i32
        } else {
            i32::try_from(entity.id()).ok()?
        };
        if let Some(r) = &self.remote {
            if r.native_epoch() != Some(entity.epoch()) {
                return None;
            }
            // This window's own player is the one in place.
            if id == r.entity_id {
                return self.player.as_ref().map(|_| Target::Player);
            }
        } else if self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return None;
        }
        let player = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .find(|g| g.placed && g.entity_id == id);
        if let Some(g) = player {
            // `as_guest` swaps the acting player's state into the game.
            let acting = self.lan_acting == Some(g.conn) && self.player.is_some();
            return Some(if acting {
                Target::Player
            } else {
                Target::Guest(g.conn)
            });
        }
        match self.level_entity(id)? {
            Target::Mob(id) if !self.mobs.iter().any(|m| m.id == id && !m.removed) => None,
            // The mod API has no natural fence knots (#13036 adds them for
            // commands only).
            Target::Knot(_) => None,
            t => Some(t),
        }
    }
    /// An entity a client tracks that no entity list of the game holds (a
    /// display, an interaction, a marker, a leash knot): its type and its
    /// position as the server last sent it. None on a server.
    fn native_client_tracked(&self, entity: EntityRef) -> Option<(&str, [f64; 3])> {
        let r = self.remote.as_ref()?;
        if r.native_epoch() != Some(entity.epoch()) {
            return None;
        }
        r.tracked_entity(entity.id() as i32)
    }
    /// The entity `id` (the id events carry) as a handle for the `entity_*`
    /// methods: None with no world open, or for no such entity in the level
    /// in place or among the server's players. On a client (API
    /// @API:client_entity_refs@), any entity the server shows this window,
    /// this window's own player included. API 373.
    pub(super) fn native_entity(&self, id: u32) -> Option<EntityRef> {
        let epoch = match &self.remote {
            Some(r) => r.native_epoch()?,
            None => self.native_lifecycle.epoch()?,
        };
        let entity = EntityRef::new(epoch, id);
        (self.native_entity_target(entity).is_some()
            || self.native_client_tracked(entity).is_some())
        .then_some(entity)
    }
    /// The level `dimension` (`minecraft:the_nether`) as a handle: None on a
    /// client, with no world open, or for a dimension the server hasn't
    /// loaded. API 373.
    pub(super) fn native_level(&self, dimension: &str) -> Option<LevelRef> {
        if self.remote.is_some() {
            return None;
        }
        let epoch = self.native_lifecycle.epoch()?;
        let dim = std::iter::once(self.dim)
            .chain(self.levels.parked_dims())
            .find(|d| d.id() == dimension)?;
        Some(LevelRef::new(epoch, dim.id()))
    }
    /// The entity's position (`Entity.position`): its feet, the middle of
    /// its box for a painting or an item frame. On a client, where the
    /// window shows it (another entity eases toward the server's moves); an
    /// entity tracked without a list of its own is where the server last put
    /// it. API 373.
    pub(super) fn native_entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        match self.native_entity_target(entity) {
            Some(t) => self.target_position(t),
            None => self.native_client_tracked(entity).map(|(_, at)| at),
        }
    }
    /// The block the entity's position is in (`Entity.blockPosition`). API
    /// 373.
    pub(super) fn native_entity_block_position(&self, entity: EntityRef) -> Option<[i32; 3]> {
        self.native_entity_position(entity)
            .map(|p| p.map(|v| v.floor() as i32))
    }
    /// The entity's yaw and pitch in degrees (`Entity.getYRot`, `getXRot`); 0
    /// for the kinds that don't turn. API 373.
    pub(super) fn native_entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        Some(self.target_rotation(self.native_entity_target(entity)?))
    }
    /// The entity's type (`Entity.getType`): `minecraft:zombie`. API
    /// 373.
    pub(super) fn native_entity_kind(&self, entity: EntityRef) -> Option<String> {
        match self.native_entity_target(entity) {
            Some(t) => Some(self.target_type(t)),
            None => self
                .native_client_tracked(entity)
                .map(|(kind, _)| kind.to_owned()),
        }
    }
    /// The entity's UUID (`Entity.getUUID`), the one it is saved with; an
    /// entity that has none yet gets it now, as a new vanilla entity does.
    /// On a client, the one the server added it with (a player's is its
    /// profile's). API 373.
    pub(super) fn native_entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
        if let Some(r) = &self.remote {
            let id = entity.id() as i32;
            if self.native_entity_target(entity).is_none()
                && self.native_client_tracked(entity).is_none()
            {
                return None;
            }
            if let Some((uuid, _)) = r.level_player(id) {
                return Some(uuid);
            }
            return self
                .persist
                .uuids
                .get(&entity.id())
                .copied()
                .map(uuid_from_words);
        }
        let words = match self.native_entity_target(entity)? {
            Target::Player => return self.command_guest(self.lan_acting?).map(|g| g.uuid),
            Target::Guest(conn) => return self.command_guest(conn).map(|g| g.uuid),
            _ => self.uuid_for(entity.id()),
        };
        Some(uuid_from_words(words))
    }
    /// The level the entity is in (`Entity.level`): on a client, the
    /// client's level. API 373.
    pub(super) fn native_entity_level(&self, entity: EntityRef) -> Option<LevelRef> {
        let dim: Dim = match self.native_entity_target(entity) {
            Some(t) => self.target_level(t),
            None => self.native_client_tracked(entity).map(|_| self.dim)?,
        };
        Some(LevelRef::new(entity.epoch(), dim.id()))
    }
}

/// A saved UUID's four ints (`UUIDUtil.uuidFromIntArray`) as one number.
fn uuid_from_words(words: [i32; 4]) -> u128 {
    words
        .iter()
        .fold(0, |uuid, w| (uuid << 32) | u128::from(*w as u32))
}

#[cfg(test)]
mod tests {
    #[test]
    fn uuid_words_are_most_significant_first() {
        let words = [0x0102_0304, -1, 0, 0x7fff_ffff];
        assert_eq!(
            super::uuid_from_words(words),
            0x0102_0304_ffff_ffff_0000_0000_7fff_ffff
        );
    }
}

/// A window joined to a server through memory, fed the server's packets as
/// it would receive them: what its native mods read of the entities and the
/// menu the server showed it.
#[cfg(test)]
mod client_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::mod_api::EquipmentSlot;
    use horizon_protocol::entity_data::Value;
    use horizon_protocol::item::{Component, Stack, component};
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use horizon_world::nbt::Tag;

    fn spawn(dimension: &str) -> play::SpawnInfo {
        play::SpawnInfo {
            dimension_type: 0,
            dimension: dimension.into(),
            seed: 1,
            game_type: 0,
            previous_game_type: None,
            debug: false,
            flat: false,
            last_death: None,
            portal_cooldown: 0,
            sea_level: 63,
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_client_reads_the_entities_and_menu_the_server_shows_it() {
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
        let (pipe, _server) = horizon_protocol::connection::Pipe::pair();
        game.connect_local(pipe, "Steve").unwrap();
        let builtin = game.remote.as_ref().unwrap().builtin.clone();
        let id = |registry: &str, name: &str| builtin.get(registry).unwrap().id(name).unwrap();
        let item = |name: &str| id("minecraft:item", name);
        let add = |entity: i32, uuid: u128, kind: &str, position: [f64; 3]| {
            C::AddEntity(play::AddEntity {
                id: entity,
                uuid,
                kind: id("minecraft:entity_type", kind),
                position,
                velocity: [0.0; 3],
                pitch: 0,
                yaw: 64,
                head_yaw: 64,
                data: 0,
            })
        };
        // No client level yet: no handle.
        assert_eq!(game.native_entity(20), None);
        game.handle_packet(C::Login(play::Login {
            player_id: 7,
            hardcore: false,
            levels: vec!["minecraft:overworld".into(), "minecraft:the_nether".into()],
            max_players: 20,
            chunk_radius: 8,
            simulation_distance: 8,
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: spawn("minecraft:overworld"),
            online_mode: false,
            enforces_secure_chat: false,
        }))
        .unwrap();

        // The window's own player, once the server's first teleport made it.
        game.handle_packet(C::PlayerPosition(play::PlayerPosition {
            teleport: 1,
            position: [0.5, 65.0, 0.5],
            delta: [0.0; 3],
            yaw: 45.0,
            pitch: 10.0,
            relatives: 0,
        }))
        .unwrap();
        let me = game.native_entity(7).expect("the own player");
        assert_eq!(
            game.native_entity_kind(me).as_deref(),
            Some("minecraft:player")
        );
        assert_eq!(game.native_entity_position(me), Some([0.5, 65.0, 0.5]));
        assert_eq!(game.native_entity_rotation(me), Some([45.0, 10.0]));
        // The profile the login named (the net thread's `Joined`).
        game.remote.as_mut().unwrap().profile_name = "Steve".into();
        assert_eq!(game.native_entity_name(me).as_deref(), Some("Steve"));

        // An armor stand with a custom name and a helmet.
        let uuid = 0x0011_2233_4455_6677_8899_aabb_ccdd_eeff;
        game.handle_packet(add(20, uuid, "minecraft:armor_stand", [1.5, 64.0, 2.5]))
            .unwrap();
        game.handle_packet(C::SetEntityData(play::SetEntityData {
            id: 20,
            values: vec![(2, 6, Value::OptText(Some(Tag::String("Greeter".into()))))],
        }))
        .unwrap();
        game.handle_packet(C::SetEquipment(play::SetEquipment {
            id: 20,
            slots: vec![(5, Some(Stack::new(item("minecraft:diamond_helmet"), 1)))],
        }))
        .unwrap();
        let stand = game.native_entity(20).expect("the stand is tracked");
        assert_eq!(
            game.native_entity_kind(stand).as_deref(),
            Some("minecraft:armor_stand")
        );
        assert_eq!(game.native_entity_position(stand), Some([1.5, 64.0, 2.5]));
        assert_eq!(game.native_entity_block_position(stand), Some([1, 64, 2]));
        assert_eq!(game.native_entity_rotation(stand), Some([90.0, 0.0]));
        assert_eq!(game.native_entity_uuid(stand), Some(uuid));
        assert_eq!(game.native_entity_name(stand).as_deref(), Some("Greeter"));
        let living = game.native_living(stand).expect("an armor stand is living");
        let head = game.native_living_equipment(living, EquipmentSlot::Head);
        assert_eq!(
            head.map(|s| s.item.to_string()).as_deref(),
            Some("minecraft:diamond_helmet")
        );
        assert_eq!(
            game.native_living_equipment(living, EquipmentSlot::Chest),
            None
        );
        // A client knows no saved data or tags, and changes nothing itself.
        assert_eq!(game.native_entity_nbt(stand), None);
        assert_eq!(game.native_entity_tags(stand), None);
        assert!(!game.native_entity_add_tag(stand, "seen"));
        assert!(!game.native_living_set_equipment(living, EquipmentSlot::Head, None));
        assert!(
            game.native_living_equipment(living, EquipmentSlot::Head)
                .is_some()
        );
        assert!(!game.native_entity_set_velocity(stand, [0.0, 1.0, 0.0]));

        // An entity the client keeps no list of its own for: its type and
        // where the server put it.
        game.handle_packet(add(21, 5, "minecraft:text_display", [4.0, 70.0, -3.0]))
            .unwrap();
        let display = game.native_entity(21).expect("the display is tracked");
        assert_eq!(
            game.native_entity_kind(display).as_deref(),
            Some("minecraft:text_display")
        );
        assert_eq!(
            game.native_entity_position(display),
            Some([4.0, 70.0, -3.0])
        );
        assert_eq!(game.native_entity_uuid(display), Some(5));
        assert_eq!(game.native_entity(99), None);

        // A chest the server opened, by its slot numbers: the chest's 9,
        // then the inventory's 27, then the hotbar's 9.
        assert_eq!(game.native_menu_stacks(), None);
        game.handle_packet(C::OpenScreen(play::OpenScreen {
            container: 3,
            menu: id("minecraft:menu", "minecraft:generic_9x1"),
            title: Tag::String("Loot".into()),
        }))
        .unwrap();
        let mut named = Stack::new(item("minecraft:diamond"), 2);
        named.added.push((
            component::CUSTOM_NAME,
            Component::Text(Tag::String("Gem".into())),
        ));
        let mut items = vec![None; 45];
        items[0] = Some(named);
        items[36] = Some(Stack::new(item("minecraft:stone"), 5));
        game.handle_packet(C::ContainerSetContent(play::ContainerSetContent {
            container: 3,
            state: 1,
            items,
            carried: None,
        }))
        .unwrap();
        let stacks = game.native_menu_stacks().expect("a server menu is open");
        assert_eq!(stacks.len(), 45);
        let gem = stacks[0].as_ref().expect("the chest's first slot");
        assert_eq!((&*gem.item, gem.count), ("minecraft:diamond", 2));
        assert_eq!(gem.custom_name().as_deref(), Some("Gem"));
        assert!(stacks[1].is_none());
        let stone = stacks[36].as_ref().expect("the hotbar's first slot");
        assert_eq!((&*stone.item, stone.count), ("minecraft:stone", 5));
        // The hotbar's first slot is the player's inventory slot 0.
        assert_eq!(game.inventory[0].as_ref().map(|s| s.count), Some(5));
        // The player's own inventory answers the same with the menu up: the
        // hotbar is slots 0-8, the chest's slots are not part of it.
        let own = game.native_own_inventory().expect("a client has one");
        assert_eq!(own.len(), 36);
        let stone = own[0].as_ref().expect("the hotbar's first slot");
        assert_eq!((&*stone.item, stone.count), ("minecraft:stone", 5));
        assert!(own[1..].iter().all(Option::is_none));

        // A new client level: the handles of the old one read as None.
        game.handle_packet(C::Respawn(play::Respawn {
            spawn: spawn("minecraft:the_nether"),
            keep: 0,
        }))
        .unwrap();
        assert_eq!(game.native_entity_position(stand), None);
        assert_eq!(game.native_entity_kind(display), None);
    }

    /// #8589: the server sends entity event 42 (the raid sweat) for a villager;
    /// the client must draw the particles, as it does for the events it let
    /// through before. An event with no particle table row (2) draws nothing.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_villager_raid_sweat_event_draws_particles() {
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
        let (pipe, _server) = horizon_protocol::connection::Pipe::pair();
        game.connect_local(pipe, "Steve").unwrap();
        let builtin = game.remote.as_ref().unwrap().builtin.clone();
        let kind = builtin
            .get("minecraft:entity_type")
            .unwrap()
            .id("minecraft:villager")
            .unwrap();
        game.handle_packet(C::Login(play::Login {
            player_id: 7,
            hardcore: false,
            levels: vec!["minecraft:overworld".into()],
            max_players: 20,
            chunk_radius: 8,
            simulation_distance: 8,
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: spawn("minecraft:overworld"),
            online_mode: false,
            enforces_secure_chat: false,
        }))
        .unwrap();
        game.handle_packet(C::PlayerPosition(play::PlayerPosition {
            teleport: 1,
            position: [0.5, 65.0, 0.5],
            delta: [0.0; 3],
            yaw: 45.0,
            pitch: 10.0,
            relatives: 0,
        }))
        .unwrap();
        game.handle_packet(C::AddEntity(play::AddEntity {
            id: 21,
            uuid: 0x0011_2233_4455_6677_8899_aabb_ccdd_eeff,
            kind,
            // Within 32 blocks of the camera, which starts at the origin: the
            // particle engine drops a particle farther away than that.
            position: [1.5, 1.0, 2.5],
            velocity: [0.0; 3],
            pitch: 0,
            yaw: 64,
            head_yaw: 64,
            data: 0,
        }))
        .unwrap();
        // `len` counts only particles already ticked in; `made` counts every
        // particle created, which is what the event handler does at once.
        let made = |g: &Game| g.particles.made().values().sum::<u64>();
        let before = made(&game);
        game.handle_packet(C::EntityEvent(play::EntityEvent { id: 21, event: 2 }))
            .unwrap();
        assert_eq!(made(&game), before, "event 2 has no particle row");
        game.handle_packet(C::EntityEvent(play::EntityEvent { id: 21, event: 42 }))
            .unwrap();
        assert!(made(&game) > before, "event 42 draws the sweat");
    }
}
