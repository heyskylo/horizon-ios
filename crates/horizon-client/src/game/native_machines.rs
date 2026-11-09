//! What a mod's machine does to the world as a player's hand would, with no
//! player: break a block with a tool and keep what drops (a drill, a saw, a
//! harvester), show its crack while it works, and use an item as a player
//! would (a deployer) (docs/native-mods.md, "Machines").
use super::{Game, GuestUse, Hit, InteractTarget, Screen};
use horizon_client::{
    command::args::GameMode,
    containers::BlockEntity,
    first_person::Hand,
    items::ItemStack,
    mod_api::{NativeFakeUse, NativeUseTarget},
};
use horizon_core::{geometry::Vec3, position::BlockPos};
use winit::keyboard::KeyCode;

/// `Player.getStandingEyeHeight`: a fake player's eyes above its feet.
const FAKE_EYE: f32 = 1.62;

// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// Breaks the block at `pos` as a machine does, with no player: vanilla's
    /// `Level.destroyBlock(pos, true)` with the loot rolled for `tool`
    /// (`Block.dropResources(state, level, pos, blockEntity, null, tool)`):
    /// the break particles and sound, the block's drops with what its block
    /// entity copies into them (a shulker box's contents), experience for an
    /// ore unless `tool` has silk touch (`EnchantmentHelper.processBlockExperience`),
    /// then the block's fluid in its place. Blocks that pop off because of
    /// the break drop as with no tool, and pop whatever `collect` says. `tool` picks
    /// the drops as a player's held item does (silk touch, fortune, shears);
    /// `None` is an empty hand. The mods' say on drops (`native_loot_drops`)
    /// applies. With `collect`, the drops are returned and nothing is popped
    /// (a harvester storing them); without, they pop as item entities and the
    /// list is empty. Nothing drops where `doTileDrops` is off. None, with
    /// nothing broken, for air, an unloaded block and off a server.
    /// API @API:machine_breaking@.
    pub(super) fn native_machine_destroy_block(
        &mut self,
        pos: [i32; 3],
        tool: Option<&ItemStack>,
        collect: bool,
    ) -> Option<Vec<ItemStack>> {
        if !self.native_authoritative() || !self.world.loaded(pos[0], pos[2]) {
            return None;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if self.res.behaviours().is_air(state) {
            return None;
        }
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        let tool_kept = tool.cloned();
        let mut drops = self.with_level(|b, level, ticks| {
            level.drop_tool = Some(((pos[0], pos[1], pos[2]), tool_kept));
            level.drop_into = Some(Vec::new());
            b.destroy_block(level, ticks, at, true);
            level.drop_tool = None;
            level.drop_into.take().unwrap_or_default()
        });
        let _ = self.process_level_events();
        self.native_block_drops(pos, state, None, tool, None, &mut drops);
        if collect {
            return Some(drops);
        }
        for stack in drops {
            self.pop_resource(pos, stack);
        }
        Some(Vec::new())
    }

    /// `ServerLevel.destroyBlockProgress`: shows breaker `breaker`'s crack on
    /// the block at `pos` at stage `progress` (0 to 9) to the players within 64
    /// blocks, as a zombie breaking a door shows its; any other stage takes the
    /// crack away. A machine passes an id of its own that no entity has (a
    /// negative one), the same each time, so a new stage replaces its old one;
    /// a crack not renewed for 400 ticks fades. Off a server nothing happens.
    /// API @API:machine_breaking@.
    pub(super) fn native_set_break_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {
        if !self.native_authoritative() {
            return;
        }
        self.server_destroy_block_progress(breaker, pos, progress);
    }

    /// Uses `stack` as a player standing with its eyes at `eye`, looking
    /// `yaw` and `pitch` degrees (as `Entity.getYRot` and `getXRot`), would:
    /// NeoForge's `FakePlayer` doing what a deployer does. On a block
    /// (`NativeUseTarget::Block`) it is `ServerPlayerGameMode.useItemOn`:
    /// the block reacts first (a lever flips, a cauldron fills), else the
    /// item is used on it (a block placed, bone meal spread, a bucket
    /// filled); in the air (`Item`) `useItem` (a bucket picks up the fluid it
    /// looks at); on an entity (`Entity`) `Player.interactOn` (a sheep
    /// sheared, a cow milked). With `sneaking` it is a secondary use, as a
    /// player holding sneak's. The fake player is in survival, holds `stack`
    /// in its main hand and nothing else, and has no connection: what it
    /// opens closes again through the real close paths before this returns
    /// (a lid opens and shuts, a trader is free again), and its stats and
    /// advancements count for no one. The click still counts as used, as a
    /// player's would. The answer is what it holds and carries afterwards.
    /// None, with nothing used, off a server and while a player's action is
    /// being run (from inside a player's hook: run it from a block entity's
    /// tick).
    /// API @API:fake_player@.
    pub(super) fn native_use_as_player(
        &mut self,
        stack: ItemStack,
        eye: [f64; 3],
        yaw: f32,
        pitch: f32,
        target: NativeUseTarget,
        sneaking: bool,
    ) -> Option<NativeFakeUse> {
        if !self.native_authoritative()
            || self.remote.is_some()
            || self.player.is_some()
            || self.lan_acting.is_some()
            || !eye.iter().all(|v| v.is_finite())
            || !(yaw.is_finite() && pitch.is_finite())
        {
            return None;
        }
        let feet = Vec3::new(eye[0], eye[1] - f64::from(FAKE_EYE), eye[2]);
        let body = horizon_client::motion::PlayerMotion::new(self.motion, feet).ok()?;
        let slots = self.inventory.len().max(36);
        let mut inventory = vec![None; slots];
        inventory[0] = (stack.count > 0).then_some(stack);
        let keys = if sneaking {
            [KeyCode::ShiftLeft].into_iter().collect()
        } else {
            Default::default()
        };
        let (forced_hit, guest_use) = match target {
            NativeUseTarget::Block { pos, face, point } => (
                Some(Hit {
                    pos,
                    face: face.normal(),
                    point,
                }),
                GuestUse::OnBlock(Hand::Main),
            ),
            NativeUseTarget::Item => (None, GuestUse::Item(Hand::Main)),
            NativeUseTarget::Entity { id, location } => (
                None,
                GuestUse::Entity(Hand::Main, InteractTarget { id, location }),
            ),
        };
        // The server's own player state, empty while nobody acts, kept to put
        // back: the fake player leaves nothing of itself behind.
        let player = self.player.replace(body);
        let eye_height = std::mem::replace(&mut self.eye, FAKE_EYE);
        let look = (
            std::mem::replace(&mut self.yaw, yaw),
            std::mem::replace(&mut self.pitch, pitch),
        );
        let inventory = std::mem::replace(&mut self.inventory, inventory);
        let selected = std::mem::replace(&mut self.selected, 0);
        let armor = std::mem::take(&mut self.armor);
        let offhand = self.offhand.take();
        let carried = self.carried.take();
        let keys = std::mem::replace(&mut self.keys, keys);
        let screen = std::mem::replace(&mut self.screen, Screen::Playing);
        let container = self.container.take();
        let sign_edit = self.sign_edit.take();
        let book = self.book.take();
        let command_block = self.command_block_screen.take();
        let mount_menu = std::mem::take(&mut self.mount_menu);
        let craft_size = std::mem::replace(&mut self.craft_size, 2);
        let craft_grid = std::mem::replace(&mut self.craft_grid, vec![None; 9]);
        let mode = std::mem::replace(&mut self.mode, GameMode::Survival);
        let creative = std::mem::replace(&mut self.creative, false);
        let mining = self.mining.take();
        let using = self.using.take();
        let use_delay = std::mem::replace(&mut self.use_delay, 0);
        let cooldowns = std::mem::take(&mut self.cooldowns);
        let riding = self.riding.take();
        let sleeping = self.sleeping.take();
        let reach = (self.block_reach, self.entity_reach);
        let hit = std::mem::replace(&mut self.forced_hit, forced_hit);
        let tick_hit = self.tick_hit.take();
        let used_as = self.guest_use.replace(guest_use);
        let failed = std::mem::replace(&mut self.use_failed, false);
        let station = self.station.take();
        let craft_table = self.craft_table.take();
        let experience = std::mem::take(&mut self.experience);
        let hook = self.hook.take();
        let respawn = (
            self.respawn_config.take(),
            self.spawn.take(),
            self.level.spawn,
        );
        // An ender inventory left open goes back while the fake player acts,
        // and its stand-in returns afterwards, as a guest's turn does.
        let ender_open = self.ender_open;
        self.close_ender_chest();
        self.block_reach = self.interaction_range("minecraft:block_interaction_range", 4.5, 5.0);
        self.entity_reach = self.interaction_range("minecraft:entity_interaction_range", 3.0, 5.0);

        let consumed = matches!(self.use_item(), Ok(Some(_))) && !self.use_failed;
        // The close side of what the use opened, with the fake inventory still
        // in place: what a menu holds comes back to the fake player.
        self.close_container();
        self.close_book();

        let mut left = std::mem::replace(&mut self.inventory, inventory);
        let held = left
            .get_mut(0)
            .and_then(Option::take)
            .filter(|s| s.count > 0);
        let other = left
            .into_iter()
            .chain([self.offhand.take(), self.carried.take()])
            .chain(std::mem::take(&mut self.armor))
            .flatten()
            .filter(|s| s.count > 0)
            .collect();
        self.player = player;
        self.eye = eye_height;
        (self.yaw, self.pitch) = look;
        self.selected = selected;
        self.armor = armor;
        self.offhand = offhand;
        self.carried = carried;
        self.keys = keys;
        self.screen = screen;
        self.container = container;
        self.sign_edit = sign_edit;
        self.book = book;
        self.command_block_screen = command_block;
        self.mount_menu = mount_menu;
        self.craft_size = craft_size;
        self.craft_grid = craft_grid;
        self.mode = mode;
        self.creative = creative;
        self.mining = mining;
        self.using = using;
        self.use_delay = use_delay;
        self.cooldowns = cooldowns;
        self.riding = riding;
        self.sleeping = sleeping;
        (self.block_reach, self.entity_reach) = reach;
        self.forced_hit = hit;
        self.tick_hit = tick_hit;
        self.guest_use = used_as;
        self.use_failed = failed;
        self.station = station;
        self.craft_table = craft_table;
        self.experience = experience;
        // A hook this use cast has not ticked and no client has seen it.
        self.hook = hook;
        (self.respawn_config, self.spawn, self.level.spawn) = respawn;
        if let Some(key) = ender_open {
            self.world
                .block_entities
                .insert(key, BlockEntity::Chest(self.ender_items.clone()));
            self.ender_open = Some(key);
        }
        Some(NativeFakeUse {
            consumed,
            held,
            other,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{
        Audio, Options, Progress, Settings,
        boats::Boat,
        load_resources,
        minecarts::{CART_KEY_Y, CartKind, Minecart},
        respawn_anchor::RespawnConfig,
        stations::{Kind, Station},
    };
    use super::*;
    use horizon_client::chunk::Chunk;
    use horizon_core::geometry::Direction;
    use std::sync::Arc;

    /// A server's game on the prepared resources: nobody is playing, and the
    /// chunk at the origin is loaded.
    pub(crate) fn server_game() -> Game<'static> {
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES")
                .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
        );
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
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let res: &'static _ = Box::leak(Box::new(loaded.resources));
        let mut game = Game::new(
            res,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.options.dedicated = Some(Vec::new());
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game
    }

    fn fake_use(game: &mut Game<'_>, target: NativeUseTarget, sneaking: bool) -> NativeFakeUse {
        game.native_use_as_player(
            ItemStack::new("minecraft:stick", 1),
            [8.5, 66.0, 8.5],
            0.0,
            0.0,
            target,
            sneaking,
        )
        .expect("a server with nobody acting")
    }
    fn on_entity(id: u32) -> NativeUseTarget {
        NativeUseTarget::Entity {
            id: id as i32,
            location: [0.0; 3],
        }
    }
    /// Clicks the top of the block at `pos`.
    fn click(game: &mut Game<'_>, pos: [i32; 3]) -> NativeFakeUse {
        let point = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 1.0,
            f64::from(pos[2]) + 0.5,
        ];
        let target = NativeUseTarget::Block {
            pos,
            face: Direction::Up,
            point,
        };
        fake_use(game, target, false)
    }
    /// Puts `block` at `pos` and clicks it.
    fn click_new(game: &mut Game<'_>, block: &str, pos: [i32; 3]) -> NativeFakeUse {
        let state = game
            .pack
            .assets
            .registry
            .default_state(block)
            .expect("a vanilla block");
        game.world.set(pos[0], pos[1], pos[2], state).unwrap();
        click(game, pos)
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_trader_a_fake_player_used_trades_with_the_next_one() {
        let mut game = server_game();
        let id = game
            .spawn_mob("minecraft:wandering_trader", [8.5, 65.0, 8.5], true)
            .expect("a wandering trader");
        assert!(fake_use(&mut game, on_entity(id), false).consumed);
        let trader = game.mobs.iter().find(|m| m.id == id).unwrap();
        // With no offers the menu never opens, and this test would say nothing.
        let offers = trader.extra.trade.as_ref().map_or(0, |t| t.offers.len());
        assert!(offers > 0, "a wandering trader has offers");
        assert!(!trader.extra.flag2, "the trader is not left trading");
        assert!(game.station.is_none(), "the session is not left open");
        assert!(
            fake_use(&mut game, on_entity(id), false).consumed,
            "the next use opens it again",
        );
    }

    /// A chest minecart, a hopper minecart and a chest boat: the items stay on
    /// the vehicle, and the menu's stand-in is out of the level, where a
    /// hopper transfer would edit it instead of the vehicle.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_vehicles_chest_a_fake_player_used_keeps_its_items_and_no_stand_in() {
        let mut game = server_game();
        let at = Vec3::new(8.5, 65.0, 8.5);
        let diamonds = Some(ItemStack::new("minecraft:diamond", 5));
        let mut vehicles = Vec::new();
        for kind in [CartKind::Chest, CartKind::Hopper] {
            game.next_entity += 1;
            let mut cart = Minecart::new(game.next_entity, kind, at);
            cart.items[3] = diamonds.clone();
            vehicles.push((game.next_entity, false));
            game.carts.push(cart);
        }
        game.next_entity += 1;
        let mut boat = Boat::new(game.next_entity, "oak".into(), at, 0.0).with_chest();
        boat.items[3] = diamonds.clone();
        // ChestBoat.interact opens the chest for a sneaking player.
        vehicles.push((game.next_entity, true));
        game.boats.push(boat);

        for (id, sneaking) in vehicles {
            // Twice: the second use is the next one finding it as it was.
            for _ in 0..2 {
                assert!(fake_use(&mut game, on_entity(id), sneaking).consumed);
                let key = (i32::MAX - 15, CART_KEY_Y, id as i32);
                assert!(
                    game.world.block_entities.get(&key).is_none(),
                    "vehicle {id} left a stand-in",
                );
                assert!(game.container.is_none());
                let items = game
                    .carts
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| &c.items)
                    .or_else(|| game.boats.iter().find(|b| b.id == id).map(|b| &b.items))
                    .unwrap();
                assert_eq!(items[3], diamonds, "vehicle {id} lost its items");
            }
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_shulker_box_and_an_ender_chest_a_fake_player_used_are_left_shut() {
        let mut game = server_game();
        let shulker = [8, 64, 8];
        let count = |game: &Game<'_>| game.be_anims.shulkers.get(&shulker).map(|s| s.open_count);
        assert!(click_new(&mut game, "minecraft:shulker_box", shulker).consumed);
        assert_eq!(count(&game), Some(0), "the use opened it, and it shut");
        assert!(game.container.is_none());
        assert!(click(&mut game, shulker).consumed, "the next use opens it");
        assert_eq!(count(&game), Some(0), "the count doesn't climb");

        let ender = [10, 64, 8];
        let key = (ender[0], ender[1], ender[2]);
        let diamonds = Some(ItemStack::new("minecraft:diamond", 5));
        game.ender_items[0] = diamonds.clone();
        assert!(click_new(&mut game, "minecraft:ender_chest", ender).consumed);
        assert!(game.ender_open.is_none());
        // An ender chest keeps its block entity (`EnderChestBlock.newBlockEntity`), but
        // `EnderChestBlockEntity` holds no items: the player's own are not left in it.
        match game.world.block_entities.get(&key) {
            Some(BlockEntity::Chest(items)) => {
                assert!(
                    items.iter().all(Option::is_none),
                    "the player's items stayed"
                );
            }
            other => panic!("the ender chest lost its block entity: {other:?}"),
        }
        assert!(!game.lid_openers.contains_key(&key), "its lid stayed open");
        assert_eq!(
            game.ender_items[0], diamonds,
            "the server's own ender items changed"
        );
    }

    /// Each field holds a value the use can't have written, so a swap that
    /// forgets one fails whether the use changed it or not.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_servers_own_player_state_comes_back_after_a_fake_use() {
        let mut game = server_game();
        let config = RespawnConfig {
            dim: game.dim,
            pos: [1, 2, 3],
            yaw: 0.0,
            pitch: 0.0,
            forced: false,
        };
        let spawn = Vec3::new(1.0, 2.0, 3.0);
        game.station = Some(Station::new_kind(Kind::Anvil, [4, 5, 6]));
        game.craft_table = Some([7, 8, 9]);
        game.experience.level = 7;
        game.respawn_config = Some(config);
        game.spawn = Some(spawn);
        let stand_in = (12, 64, 8);
        game.open_ender_chest(stand_in);
        game.container = None;

        assert!(click_new(&mut game, "minecraft:crafting_table", [8, 64, 8]).consumed);
        assert_eq!(
            game.station.as_ref().map(|s| (s.kind, s.pos)),
            Some((Kind::Anvil, [4, 5, 6])),
        );
        assert_eq!(game.craft_table, Some([7, 8, 9]));
        assert_eq!(game.experience.level, 7);
        assert_eq!(game.respawn_config, Some(config));
        assert_eq!(game.spawn, Some(spawn));
        assert_eq!(game.ender_open, Some(stand_in));
        assert!(game.world.block_entities.get(&stand_in).is_some());

        let rod = game
            .native_use_as_player(
                ItemStack::new("minecraft:fishing_rod", 1),
                [8.5, 66.0, 8.5],
                0.0,
                0.0,
                NativeUseTarget::Item,
                false,
            )
            .expect("a server with nobody acting");
        assert!(rod.consumed, "the rod casts");
        assert!(game.hook.is_none(), "no hook is left for nobody to reel in");
    }
}
