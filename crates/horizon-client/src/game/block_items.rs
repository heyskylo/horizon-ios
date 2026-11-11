//! Block entities and the items that carry them
//! (`horizon_client::block_entity_item`): the item Pick Block gives, with the
//! block entity's data on Ctrl in creative, and the data and components a
//! placed item gives its block entity.
use super::Game;
use horizon_client::{
    Result, block_entity_item, block_entity_nbt, containers::BlockEntity, item_data,
    items::ItemStack, save,
};
use horizon_world::{
    level::{Behaviours, classes::Class},
    nbt::{Compound, JavaString, Tag},
    registry::BlockStateId,
};

/// The component patch of `stack`.
fn components(stack: &ItemStack) -> Compound {
    match item_data::encode_components(stack) {
        Some(Tag::Compound(c)) => c,
        _ => Compound::new(),
    }
}

/// `BlockState.getCloneItemStack`'s item: the block's own item, unless the
/// block overrides it. A block with no item of its own still has one to pick:
/// a stem's seeds, a crop's seed item, a vine's harvest.
fn clone_item<'a>(name: &str, block_item: &'a str) -> &'a str {
    match name {
        // StemBlock and AttachedStemBlock: the seeds it grew from.
        "minecraft:pumpkin_stem" | "minecraft:attached_pumpkin_stem" => "minecraft:pumpkin_seeds",
        "minecraft:melon_stem" | "minecraft:attached_melon_stem" => "minecraft:melon_seeds",
        // CropBlock.getCloneItemStack: new ItemStack(getBaseSeedId()).
        "minecraft:wheat" => "minecraft:wheat_seeds",
        "minecraft:carrots" => "minecraft:carrot",
        "minecraft:potatoes" => "minecraft:potato",
        "minecraft:beetroots" => "minecraft:beetroot_seeds",
        "minecraft:torchflower_crop" => "minecraft:torchflower_seeds",
        // BambooSaplingBlock, SweetBerryBushBlock and the two cave vine
        // blocks: new ItemStack(their harvest item).
        "minecraft:bamboo_sapling" => "minecraft:bamboo",
        "minecraft:sweet_berry_bush" => "minecraft:sweet_berries",
        "minecraft:cave_vines" | "minecraft:cave_vines_plant" => "minecraft:glow_berries",
        // BigDripleafStemBlock: new ItemStack(Blocks.BIG_DRIPLEAF).
        "minecraft:big_dripleaf_stem" => "minecraft:big_dripleaf",
        // GrowingPlantBodyBlock: new ItemStack(getHeadBlock()), the block that
        // carries the item.
        "minecraft:kelp_plant" => "minecraft:kelp",
        "minecraft:twisting_vines_plant" => "minecraft:twisting_vines",
        "minecraft:weeping_vines_plant" => "minecraft:weeping_vines",
        // CandleCakeBlock: new ItemStack(Blocks.CAKE), for every colour and
        // the plain `minecraft:candle_cake`, whose own item is air.
        _ if name == "minecraft:candle_cake" || name.ends_with("_candle_cake") => "minecraft:cake",
        _ => block_item,
    }
}

fn picked_item<'a>(
    behaviours: &'a Behaviours,
    state: BlockStateId,
    item: &'a str,
) -> Option<&'a str> {
    let plant = behaviours.data(state);
    let potted = plant
        .is(Class::FlowerPot)
        .then_some(plant.link)
        .flatten()
        .map(|block| &*behaviours.block_data(block).item)
        .filter(|item| *item != "minecraft:air");
    Some(potted.unwrap_or(item)).filter(|item| *item != "minecraft:air")
}

