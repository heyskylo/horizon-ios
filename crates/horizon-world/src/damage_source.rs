//! Version-checked damage definitions/tags and ordinary source identity queries.
//! Catalogs are immutable snapshots; runtime registry reload and wire IDs remain
//! separate from these local definition indices.
use crate::{
    DATA_VERSION,
    compression::invalid,
    worldgen::tags::{TagDefinitions, identifier},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};
mod resources;
pub use resources::{resource_files, resource_tags};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageTypeId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    Never,
    LivingNonPlayer,
    Always,
}
impl Scaling {
    pub fn name(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::LivingNonPlayer => "when_caused_by_living_non_player",
            Self::Always => "always",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub name: String,
    pub message_id: String,
    pub exhaustion: f32,
    pub scaling: Scaling,
    pub effects: String,
    pub death_message_type: String,
}
#[derive(Clone, Debug)]
pub struct Catalog {
    definitions: Vec<Definition>,
    ids: BTreeMap<String, DamageTypeId>,
    tags: BTreeMap<String, BTreeSet<DamageTypeId>>,
}
fn string(v: &Value) -> io::Result<&str> {
    v.as_str()
        .ok_or_else(|| invalid("expected damage catalog string"))
}
impl Catalog {
    /// Imports native decoded definitions and resolves raw tag resources with
    /// the shared tag implementation. No data-pack or network IDs are invented.
    pub fn from_json(v: &Value, resources: &Value) -> io::Result<Self> {
        if v["data_version"].as_i64() != Some(i64::from(DATA_VERSION)) {
            return Err(invalid("damage catalog data version mismatch"));
        }
        let mut definitions = Vec::new();
        let mut ids = BTreeMap::new();
        for d in v["types"]
            .as_array()
            .ok_or_else(|| invalid("missing damage definitions"))?
        {
            let name = string(&d["id"])?;
            if identifier(name).map_err(|e| invalid(e.to_string()))? != name
                || ids
                    .insert(name.into(), DamageTypeId(definitions.len()))
                    .is_some()
            {
                return Err(invalid("noncanonical or duplicate damage type"));
            }
            let bits = string(&d["exhaustion"])?;
            if bits.len() != 8 {
                return Err(invalid("expected damage exhaustion float bits"));
            }
            let exhaustion =
                f32::from_bits(u32::from_str_radix(bits, 16).map_err(|e| invalid(e.to_string()))?);
            let scaling = match string(&d["scaling"])? {
                "never" => Scaling::Never,
                "when_caused_by_living_non_player" => Scaling::LivingNonPlayer,
                "always" => Scaling::Always,
                _ => return Err(invalid("unknown damage scaling")),
            };
            let effects = string(&d["effects"])?;
            if ![
                "hurt", "thorns", "drowning", "burning", "poking", "freezing",
            ]
            .contains(&effects)
            {
                return Err(invalid("unknown damage effects"));
            }
            let death = string(&d["death_message_type"])?;
            if !["default", "fall_variants", "intentional_game_design"].contains(&death) {
                return Err(invalid("unknown death message type"));
            }
            definitions.push(Definition {
                name: name.into(),
                message_id: string(&d["message_id"])?.into(),
                exhaustion,
                scaling,
                effects: effects.into(),
                death_message_type: death.into(),
            });
        }
        let mut tags = TagDefinitions::default();
        for (id, json) in resources
            .as_object()
            .ok_or_else(|| invalid("expected damage tag resources"))?
        {
            tags.apply_json(id, "vanilla", string(json)?)
                .map_err(|e| invalid(e.to_string()))?;
        }
        let tags = tags
            .build(|id, _| ids.get(id).copied())
            .map_err(|e| invalid(e.to_string()))?;
        if !tags.missing.is_empty() {
            return Err(invalid("unresolved required damage tag references"));
        }
        Ok(Self {
            definitions,
            ids,
            tags: tags
                .tags
                .into_iter()
                .map(|(id, v)| (id, v.into_iter().collect()))
                .collect(),
        })
    }
    pub fn id(&self, name: &str) -> Option<DamageTypeId> {
        self.ids.get(name).copied()
    }
    pub fn definition(&self, id: DamageTypeId) -> Option<&Definition> {
        self.definitions.get(id.0)
    }
    pub fn entries(&self) -> impl Iterator<Item = (DamageTypeId, &Definition)> {
        self.definitions
            .iter()
            .enumerate()
            .map(|(i, d)| (DamageTypeId(i), d))
    }
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.keys().map(String::as_str)
    }
    pub fn in_tag(&self, id: DamageTypeId, tag: &str) -> bool {
        self.tags.get(tag).is_some_and(|v| v.contains(&id))
    }
    pub fn source<E: Copy + Eq>(
        &self,
        id: DamageTypeId,
        direct: Option<E>,
        causing: Option<E>,
        position: Option<[f64; 3]>,
    ) -> Option<DamageSource<'_, E>> {
        self.definition(id)?;
        Some(DamageSource {
            catalog: self,
            id,
            direct,
            causing,
            position,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Other,
    Living,
    Player,
}
pub trait SourceWorld {
    type Error;
    type Entity: Copy + Eq;
    type Weapon;
    fn kind(&self, entity: Self::Entity) -> EntityKind;
    fn weapon(&mut self, entity: Self::Entity) -> Result<Option<Self::Weapon>, Self::Error>;
    fn position(&mut self, entity: Self::Entity) -> Result<[f64; 3], Self::Error>;
    /// Virtual getEntity dispatch is used by the creative-source predicate;
    /// difficulty scaling deliberately reads the stored causing entity instead.
    fn causing_entity(
        &mut self,
        stored: Option<Self::Entity>,
    ) -> Result<Option<Self::Entity>, Self::Error>;
    fn instabuild(&mut self, player: Self::Entity) -> Result<bool, Self::Error>;
}
#[derive(Clone, Copy, Debug)]
pub struct DamageSource<'a, E> {
    catalog: &'a Catalog,
    id: DamageTypeId,
    direct: Option<E>,
    causing: Option<E>,
    position: Option<[f64; 3]>,
}
impl<E: Copy + Eq> DamageSource<'_, E> {
    pub fn definition(&self) -> &Definition {
        &self.catalog.definitions[self.id.0]
    }
    pub fn type_id(&self) -> DamageTypeId {
        self.id
    }
    pub fn is_type(&self, name: &str) -> bool {
        self.definition().name == name
    }
    pub fn in_tag(&self, tag: &str) -> bool {
        self.catalog.in_tag(self.id, tag)
    }
    pub fn bypasses(&self, kind: crate::damage_reduction::Bypass) -> bool {
        use crate::damage_reduction::Bypass;
        self.in_tag(match kind {
            Bypass::Armor => "minecraft:bypasses_armor",
            Bypass::Effects => "minecraft:bypasses_effects",
            Bypass::Resistance => "minecraft:bypasses_resistance",
            Bypass::Enchantments => "minecraft:bypasses_enchantments",
        })
    }
    pub fn hurt_tag(&self, tag: crate::living_hurt::HurtTag) -> bool {
        use crate::living_hurt::HurtTag;
        self.in_tag(match tag {
            HurtTag::Fire => "minecraft:is_fire",
            HurtTag::Freezing => "minecraft:is_freezing",
            HurtTag::DamagesHelmet => "minecraft:damages_helmet",
            HurtTag::BypassesCooldown => "minecraft:bypasses_cooldown",
            HurtTag::NoImpact => "minecraft:no_impact",
            HurtTag::NoKnockback => "minecraft:no_knockback",
        })
    }
    pub fn immunity_tag(&self, tag: crate::damage_immunity::ImmunityTag) -> bool {
        use crate::damage_immunity::ImmunityTag;
        self.in_tag(match tag {
            ImmunityTag::BypassesInvulnerability => "minecraft:bypasses_invulnerability",
            ImmunityTag::Fire => "minecraft:is_fire",
            ImmunityTag::Fall => "minecraft:is_fall",
            ImmunityTag::Drowning => "minecraft:is_drowning",
            ImmunityTag::Freezing => "minecraft:is_freezing",
        })
    }
    pub fn direct_entity(&self) -> Option<E> {
        self.direct
    }
    pub fn causing_entity(&self) -> Option<E> {
        self.causing
    }
    pub fn is_direct(&self) -> bool {
        self.direct == self.causing
    }
    pub fn position_raw(&self) -> Option<[f64; 3]> {
        self.position
    }
    pub fn food_exhaustion(&self) -> f32 {
        self.definition().exhaustion
    }
    pub fn scales_with_difficulty<W: SourceWorld<Entity = E>>(&self, w: &W) -> bool {
        match self.definition().scaling {
            Scaling::Never => false,
            Scaling::Always => true,
            Scaling::LivingNonPlayer => self
                .causing
                .is_some_and(|e| w.kind(e) == EntityKind::Living),
        }
    }
    pub fn weapon<W: SourceWorld<Entity = E>>(
        &self,
        w: &mut W,
    ) -> Result<Option<W::Weapon>, W::Error> {
        match self.direct {
            Some(e) => w.weapon(e),
            None => Ok(None),
        }
    }
    pub fn position<W: SourceWorld<Entity = E>>(
        &self,
        w: &mut W,
    ) -> Result<Option<[f64; 3]>, W::Error> {
        if let Some(p) = self.position {
            return Ok(Some(p));
        }
        match self.direct {
            Some(e) => w.position(e).map(Some),
            None => Ok(None),
        }
    }
    pub fn creative_player<W: SourceWorld<Entity = E>>(&self, w: &mut W) -> Result<bool, W::Error> {
        match w.causing_entity(self.causing)? {
            Some(e) if w.kind(e) == EntityKind::Player => w.instabuild(e),
            _ => Ok(false),
        }
    }
}
