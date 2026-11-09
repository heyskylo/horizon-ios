//! `/place template` (PlaceCommand): a saved structure template placed with
//! rotation, mirroring and integrity.
use super::{Arg, Context, Game, R, err};
use crate::game::structure_blocks::{load_settings, template_size};
use horizon_client::text::Text;
use horizon_core::position::BlockPos;
use horizon_world::{
    compression::Compression,
    nbt::{self, Limits, Tag},
    worldgen::structure::{Mirror, Rotation, StateTransforms},
};
use std::sync::OnceLock;

fn num(n: impl std::fmt::Display) -> Text {
    Text::literal(n.to_string())
}

impl Game<'_> {
    pub(in crate::game) fn state_transforms(&self) -> Option<&'static StateTransforms> {
        static TRANSFORMS: OnceLock<Option<StateTransforms>> = OnceLock::new();
        TRANSFORMS
            .get_or_init(|| {
                let json = std::fs::read_to_string(
                    self.options
                        .resources
                        .join("worldgen/blocks/structure-blocks.json"),
                )
                .ok()?;
                StateTransforms::from_json(&json, self.pack.assets.registry.len()).ok()
            })
            .as_ref()
    }
    /// A template's tree by id, as `StructureTemplateManager` reads it: the
    /// world's saved structures, the enabled data packs from the highest, and
    /// the game's own.
    pub(in crate::game) fn read_template_tag(&self, id: &str) -> Option<Tag> {
        let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
        let generated = self
            .world_root
            .join("generated")
            .join(ns)
            .join("structures")
            .join(format!("{path}.nbt"));
        let in_pack = format!("data/{ns}/structure/{path}.nbt");
        let prepared = self
            .options
            .resources
            .join("worldgen/structure")
            .join(ns)
            .join(format!("{path}.nbt"));
        let candidates = [
            std::fs::read(&generated).ok(),
            self.fns.pack_file(&in_pack),
            std::fs::read(&prepared).ok(),
        ];
        for bytes in candidates.into_iter().flatten() {
            let Ok(raw) = Compression::Gzip.decode(&bytes, 1 << 28) else {
                continue;
            };
            let Ok((named, _)) = nbt::read_named(&raw, Limits::templates()) else {
                continue;
            };
            return Some(named.tag);
        }
        None
    }
    /// `BlockPosArgument.getLoadedBlockPos(pos)`, or the source's block.
    fn place_pos(&self, ctx: &Context, name: &str) -> R<[i32; 3]> {
        if ctx.get(name).is_some() {
            return self.block_pos_arg(ctx, name);
        }
        Ok(self.source_position().map(|v| v.floor() as i32))
    }
    /// `PlaceCommand.checkLoaded`: every chunk of the inclusive range.
    fn place_check_loaded(&self, min: [i32; 2], max: [i32; 2]) -> R<()> {
        let dimension = self.command_source.map_or(self.dim, |s| s.dimension);
        for cx in min[0]..=max[0] {
            for cz in min[1]..=max[1] {
                if dimension != self.dim || !self.world.chunks.contains_key(&(cx, cz)) {
                    return Err(err("argument.pos.unloaded", &[]));
                }
            }
        }
        Ok(())
    }
    /// `PlaceCommand.placeFeature`, `placeJigsaw` and `placeStructure`.
    pub(super) fn cmd_place_generated(&mut self, id: &str, ctx: &Context) -> R<i32> {
        match id {
            "place.feature" => {
                let raw = match ctx.get("feature") {
                    Some(Arg::Resource(s) | Arg::Str(s)) => s.clone(),
                    _ => return Ok(0),
                };
                let pos = self.place_pos(ctx, "pos")?;
                let (cx, cz) = (pos[0] >> 4, pos[2] >> 4);
                self.place_check_loaded([cx - 1, cz - 1], [cx + 1, cz + 1])?;
                // ResourceOrHolderArgument: an inline feature value goes
                // through the same codec and `Feature.place`; a value that
                // does not decode keeps the plain failure.
                if raw.starts_with('{') {
                    let (tag, _) = horizon_world::nbt::parse_snbt(&raw)
                        .map_err(|_| err("command.failed", &[]))?;
                    let json = horizon_client::scoreboard::tag_to_json(&tag).to_string();
                    let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
                    let placed = self
                        .with_level(|b, level, ticks| {
                            b.place_inline_feature(level, ticks, &json, at)
                        })
                        .ok_or_else(|| err("command.failed", &[]))?;
                    self.process_level_events()
                        .map_err(|_| err("command.failed", &[]))?;
                    if !placed {
                        return Err(err("commands.place.feature.failed", &[]));
                    }
                    self.feedback(Text::translate(
                        "commands.place.feature.success.inline",
                        &[num(pos[0]), num(pos[1]), num(pos[2])],
                    ));
                    return Ok(1);
                }
                let name = if raw.contains(':') {
                    raw
                } else {
                    format!("minecraft:{raw}")
                };
                let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
                let placed = self
                    .with_level(|b, level, ticks| b.place_feature(level, ticks, &name, at))
                    .ok_or_else(|| err("command.failed", &[]))?;
                self.process_level_events()
                    .map_err(|_| err("command.failed", &[]))?;
                if !placed {
                    return Err(err("commands.place.feature.failed", &[]));
                }
                self.feedback(Text::translate(
                    "commands.place.feature.success",
                    &[Text::literal(name), num(pos[0]), num(pos[1]), num(pos[2])],
                ));
                Ok(1)
            }
            "place.jigsaw" => {
                let Some(Arg::Resource(pool)) = ctx.get("pool").cloned() else {
                    return Ok(0);
                };
                if !self
                    .ui
                    .registries
                    .ids("minecraft:worldgen/template_pool")
                    .contains(&pool)
                {
                    return Err(err("commands.place.jigsaw.invalid", &[&pool]));
                }
                let target = match ctx.get("target") {
                    Some(Arg::Resource(s) | Arg::Str(s)) => s.clone(),
                    _ => return Ok(0),
                };
                let max_depth = match ctx.get("max_depth") {
                    Some(Arg::Int(n)) => *n,
                    _ => return Ok(0),
                };
                let pos = self.place_pos(ctx, "position")?;
                self.place_check_loaded([pos[0] >> 4, pos[2] >> 4], [pos[0] >> 4, pos[2] >> 4])?;
                let terrain = self
                    .end
                    .locator()
                    .ok_or_else(|| err("command.failed", &[]))?;
                let query = horizon_world::worldgen::generator::JigsawQuery {
                    pool: &pool,
                    target: &target,
                    max_depth,
                    position: pos,
                };
                let (placed, entities) = self.with_level(|b, level, ticks| {
                    b.place_jigsaw(level, ticks, pos, false, || terrain.jigsaw_pieces(&query))
                });
                for entity in entities {
                    self.spawn_entity_from_nbt(Tag::Compound(entity));
                }
                self.process_level_events()
                    .map_err(|_| err("command.failed", &[]))?;
                if !placed {
                    return Err(err("commands.place.jigsaw.failed", &[]));
                }
                self.feedback(Text::translate(
                    "commands.place.jigsaw.success",
                    &[num(pos[0]), num(pos[1]), num(pos[2])],
                ));
                Ok(1)
            }
            _ => {
                let Some(Arg::Resource(structure)) = ctx.get("structure").cloned() else {
                    return Ok(0);
                };
                if !self
                    .ui
                    .registries
                    .ids("minecraft:worldgen/structure")
                    .contains(&structure)
                {
                    return Err(err("commands.place.structure.invalid", &[&structure]));
                }
                let pos = self.place_pos(ctx, "pos")?;
                // PlaceCommand.placeStructure: Structure.generate for the
                // position's chunk with its `biome -> true` predicate, then
                // placeInChunk for every chunk of the start's box.
                let terrain = self
                    .end
                    .locator()
                    .ok_or_else(|| err("command.failed", &[]))?;
                let start = terrain
                    .structure_start(&structure, pos[0] >> 4, pos[2] >> 4)
                    .ok_or_else(|| err("commands.place.structure.failed", &[]))?;
                let (min, max) = (start.bounding_box.min, start.bounding_box.max);
                self.place_check_loaded([min[0] >> 4, min[2] >> 4], [max[0] >> 4, max[2] >> 4])?;
                let (placed, entities) =
                    self.with_level(|b, level, ticks| b.place_structure(level, ticks, &start));
                for entity in entities {
                    self.spawn_entity_from_nbt(Tag::Compound(entity));
                }
                if !placed {
                    return Err(err("commands.place.structure.failed", &[]));
                }
                self.feedback(Text::translate(
                    "commands.place.structure.success",
                    &[
                        Text::literal(structure),
                        num(pos[0]),
                        num(pos[1]),
                        num(pos[2]),
                    ],
                ));
                Ok(1)
            }
        }
    }
    pub(super) fn cmd_place(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::Resource(id)) = ctx.get("template").cloned() else {
            return Ok(0);
        };
        let pos = self.place_pos(ctx, "pos")?;
        let rotation = match ctx.get("rotation") {
            Some(Arg::Str(r)) => Rotation::from_name(r).unwrap_or(Rotation::None),
            _ => Rotation::None,
        };
        let mirror = match ctx.get("mirror") {
            Some(Arg::Str(m)) => match m.as_str() {
                "left_right" => Mirror::LeftRight,
                "front_back" => Mirror::FrontBack,
                _ => Mirror::None,
            },
            _ => Mirror::None,
        };
        let integrity = match ctx.get("integrity") {
            Some(Arg::Float(f)) => *f,
            _ => 1.0,
        };
        let seed = match ctx.get("seed") {
            Some(Arg::Int(s)) => *s,
            _ => 0,
        };
        let strict = ctx.has("strict");
        let entry = self
            .template_entry(&id)
            .ok_or_else(|| err("commands.place.template.invalid", &[&id]))?;
        let size = template_size(&entry.tag);
        let end = [pos[0] + size[0], pos[2] + size[2]];
        self.place_check_loaded([pos[0] >> 4, pos[2] >> 4], [end[0] >> 4, end[1] >> 4])?;
        // `placeInWorld(level, pos, pos, settings, createRandom(seed), flags)`.
        let settings = load_settings(mirror, rotation, integrity, i64::from(seed), strict);
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        if !self.place_template_entry(&entry, at, settings, mirror, rotation) {
            return Err(err("commands.place.template.failed", &[]));
        }
        self.feedback(Text::translate(
            "commands.place.template.success",
            &[Text::literal(id), num(pos[0]), num(pos[1]), num(pos[2])],
        ));
        Ok(1)
    }
}

