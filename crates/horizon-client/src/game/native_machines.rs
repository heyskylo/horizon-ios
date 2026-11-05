//! What a mod's machine does to the world as a player's hand would, with no
//! player: break a block with a tool and keep what drops (a drill, a saw, a
//! harvester), show its crack while it works, and use an item as a player
//! would (a deployer) (docs/native-mods.md, "Machines").
use super::{Game, GuestUse, Hit, InteractTarget, Screen};
use horizon_client::{
    command::args::GameMode,
    first_person::Hand,
    items::ItemStack,
    mod_api::{NativeFakeUse, NativeUseTarget},
};
use horizon_core::{geometry::Vec3, position::BlockPos};
use winit::keyboard::KeyCode;

/// `Player.getStandingEyeHeight`: a fake player's eyes above its feet.
const FAKE_EYE: f32 = 1.62;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
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
    /// in its main hand and nothing else, and has no connection: no screen
    /// it opens stays open, and its stats and advancements count for no
    /// one. The answer is what it holds and carries afterwards. None, with
    /// nothing used, off a server and while a player's action is being run
    /// (from inside a player's hook: run it from a block entity's tick).
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
        self.block_reach = self.interaction_range("minecraft:block_interaction_range", 4.5, 5.0);
        self.entity_reach = self.interaction_range("minecraft:entity_interaction_range", 3.0, 5.0);

        let consumed = matches!(self.use_item(), Ok(Some(_))) && !self.use_failed;

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
        Some(NativeFakeUse {
            consumed,
            held,
            other,
        })
    }
}