impl Game<'_> {
    /// Plain projection for the existing container-title consumers. The live
    /// `BaseContainerBlockEntity` name remains a Component, not rendered text.
    pub(super) fn block_name_plain(&self, key: (i32, i32, i32)) -> Option<String> {
        self.world.block_names.get(&key).map(|name| match name {
            Tag::String(text) => text.to_string_lossy(),
            component => horizon_client::component::name_text(component).plain(),
        })
    }

    /// The block entity at `key` as vanilla saves it, with its custom name.
    fn saved_block_entity(&self, key: (i32, i32, i32)) -> Option<Tag> {
        let entity = self.world.block_entities.get(&key)?;
        let block = &self
            .res
            .facts()
            .get(self.world.block(key.0, key.1, key.2))
            .name;
        let mut tag = save::encode_block_entity(key, entity, block);
        if let (Some(name), Tag::Compound(map)) = (self.world.block_names.get(&key), &mut tag) {
            map.insert("CustomName".into(), name.clone());
        }
        if matches!(entity, BlockEntity::Mod(_)) {
            // The mod owns its serializer; the vanilla canonicalizer only knows
            // vanilla entity types and must not discard this instance's data.
            Some(tag)
        } else {
            block_entity_nbt::canonical(&tag)
        }
    }

    /// The `CustomName` a block entity loads from `tag`
    /// (`BaseContainerBlockEntity.loadAdditional`): kept beside the model in
    /// `block_names`, and dropped when the tag has none. Banners and signs
    /// keep their name themselves. True when the name changed.
    pub(super) fn load_block_name(
        &mut self,
        key: (i32, i32, i32),
        entity: &BlockEntity,
        tag: &Tag,
    ) -> bool {
        if matches!(entity, BlockEntity::Banner(_) | BlockEntity::Sign(_)) {
            return false;
        }
        let name = if matches!(entity, BlockEntity::Mod(_)) {
            save::text(tag.get("CustomName")).map(|name| Tag::String(name.as_str().into()))
        } else {
            tag.get("CustomName").and_then(block_entity_nbt::component)
        };
        if self.world.block_names.get(&key) == name.as_ref() {
            return false;
        }
        match name {
            Some(name) => self.world.block_names.insert(key, name),
            None => self.world.block_names.remove(&key),
        };
        true
    }

    /// `BlockState.getCloneItemStack` of the block at `pos`, then, with
    /// `include_data`, `ServerGamePacketListenerImpl.addBlockDataToItem`.
    pub(super) fn picked_stack(&self, pos: [i32; 3], include_data: bool) -> Option<ItemStack> {
        let key = (pos[0], pos[1], pos[2]);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let facts = self.res.facts().get(state);
        // PistonHeadBlock.getCloneItemStack: the piston the head belongs to.
        let head = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name.as_str() == "minecraft:piston_head")
            .map(|s| match s.properties.get("type") {
                Some(t) if t == "sticky" => "minecraft:sticky_piston",
                _ => "minecraft:piston",
            });
        let mut custom = None;
        self.native_block_clone_item(&facts.name, pos, state, &mut custom);
        let mut stack = match custom {
            Some(stack) => {
                if stack.count == 0
                    || stack.item.as_ref() == "minecraft:air"
                    || self.items().get(&stack.item).is_none()
                {
                    return None;
                }
                stack
            }
            None => {
                let item = clone_item(&facts.name, head.unwrap_or(&facts.item));
                // FlowerPotBlock.getCloneItemStack: a potted plant picks as
                // that plant (`new ItemStack(potted)`), an empty pot as itself.
                let item = picked_item(self.res.behaviours(), state, item)?;
                ItemStack::new(item, 1)
            }
        };
        let mut patch = Compound::new();
        // BeehiveBlock.getCloneItemStack: the honey level as the item's block state.
        if include_data
            && let Some(level) = self
                .pack
                .assets
                .registry
                .state(state)
                .filter(|s| matches!(s.name.as_str(), "minecraft:beehive" | "minecraft:bee_nest"))
                .and_then(|s| s.properties.get("honey_level"))
        {
            patch.insert(
                "minecraft:block_state".into(),
                Tag::compound([("honey_level", Tag::String(level.as_str().into()))]),
            );
        }
        // CopperGolemStatueBlock.getCloneItemStack: the statue's pose as the item's
        // block_state (`CopperGolemStatueBlockEntity.getItem`), data or not.
        if let Some(pose) = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name.ends_with("copper_golem_statue"))
            .and_then(|s| s.properties.get("copper_golem_pose"))
        {
            patch.insert(
                "minecraft:block_state".into(),
                Tag::compound([("copper_golem_pose", Tag::String(pose.as_str().into()))]),
            );
        }
        // LightBlock.getCloneItemStack, setLightOnStack: the light's level as
        // the item's block state, whether or not the block data is picked.
        if let Some(level) = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name.as_str() == "minecraft:light")
            .and_then(|s| s.properties.get("level"))
        {
            patch.insert(
                "minecraft:block_state".into(),
                Tag::compound([("level", Tag::String(level.as_str().into()))]),
            );
        }
        // AbstractBannerBlock.getCloneItemStack: the banner's own item
        // (BannerBlockEntity.getItem), data or not.
        let banner = matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Banner(_))
        );
        // DecoratedPotBlock.getCloneItemStack: createDecoratedPotInstance, the
        // sherds only; the pot's item comes with include_data.
        let pot = &*facts.name == "minecraft:decorated_pot";
        // CopperGolemStatueBlockEntity.getItem: collectComponents (the golem's
        // custom name), data or not.
        let statue = matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::CopperGolemStatue(_))
        );
        if (include_data || banner || pot || statue)
            && let Some(saved) = self.saved_block_entity(key)
        {
            let mut picked = block_entity_item::pick(&saved);
            if pot && !include_data {
                picked.retain(|k, _| *k == JavaString::from("minecraft:pot_decorations"));
            }
            patch.extend(picked);
        }
        if !patch.is_empty() {
            let mut all = components(&stack);
            all.extend(patch);
            (stack.damage, stack.extra) = item_data::decode_components(Some(&Tag::Compound(all)));
        }
        Some(stack)
    }

    /// `ShulkerBoxBlock.playerWillDestroy`: a Creative player's filled box is
    /// one item at the block center, with collectComponents and the default pickup delay.
    /// This direct spawn intentionally does not use the block_drops gamerule.
    pub(super) fn drop_creative_shulker(&mut self, pos: [i32; 3]) {
        let key = (pos[0], pos[1], pos[2]);
        let filled = matches!(self.world.block_entities.get(&key), Some(BlockEntity::Chest(items))
            if items.iter().flatten().any(|s| s.count > 0));
        if !filled {
            return;
        }
        let Some(saved) = self.saved_block_entity(key) else {
            return;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let mut stack = ItemStack::new(&self.res.facts().get(state).item, 1);
        let patch = block_entity_item::collected_components(&saved);
        (stack.damage, stack.extra) = item_data::decode_components(Some(&Tag::Compound(patch)));
        self.spawn_item_own(stack, pos.map(|v| f64::from(v) + 0.5), 10);
    }

    /// `BlockItem.updateBlockStateFromTag`: the properties of the item's
    /// `block_state` component (a hive's honey level, a statue's pose) set on
    /// the block just placed at `pos`. Returns the state it has now.
    pub(super) fn apply_item_block_state(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        stack: &ItemStack,
    ) -> Result<BlockStateId> {
        let Some(Tag::Compound(saved)) =
            stack.data().and_then(|d| d.other("minecraft:block_state"))
        else {
            return Ok(state);
        };
        let Some(current) = self.pack.assets.registry.state(state) else {
            return Ok(state);
        };
        // Only the properties the block has.
        let props: Vec<(String, String)> = saved
            .iter()
            .filter_map(|(k, v)| match v {
                Tag::String(v) => Some((k.to_string_lossy(), v.to_string_lossy())),
                _ => None,
            })
            .filter(|(k, v)| current.properties.get(k).is_some_and(|old| old != v))
            .collect();
        let props: Vec<(&str, &str)> = props.iter().map(|(k, v)| (&**k, &**v)).collect();
        match self.with_props(state, &props) {
            Some(new) if !props.is_empty() && new != state => {
                self.level_set(pos, new)?;
                Ok(new)
            }
            _ => Ok(state),
        }
    }

    /// `BlockItem.updateBlockEntityComponents` on a client's level, for the
    /// block it predicts at `pos`: the block entity takes the stack's
    /// components but not its `block_entity_data` (`updateCustomBlockEntityTag`
    /// returns on a client), so a copied sign's editor opens with its text (#8102).
    pub(super) fn apply_predicted_item(&mut self, pos: [i32; 3], stack: &ItemStack) {
        let mut stack = stack.clone();
        if stack.data().is_some() {
            stack
                .data_mut()
                .set_other(block_entity_item::BLOCK_ENTITY_DATA, None);
        }
        self.apply_placed_item(pos, &stack, false);
    }

    /// `BlockItem.updateCustomBlockEntityTag` and `updateBlockEntityComponents`
    /// for the block entity just placed at `pos` from `stack`; `game_master`:
    /// the placer can use game master blocks (a dispenser can't).
    pub(super) fn apply_placed_item(
        &mut self,
        pos: [i32; 3],
        stack: &ItemStack,
        game_master: bool,
    ) {
        let key = (pos[0], pos[1], pos[2]);
        self.pending.banner.remove(&pos);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.sculk_place_entity(state);
        let Some(saved) = self.saved_block_entity(key) else {
            return;
        };
        let placed = block_entity_item::place(&saved, &components(stack), game_master);
        if placed == saved {
            return;
        }
        let Some((_, mut entity)) = save::decode_block_entity(&placed) else {
            return;
        };
        // The new crafter's state flag (CrafterBlock.newBlockEntity), which
        // its saved data has no part of.
        if let (BlockEntity::Crafter(new), Some(BlockEntity::Crafter(old))) =
            (&mut entity, self.world.block_entities.get(&key))
        {
            new.state_triggered = old.state_triggered;
        }
        self.load_block_name(key, &entity, &placed);
        self.world.block_entities.insert(key, entity);
        self.update_spawner_display(key);
    }
}