#[cfg(test)]
mod live_dungeon_spawner_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{
        chunk::Chunk,
        containers::BlockEntity,
        mobs::spawner::{SpawnDataExt, SpawnerExt},
        motion::PlayerMotion,
    };
    use horizon_core::{geometry::Vec3, position::ChunkPos, random::RandomSource};
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
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
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h173-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        crate::game::growth::install_live_features(&loaded.resources, &loaded.pack, &options);
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

        for cx in -1..=1 {
            for cz in -1..=1 {
                game.world
                    .chunks
                    .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
                game.block_ticks
                    .add_empty_chunk(ChunkPos::new(cx, cz))
                    .unwrap();
            }
        }
        game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("forceload add -16 -16 31 31"));
        assert!(game.run_command("tick freeze"));
        assert!(game.run_command("fill 0 198 0 31 199 31 stone"));
        test(&mut game);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_registered_live_dungeon_keeps_its_real_selected_spawner() {
        with_game(|game| {
            let clock = game.world.game_time;
            assert!(
                game.res
                    .behaviours()
                    .live_features()
                    .unwrap()
                    .configured("minecraft:monster_room")
                    .is_some()
            );
            for cmd in [
                "fill 3 199 3 13 204 13 barrier strict",
                "fill 3 199 3 13 204 13 stone strict",
                "fill 6 200 6 10 203 10 air strict",
                "fill 11 200 8 13 201 8 air strict",
                "setblock 24 200 24 spawner{SpawnData:{entity:{id:\"minecraft:zombie\"}},Delay:20s} strict",
            ] {
                assert!(game.run_command(cmd), "{cmd}");
            }
            assert!(game.run_command("execute if block 8 200 8 air"));
            assert!(game.world.block_entities.get(&(8, 200, 8)).is_none());
            let Some(BlockEntity::Spawner(direct)) = game.world.block_entities.get(&(24, 200, 24))
            else {
                panic!("direct spawner positive")
            };
            assert_eq!(
                direct.next_data().and_then(|d| d.id()).as_deref(),
                Some("minecraft:zombie")
            );
            assert!(game.world.chunks.contains_key(&(-1, -1)));
            assert!(game.world.chunks.contains_key(&(1, 1)));
            game.world.random.set_seed(173);
            assert!(
                game.run_command("place feature minecraft:monster_room 8 200 8"),
                "actual live feature admission"
            );
            assert!(game.run_command("execute if block 8 200 8 spawner"));
            let mut after = game.world.random.clone();
            let continuation: Vec<i64> = (0..4).map(|_| after.next_long()).collect();
            println!(
                "h173-continuation {}",
                serde_json::to_string(&continuation).unwrap()
            );
            let Some(BlockEntity::Spawner(spawner)) = game.world.block_entities.get(&(8, 200, 8))
            else {
                panic!("actual generated spawner is missing from live collection")
            };
            let chosen = spawner
                .next_data()
                .and_then(|d| d.id())
                .expect("actual selected entity");
            assert!(
                ["minecraft:skeleton", "minecraft:zombie", "minecraft:spider"]
                    .contains(&chosen.as_str())
            );
            assert!(spawner.potentials.is_empty());
            let tag = game.block_entity_nbt([8, 200, 8]).unwrap();
            for (key, value) in [
                ("id", Tag::String("minecraft:mob_spawner".into())),
                ("x", Tag::Int(8)),
                ("y", Tag::Int(200)),
                ("z", Tag::Int(8)),
            ] {
                assert_eq!(tag.get(key), Some(&value));
            }
            assert_eq!(
                tag.get("SpawnData")
                    .and_then(|t| t.get("entity"))
                    .and_then(|t| t.get("id")),
                Some(&Tag::String(chosen.as_str().into()))
            );
            assert!(game.run_command("execute if data block 8 200 8 SpawnData.entity.id"));
            assert!(game.run_command("execute if data block 24 200 24 {Delay:20s,SpawnData:{entity:{id:\"minecraft:zombie\"}}}"));
            assert_eq!(game.world.game_time, clock);
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn creation_boundary_preserves_existing_entries_and_rejects_other_blocks() {
        with_game(|game| {
            use horizon_world::level::LevelAccess;
            let request=match horizon_world::nbt::parse_snbt(r#"{id:"minecraft:mob_spawner",SpawnData:{entity:{id:"minecraft:skeleton"}},SpawnPotentials:[]}"#).unwrap().0 {Tag::Compound(c)=>c,_=>unreachable!()};
            assert!(game.run_command(r#"setblock 24 200 24 spawner{Delay:77s,MaxNearbyEntities:9s,SpawnData:{entity:{id:"minecraft:zombie",UUID:[I;173,4,5,6],Health:7f}},SpawnPotentials:[]} strict"#));
            if let Some(BlockEntity::Spawner(s)) = game.world.block_entities.get_mut(&(24, 200, 24))
            {
                s.spin = 12.5;
                s.o_spin = 6.25;
                s.client_delay = 33;
            }
            let before = game.block_entity_nbt([24, 200, 24]).unwrap();
            assert_eq!(
                before
                    .get("SpawnData")
                    .and_then(|v| v.get("entity"))
                    .and_then(|v| v.get("UUID")),
                Some(&Tag::IntArray(vec![173, 4, 5, 6]))
            );
            let model = format!("{:?}", game.world.block_entities.get(&(24, 200, 24)));
            let random = game.world.random.clone().next_long();
            game.with_level(|_, level, _| {
                level.create_feature_spawner(BlockPos::new(24, 200, 24), request.clone())
            });
            assert_eq!(game.block_entity_nbt([24, 200, 24]).unwrap(), before);
            assert_eq!(
                format!("{:?}", game.world.block_entities.get(&(24, 200, 24))),
                model
            );
            assert_eq!(game.world.random.clone().next_long(), random);
            assert!(game.run_command("setblock 26 200 24 spawner strict"));
            let mut wrong = BlockEntity::for_block("minecraft:chest").unwrap();
            wrong.slots_mut()[0] = Some(horizon_client::items::ItemStack::new(
                "minecraft:diamond",
                3,
            ));
            game.world.block_entities.insert((26, 200, 24), wrong);
            let before = format!("{:?}", game.world.block_entities.get(&(26, 200, 24)));
            game.with_level(|_, level, _| {
                level.create_feature_spawner(BlockPos::new(26, 200, 24), request.clone())
            });
            assert_eq!(
                format!("{:?}", game.world.block_entities.get(&(26, 200, 24))),
                before
            );
            assert!(game.run_command("setblock 28 200 24 stone strict"));
            let revision = game.world.revision;
            let count = game.world.block_entities.len();
            game.with_level(|_, level, _| {
                level.create_feature_spawner(BlockPos::new(28, 200, 24), request.clone())
            });
            assert!(game.world.block_entities.get(&(28, 200, 24)).is_none());
            assert_eq!(game.world.revision, revision);
            assert_eq!(game.world.block_entities.len(), count);
        });
    }
}
