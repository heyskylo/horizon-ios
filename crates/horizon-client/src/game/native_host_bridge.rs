//! `Host` forms of documented `Game` helpers that `dyn Host` could not carry:
//! the generic `native_with_mod_block_entity`, the level services only the
//! level hosts had (`play_sound`, `send_particles`), and what a client mod
//! reads of its own player and of the block entities it draws
//! (docs/native-mods.md, "Trait mods"). Each goes through the helper or the
//! state the game's own code uses, so both answer alike.
use super::Game;
use horizon_client::actions::Action;
use horizon_client::{
    containers::{BlockEntity, ModBlockEntity},
    items::ItemStack,
    particles::ParticleOptions,
};
use horizon_world::{player_inventory::EquipmentSlot, registry::BlockStateId};

// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// A copy of the mod block entity at `pos`, to read without changing it:
    /// a press reading the basin under it, a client mod drawing a machine
    /// from its neighbour's state. On a server, its saved compound, item
    /// slots and data slots; on a client, what the server sent of it
    /// (`native_block_entity_update_tag`, merged into `nbt`), with no item
    /// slots. None for another kind of block entity, a block with none, and
    /// on a server for the one ticking now, which is out of the world (as
    /// for `native_with_mod_block_entity`). API @API:host_bridge@.
    pub(super) fn native_mod_block_entity(&self, pos: [i32; 3]) -> Option<ModBlockEntity> {
        match self.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
            Some(BlockEntity::Mod(m)) if !m.id.is_empty() => Some(ModBlockEntity::clone(m)),
            _ => None,
        }
    }

    /// `native_with_mod_block_entity` through `dyn Host`: runs `change` on
    /// the mod block entity at `pos` (a burner heating the basin above it),
    /// then, as its `setChanged` would, saves its chunk next time and lets
    /// comparators read it again when its items changed. False, with
    /// `change` not run, where `native_with_mod_block_entity` finds none.
    /// Call `native_block_entity_sync` too when what clients draw changed.
    /// API @API:host_bridge@.
    pub(super) fn native_change_mod_block_entity(
        &mut self,
        pos: [i32; 3],
        change: &mut dyn FnMut(&mut ModBlockEntity),
    ) -> bool {
        self.native_with_mod_block_entity(pos, |m| change(m))
            .is_some()
    }

    /// Plays sound event `sound` at `at` for the players near it in the
    /// level in place, as a level hook's `LevelWriteHost::play_sound` does
    /// (`Level.playSound` with no player): a machine's hum from its block
    /// entity's tick. `source` is the `SoundSource` ordinal (4 for
    /// `BLOCKS`). True when queued, not heard; false off a server and where
    /// the level host's `play_sound` refuses. API @API:host_bridge@.
    pub(super) fn native_level_play_sound(
        &mut self,
        sound: &str,
        at: [f64; 3],
        source: usize,
        volume: f32,
        pitch: f32,
    ) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        self.with_level(|_, level, _| level.native_play_sound(sound, at, source, volume, pitch))
    }

    /// `ServerLevel.sendParticles` in the level in place, as a level hook's
    /// `LevelWriteHost::send_particles` sends them: the players near `at`
    /// see `count` particles spread by `spread` (a zero count sends one
    /// moving `speed` along `spread`). `state` is the block of block
    /// particles. True when accepted; false off a server and where the level
    /// host's `send_particles` refuses. API @API:host_bridge@.
    pub(super) fn native_level_send_particles(
        &mut self,
        options: &ParticleOptions,
        state: Option<BlockStateId>,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: i32,
    ) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        self.with_level(|_, level, _| {
            level.native_send_particles(options, state, at, spread, speed, count)
        })
    }

    /// The stack in the player's equipment slot `slot`
    /// (`LivingEntity.getItemBySlot`): on a client the player's own, as
    /// goggles worn or a tank on the back show their overlay; on a server the
    /// acting player's. None for an empty slot, and for `Body` and `Saddle`,
    /// which a player has none of. API @API:host_bridge@.
    pub(super) fn native_equipment(&self, slot: EquipmentSlot) -> Option<ItemStack> {
        match slot {
            // The selected slot, not `held`: that is the acting hand, the off hand's stack during
            // the off-hand pass of a use (`with_hand(Hand::Off, ..)`).
            EquipmentSlot::MainHand => self.inventory.get(self.selected).cloned().flatten(),
            EquipmentSlot::OffHand => self.offhand.clone(),
            EquipmentSlot::Head => self.armor[0].clone(),
            EquipmentSlot::Chest => self.armor[1].clone(),
            EquipmentSlot::Legs => self.armor[2].clone(),
            EquipmentSlot::Feet => self.armor[3].clone(),
            EquipmentSlot::Body | EquipmentSlot::Saddle => None,
        }
        .filter(|stack| stack.count > 0)
    }

    /// Whether the player acting now holds sneak (`Player.isShiftKeyDown`,
    /// what `UseOnContext.isSecondaryUseActive` reads in a use hook): a
    /// wrench that picks a block up rather than turning it, a crank turned
    /// backwards. On a server the acting guest's, as their use runs
    /// (`as_guest` holds sneak for them); on a client the player's own key.
    /// API @API:host_bridge@.
    pub(super) fn native_sneaking(&self) -> bool {
        self.keys.held(Action::Sneak)
    }
}

#[cfg(test)]
mod tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::{first_person::Hand, items::ItemStack};
    use horizon_world::player_inventory::EquipmentSlot;

    /// `LivingEntity.getItemBySlot` doesn't depend on the hand acting: in the
    /// off-hand pass of a use, the main hand is still the selected slot's.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn equipment_reads_the_main_hand_in_an_off_hand_pass() {
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
        let name = |stack: Option<ItemStack>| stack.map(|s| s.item.to_string());
        game.inventory[game.selected] = Some(ItemStack::new("minecraft:stick", 1));
        game.offhand = Some(ItemStack::new("minecraft:torch", 1));
        for hand in [Hand::Main, Hand::Off] {
            game.with_hand(hand, |g| {
                assert_eq!(
                    name(g.native_equipment(EquipmentSlot::MainHand)).as_deref(),
                    Some("minecraft:stick"),
                    "{hand:?}"
                );
                assert_eq!(
                    name(g.native_equipment(EquipmentSlot::OffHand)).as_deref(),
                    Some("minecraft:torch"),
                    "{hand:?}"
                );
            });
        }
        game.inventory[game.selected] = None;
        game.with_hand(Hand::Off, |g| {
            assert!(g.native_equipment(EquipmentSlot::MainHand).is_none());
        });
    }
}
