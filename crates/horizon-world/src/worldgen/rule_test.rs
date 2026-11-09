//! Registry-bound replacement predicates with ordered short-circuit evaluation.
use super::{
    DecodeError, Object, block_feature::FeatureBlockBindings, density::identifier, integer,
    number_text, required,
};
use crate::{
    block_properties::BoundBlockProperty,
    fluid::{FluidKind, FluidStateTable},
    registry::BlockStateId,
};
use horizon_core::random::RandomSource;
use serde_json::value::RawValue;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Debug)]
pub struct RuleTest(Node);
#[derive(Clone, Debug)]
enum Node {
    True,
    Not(Box<RuleTest>),
    All(Vec<RuleTest>),
    Any(Vec<RuleTest>),
    States(Arc<BTreeSet<BlockStateId>>),
    State(BlockStateId),
    RandomStates(Arc<BTreeSet<BlockStateId>>, f32),
    RandomState(BlockStateId, f32),
    Height(i32, i32),
    Phase(Box<MatterPhase>),
}
/// `horizon:matter_phase`: Repurposed Structures' MatterPhaseRuleTest, which
/// its mineshafts and Nether monument use to match a block by what it is
/// made of (`phase_to_test_for`), optionally inverted (`invert_condition`).
#[derive(Clone, Debug)]
struct MatterPhase {
    phase: Phase,
    invert: bool,
    air: [BlockStateId; 3],
    fluids: Arc<FluidStateTable>,
    /// Only for `SOLID`.
    can_occlude: Option<BoundBlockProperty>,
    /// Blocks.RAIL and Blocks.CHAIN (26.3's iron chain).
    rail_or_chain: BTreeSet<BlockStateId>,
}
#[derive(Clone, Copy, Debug)]
enum Phase {
    Solid,
    Liquid,
    Air,
    AirRailOrChain,
    LiquidRailOrChain,
}
impl MatterPhase {
    fn decode(object: &Object, bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        let name: String = serde_json::from_str(required(object, "phase_to_test_for")?.get())?;
        let phase = match name.as_str() {
            "SOLID" => Phase::Solid,
            "LIQUID" => Phase::Liquid,
            "AIR" => Phase::Air,
            "AIR_RAIL_OR_CHAIN" => Phase::AirRailOrChain,
            "LIQUID_RAIL_OR_CHAIN" => Phase::LiquidRailOrChain,
            _ => return Err(DecodeError::Field("matter phase")),
        };
        let invert = match object.get("invert_condition") {
            Some(v) => serde_json::from_str(v.get())?,
            None => false,
        };
        let can_occlude = match phase {
            Phase::Solid => Some(
                bindings
                    .can_occlude
                    .clone()
                    .ok_or(DecodeError::Field("matter phase block properties"))?,
            ),
            _ => None,
        };
        let mut rail_or_chain = BTreeSet::new();
        for name in ["minecraft:rail", "minecraft:iron_chain"] {
            rail_or_chain.extend(bindings.blocks.get(name).into_iter().flat_map(|s| s.iter()));
        }
        Ok(Self {
            phase,
            invert,
            air: bindings.air,
            fluids: bindings.fluids.table().clone(),
            can_occlude,
            rail_or_chain,
        })
    }
    fn test(&self, state: BlockStateId) -> bool {
        let air = self.air.contains(&state);
        let liquid = self
            .fluids
            .get(state)
            .is_some_and(|f| f.kind() != FluidKind::Empty);
        let rail_or_chain = || self.rail_or_chain.contains(&state);
        let matched = match self.phase {
            Phase::Solid => {
                !air && !liquid && self.can_occlude.as_ref().is_some_and(|c| c.contains(state))
            }
            Phase::Liquid => liquid,
            Phase::Air => air,
            Phase::AirRailOrChain => air || rail_or_chain(),
            Phase::LiquidRailOrChain => liquid || rail_or_chain(),
        };
        matched != self.invert
    }
}
impl RuleTest {
    pub fn from_json(json: &str, bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        Self::decode(json, bindings, 0, &mut 65536)
    }
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        if depth > 128 || *remaining == 0 {
            return Err(DecodeError::Field("replacement rule depth or node count"));
        }
        *remaining -= 1;
        let object: Object = serde_json::from_str(json)?;
        let name = |field| -> Result<String, DecodeError> {
            identifier(&serde_json::from_str::<String>(
                required(&object, field)?.get(),
            )?)
        };
        let state = || {
            bindings
                .codec
                .decode(required(&object, "block_state")?.get(), false)
                .map_err(|e| DecodeError::Registry(e.to_string()))
        };
        let block = || {
            let name = name("block")?;
            bindings
                .blocks
                .get(&name)
                .cloned()
                .ok_or_else(|| DecodeError::Registry(format!("unknown block {name}")))
        };
        // These native codecs use unrestricted FLOAT, including out-of-range
        // probabilities. Matching the state still consumes a draw at zero/one.
        let probability = || {
            number_text(required(&object, "probability")?)?
                .parse::<f32>()
                .map_err(|_| DecodeError::Field("probability"))
        };
        let node = match name("predicate_type")?.as_str() {
            "minecraft:always_true" => Node::True,
            "minecraft:block_match" => Node::States(block()?),
            "minecraft:blockstate_match" => Node::State(state()?),
            "minecraft:tag_match" => Node::States(
                bindings
                    .tags
                    .get(&super::tags::identifier(&serde_json::from_str::<String>(
                        required(&object, "tag")?.get(),
                    )?)?)
                    .cloned()
                    .unwrap_or_default(),
            ),
            "minecraft:random_block_match" => Node::RandomStates(block()?, probability()?),
            "minecraft:random_blockstate_match" => Node::RandomState(state()?, probability()?),
            "minecraft:height_match" => Node::Height(
                integer(required(&object, "min_inclusive")?)?,
                integer(required(&object, "max_inclusive")?)?,
            ),
            "minecraft:not" => Node::Not(Box::new(Self::decode(
                required(&object, "rule")?.get(),
                bindings,
                depth + 1,
                remaining,
            )?)),
            kind @ ("minecraft:all_of" | "minecraft:any_of") => {
                let values: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&object, "rules")?.get())?;
                if values.len() > 8192 {
                    return Err(DecodeError::Field("replacement rule list"));
                }
                let values = values
                    .iter()
                    .map(|v| Self::decode(v.get(), bindings, depth + 1, remaining))
                    .collect::<Result<_, _>>()?;
                if kind == "minecraft:all_of" {
                    Node::All(values)
                } else {
                    Node::Any(values)
                }
            }
            "horizon:matter_phase" => {
                Node::Phase(Box::new(MatterPhase::decode(&object, bindings)?))
            }
            _ => return Err(DecodeError::Field("replacement rule type")),
        };
        Ok(Self(node))
    }
    /// The Y ranges of its height tests (its only use of the position),
    /// or `None` when a test draws from the random source: then the result
    /// is not a function of the state and those ranges alone.
    pub(crate) fn pure_heights(&self, out: &mut Vec<(i32, i32)>) -> Option<()> {
        match &self.0 {
            Node::True | Node::States(_) | Node::State(_) | Node::Phase(_) => Some(()),
            Node::RandomStates(..) | Node::RandomState(..) => None,
            Node::Height(min, max) => {
                if !out.contains(&(*min, *max)) {
                    out.push((*min, *max));
                }
                Some(())
            }
            Node::Not(rule) => rule.pure_heights(out),
            Node::All(rules) | Node::Any(rules) => {
                rules.iter().try_for_each(|rule| rule.pure_heights(out))
            }
        }
    }
    pub fn test(
        &self,
        state: BlockStateId,
        position: [i32; 3],
        random: &mut impl RandomSource,
    ) -> bool {
        match &self.0 {
            Node::True => true,
            Node::Not(rule) => !rule.test(state, position, random),
            Node::All(rules) => rules.iter().all(|rule| rule.test(state, position, random)),
            Node::Any(rules) => rules.iter().any(|rule| rule.test(state, position, random)),
            Node::States(states) => states.contains(&state),
            Node::State(expected) => state == *expected,
            Node::RandomStates(states, probability) => {
                states.contains(&state) && random.next_float() < *probability
            }
            Node::RandomState(expected, probability) => {
                state == *expected && random.next_float() < *probability
            }
            Node::Height(min, max) => *min <= position[1] && position[1] <= *max,
            Node::Phase(phase) => phase.test(state),
        }
    }
}
