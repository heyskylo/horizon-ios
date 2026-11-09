use horizon_core::{
    density::{ConstantSampler, DensitySampler},
    position::BlockPos,
    random::{RandomSource, XoroshiroRandom},
};
use horizon_world::{
    registry::BlockStateId,
    worldgen::{
        DecodeError,
        anchor::WorldGenerationContext,
        material_condition::{
            BandRule, MaterialCondition, MaterialConditions, MaterialRule, MaterialRuleError,
            MaterialSurfaceQueries,
        },
    },
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Default)]
struct Queries {
    bindings: Vec<bool>,
    temperatures: usize,
    biomes: usize,
    fail_binding: bool,
    fail_temperature: bool,
    any_biome: bool,
}
impl MaterialSurfaceQueries for Queries {
    fn possible_biomes(&self) -> Option<BTreeSet<String>> {
        (!self.any_biome).then(|| BTreeSet::from(["minecraft:plains".into()]))
    }
    fn surface_depth(&mut self, _: i32, _: i32) -> i32 {
        1
    }
    fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
        0.
    }
    fn bind_density(
        &mut self,
        _: Arc<dyn DensitySampler>,
        eager: bool,
    ) -> Result<usize, DecodeError> {
        self.bindings.push(eager);
        if self.fail_binding {
            return Err(DecodeError::Field("test binding failure"));
        }
        Ok(self.bindings.len() - 1)
    }
    fn biome(&mut self, _: BlockPos) -> Result<Option<Arc<str>>, MaterialRuleError> {
        self.biomes += 1;
        Ok(Some(Arc::from("minecraft:plains")))
    }
    fn cold_enough_to_snow(
        &mut self,
        _: &str,
        _: BlockPos,
        _: i32,
    ) -> Result<bool, MaterialRuleError> {
        self.temperatures += 1;
        if self.fail_temperature {
            Err(MaterialRuleError::BiomeQueryFailed)
        } else {
            Ok(true)
        }
    }
}
fn worker(surface: Queries) -> MaterialConditions<XoroshiroRandom, Queries> {
    MaterialConditions::new(
        WorldGenerationContext::new(-64, 384, 63),
        XoroshiroRandom::new(7).fork_positional(),
        surface,
    )
}
fn condition(if_true: MaterialCondition, then_run: MaterialRule) -> MaterialRule {
    MaterialRule::Condition {
        if_true,
        then_run: Box::new(then_run),
    }
}
fn biome(name: &str) -> MaterialCondition {
    MaterialCondition::Biome(vec![name.into()])
}
fn ore() -> MaterialRule {
    vein([i32::MIN, i32::MAX])
}
fn vein(positive_y: [i32; 2]) -> MaterialRule {
    MaterialRule::OreVein {
        ore: BlockStateId(1),
        raw_ore: BlockStateId(2),
        filler: BlockStateId(3),
        raw_ore_chance: 0.02,
        density: Arc::new(ConstantSampler(1.)),
        richness: Arc::new(ConstantSampler(1.)),
        filler_gap: Arc::new(ConstantSampler(-1.)),
        positive_y,
    }
}
#[test]
fn constant_branches_keep_binding_order_and_short_circuit_queries() {
    let mut w = worker(Queries::default());
    let rule = w
        .add_rule(MaterialRule::Sequence(vec![
            condition(biome("minecraft:desert"), ore()),
            condition(
                MaterialCondition::Not(Box::new(biome("minecraft:desert"))),
                MaterialRule::Block(BlockStateId(9)),
            ),
            ore(),
        ]))
        .unwrap();
    w.update_xz(4, 5, 0, 0);
    w.update_y(1, 1, i32::MIN, 64);
    for _ in 0..2 {
        assert_eq!(
            w.try_apply(rule, BlockPos::new(4, 64, 5)),
            Ok(Some(BlockStateId(9)))
        );
    }
    let q = w.into_surface();
    assert_eq!(q.bindings, [true, true, false, true, true, false]);
    assert_eq!(q.biomes, 0);
    assert_eq!(q.temperatures, 0);
}
#[test]
fn an_empty_biome_list_matches_nothing_on_a_surface_without_biome_ids() {
    let mut w = worker(Queries {
        any_biome: true,
        ..Queries::default()
    });
    let rule = w
        .add_rule(condition(
            MaterialCondition::Biome(Vec::new()),
            MaterialRule::Block(BlockStateId(9)),
        ))
        .unwrap();
    w.update_xz(4, 5, 0, 0);
    w.update_y(1, 1, i32::MIN, 64);
    assert_eq!(w.try_apply(rule, BlockPos::new(4, 64, 5)), Ok(None));
}
#[test]
fn unreachable_children_still_fail_compilation_without_adding_a_rule() {
    for preceding_block in [false, true] {
        let mut w = worker(Queries {
            fail_binding: true,
            ..Queries::default()
        });
        let dead = condition(biome("minecraft:desert"), ore());
        let rule = if preceding_block {
            MaterialRule::Sequence(vec![MaterialRule::Block(BlockStateId(9)), dead])
        } else {
            dead
        };
        assert!(w.add_rule(rule).is_err());
        assert_eq!(
            w.try_apply(0, BlockPos::new(0, 0, 0)),
            Err(MaterialRuleError::UnknownRule)
        );
        assert_eq!(w.into_surface().bindings, [true]);
    }
}
#[test]
fn dynamic_predicate_on_empty_child_retains_each_temperature_call_and_error() {
    for fail in [false, true] {
        let mut w = worker(Queries {
            fail_temperature: fail,
            ..Queries::default()
        });
        let rule = w
            .add_rule(condition(
                MaterialCondition::Temperature,
                condition(
                    biome("minecraft:desert"),
                    MaterialRule::Block(BlockStateId(9)),
                ),
            ))
            .unwrap();
        w.update_xz(0, 0, 0, 0);
        w.update_y(1, 1, i32::MIN, 64);
        for _ in 0..2 {
            assert_eq!(
                w.try_apply(rule, BlockPos::new(0, 64, 0)),
                if fail {
                    Err(MaterialRuleError::BiomeQueryFailed)
                } else {
                    Ok(None)
                }
            );
        }
        let q = w.into_surface();
        assert_eq!(q.temperatures, 2);
        assert_eq!(q.biomes, 1);
    }
}
#[test]
fn ore_veins_are_dropped_from_the_bands_where_their_density_is_never_positive() {
    let mut w = worker(Queries::default());
    let only = w.add_rule(vein([0, 49])).unwrap();
    let mut variants = w.surface_variants(only).unwrap();
    assert!(!variants.surface);
    for (y, kept) in [
        (-1, false),
        (0, true),
        (49, true),
        (50, false),
        (300, false),
    ] {
        let band = variants.band(y);
        assert_eq!(
            w.surface_variant(&mut variants, band, false).is_some(),
            kept,
            "{y}"
        );
    }
    assert_eq!(variants.band_bounds(variants.band(10)), [0, 49]);
    let rule = w
        .add_rule(MaterialRule::Sequence(vec![
            vein([0, 49]),
            MaterialRule::Block(BlockStateId(9)),
        ]))
        .unwrap();
    let mut variants = w.surface_variants(rule).unwrap();
    let band = variants.band(64);
    let variant = w.surface_variant(&mut variants, band, false).unwrap();
    w.update_xz(0, 0, 0, 0);
    w.update_y(1, 1, i32::MIN, 64);
    // The vein's density is not asked for where it cannot be positive.
    assert_eq!(
        w.try_apply(variant.rule(), BlockPos::new(0, 64, 0)),
        Ok(Some(BlockStateId(9)))
    );
    assert_eq!(
        w.into_surface().bindings,
        [true, true, false, true, true, false]
    );
}

