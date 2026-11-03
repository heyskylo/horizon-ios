use horizon_core::{
    density::{
        CachingDensitySampler, ConstantSampler, ContextBoundSampler, DensitySampler, SampleError,
        SamplerContext, WorldField, WorldFields,
    },
    noise::{DensityVolume, NanRange, Noise},
    position::{BlockPos, ChunkPos},
    random::{RandomSource, XoroshiroRandom},
};
use horizon_world::{
    DATA_VERSION,
    heightmap::HeightmapState,
    nbt::Tag,
    registry::{BlockRegistry, BlockStateId},
    worldgen::{
        anchor::WorldGenerationContext,
        material_condition::{
            MaterialCondition, MaterialRule, MaterialRuleError, MaterialSurfaceQueries,
        },
        material_extensions::{ExtensionBlocks, ExtensionNoises, MaterialExtensions},
        material_stage::{SurfaceInputs, TopMaterialInputs, build_surface, top_material},
        terrain::{TerrainBlockStates, TerrainChunk},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy)]
struct State(u32);
impl HeightmapState for State {
    fn is_air(self) -> bool {
        self.0 < 2
    }
    fn is_air_block(self) -> bool {
        self.0 == 0
    }
    fn has_fluid(self) -> bool {
        self.0 == 3
    }
    fn blocks_motion_in_heightmap(self) -> bool {
        self.0 == 2
    }
    fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
        self.0 == 2
    }
}
struct Properties;
impl TerrainBlockStates for Properties {
    type State = State;
    fn heightmap_state(&self, id: BlockStateId) -> State {
        State(id.0)
    }
}
#[derive(Debug)]
struct Zero;
impl Noise for Zero {
    fn range(&self) -> Result<[f32; 2], NanRange> {
        Ok([0.; 2])
    }
    fn get2(&self, _: f64, _: f64) -> f32 {
        0.
    }
    fn get3(&self, _: f64, _: f64, _: f64) -> f32 {
        0.
    }
}
struct Surface {
    calls: usize,
    fail_at: Option<usize>,
}
impl MaterialSurfaceQueries for Surface {
    fn surface_depth(&mut self, _: i32, _: i32) -> i32 {
        1
    }
    fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
        0.
    }
    fn biome(&mut self, _: BlockPos) -> Result<Option<Arc<str>>, MaterialRuleError> {
        self.calls += 1;
        if self.fail_at == Some(self.calls) {
            Err(MaterialRuleError::MissingBiome)
        } else {
            Ok(Some(Arc::from("minecraft:plains")))
        }
    }
}
#[derive(Debug)]
struct Count(Arc<AtomicUsize>);
impl DensitySampler for Count {
    fn sample_value(&self, _: &mut SamplerContext, _: [i32; 3]) -> Result<f32, SampleError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(42.)
    }
}
#[derive(Debug)]
struct Fails;
impl DensitySampler for Fails {
    fn sample_value(&self, _: &mut SamplerContext, _: [i32; 3]) -> Result<f32, SampleError> {
        Err(SampleError::RecursiveCache)
    }
}
fn registry() -> BlockRegistry {
    let names = [
        "minecraft:air",
        "minecraft:void_air",
        "minecraft:stone",
        "minecraft:water",
    ];
    BlockRegistry::from_nbt(&Tag::compound([
        ("DataVersion", Tag::Int(DATA_VERSION)),
        (
            "states",
            Tag::List(
                names
                    .iter()
                    .map(|name| Tag::compound([("id", Tag::String((*name).into()))]))
                    .collect(),
            ),
        ),
        (
            "defaults",
            Tag::Compound(
                names
                    .iter()
                    .enumerate()
                    .map(|(id, name)| ((*name).into(), Tag::Int(id as i32)))
                    .collect(),
            ),
        ),
    ]))
    .unwrap()
}