#[cfg(test)]
mod tests {
    use super::{clone_item, picked_item};

    #[test]
    fn a_potted_plant_picks_its_plant_and_an_empty_pot_the_pot() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("block-behaviours.json").exists() {
            return;
        }
        let registry = horizon_world::level::data::load_registry(&root).unwrap();
        let behaviours = horizon_world::level::Behaviours::load(&registry, &root).unwrap();
        let pick = |block: &str, own: &'static str| {
            let state = registry.default_state(block).unwrap();
            picked_item(&behaviours, state, own).map(str::to_owned)
        };
        assert_eq!(
            pick("minecraft:potted_dandelion", "minecraft:air").as_deref(),
            Some("minecraft:dandelion")
        );
        assert_eq!(
            pick("minecraft:potted_oak_sapling", "minecraft:air").as_deref(),
            Some("minecraft:oak_sapling")
        );
        assert_eq!(
            pick("minecraft:flower_pot", "minecraft:flower_pot").as_deref(),
            Some("minecraft:flower_pot")
        );
        assert_eq!(
            pick("minecraft:stone", "minecraft:stone").as_deref(),
            Some("minecraft:stone")
        );
        assert_eq!(pick("minecraft:fire", "minecraft:air"), None);
    }

    /// `StemBlock.getCloneItemStack` and `AttachedStemBlock.getCloneItemStack`:
    /// pumpkin and melon stems, attached or not, pick the seed they grew from;
    /// a block without an override still picks its own item.
    #[test]
    fn stems_pick_their_seeds_and_blocks_their_item() {
        for (stem, seed) in [
            ("minecraft:pumpkin_stem", "minecraft:pumpkin_seeds"),
            ("minecraft:attached_pumpkin_stem", "minecraft:pumpkin_seeds"),
            ("minecraft:melon_stem", "minecraft:melon_seeds"),
            ("minecraft:attached_melon_stem", "minecraft:melon_seeds"),
        ] {
            assert_eq!(clone_item(stem, "minecraft:air"), seed, "{stem}");
        }
        assert_eq!(
            clone_item("minecraft:stone", "minecraft:stone"),
            "minecraft:stone"
        );
    }
    /// The blocks whose `getCloneItemStack` override returns something other
    /// than their own item: a crop's seed item, a sapling's bamboo, a cave
    /// vine's glow berries, a stem block's head. Each of them has no item of
    /// its own, so pick block gave nothing before; a block with an item is
    /// still untouched.
    #[test]
    fn blocks_without_an_item_pick_as_their_harvest() {
        for (block, item) in [
            ("minecraft:wheat", "minecraft:wheat_seeds"),
            ("minecraft:carrots", "minecraft:carrot"),
            ("minecraft:potatoes", "minecraft:potato"),
            ("minecraft:beetroots", "minecraft:beetroot_seeds"),
            ("minecraft:torchflower_crop", "minecraft:torchflower_seeds"),
            ("minecraft:bamboo_sapling", "minecraft:bamboo"),
            ("minecraft:sweet_berry_bush", "minecraft:sweet_berries"),
            ("minecraft:cave_vines", "minecraft:glow_berries"),
            ("minecraft:cave_vines_plant", "minecraft:glow_berries"),
            ("minecraft:big_dripleaf_stem", "minecraft:big_dripleaf"),
            ("minecraft:kelp_plant", "minecraft:kelp"),
            ("minecraft:twisting_vines_plant", "minecraft:twisting_vines"),
            ("minecraft:weeping_vines_plant", "minecraft:weeping_vines"),
            ("minecraft:candle_cake", "minecraft:cake"),
            ("minecraft:white_candle_cake", "minecraft:cake"),
            ("minecraft:light_blue_candle_cake", "minecraft:cake"),
        ] {
            assert_eq!(clone_item(block, "minecraft:air"), item, "{block}");
        }
        assert_eq!(
            clone_item("minecraft:sweet_berries", "minecraft:sweet_berries"),
            "minecraft:sweet_berries"
        );
        assert_eq!(
            clone_item("minecraft:cake", "minecraft:cake"),
            "minecraft:cake"
        );
    }
}