/// Vein densities read by position: the first vein's `density` binding is
/// positive only at y 20, the second's only at y 30.
struct ScriptedDensities {
    bindings: usize,
}
impl MaterialSurfaceQueries for ScriptedDensities {
    fn possible_biomes(&self) -> Option<BTreeSet<String>> {
        None
    }
    fn surface_depth(&mut self, _: i32, _: i32) -> i32 {
        0
    }
    fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
        0.
    }
    fn bind_density(&mut self, _: Arc<dyn DensitySampler>, _: bool) -> Result<usize, DecodeError> {
        // Six bindings, in the veins' order.
        self.bindings += 1;
        Ok(self.bindings - 1)
    }
    fn density(&mut self, index: usize, position: BlockPos) -> Result<f32, MaterialRuleError> {
        Ok(match (index, position.y) {
            (0, 20) | (3, 30) => 1.0,
            _ => 0.0,
        })
    }
}

#[test]
fn run_fill_gives_its_block_exactly_where_no_vein_can_place_one() {
    let mut w = MaterialConditions::<XoroshiroRandom, _>::new(
        WorldGenerationContext::new(-64, 384, 63),
        XoroshiroRandom::new(7).fork_positional(),
        ScriptedDensities { bindings: 0 },
    );
    let rule = w
        .add_rule(MaterialRule::Sequence(vec![
            vein([-64, 320]),
            vein([-64, 320]),
            MaterialRule::Block(BlockStateId(9)),
        ]))
        .unwrap();
    let mut variants = w.surface_variants(rule).unwrap();
    let band = variants.band(0);
    let variant = w.surface_variant(&mut variants, band, false).unwrap();
    let BandRule::Fill(rule, fill) = variant else {
        panic!("a sequence of veins then a block is a run fill");
    };
    let fill = *variants.fill(fill);
    assert_eq!(fill.block, BlockStateId(9));
    assert_eq!(fill.veins(), [0, 3]);
    w.update_xz(0, 0, 0, 0);
    for y in 0..=40 {
        let position = BlockPos::new(0, y, 0);
        w.update_y(1, 1, i32::MIN, y);
        let miss = w.veins_miss(&fill, position).unwrap();
        assert_eq!(miss, y != 20 && y != 30, "{y}");
        // Wherever every vein misses, applying the rule gives the fill's
        // block: it neither reads the context nor draws a random.
        let state = w.try_apply(rule, position).unwrap();
        if miss {
            assert_eq!(state, Some(BlockStateId(9)), "{y}");
        } else {
            assert!(state.is_some(), "{y}");
            assert_ne!(state, Some(BlockStateId(9)), "{y}");
        }
    }
}
