//! Resolved fluid holder sets bound to immutable vanilla block-state identities.
use super::{DecodeError, density::identifier};
use crate::{
    fluid::{FluidKind, FluidStateTable},
    registry::{BlockRegistry, BlockStateId},
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub struct FluidPredicateBindings {
    table: Arc<FluidStateTable>,
    tags: BTreeMap<String, crate::fluid::FluidMask>,
}
impl FluidPredicateBindings {
    pub fn new(registry: &BlockRegistry) -> Result<Self, DecodeError> {
        Ok(Self {
            table: Arc::new(
                FluidStateTable::new(registry).map_err(|e| DecodeError::Registry(e.to_string()))?,
            ),
            tags: BTreeMap::new(),
        })
    }
    pub fn with_resolved_tags(
        mut self,
        tags: &BTreeMap<String, Vec<String>>,
    ) -> Result<Self, DecodeError> {
        if tags.len() > 8192 {
            return Err(DecodeError::Field("fluid tag count"));
        }
        let mut resolved = BTreeMap::new();
        for (tag, names) in tags {
            if names.len() > 8192 {
                return Err(DecodeError::Field("fluid tag size"));
            }
            let mut mask = 0;
            for name in names {
                mask |= fluid(name)?.mask();
            }
            if resolved
                .insert(super::tags::identifier(tag)?, mask)
                .is_some()
            {
                return Err(DecodeError::Field("duplicate fluid tag"));
            }
        }
        self.tags = resolved;
        Ok(self)
    }
    pub(crate) fn table(&self) -> &Arc<FluidStateTable> {
        &self.table
    }
    pub fn state(&self, block: BlockStateId) -> Option<crate::fluid::FluidState> {
        self.table.get(block)
    }
    /// Missing tags have no members, as for a native tag-key membership query.
    pub fn state_matches_tag(&self, block: BlockStateId, tag: &str) -> bool {
        self.table.get(block).is_some_and(|state| {
            self.tags
                .get(tag)
                .is_some_and(|mask| state.kind().mask() & mask != 0)
        })
    }
    pub fn resolve(&self, json: &str) -> Result<BoundFluidSet, DecodeError> {
        let mask = if let Ok(name) = serde_json::from_str::<String>(json) {
            if let Some(tag) = name.strip_prefix('#') {
                *self
                    .tags
                    .get(&super::tags::identifier(tag)?)
                    .ok_or_else(|| DecodeError::Registry(format!("unknown fluid tag {tag}")))?
            } else {
                fluid(&name)?.mask()
            }
        } else {
            let names: Vec<String> = serde_json::from_str(json)?;
            if names.len() > 8192 {
                return Err(DecodeError::Field("fluid holder list"));
            }
            let mut mask = 0;
            for name in names {
                mask |= fluid(&name)?.mask();
            }
            mask
        };
        Ok(BoundFluidSet {
            table: self.table.clone(),
            mask,
        })
    }
}
fn fluid(name: &str) -> Result<FluidKind, DecodeError> {
    FluidKind::from_name(&identifier(name)?)
        .ok_or_else(|| DecodeError::Registry(format!("unknown fluid {name}")))
}

/// A fluid-family mask over one shared immutable state table. Membership needs
/// one indexed lookup and avoids allocating large sets of equivalent states.
#[derive(Clone, Debug)]
pub struct BoundFluidSet {
    table: Arc<FluidStateTable>,
    mask: crate::fluid::FluidMask,
}
impl BoundFluidSet {
    pub fn contains(&self, state: BlockStateId) -> bool {
        self.table
            .get(state)
            .is_some_and(|value| value.kind().mask() & self.mask != 0)
    }
}