#[cfg(test)]
mod plain_name_overlay_tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    use horizon_core::position::BlockPos;

    /// BaseContainerBlockEntity's removed external name must not be restored
    /// from the original saved source by the chunk writer.
    #[test]
    #[ignore = "requires real prepared resources and scratch homes/cwd; must execute"]
    fn removed_actual_plain_name_stays_absent_in_saved_and_reloaded_model() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        let clock = game.world.game_time;
        let mut failures = Vec::new();
        for (x, kind, lock) in [(2, "chest", ""), (5, "barrel", ",lock:{items:diamond}")] {
            assert!(game.run_command(&format!("setblock {x} 200 4 {kind}{{CustomName:Original,Items:[{{Slot:0b,id:diamond,count:3}}]{lock}}}")));
            let pos = (x, 200, 4);
            assert!(game.world.loaded(x, 4));
            assert!(game.world.block_names.contains_key(&pos));
            assert_eq!(
                game.block_entity_nbt([x, 200, 4])
                    .unwrap()
                    .get("CustomName"),
                Some(&Tag::String("Original".into()))
            );
            let Some(BlockEntity::Chest(slots)) = game.world.block_entities.get(&pos) else {
                panic!("actual container")
            };
            assert_eq!(&*slots[0].as_ref().unwrap().item, "minecraft:diamond");
            assert_eq!(slots[0].as_ref().unwrap().count, 3);
            if x == 5 {
                assert!(game.world.block_lock(pos).is_some());
                let chunk = game.world.chunks.get(&(0, 0)).unwrap();
                let source = &chunk
                    .saved
                    .as_ref()
                    .unwrap()
                    .block_entity_sources
                    .iter()
                    .find(|(p, _)| *p == pos)
                    .unwrap()
                    .1;
                assert_eq!(
                    source.get("CustomName"),
                    Some(&Tag::String("Original".into()))
                );
            }
            assert!(game.run_command(&format!("data remove block {x} 200 4 CustomName")));
            assert!(!game.world.block_names.contains_key(&pos));
            assert!(
                game.block_entity_nbt([x, 200, 4])
                    .unwrap()
                    .get("CustomName")
                    .is_none()
            );
            let names = game.world.block_names.clone();
            let resident = game.world.chunks.get(&(0, 0)).unwrap();
            if x == 5 {
                let source = &resident
                    .saved
                    .as_ref()
                    .unwrap()
                    .block_entity_sources
                    .iter()
                    .find(|(p, _)| *p == pos)
                    .unwrap()
                    .1;
                assert_eq!(
                    source.get("CustomName"),
                    Some(&Tag::String("Original".into())),
                    "real retained source witness"
                );
                assert!(source.get("lock").is_some());
            }
            let snapshot = game.with_entities([resident]).remove(0);
            let encoded = save::encode(&snapshot, game.pack.assets.registry, game.res.biomes());
            let Tag::List(entries) = encoded.tag.get("block_entities").unwrap() else {
                panic!("actual saved entries")
            };
            let saved = entries
                .iter()
                .find(|e| save::number(e.get("x")) == Some(i64::from(x)))
                .unwrap();
            let Tag::List(items) = saved.get("Items").unwrap() else {
                panic!("actual saved contents")
            };
            assert_eq!(save::number(items[0].get("count")), Some(3));
            if x == 5 {
                assert!(saved.get("lock").is_some());
            }
            let reloaded =
                save::decode(&encoded, game.pack.assets.registry, game.res.biomes()).unwrap();
            if saved.get("CustomName").is_some()
                || reloaded.block_names.iter().any(|(p, _)| *p == pos)
            {
                failures.push((pos, saved.clone()));
            }
            assert_eq!(
                game.world.block_names, names,
                "save is read-only for the name model"
            );
        }
        assert_eq!(game.world.game_time, clock);
        assert!(
            failures.is_empty(),
            "removed actual plain name reappeared from source: {failures:?}"
        );
    }
}