#[test]
fn density_context_survives_success_binding_failure_and_partial_chunk_failure() {
    let registry = registry();
    for scenario in 0..3 {
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(0, 0), 0, 16, &registry, Properties).unwrap();
        chunk.set_block_raw(0, 5, 0, BlockStateId(2)).unwrap();
        chunk.set_block_raw(0, 5, 1, BlockStateId(2)).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let cached = CachingDensitySampler::new(0, Arc::new(Count(count.clone()))).unwrap();
        let mut context = SamplerContext::cached().with_world_fields(
            WorldFields::default()
                .with_field(WorldField::BlendOffset, Arc::new(ConstantSampler(73.))),
        );
        assert_eq!(cached.sample_value(&mut context, [100, 2, 3]).unwrap(), 42.);
        let root = XoroshiroRandom::new(0).fork_positional();
        let extensions = MaterialExtensions::new(
            root,
            ExtensionBlocks {
                default: BlockStateId(2),
                water: BlockStateId(3),
                packed_ice: BlockStateId(2),
                snow: BlockStateId(2),
            },
            ExtensionNoises {
                badlands_surface: Arc::new(Zero),
                badlands_pillar: Arc::new(Zero),
                badlands_roof: Arc::new(Zero),
                iceberg_surface: Arc::new(Zero),
                iceberg_pillar: Arc::new(Zero),
                iceberg_roof: Arc::new(Zero),
            },
            8,
        );
        let mut surface = Surface {
            calls: 0,
            fail_at: (scenario == 1).then_some(2),
        };
        let rule = if scenario == 2 {
            MaterialRule::OreVein {
                ore: BlockStateId(2),
                raw_ore: BlockStateId(2),
                filler: BlockStateId(2),
                raw_ore_chance: 0.5,
                density: Arc::new(Fails),
                richness: Arc::new(ConstantSampler(1.)),
                filler_gap: Arc::new(ConstantSampler(-1.)),
                positive_y: [i32::MIN, i32::MAX],
            }
        } else {
            MaterialRule::Block(BlockStateId(3))
        };
        let result = build_surface(
            &mut chunk,
            rule,
            SurfaceInputs::<XoroshiroRandom, _> {
                generation: WorldGenerationContext::new(0, 16, 8),
                noise_volume: DensityVolume::new([16; 3], [0; 3], [1; 3]).unwrap(),
                root,
                surface: &mut surface,
                preliminary_surface: Arc::new(ConstantSampler(0.)),
                density_context: &mut context,
                write_min_y: 0,
                write_max_y: 15,
            },
            &extensions,
        );
        assert_eq!(result.is_ok(), scenario == 0);
        assert_eq!(cached.sample_value(&mut context, [100, 2, 3]).unwrap(), 42.);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "the caller's cache was lost"
        );
        assert_eq!(
            ContextBoundSampler::new(WorldField::BlendOffset)
                .sample_value(&mut context, [0; 3])
                .unwrap(),
            73.
        );
        assert_eq!(
            chunk.block(0, 5, 0),
            BlockStateId(if scenario == 2 { 2 } else { 3 })
        );
        assert_eq!(
            chunk.block(0, 5, 1),
            BlockStateId(if scenario == 0 { 3 } else { 2 })
        );
        assert_eq!(chunk.postprocessing()[0].len(), [2, 1, 0][scenario]);
        assert_eq!(surface.calls, [256, 2, 0][scenario]);
    }
}

#[test]
fn top_material_retains_context_on_failure_and_ignores_possible_biome_shortcuts() {
    struct Restricted(Surface);
    impl MaterialSurfaceQueries for Restricted {
        fn possible_biomes(&self) -> Option<std::collections::BTreeSet<String>> {
            Some(std::collections::BTreeSet::new())
        }
        fn surface_depth(&mut self, _: i32, _: i32) -> i32 {
            1
        }
        fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
            0.
        }
        fn biome(&mut self, p: BlockPos) -> Result<Option<Arc<str>>, MaterialRuleError> {
            self.0.biome(p)
        }
    }
    let registry = registry();
    for fail in [false, true] {
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(0, 0), 0, 16, &registry, Properties).unwrap();
        chunk.set_block_raw(0, 5, 0, BlockStateId(2)).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let cached = CachingDensitySampler::new(0, Arc::new(Count(count.clone()))).unwrap();
        let mut context = SamplerContext::cached().with_world_fields(
            WorldFields::default()
                .with_field(WorldField::BlendOffset, Arc::new(ConstantSampler(73.))),
        );
        assert_eq!(cached.sample_value(&mut context, [100, 2, 3]).unwrap(), 42.);
        let mut surface = Restricted(Surface {
            calls: 0,
            fail_at: fail.then_some(1),
        });
        let rule = MaterialRule::Condition {
            if_true: MaterialCondition::Biome(vec!["minecraft:plains".to_owned()]),
            then_run: Box::new(MaterialRule::Block(BlockStateId(3))),
        };
        let result = top_material(
            &mut chunk,
            rule,
            TopMaterialInputs::<XoroshiroRandom, _> {
                generation: WorldGenerationContext::new(0, 16, 8),
                root: XoroshiroRandom::new(0).fork_positional(),
                surface: &mut surface,
                preliminary_surface: Arc::new(ConstantSampler(0.)),
                density_context: &mut context,
            },
            BlockPos::new(0, 5, 0),
            true,
        );
        if fail {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), Some(BlockStateId(3)));
        }
        assert_eq!(surface.0.calls, 1);
        assert_eq!(cached.sample_value(&mut context, [100, 2, 3]).unwrap(), 42.);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            ContextBoundSampler::new(WorldField::BlendOffset)
                .sample_value(&mut context, [0; 3])
                .unwrap(),
            73.
        );
        assert_eq!(chunk.block(0, 5, 0), BlockStateId(2));
        assert!(chunk.postprocessing()[0].is_empty());
    }
}