#[cfg(test)]
mod styled_name_model_tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    use horizon_core::position::BlockPos;

    fn parse(input: &str) -> Tag {
        let (tag, used) = horizon_world::nbt::parse_snbt(input).unwrap();
        assert_eq!(used, input.len());
        tag
    }
    fn chunk_saved(game: &Game<'_>, pos: (i32, i32, i32)) -> Tag {
        let resident = game.world.chunks.get(&(0, 0)).unwrap();
        let snapshot = game.with_entities([resident]).remove(0);
        let encoded = save::encode(&snapshot, game.pack.assets.registry, game.res.biomes());
        let Tag::List(entities) = encoded.tag.get("block_entities").unwrap() else {
            panic!("actual chunk entities")
        };
        let saved = entities
            .iter()
            .find(|tag| {
                save::number(tag.get("x")) == Some(i64::from(pos.0))
                    && save::number(tag.get("y")) == Some(i64::from(pos.1))
                    && save::number(tag.get("z")) == Some(i64::from(pos.2))
            })
            .expect("actual saved block entity")
            .clone();
        let decoded = save::decode(&encoded, game.pack.assets.registry, game.res.biomes()).unwrap();
        assert!(decoded.block_entities.iter().any(|(at, _)| *at == pos));
        saved
    }
    fn views(game: &Game<'_>, pos: (i32, i32, i32), want: Option<&Tag>, missing: &mut Vec<String>) {
        assert!(game.world.loaded(pos.0, pos.2));
        let Some(BlockEntity::Chest(slots)) = game.world.block_entities.get(&pos) else {
            panic!("actual modeled container")
        };
        assert_eq!(slots[0].as_ref().unwrap().count, 3);
        assert_eq!(slots[26].as_ref().unwrap().count, 5);
        assert_eq!(&*slots[0].as_ref().unwrap().item, "minecraft:diamond");
        assert_eq!(&*slots[26].as_ref().unwrap().item, "minecraft:apple");
        let no_raw = game.saved_block_entity(pos).unwrap();
        let command = game.block_entity_nbt([pos.0, pos.1, pos.2]).unwrap();
        let chunk = chunk_saved(game, pos);
        for (route, saved) in [
            ("modeled/pick", no_raw),
            ("command", command),
            ("chunk", chunk),
        ] {
            assert_eq!(save::number(saved.get("x")), Some(i64::from(pos.0)));
            assert_eq!(save::number(saved.get("y")), Some(i64::from(pos.1)));
            assert_eq!(save::number(saved.get("z")), Some(i64::from(pos.2)));
            if saved.get("CustomName") != want {
                missing.push(format!(
                    "{route} {pos:?}: {:?} != {want:?}",
                    saved.get("CustomName")
                ));
            }
        }
    }

    #[test]
    #[ignore = "requires real prepared resources and scratch homes/cwd; must execute"]
    fn actual_container_names_load_place_mutate_and_save_without_raw() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.world.loaded(4, 4));
        let clock = game.world.game_time;
        let contents = "Items:[{Slot:0b,id:\"minecraft:diamond\",count:3,components:{\"minecraft:custom_data\":{h210:1}}},{Slot:26b,id:\"minecraft:apple\",count:5}]";
        let styled = "{text:\"Styled\",color:\"aqua\",bold:0b,extra:[{text:\"Box\",italic:1b}]}";
        let translated = "{translate:\"container.chest\",color:\"red\",italic:0b}";
        let changed = "{text:\"Changed\",color:\"gold\",italic:1b}";
        let mut missing = Vec::new();
        for (x, kind, name) in [
            (2, "chest", Some("\"Plain\"")),
            (5, "barrel", Some(styled)),
            (8, "shulker_box", Some(translated)),
            (11, "chest", None),
        ] {
            let extra = name.map_or(String::new(), |n| format!(",CustomName:{n}"));
            assert!(game.run_command(&format!(
                "setblock {x} 200 4 minecraft:{kind}{{{contents}{extra}}}"
            )));
            assert_eq!(
                &*game.res.facts().get(game.world.block(x, 200, 4)).name,
                format!("minecraft:{kind}")
            );
            let want = name.map(parse);
            if x == 2 {
                assert_eq!(
                    game.block_entity_nbt([x, 200, 4])
                        .unwrap()
                        .get("CustomName"),
                    want.as_ref(),
                    "plain-name admission"
                );
            }
            game.world.forget_block_entity_source((x, 200, 4));
            assert!(
                game.world
                    .chunks
                    .get(&(0, 0))
                    .unwrap()
                    .saved
                    .as_ref()
                    .is_none_or(|s| !s
                        .block_entity_sources
                        .iter()
                        .any(|(p, _)| *p == (x, 200, 4)))
            );
            views(&game, (x, 200, 4), want.as_ref(), &mut missing);
        }
        // The real source snapshot stays Old while an actual data command changes
        // only the live name. It must not be used as the saved answer.
        assert!(game.run_command(&format!("setblock 5 200 8 barrel{{{contents},CustomName:\"Old\",lock:{{items:\"minecraft:diamond\"}}}}")));
        let stale_pos = (5, 200, 8);
        assert!(game.world.block_lock(stale_pos).is_some());
        assert!(game.run_command(&format!(
            "data merge block 5 200 8 {{CustomName:{changed}}}"
        )));
        let source = game
            .world
            .chunks
            .get(&(0, 0))
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .block_entity_sources
            .iter()
            .find(|(p, _)| *p == stale_pos)
            .unwrap();
        assert_eq!(
            source.1.get("CustomName"),
            Some(&parse("\"Old\"")),
            "actual stale source admission"
        );
        let want = parse(changed);
        views(&game, stale_pos, Some(&want), &mut missing);
        game.world.forget_block_entity_source(stale_pos);
        views(&game, stale_pos, Some(&want), &mut missing);
        if !game.run_command("data remove block 5 200 8 CustomName") {
            missing.push("actual styled-name remove had no modeled source".into());
        }
        views(&game, stale_pos, None, &mut missing);
        // The real Game placement helper consumes the source stack components;
        // connected-owner consumption/ACK is independently covered at runtime.
        assert!(game.run_command(&format!("setblock 8 200 11 chest{{{contents}}}")));
        let source = parse(&format!(
            "{{id:\"minecraft:chest\",count:1,components:{{\"minecraft:custom_name\":{styled},\"minecraft:container\":[{{slot:0,item:{{id:\"minecraft:diamond\",count:3,components:{{\"minecraft:custom_data\":{{h210:1}}}}}}}},{{slot:26,item:{{id:\"minecraft:apple\",count:5}}}}]}}}}"
        ));
        let stack = save::item_from_nbt(&source).expect("real item codec source");
        assert_eq!(stack.count, 1);
        assert_eq!(
            stack.data().unwrap().custom_name.as_ref(),
            Some(&parse(styled))
        );
        game.apply_placed_item([8, 200, 11], &stack, false);
        views(&game, (8, 200, 11), Some(&parse(styled)), &mut missing);
        assert_eq!(stack.count, 1, "placement-helper source is unchanged");
        assert!(game.run_command("clone 8 200 4 8 200 4 11 200 8 replace"));
        views(&game, (11, 200, 8), Some(&parse(translated)), &mut missing);
        assert_eq!(game.world.game_time, clock);
        assert!(
            missing.is_empty(),
            "actual styled-name model/save mismatches: {missing:?}"
        );
    }
}
