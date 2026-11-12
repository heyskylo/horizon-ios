//! Vanilla 26.3 entity NBT (`Entity.saveWithoutId` and the subclasses'
//! `addAdditionalSaveData`) for the entities Horizon simulates: mobs, item
//! entities, experience orbs, arrows and falling blocks. Encoding starts from
//! the compound the entity was loaded with, when there is one, so fields
//! Horizon does not model (variants, attributes, equipment, custom names)
//! survive a Horizon save.
use crate::{
    arrows::Pickup,
    entities::{Cushion, ExperienceOrb, ItemEntity, ItemFrame, Painting},
    mobs::{Mob, MobType, Who},
    nbt_path::int_value,
    projectiles::{Projectile, ProjectileKind},
    save::{
        doubles, float, floats, item_from_nbt, item_to_nbt, number, put, read_doubles, text,
        uuid_from_tag, uuid_tag,
    },
};
use horizon_core::geometry::Vec3;
use horizon_world::{
    chunk::BlockState,
    nbt::{Compound, Tag},
    registry::{BlockRegistry, BlockStateId},
};
use std::{collections::HashMap, sync::Arc};
mod boat_load;
pub use boat_load::boat_from_nbt;
mod projectile_fields;
use projectile_fields::{load_projectile_entity_fields, put_projectile_entity_fields};
mod saved_attributes;

/// Saved data carried by `ConversionType.convertCommon` outside the modelled
/// mob fields. The converted entity has its own identity and type-specific data;
/// only its command tags are inherited from the old entity's raw compound.
pub fn conversion_tags(raw: Option<&Tag>) -> Option<Tag> {
    let Tag::List(tags) = raw?.get("Tags")? else {
        return None;
    };
    (!tags.is_empty()).then(|| Tag::compound([("Tags", Tag::List(tags.clone()))]))
}

fn string(s: &str) -> Tag {
    Tag::String(s.into())
}
mod refs;
pub use refs::who_by_uuid;
/// Entity type ID (`id`).
pub fn kind(tag: &Tag) -> Option<String> {
    horizon_client_core::identifier::normalize_id(&text(tag.get("id"))?)
}
pub fn position(tag: &Tag) -> Option<Vec3> {
    read_doubles::<3>(tag.get("Pos")).map(|p| Vec3::new(p[0], p[1], p[2]))
}
pub fn uuid(tag: &Tag) -> Option<[i32; 4]> {
    uuid_from_tag(tag.get("UUID"))
}
pub fn motion(tag: &Tag) -> Vec3 {
    read_doubles::<3>(tag.get("Motion")).map_or(Vec3::ZERO, |m| Vec3::new(m[0], m[1], m[2]))
}
/// A saved Motion as `Entity.load` takes it: a component faster than 10 blocks
/// a tick loads as 0.
pub fn loaded_motion(tag: &Tag) -> Vec3 {
    let m = motion(tag);
    let clamp = |v: f64| if v.abs() > 10.0 { 0.0 } else { v };
    Vec3::new(clamp(m.x), clamp(m.y), clamp(m.z))
}
pub fn rotation(tag: &Tag) -> [f32; 2] {
    read_doubles::<2>(tag.get("Rotation")).map_or([0.0; 2], |r| [r[0] as f32, r[1] as f32])
}
/// `TagValueInput.getBooleanOr`: a numeric tag's byteValue is nonzero, with a false default.
pub fn flag(tag: &Tag, key: &str) -> bool {
    flag_or(tag, key, false)
}
/// `getBooleanOr(key, default)`: any numeric tag's nonzero test, else `default`.
pub fn flag_or(tag: &Tag, key: &str, default: bool) -> bool {
    // NBT floats floor before int narrowing; the final byte keeps the low bits.
    tag.get(key)
        .and_then(int_value)
        .map_or(default, |value| value as i8 != 0)
}

/// `Entity.saveWithoutId`: write the movement flags and the flags retained
/// from `Entity.load` as booleans. Only OnGround and Invulnerable write false.
pub fn write_saved_flags(tag: &mut Tag, on_ground: bool, no_gravity: bool) {
    let invulnerable = flag(tag, "Invulnerable");
    let optional = [
        ("NoGravity", no_gravity),
        ("CustomNameVisible", flag(tag, "CustomNameVisible")),
        ("Silent", flag(tag, "Silent")),
        ("Glowing", flag(tag, "Glowing")),
        ("HasVisualFire", flag(tag, "HasVisualFire")),
    ];
    let Tag::Compound(map) = tag else {
        return;
    };
    map.insert("OnGround".into(), Tag::Byte(i8::from(on_ground)));
    map.insert("Invulnerable".into(), Tag::Byte(i8::from(invulnerable)));
    for (key, value) in optional {
        let key = key.into();
        if value {
            map.insert(key, Tag::Byte(1));
        } else {
            map.remove(&key);
        }
    }
}

/// `Entity.saveWithoutId` fields every entity has, over the loaded compound.
struct Base<'a> {
    id: &'a str,
    uuid: [i32; 4],
    position: Vec3,
    motion: Vec3,
    rotation: [f32; 2],
    on_ground: bool,
    fall_distance: f64,
}
impl Base<'_> {
    fn tag(&self, raw: Option<&Tag>) -> Tag {
        let mut tag = match raw {
            Some(t @ Tag::Compound(_)) => t.clone(),
            _ => Tag::Compound(Compound::new()),
        };
        for (key, value) in [
            ("id", string(self.id)),
            ("UUID", Tag::IntArray(self.uuid.to_vec())),
            (
                "Pos",
                doubles(&[self.position.x, self.position.y, self.position.z]),
            ),
            (
                "Motion",
                doubles(&[self.motion.x, self.motion.y, self.motion.z]),
            ),
            ("Rotation", floats(&self.rotation)),
            ("OnGround", Tag::Byte(i8::from(self.on_ground))),
            ("fall_distance", Tag::Double(self.fall_distance.to_bits())),
        ] {
            put(&mut tag, key, value);
        }
        if let Tag::Compound(map) = &mut tag {
            // Entity.saveWithoutId writes Tags only for a nonempty live set.
            if matches!(map.get(&"Tags".into()), Some(Tag::List(tags)) if tags.is_empty()) {
                map.remove(&"Tags".into());
            }
            for (key, value) in [
                // Entity.remainingFireTicks starts at 0 (-1 was the old default).
                ("Fire", Tag::Short(0)),
                ("Air", Tag::Short(300)),
                ("Invulnerable", Tag::Byte(0)),
                ("PortalCooldown", Tag::Int(0)),
            ] {
                map.entry(key.into()).or_insert(value);
            }
        }
        let no_gravity = flag(&tag, "NoGravity");
        write_saved_flags(&mut tag, self.on_ground, no_gravity);
        tag
    }
    /// Preserve Entity data without flags owned by Mob's save writer.
    /// Only the dedicated boat, item and armor-stand writers use this path.
    fn non_mob_tag(&self, raw: Option<&Tag>) -> Tag {
        let mut tag = self.tag(raw);
        if let Tag::Compound(map) = &mut tag {
            map.remove(&"NoAI".into());
            map.remove(&"PersistenceRequired".into());
        }
        tag
    }
}

/// `Entity.saveWithoutId`'s common fields for entities Horizon keeps as raw
/// compounds: each one is inserted only when the loaded compound has no value
/// for it, so explicit values survive.
pub fn fill_common_defaults(tag: &mut Tag) {
    let Tag::Compound(map) = tag else {
        return;
    };
    for (key, value) in [
        ("Motion", doubles(&[0.0, 0.0, 0.0])),
        ("OnGround", Tag::Byte(0)),
        ("fall_distance", Tag::Double(0.0f64.to_bits())),
        ("Air", Tag::Short(300)),
        ("Invulnerable", Tag::Byte(0)),
        ("PortalCooldown", Tag::Int(0)),
    ] {
        map.entry(key.into()).or_insert(value);
    }
}

/// The mobs that save `Age` and `AgeLocked`.
fn saves_age(s: crate::mobs::Species) -> bool {
    use crate::mobs::{Species, finalize::ageable, species::can_be_a_baby};
    s == Species::Tadpole || (ageable(s) && can_be_a_baby(s))
}
/// `LivingEntity` + `Mob` (+ `AgeableMob`, creeper and skeleton specifics).
/// The mobs whose class extends vanilla's `Animal` (and so save `InLove`):
/// not allays, villagers, golems, dolphins or fish.
fn is_animal(id: &str) -> bool {
    matches!(
        id.strip_prefix("minecraft:").unwrap_or(id),
        "pig"
            | "cow"
            | "mooshroom"
            | "sheep"
            | "chicken"
            | "rabbit"
            | "horse"
            | "donkey"
            | "mule"
            | "zombie_horse"
            | "skeleton_horse"
            | "llama"
            | "trader_llama"
            | "goat"
            | "panda"
            | "polar_bear"
            | "fox"
            | "wolf"
            | "cat"
            | "ocelot"
            | "parrot"
            | "bee"
            | "turtle"
            | "armadillo"
            | "frog"
            | "axolotl"
            | "hoglin"
            | "strider"
            | "camel"
            | "camel_husk"
            | "sniffer"
            | "happy_ghast"
            | "nautilus"
            | "zombie_nautilus"
    )
}
use crate::mobs::persistent_anger;
/// The single player's UUID (`offline_uuid("Player")`): the entity the
/// neutral mobs are angry at.
pub(crate) fn player_uuid() -> [i32; 4] {
    let u = horizon_protocol::crypto::offline_uuid("Player");
    [
        (u >> 96) as u32 as i32,
        (u >> 64) as u32 as i32,
        (u >> 32) as u32 as i32,
        u as u32 as i32,
    ]
}
/// The UUID a save writes for a reference to `who`. A mob is only an id here:
/// its UUID lives in the game layer (`EntityMemory.uuids`), which is what
/// `uuid_of` answers from. When it answers `None` (the mob is no longer in the
/// level) nothing is written, rather than a made-up UUID that would name nothing
/// when the save is read back. The reference comes back with [`who_by_uuid`].
///
/// `mob_to_nbt`'s `uuid_of` is this for a field of a mob's own compound; the
/// game layer's `Game::save_who_uuid` is the same rule for a field written
/// outside one (a bullet's `Target`), answered from `EntityMemory::uuid_of`.
pub fn who_uuid(who: Who, uuid_of: &mut impl FnMut(u32) -> Option<[i32; 4]>) -> Option<[i32; 4]> {
    match who {
        Who::Player => Some(player_uuid()),
        Who::Mob(id) => uuid_of(id),
    }
}
/// Replace every reference to the player `old` in a saved entity with `new`. Older
/// Horizon worlds named their single player by the offline UUID (`Owner` on tamed
/// animals and horses, a projectile's `Owner` and an item's `Thrower`, a fox's
/// `Trusted`, `angry_at`, a leash's `UUID`); when such a world opens under the
/// account (`lan::playerdata::migrate_owner`), a chunk's entities are rewritten as
/// they load, and their next save keeps the account's UUID. A reference is the
/// player's four ints, wherever the entity keeps it; an entity's own `UUID` never
/// names the player.
pub fn migrate_player_uuid(tag: &mut Tag, old: [i32; 4], new: [i32; 4]) {
    if old == new {
        return;
    }
    match tag {
        Tag::IntArray(uuid) if uuid.as_slice() == old.as_slice() => uuid.copy_from_slice(&new),
        Tag::List(list) => {
            for tag in list {
                migrate_player_uuid(tag, old, new);
            }
        }
        Tag::Compound(map) => {
            for tag in map.values_mut() {
                migrate_player_uuid(tag, old, new);
            }
        }
        _ => {}
    }
}
/// `AngerManagement.SUSPECT_ANGER_PAIR`: one suspect's UUID and how angry the
/// warden is at it.
fn suspect_anger(uuid: [i32; 4], anger: i32) -> Tag {
    Tag::compound([
        ("uuid", Tag::IntArray(uuid.to_vec())),
        ("anger", Tag::Int(anger)),
    ])
}
/// `AngerManagement.codec`: `SUSPECT_ANGER_PAIR.listOf()` under `suspects`, in
/// the order `createUuidAngerPairs` builds it - the suspects that are in the
/// level, by the UUID each entity has, then the ones that are not, which are
/// already UUIDs. Nothing else is stored: the highest anger, the conversion
/// delay and the `NeutralMob` anger times are runtime only. An anger with no
/// suspect at all writes no `suspects` key, `lenientOptionalFieldOf`'s default.
fn warden_anger_to_nbt(
    anger: &crate::mobs::warden::AngerManagement,
    uuid_of: &mut impl FnMut(u32) -> Option<[i32; 4]>,
) -> Tag {
    let loaded = anger.suspects().iter().filter_map(|who| {
        let uuid = who_uuid(*who, uuid_of)?;
        Some(suspect_anger(uuid, anger.anger_of(*who)))
    });
    let away = anger
        .unloaded()
        .map(|(uuid, anger)| suspect_anger(uuid, anger));
    let pairs: Vec<Tag> = loaded.chain(away).collect();
    if pairs.is_empty() {
        Tag::compound(Vec::<(&str, Tag)>::new())
    } else {
        Tag::compound([("suspects", Tag::List(pairs))])
    }
}
/// [`warden_anger_to_nbt`] read back: every pair goes into the UUID map, where
/// the anger ticks resolve the ones whose entity is in the level again. A pair
/// with no four-int UUID or a negative anger drops the whole list, as
/// `lenientOptionalFieldOf` does with a value that does not decode.
fn warden_anger_from_nbt(tag: &Tag) -> Vec<([i32; 4], i32)> {
    let Some(Tag::List(pairs)) = tag.get("suspects") else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let (Some(uuid), Some(anger)) =
            (uuid_from_tag(pair.get("uuid")), number(pair.get("anger")))
        else {
            return Vec::new();
        };
        let Ok(anger) = i32::try_from(anger) else {
            return Vec::new();
        };
        if anger < 0 {
            return Vec::new();
        }
        out.push((uuid, anger));
    }
    out
}
/// `ResolvableProfile.CODEC`: the name, id and properties of a profile, then the
/// skin's texture ids and model (`PlayerSkin.Patch`: "texture", "cape", "elytra", "model").
fn profile_nbt(p: &horizon_protocol::entity_data::Profile) -> Tag {
    let mut entries: Vec<(&'static str, Tag)> = Vec::new();
    if let Some(name) = &p.name {
        entries.push(("name", string(name)));
    }
    if let Some(id) = p.uuid {
        entries.push((
            "id",
            Tag::IntArray(vec![
                (id >> 96) as i32,
                (id >> 64) as i32,
                (id >> 32) as i32,
                id as i32,
            ]),
        ));
    }
    if !p.properties.is_empty() {
        let properties = p
            .properties
            .iter()
            .map(|property| {
                let mut entries: Vec<(&'static str, Tag)> = vec![
                    ("name", string(&property.name)),
                    ("value", string(&property.value)),
                ];
                if let Some(signature) = &property.signature {
                    entries.push(("signature", string(signature)));
                }
                Tag::compound(entries)
            })
            .collect();
        entries.push(("properties", Tag::List(properties)));
    }
    for (key, texture) in [
        ("texture", &p.body),
        ("cape", &p.cape),
        ("elytra", &p.elytra),
    ] {
        if let Some(texture) = texture {
            entries.push((key, string(texture)));
        }
    }
    if let Some(slim) = p.slim {
        entries.push(("model", string(if slim { "slim" } else { "wide" })));
    }
    Tag::compound(entries)
}
/// `ResolvableProfile.CODEC` read back: the compound `profile_nbt` writes, or a
/// bare player name (`Codec.withAlternative(FULL_CODEC, PLAYER_NAME, ...)`).
fn profile_from_nbt(tag: &Tag) -> Option<horizon_protocol::entity_data::Profile> {
    if let Tag::String(name) = tag {
        return Some(horizon_protocol::entity_data::Profile {
            name: Some(name.to_string_lossy()),
            uuid: None,
            properties: Vec::new(),
            body: None,
            cape: None,
            elytra: None,
            slim: None,
        });
    }
    let property = |name: String, value: &Tag, signature: Option<String>| match value {
        Tag::String(v) => Some(horizon_protocol::packets::login::Property {
            name,
            value: v.to_string_lossy(),
            signature,
        }),
        _ => None,
    };
    let properties: Vec<horizon_protocol::packets::login::Property> = match tag.get("properties") {
        // `ExtraCodecs.PROPERTY_MAP`: a list of properties ...
        Some(Tag::List(list)) => list
            .iter()
            .filter_map(|p| {
                let name = match p.get("name")? {
                    Tag::String(s) => s.to_string_lossy(),
                    _ => return None,
                };
                let signature = match p.get("signature") {
                    Some(Tag::String(s)) => Some(s.to_string_lossy()),
                    _ => None,
                };
                property(name, p.get("value")?, signature)
            })
            .collect(),
        // ... or a map from a name to its values.
        Some(Tag::Compound(map)) => map
            .iter()
            .flat_map(|(name, values)| match values {
                Tag::List(values) => values
                    .iter()
                    .filter_map(|v| property(name.to_string_lossy(), v, None))
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    };
    let text = |key: &str| match tag.get(key) {
        Some(Tag::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    Some(horizon_protocol::entity_data::Profile {
        name: text("name"),
        uuid: uuid_from_tag(tag.get("id")).map(|a| {
            a.iter()
                .fold(0u128, |u, &x| (u << 32) | u128::from(x as u32))
        }),
        properties,
        body: text("texture"),
        cape: text("cape"),
        elytra: text("elytra"),
        slim: text("model").map(|model| model == "slim"),
    })
}
/// `ConversionTracker.addAdditionalSaveData`: vanilla's conversion time, -1
/// while the mob is not converting. `counter2` is that time plus one while it
/// converts and 0 when it does not (see `mobs/undead.rs`).
fn conversion_time(m: &Mob) -> i32 {
    if m.extra.counter2 > 0 {
        m.extra.counter2 - 1
    } else {
        -1
    }
}
/// `BlockState.CODEC`: the block's name when the state is that block's
/// default, `{id, properties}` for any other state.
pub fn state_to_nbt(registry: &BlockRegistry, state: BlockStateId) -> Tag {
    match registry.state(state) {
        Some(s) if registry.default_state(&s.name) == Some(state) => string(&s.name),
        Some(s) => s.to_nbt(),
        None => string("minecraft:air"),
    }
}
/// `BlockState.CODEC` read back: a block name or `{id, properties}` (or the
/// legacy `{Name, Properties}`). A name the registry knows only as a block
/// reads as its default state, and one it does not know at all as `None`.
pub fn state_from_nbt(registry: &BlockRegistry, tag: &Tag) -> Option<BlockStateId> {
    let state = match tag.get("Name") {
        Some(Tag::String(name)) => BlockState {
            name: name.to_string_lossy(),
            properties: match tag.get("Properties") {
                Some(Tag::Compound(p)) => p
                    .iter()
                    .filter_map(|(k, v)| match v {
                        Tag::String(v) => Some((k.to_string_lossy(), v.to_string_lossy())),
                        _ => None,
                    })
                    .collect(),
                _ => Default::default(),
            },
        },
        _ => BlockState::from_nbt(tag).ok()?,
    };
    registry
        .id(&state)
        .or_else(|| registry.default_state(&state.name))
}
/// `BlockState.isAir`: air, cave air and void air.
fn is_air(registry: &BlockRegistry, state: BlockStateId) -> bool {
    registry.state(state).is_some_and(|s| {
        matches!(
            s.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    })
}
/// The Mob codec's existing numeric reads, recursively for the same effect.
fn mob_hidden_effect(tag: &Tag, id: &'static str) -> Option<crate::effects::EffectInstance> {
    if !matches!(tag, Tag::Compound(_)) {
        return None;
    }
    let visible = number(tag.get("show_particles")).is_none_or(|v| v != 0);
    Some(crate::effects::EffectInstance {
        id,
        duration: number(tag.get("duration")).unwrap_or(0) as i32,
        amplifier: number(tag.get("amplifier")).unwrap_or(0) as i32,
        ambient: number(tag.get("ambient")).is_some_and(|v| v != 0),
        visible,
        show_icon: number(tag.get("show_icon")).map_or(visible, |v| v != 0),
        hidden: tag
            .get("hidden_effect")
            .and_then(|h| mob_hidden_effect(h, id))
            .map(Box::new),
    })
}
/// `MobEffectInstance.Details`: the active tuple and its modeled weaker nodes.
fn mob_effect_tag(e: &crate::effects::EffectInstance, root: bool) -> Tag {
    let mut fields = Vec::new();
    if root {
        fields.push(("id", string(crate::brewing::namespaced(e.id))));
    }
    if e.amplifier != 0 {
        fields.push((
            "amplifier",
            Tag::Byte(e.amplifier.clamp(0, 255) as u8 as i8),
        ));
    }
    fields.push(("duration", Tag::Int(e.duration)));
    if e.ambient {
        fields.push(("ambient", Tag::Byte(1)));
    }
    if !e.visible {
        fields.push(("show_particles", Tag::Byte(0)));
    }
    fields.push(("show_icon", Tag::Byte(i8::from(e.show_icon))));
    if let Some(hidden) = e.hidden.as_deref() {
        fields.push(("hidden_effect", mob_effect_tag(hidden, false)));
    }
    Tag::compound(fields)
}

/// The mob's entity compound. `registry` is what writes a field holding a block
/// state (an enderman's `carriedBlockState`): without one, a caller that has no
/// resources (unit tests, structure templates) keeps whatever the loaded
/// compound had for it.
///
/// `uuid_of` is how a field that names another entity by UUID (a mob's
/// `angry_at`) is written: it answers the UUID of a mob by its id, from the game
/// layer's map, and `None` for one the level no longer has. A caller with no
/// such map passes `|_| None` and writes no reference.
pub fn mob_to_nbt(
    m: &Mob,
    uuid: [i32; 4],
    raw: Option<&Tag>,
    registry: Option<&BlockRegistry>,
    mut uuid_of: impl FnMut(u32) -> Option<[i32; 4]>,
) -> Tag {
    use crate::mobs::Species as S;
    let base = Base {
        id: &m.kind.id,
        uuid,
        position: m.position,
        motion: m.velocity,
        rotation: [m.yaw, m.head_pitch],
        on_ground: m.on_ground,
        fall_distance: m.fall_distance,
    };
    let mut tag = base.tag(raw);
    for (key, value) in [
        (
            "Fire",
            Tag::Short(m.fire_ticks.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16),
        ),
        ("Air", Tag::Short(m.air as i16)),
        ("Health", Tag::Float(m.health.max(0.0).to_bits())),
        ("AbsorptionAmount", Tag::Float(m.absorption.to_bits())),
        // LivingEntity.addAdditionalSaveData: (short) casts, nothing clamped.
        ("HurtTime", Tag::Short(m.hurt_time as i16)),
        ("DeathTime", Tag::Short(m.death_time as i16)),
        // Mob.addAdditionalSaveData writes the current virtual getter, not raw input.
        ("CanPickUpLoot", Tag::Byte(i8::from(m.can_pick_up_loot()))),
        ("PersistenceRequired", Tag::Byte(i8::from(m.persistent))),
    ] {
        put(&mut tag, key, value);
    }
    // Mob.addAdditionalSaveData: hasHome means radius != -1, including -2.
    // Species writers below retain their own home_pos override.
    let Tag::Compound(map) = &mut tag else {
        unreachable!()
    };
    if let Some((pos, radius)) = m.ai.home.filter(|(_, radius)| *radius != -1) {
        map.insert("home_radius".into(), Tag::Int(radius));
        map.insert("home_pos".into(), Tag::IntArray(pos.to_vec()));
    } else {
        map.remove(&"home_radius".into());
        map.remove(&"home_pos".into());
    }
    // AbstractFish/Axolotl.addAdditionalSaveData always write the flag.
    // Tadpole.fromBucket is always true, and its setter ignores loaded input.
    let from_bucket = match m.species() {
        S::Tadpole => Some(true),
        S::Cod | S::Salmon | S::Pufferfish | S::TropicalFish | S::Axolotl => {
            Some(m.extra.from_bucket)
        }
        _ => None,
    };
    if let Some(value) = from_bucket {
        put(&mut tag, "FromBucket", Tag::Byte(i8::from(value)));
    }
    // Mob.addAdditionalSaveData: the death loot table, and its seed when set.
    if let Some(table) = &m.extra.death_loot_table {
        put(&mut tag, "DeathLootTable", string(table));
    }
    if m.extra.death_loot_seed != 0 {
        put(
            &mut tag,
            "DeathLootTableSeed",
            Tag::Long(m.extra.death_loot_seed),
        );
    }
    // `NeutralMob.addPersistentAngerSaveData`: the anger ends at an
    // absolute game time, and the target is the entity the mob is angry at.
    if persistent_anger(m.species()) {
        put(
            &mut tag,
            "anger_end_time",
            // The absolute value as it is, also zero or one already past.
            Tag::Long(m.extra.anger_end),
        );
        // `angry_at` only while there is an anger target: the UUID of the one
        // the level has (`uuid_of`). A mob target read from the save that is
        // not in the world (`Who::Mob(0)`) keeps the loaded UUID.
        match m.extra.angry_at {
            Some(crate::mobs::Who::Mob(0)) => {}
            target => match target.and_then(|who| who_uuid(who, &mut uuid_of)) {
                Some(uuid) => put(&mut tag, "angry_at", Tag::IntArray(uuid.to_vec())),
                None => {
                    if let Tag::Compound(map) = &mut tag {
                        map.remove(&"angry_at".into());
                    }
                }
            },
        }
    }
    // `Mannequin.addAdditionalSaveData`: the profile whose skin it wears, whether it
    // is `immovable`, and its description (the default is not written; none is `hide_description`).
    if m.species() == crate::mobs::Species::Mannequin {
        // The empty static profile (`DEFAULT_PROFILE`) when it has none.
        let profile = m
            .profile
            .as_ref()
            .map_or_else(|| Tag::Compound(Compound::new()), profile_nbt);
        put(&mut tag, "profile", profile);
        // `LAYERS_CODEC`: the hidden layers, in `PlayerModelPart` order.
        let hidden = MODEL_PARTS
            .iter()
            .enumerate()
            .filter(|(bit, _)| m.extra.hidden_model_parts & (1 << bit) != 0)
            .map(|(_, part)| string(part))
            .collect();
        put(&mut tag, "hidden_layers", Tag::List(hidden));
        let hand = if m.extra.main_hand_left {
            "left"
        } else {
            "right"
        };
        put(&mut tag, "main_hand", string(hand));
        let pose = match m.extra.avatar_pose {
            crate::pose::Pose::Crouching => "crouching",
            crate::pose::Pose::Swimming => "swimming",
            crate::pose::Pose::FallFlying => "fall_flying",
            crate::pose::Pose::Sleeping => "sleeping",
            _ => "standing",
        };
        put(&mut tag, "pose", string(pose));
        put(
            &mut tag,
            "immovable",
            Tag::Byte(i8::from(m.extra.immovable)),
        );
        match &m.extra.description {
            crate::mobs::MannequinDescription::Default => {}
            crate::mobs::MannequinDescription::Hidden => {
                put(&mut tag, "hide_description", Tag::Byte(1));
            }
            crate::mobs::MannequinDescription::Custom(component) => {
                put(&mut tag, "description", component.clone());
            }
        }
    }
    // `Entity.saveWithoutId` writes the flag only when set.
    if m.name_visible {
        put(&mut tag, "CustomNameVisible", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"CustomNameVisible".into());
    }
    // A styled name from the save stays unless the name changed.
    if custom_name(&tag) != m.custom_name
        && let Some(name) = &m.custom_name
    {
        put(&mut tag, "CustomName", Tag::String(name.as_str().into()));
    }
    let Tag::Compound(map) = &mut tag else {
        return tag;
    };
    for (key, on) in [
        ("NoAI", m.no_ai),
        ("Silent", m.silent),
        ("NoGravity", m.no_gravity),
        ("Glowing", m.glowing),
    ] {
        if on {
            map.insert(key.into(), Tag::Byte(1));
        } else {
            map.remove(&horizon_world::nbt::JavaString::from(key));
        }
    }
    // LivingEntity.addAdditionalSaveData: `active_effects` only when there are
    // any (MobEffectInstance.CODEC), including their modeled weaker instances.
    if m.extra.effects.is_empty() {
        map.remove(&horizon_world::nbt::JavaString::from("active_effects"));
    } else {
        let effects =
            (0..m.extra.effects.len()).map(|i| mob_effect_tag(&m.effect_instance(i), true));
        map.insert("active_effects".into(), Tag::List(effects.collect()));
    }
    map.insert(
        "Invulnerable".into(),
        Tag::Byte(i8::from(m.invulnerable_flag)),
    );
    // Entity.saveWithoutId: `invulnerableTime`, only while there is some (the
    // hurt cooldown is not saved).
    if m.invulnerable_time > 0 {
        map.insert("invulnerable_time".into(), Tag::Int(m.invulnerable_time));
    } else {
        map.remove(&horizon_world::nbt::JavaString::from("invulnerable_time"));
    }
    map.insert(
        "LeftHanded".into(),
        Tag::Byte(i8::from(m.extra.left_handed)),
    );
    // Keys of older formats that structure templates still carry.
    for key in [
        "HandItems",
        "ArmorItems",
        "HandDropChances",
        "ArmorDropChances",
        "Dimension",
        "HurtByTimestamp",
        "Leashed",
    ] {
        map.remove(&horizon_world::nbt::JavaString::from(key));
    }
    if m.ticks_frozen > 0 {
        map.insert("TicksFrozen".into(), Tag::Int(m.ticks_frozen));
    } else {
        map.remove(&horizon_world::nbt::JavaString::from("TicksFrozen"));
    }
    // LivingEntity.addAdditionalSaveData: `putBoolean(FallFlying, isFallFlying())`.
    map.insert(
        "FallFlying".into(),
        Tag::Byte(i8::from(m.extra.fall_flying)),
    );
    for (key, value) in [
        ("AbsorptionAmount", Tag::Float(0)),
        (
            "Brain",
            Tag::compound([("memories", Tag::Compound(Compound::new()))]),
        ),
    ] {
        map.entry(key.into()).or_insert(value);
    }
    map.insert(
        "current_impulse_context_reset_grace_time".into(),
        Tag::Int(m.impulse_grace_time),
    );
    // What the mob's class saves: a content mod's living type saves as its
    // template does (the `id` written above stays its own).
    let id = m.kind.class_id();
    // AgeableMob.addAdditionalSaveData, only if canBeABaby (not a frog or a
    // parrot); a tadpole saves its own age.
    let species = m.species();
    if saves_age(species) {
        map.insert("Age".into(), Tag::Int(m.extra.age));
        map.insert("AgeLocked".into(), Tag::Byte(i8::from(m.extra.age_locked)));
        if species != crate::mobs::Species::Tadpole {
            map.insert("ForcedAge".into(), Tag::Int(m.extra.forced_age));
        }
    } else if crate::mobs::finalize::ageable(species) {
        // AgeableMob.addAdditionalSaveData writes nothing when canBeABaby is
        // false — the frog, the parrot, the zombie nautilus, the camel husk,
        // the slime and the magma cube: drop the ignored values a raw input
        // compound carried rather than writing them back out.
        for key in ["Age", "ForcedAge", "AgeLocked"] {
            map.remove(&key.into());
        }
    }
    // Animal.addAdditionalSaveData: the love ticks left, whatever the save had.
    if is_animal(id) {
        map.insert("InLove".into(), Tag::Int(m.extra.in_love));
        // Animal.addAdditionalSaveData: the player who fed it.
        if let Some(cause) = m.extra.love_cause {
            map.insert("LoveCause".into(), Tag::IntArray(cause.to_vec()));
        } else {
            map.remove(&"LoveCause".into());
        }
    }
    // InventoryCarrier.writeInventoryToTag: the occupied stacks, even when empty.
    if matches!(id, "minecraft:pillager" | "minecraft:wandering_trader") {
        map.insert(
            "Inventory".into(),
            Tag::List(
                m.extra
                    .inventory
                    .iter()
                    .map(crate::save::item_to_nbt)
                    .collect(),
            ),
        );
    }
    let species = m.species();
    // `VariantUtils.writeVariant`: only the species that store the name. A
    // mooshroom saves `Type` (below), a fox `Type`, a horse and a llama a
    // packed `Variant` int, so those report the component but save no name.
    if crate::mobs::finalize::saves_variant_name(species)
        && let Some((_, variant)) =
            crate::mobs::finalize::species_variant_component(species, m.extra.variant)
    {
        map.insert("variant".into(), string(&variant));
    }
    if species == crate::mobs::Species::Chicken {
        map.insert("EggLayTime".into(), Tag::Int(m.extra.egg_time));
    }
    let sounds = crate::mobs::finalize::sound_variants(species);
    if !sounds.is_empty() {
        let sound = m.extra.sound_variant.and_then(|i| sounds.get(i as usize));
        let sound = sound.copied().unwrap_or("classic");
        map.insert(
            "sound_variant".into(),
            string(&format!("minecraft:{sound}")),
        );
    }
    match id {
        // Fox.addAdditionalSaveData; Horizon models only the type, the
        // flags and no trusted players yet.
        "minecraft:fox" => {
            let kind = if m.extra.variant == 1 { "snow" } else { "red" };
            map.insert("Type".into(), string(kind));
            // The trusted UUIDs (`Fox.TRUSTED_LIST_CODEC`: a list of int arrays).
            map.insert(
                "Trusted".into(),
                Tag::List(
                    crate::mobs::fox::trusted_entities(m)
                        .map(|uuid| Tag::IntArray(uuid.to_vec()))
                        .collect(),
                ),
            );
            for (key, bit) in [
                ("Sleeping", crate::mobs::fox::SLEEPING),
                ("Sitting", crate::mobs::fox::SITTING),
                ("Crouching", crate::mobs::fox::CROUCHING),
            ] {
                map.insert(
                    key.into(),
                    Tag::Byte(i8::from(crate::mobs::fox::flag(m, bit))),
                );
            }
        }
        // MushroomCow.addAdditionalSaveData: the Variant codec's name and the
        // stew effects, written only while it holds any (storeNullable).
        "minecraft:mooshroom" => {
            let kind = if m.extra.variant == 1 { "brown" } else { "red" };
            map.insert("Type".into(), string(kind));
            match &m.extra.stew_effects {
                Some(effects) => {
                    let list = effects.iter().map(|(id, duration)| {
                        let mut entry = vec![("id", string(id))];
                        // SuspiciousStewEffects.Entry leaves out the default.
                        if *duration != 160 {
                            entry.push(("duration", Tag::Int(*duration)));
                        }
                        Tag::compound(entry)
                    });
                    map.insert("stew_effects".into(), Tag::List(list.collect()));
                }
                None => {
                    map.remove(&horizon_world::nbt::JavaString::from("stew_effects"));
                }
            }
        }
        // Rabbit.addAdditionalSaveData (the legacy id; 99 is the killer bunny).
        "minecraft:rabbit" => {
            map.insert("RabbitType".into(), Tag::Int(i32::from(m.extra.variant)));
            map.insert(
                "MoreCarrotTicks".into(),
                Tag::Int(m.extra.more_carrot_ticks),
            );
        }
        "minecraft:goat" => crate::mobs::goat::write_nbt(m, map),
        "minecraft:creeper" => {
            map.insert("Fuse".into(), Tag::Short(m.max_swell as i16));
            map.insert(
                "ExplosionRadius".into(),
                Tag::Byte(m.extra.explosion_radius),
            );
            map.insert("powered".into(), Tag::Byte(i8::from(m.extra.charged)));
            map.insert("ignited".into(), Tag::Byte(i8::from(m.extra.flag)));
        }
        // `Ocelot.addAdditionalSaveData`.
        "minecraft:ocelot" => {
            map.insert("Trusting".into(), Tag::Byte(i8::from(m.extra.trusting)));
        }
        "minecraft:cat" => {
            if let Some(owner) = m.extra.owner {
                map.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
            } else {
                map.remove(&"Owner".into());
            }
            map.insert("Sitting".into(), Tag::Byte(i8::from(m.extra.sitting)));
            map.insert("CollarColor".into(), Tag::Byte(m.extra.color as i8));
        }
        "minecraft:wolf" => {
            if let Some(owner) = m.extra.owner {
                map.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
            } else {
                map.remove(&"Owner".into());
            }
            map.insert("Sitting".into(), Tag::Byte(i8::from(m.extra.sitting)));
            map.insert("CollarColor".into(), Tag::Byte(m.extra.color as i8));
        }
        "minecraft:panda" => {
            use crate::mobs::panda::GENES;
            for (key, gene) in [
                ("MainGene", m.extra.variant),
                ("HiddenGene", m.extra.variant2),
            ] {
                map.insert(
                    key.into(),
                    string(GENES[usize::from(gene).min(GENES.len() - 1)]),
                );
            }
        }
        "minecraft:piglin" | "minecraft:piglin_brute" | "minecraft:hoglin" => {
            // Only Piglin.addAdditionalSaveData writes CannotHunt, always.
            if id == "minecraft:piglin_brute" {
                map.remove(&"CannotHunt".into());
            } else if id == "minecraft:piglin" {
                map.entry("CannotHunt".into()).or_insert(Tag::Byte(0));
            }
            map.insert(
                "IsImmuneToZombification".into(),
                Tag::Byte(i8::from(m.extra.flag2)),
            );
            map.insert("TimeInOverworld".into(), Tag::Int(m.extra.counter2));
            if id == "minecraft:hoglin" {
                map.insert(
                    "CannotBeHunted".into(),
                    Tag::Byte(m.extra.cannot_be_hunted as i8),
                );
            }
            if id == "minecraft:piglin" {
                crate::mobs::piglin::write_guarded_anger(m, map);
                // InventoryCarrier.writeInventoryToTag.
                map.insert(
                    "Inventory".into(),
                    Tag::List(
                        m.extra
                            .inventory
                            .iter()
                            .map(crate::save::item_to_nbt)
                            .collect(),
                    ),
                );
            }
        }
        "minecraft:horse"
        | "minecraft:donkey"
        | "minecraft:mule"
        | "minecraft:skeleton_horse"
        | "minecraft:zombie_horse"
        | "minecraft:llama"
        | "minecraft:trader_llama" => {
            map.insert("Tame".into(), Tag::Byte(i8::from(m.extra.tame)));
            map.insert("Temper".into(), Tag::Int(m.extra.equine_temper));
            map.insert("Bred".into(), Tag::Byte(i8::from(m.extra.bred)));
            // AbstractHorse.addAdditionalSaveData; the eating haystack is not modelled yet.
            map.entry("EatingHaystack".into()).or_insert(Tag::Byte(0));
            if let Some(owner) = m.extra.owner {
                map.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
            }
            if id == "minecraft:horse" {
                let loaded = map.get(&"Variant".into()).and_then(int_value);
                let look = (m.extra.variant, m.extra.variant2);
                let v = crate::mobs::equine::saved_variant(loaded, look.0, look.1);
                map.insert("Variant".into(), Tag::Int(v));
            }
            let llama = crate::mobs::llama::is_llama(m.species());
            if llama {
                crate::mobs::llama::to_nbt(m, map);
            }
            // Only llamas, donkeys and mules carry chests.
            if llama || matches!(id, "minecraft:donkey" | "minecraft:mule") {
                map.insert("ChestedHorse".into(), Tag::Byte(i8::from(m.extra.chested)));
            }
            if m.extra.chested && !m.extra.items.is_empty() {
                map.insert("Items".into(), crate::save::encode_items(&m.extra.items));
            } else {
                map.remove(&"Items".into());
            }
            let mut equipment = Vec::new();
            if let Some(saddle) = m.saddle_item() {
                equipment.push(("saddle", crate::save::item_to_nbt(&saddle)));
            }
            if let Some(armor) = &m.extra.body {
                equipment.push((
                    "body",
                    Tag::compound([("id", string(armor)), ("count", Tag::Int(1))]),
                ));
            }
            // A helmet given to the mount (say by `/item ... armor.head`)
            // keeps its own slot.
            if let Some(helmet) = &m.extra.head {
                equipment.push((
                    "head",
                    Tag::compound([("id", string(helmet)), ("count", Tag::Int(1))]),
                ));
            }
            if equipment.is_empty() {
                map.remove(&"equipment".into());
            } else {
                map.insert("equipment".into(), Tag::compound(equipment));
            }
            // AttributeMap.pack writes the instances that exist: the ones the
            // entity was loaded with. When the tag had no attributes list,
            // instances are not synthesized (SummonCommand without finalizeSpawn
            // saves attributes: []).
            if let Some(Tag::List(list)) = map.get(&"attributes".into()) {
                let modelled = [
                    ("minecraft:max_health", f64::from(m.attrs.max_health)),
                    ("minecraft:movement_speed", m.attrs.speed),
                    ("minecraft:jump_strength", m.extra.vector[0]),
                ];
                let attributes = list
                    .iter()
                    .map(|a| {
                        let id = text(a.get("id"));
                        let value = modelled.iter().find(|(m, _)| Some(*m) == id.as_deref());
                        match (a, value) {
                            (Tag::Compound(c), Some((_, v))) => {
                                let mut c = c.clone();
                                c.insert("base".into(), Tag::Double(v.to_bits()));
                                Tag::Compound(c)
                            }
                            _ => a.clone(),
                        }
                    })
                    .collect();
                map.insert("attributes".into(), Tag::List(attributes));
            }
        }
        "minecraft:pig" | "minecraft:strider" => {
            let mut equipment = match map.get(&"equipment".into()) {
                Some(Tag::Compound(c)) => c.clone(),
                _ => Compound::new(),
            };
            if let Some(saddle) = m.saddle_item() {
                equipment.insert("saddle".into(), crate::save::item_to_nbt(&saddle));
            } else {
                equipment.remove(&"saddle".into());
            }
            map.insert("equipment".into(), Tag::Compound(equipment));
        }
        "minecraft:bat" => {
            map.insert("BatFlags".into(), Tag::Byte(i8::from(m.extra.flag)));
        }
        "minecraft:turtle" => {
            map.insert("has_egg".into(), Tag::Byte(i8::from(m.extra.flag)));
            if let Some(home) = m.extra.home {
                map.insert("home_pos".into(), Tag::IntArray(home.to_vec()));
            }
        }
        "minecraft:creaking" => {
            // home_pos: the heart it is bound to.
            if let Some(home) = m.extra.home {
                map.insert("home_pos".into(), Tag::IntArray(home.to_vec()));
            } else {
                map.remove(&"home_pos".into());
            }
        }
        "minecraft:zombie_villager" => {
            map.insert("VillagerData".into(), villager_data(m));
            let xp = m.extra.trade.as_deref().map_or(0, |t| t.xp);
            map.insert("Xp".into(), Tag::Int(xp));
            put_offers(map, m);
            crate::mobs::zombie_villager::write_nbt(m, map);
        }
        "minecraft:villager" => {
            map.insert("VillagerData".into(), villager_data(m));
            // A loaded villager with VillagerData is finalized.
            map.insert("VillagerDataFinalized".into(), Tag::Byte(1));
            let t = m.extra.trade.as_deref().cloned().unwrap_or_default();
            map.insert("Xp".into(), Tag::Int(t.xp));
            map.insert("LastRestock".into(), Tag::Long(t.last_restock));
            map.insert("RestocksToday".into(), Tag::Int(t.restocks_today));
            put_offers(map, m);
            put_memories(map, m);
            crate::mobs::villager::write_nbt(m, map);
        }
        // Dolphin.addAdditionalSaveData: the fish it was fed, and how wet it is.
        "minecraft:dolphin" => {
            map.insert("GotFish".into(), Tag::Byte(i8::from(m.extra.flag)));
            map.insert("Moistness".into(), Tag::Int(m.extra.counter));
        }
        "minecraft:wandering_trader" => {
            put_offers(map, m);
            // WanderingTrader.addAdditionalSaveData: the visit's countdown and
            // the place it is walking to, a BlockPos codec (an int array).
            map.insert("DespawnDelay".into(), Tag::Int(m.extra.ints[5]));
            if let Some(p) = m.extra.point {
                // The trader keeps its target as a position; the tag is a
                // BlockPos, so only the block it is in is saved.
                map.insert(
                    "wander_target".into(),
                    Tag::IntArray(vec![
                        p[0].floor() as i32,
                        p[1].floor() as i32,
                        p[2].floor() as i32,
                    ]),
                );
            }
        }
        "minecraft:axolotl" => {
            write_axolotl_memories(m, map);
            map.insert(
                "Variant".into(),
                Tag::Int(i32::from(m.extra.variant.min(4))),
            );
        }
        "minecraft:parrot" => {
            map.insert(
                "Variant".into(),
                Tag::Int(i32::from(m.extra.variant.min(4))),
            );
            if let Some(owner) = m.extra.owner {
                map.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
            } else {
                map.remove(&"Owner".into());
            }
            map.insert("Sitting".into(), Tag::Byte(i8::from(m.extra.sitting)));
        }
        "minecraft:salmon" => {
            let size = SALMON_SIZES[usize::from(m.extra.variant).min(2)];
            map.insert("type".into(), string(size));
        }
        // Pufferfish.addAdditionalSaveData: the puff state (0 small, 1 mid,
        // 2 full).
        "minecraft:pufferfish" => {
            map.insert("PuffState".into(), Tag::Int(m.extra.size));
        }
        // TropicalFish.addAdditionalSaveData: the packed variant.
        "minecraft:tropical_fish" => {
            let pattern = m.extra.variant.min(11);
            let packed = i32::from(pattern >= 6)
                | i32::from(pattern % 6) << 8
                | i32::from(m.extra.variant2 & 15) << 16
                | i32::from(m.extra.variant3 & 15) << 24;
            map.insert("Variant".into(), Tag::Int(packed));
        }
        "minecraft:allay" => {
            // InventoryCarrier: the fetched stack; the liked player memory.
            let inv = crate::mobs::allay::inventory(m)
                .map(item_to_nbt)
                .into_iter()
                .collect();
            map.insert("Inventory".into(), Tag::List(inv));
            if let Some(st) = &m.extra.allay {
                map.insert("listener".into(), st.vibration.to_nbt());
            }
            map.insert(
                "DuplicationCooldown".into(),
                Tag::Long(i64::from(m.extra.egg_time)),
            );
            let mut memories: Vec<(&str, Tag)> = Vec::new();
            if m.extra.tame {
                // `MemorySlot.visit`: the time to live only when it can expire.
                let mut liked = vec![(
                    "value",
                    Tag::IntArray(m.extra.owner.unwrap_or([0, 0, 0, 1]).to_vec()),
                )];
                if let Some(ttl) = m.extra.allay.as_ref().and_then(|st| st.liked_player_ttl) {
                    liked.push(("ttl", Tag::Long(ttl)));
                }
                memories.push(("minecraft:liked_player", Tag::compound(liked)));
            }
            // GlobalPos.CODEC, and the cooldown while it runs.
            if let Some(liked) = m.extra.allay.as_ref().and_then(|st| st.noteblock) {
                memories.push((
                    "minecraft:liked_noteblock",
                    Tag::compound([(
                        "value",
                        Tag::compound([
                            ("dimension", string(liked.dimension)),
                            ("pos", Tag::IntArray(liked.pos.to_vec())),
                        ]),
                    )]),
                ));
            }
            if m.extra.ints[2] > 0 {
                memories.push((
                    "minecraft:liked_noteblock_cooldown_ticks",
                    Tag::compound([("value", Tag::Int(m.extra.ints[2]))]),
                ));
            }
            // ITEM_PICKUP_COOLDOWN_TICKS (`Codec.INT`), which a throw sets to 60,
            // while the memory is there (its value 0 included).
            if let Some(ticks) = crate::mobs::allay::pickup_cooldown(m) {
                memories.push((
                    "minecraft:item_pickup_cooldown_ticks",
                    Tag::compound([("value", Tag::Int(ticks))]),
                ));
            }
            // Always written, so a forgotten note block does not come back
            // from the compound the allay was loaded with.
            map.insert(
                "Brain".into(),
                Tag::compound([("memories", Tag::compound(memories))]),
            );
        }
        "minecraft:camel" | "minecraft:camel_husk" => {
            // AbstractHorse.addAdditionalSaveData, including Camel.isTamed.
            map.insert("Tame".into(), Tag::Byte(i8::from(m.extra.tame)));
            map.insert("Temper".into(), Tag::Int(m.extra.equine_temper));
            map.insert("Bred".into(), Tag::Byte(i8::from(m.extra.bred)));
            map.insert("EatingHaystack".into(), Tag::Byte(i8::from(m.extra.flag)));
            // LastPoseTick: the pose change time, negative while sitting. The
            // game time is not known here; the world's save overwrites it
            // with the real one (`camel::synced_pose_tick`).
            map.insert(
                "LastPoseTick".into(),
                Tag::Long(crate::mobs::camel::synced_pose_tick(m, 0)),
            );
            if let Some(saddle) = m.saddle_item() {
                map.insert(
                    "equipment".into(),
                    Tag::compound([("saddle", crate::save::item_to_nbt(&saddle))]),
                );
            } else {
                map.remove(&"equipment".into());
            }
        }
        "minecraft:nautilus" | "minecraft:zombie_nautilus" => {
            crate::mobs::nautilus::to_nbt(m, map);
        }
        "minecraft:sulfur_cube" => crate::mobs::sulfur_cube::to_nbt(m, map),
        "minecraft:bee" => crate::mobs::bee::to_nbt(m, map, &mut uuid_of),
        "minecraft:sniffer" => crate::mobs::sniffer::to_nbt(m, map),
        "minecraft:wither" => {
            map.insert("Invul".into(), Tag::Int(m.extra.counter));
        }
        // IronGolem.addAdditionalSaveData.
        "minecraft:iron_golem" => {
            map.insert(
                "PlayerCreated".into(),
                Tag::Byte(i8::from(m.extra.player_created)),
            );
        }
        // SnowGolem.addAdditionalSaveData: whether it still wears its pumpkin.
        "minecraft:snow_golem" => {
            map.insert("Pumpkin".into(), Tag::Byte(i8::from(!m.extra.sheared)));
        }
        // `Warden.addAdditionalSaveData`: the anger - one `{uuid, anger}` pair
        // per suspect - and the vibration listener, and nothing else. A warden
        // is no `NeutralMob`, so there is no `angry_at` or `anger_end_time`: the
        // entity a suspect is is named by its UUID, not by a live id.
        "minecraft:warden" => {
            if let Some(st) = &m.extra.warden {
                map.insert("listener".into(), st.vibration.to_nbt());
                map.insert("anger".into(), warden_anger_to_nbt(&st.anger, &mut uuid_of));
            }
            write_warden_owned_memories(m, map);
        }
        // Armadillo.addAdditionalSaveData: the time left to the next scute, and
        // the state it is in, which is always written (`idle` when it is not
        // rolled up).
        "minecraft:armadillo" => {
            map.insert("scute_time".into(), Tag::Int(m.extra.egg_time));
            map.insert(
                "state".into(),
                string(crate::mobs::armadillo::state_name(
                    crate::mobs::armadillo::state(m),
                )),
            );
        }
        "minecraft:ravager" => {
            map.insert("AttackTick".into(), Tag::Int(m.extra.counter));
            map.insert("StunTick".into(), Tag::Int(m.extra.counter2));
            map.insert("RoarTick".into(), Tag::Int(m.extra.anim_ticks));
        }
        "minecraft:vex" => crate::mobs::vex::write_nbt(m, map, &mut uuid_of),
        "minecraft:endermite" => {
            map.insert("Lifetime".into(), Tag::Int(m.extra.counter));
        }
        // Enderman.addAdditionalSaveData: the block it carries, saved only
        // while it carries one.
        "minecraft:enderman" => match (m.extra.carried, registry) {
            (Some(state), Some(registry)) => {
                map.insert("carriedBlockState".into(), state_to_nbt(registry, state));
            }
            // Nothing carries a block state out without the registry, and a
            // caller without one keeps what the loaded compound had, so the
            // key is only dropped when a registry could have written it.
            (Some(_), None) | (None, None) => {}
            (None, Some(_)) => {
                map.remove(&"carriedBlockState".into());
            }
        },
        "minecraft:shulker" => {
            map.insert("AttachFace".into(), Tag::Byte(m.extra.variant3 as i8));
            map.insert("Peek".into(), Tag::Byte(m.extra.counter as i8));
            map.insert("Color".into(), Tag::Byte(m.extra.color as i8));
        }
        // HappyGhast.addAdditionalSaveData: the still timeout (`serverStillTimeout`,
        // kept in `counter`), which vanilla writes even at 0.
        "minecraft:happy_ghast" => {
            map.insert("still_timeout".into(), Tag::Int(m.extra.counter));
        }
        // GlowSquid.addAdditionalSaveData.
        "minecraft:glow_squid" => {
            map.insert(
                "DarkTicksRemaining".into(),
                Tag::Int(crate::mobs::aquatic::dark_ticks(m)),
            );
        }
        // Phantom.addAdditionalSaveData: the size, and the circling anchor.
        // `storeNullable` writes nothing for a phantom that has none, so its
        // save carries no `anchor_pos` at all.
        "minecraft:phantom" => {
            map.insert("size".into(), Tag::Int(m.extra.size));
            match m.extra.home {
                Some(anchor) => {
                    map.insert("anchor_pos".into(), Tag::IntArray(anchor.to_vec()));
                }
                None => {
                    map.remove(&"anchor_pos".into());
                }
            }
        }
        "minecraft:slime" | "minecraft:magma_cube" => {
            map.insert("Size".into(), Tag::Int(m.extra.size.max(1) - 1));
            map.insert("wasOnGround".into(), Tag::Byte(i8::from(m.extra.flag2)));
        }
        // Skeletons are spawned holding a bow (finalizeSpawn); a mob loaded
        // from NBT keeps only its saved equipment.
        "minecraft:skeleton" | "minecraft:stray" | "minecraft:bogged"
            if raw.is_none() && m.extra.main_hand.as_deref() == Some("minecraft:bow") =>
        {
            map.entry("equipment".into()).or_insert_with(|| {
                Tag::compound([(
                    "mainhand",
                    Tag::compound([("id", string("minecraft:bow")), ("count", Tag::Int(1))]),
                )])
            });
        }
        // Zombie and Skeleton.addAdditionalSaveData (a husk keeps Zombie's):
        // `ConversionTracker`'s conversion and affliction times, -1 when it is
        // not converting or not afflicted.
        // The tracker ticks for these, so the live counters win over the loaded
        // values: one that left the water or the snow saves -1.
        "minecraft:zombie" | "minecraft:husk" => {
            for (key, value) in [
                ("DrownedConversionTime", conversion_time(m)),
                ("InWaterTime", m.extra.counter),
            ] {
                map.insert(key.into(), Tag::Int(value));
            }
        }
        "minecraft:skeleton" => {
            for (key, value) in [
                ("StrayConversionTime", conversion_time(m)),
                ("FreezingTime", m.extra.counter),
            ] {
                map.insert(key.into(), Tag::Int(value));
            }
        }
        // Drowned.convertsInWater is false, so its tracker never ticks: vanilla
        // saves -1 for both.
        "minecraft:drowned" => {
            for (key, value) in [
                ("DrownedConversionTime", Tag::Int(-1)),
                ("InWaterTime", Tag::Int(-1)),
            ] {
                map.entry(key.into()).or_insert(value);
            }
        }
        // Sheep.addAdditionalSaveData: its wool as it is now.
        "minecraft:sheep" => {
            map.insert("Color".into(), Tag::Byte((m.extra.color & 15) as i8));
            map.insert("Sheared".into(), Tag::Byte(i8::from(m.extra.sheared)));
        }
        // CopperGolem.addAdditionalSaveData.
        "minecraft:copper_golem" => {
            map.insert(
                "next_weather_age".into(),
                Tag::Long(m.extra.vector[0] as i64),
            );
            map.insert(
                "weather_state".into(),
                string(crate::mobs::copper_golem::WEATHER[m.extra.variant.min(3) as usize]),
            );
        }
        // Ghast.addAdditionalSaveData; Horizon's ghast fires power 1 whatever
        // is saved, so a loaded value is kept as is.
        "minecraft:ghast" => {
            map.entry("ExplosionPower".into()).or_insert(Tag::Byte(1));
        }
        _ => {}
    }
    if m.species() == S::Frog {
        crate::mobs::brain::write_frog(m, map);
    }
    // Bogged.addAdditionalSaveData: whether its mushrooms were sheared.
    if m.species() == S::Bogged {
        map.insert("sheared".into(), Tag::Byte(i8::from(m.extra.sheared)));
    }
    // Zombie.addAdditionalSaveData: CanBreakDoors.
    if crate::mobs::doors::is_zombie(m.species()) {
        map.insert(
            "CanBreakDoors".into(),
            Tag::Byte(i8::from(m.extra.can_break_doors)),
        );
        // ConversionTracker.addAdditionalSaveData reads isAfflicted at save
        // time. Zombie supplies its cached isEyeInFluid(WATER), even with NoAI.
        if !m.fluids.eye_in_water {
            map.insert("InWaterTime".into(), Tag::Int(-1));
        }
        // ZombieVillager and ZombifiedPiglin save Zombie's tracker too; Horizon
        // never starts their water conversion, so its time is -1.
        if matches!(m.species(), S::ZombieVillager | S::ZombifiedPiglin) {
            map.entry("DrownedConversionTime".into())
                .or_insert(Tag::Int(-1));
        }
    }
    // Skeleton supplies its cached isInPowderSnow to the same isAfflicted check.
    if m.species() == S::Skeleton && !m.in_powder_snow {
        map.insert("FreezingTime".into(), Tag::Int(-1));
    }
    if matches!(
        id,
        "minecraft:zombie"
            | "minecraft:husk"
            | "minecraft:drowned"
            | "minecraft:zombie_villager"
            | "minecraft:zombified_piglin"
            | "minecraft:piglin"
            | "minecraft:zoglin"
    ) {
        map.insert("IsBaby".into(), Tag::Byte(i8::from(m.is_baby())));
    }
    // NeutralMob.addPersistentAngerSaveData; the bee saves its own.
    if matches!(
        id,
        "minecraft:iron_golem"
            | "minecraft:wolf"
            | "minecraft:polar_bear"
            | "minecraft:zombified_piglin"
            | "minecraft:enderman"
    ) {
        map.entry("anger_end_time".into()).or_insert(Tag::Long(-1));
    }
    if let Some(a) = m.extra.attributes.as_deref() {
        put_attributes(map, m, a);
    }
    put_sulfur_attributes(map, m);
    // The instances their own construction and load make (finalize::made_at_load),
    // which AttributeMap.pack saves although the loaded tag had none.
    use crate::mobs::finalize::Attr;
    let made = crate::mobs::finalize::made_at_load(m);
    // AttributeMap.pack writes the list even when no instance exists.
    map.entry("attributes".into())
        .or_insert_with(|| Tag::List(Vec::new()));
    for attr in made {
        let base = crate::mobs::finalize::made_base(m, attr);
        let mut list = match map.get(&"attributes".into()) {
            Some(Tag::List(l)) => l.clone(),
            _ => Vec::new(),
        };
        let attr_id = format!("minecraft:{}", attr.name());
        if let Some(existing) = list
            .iter_mut()
            .find(|t| text(t.get("id")).as_deref() == Some(attr_id.as_str()))
        {
            // The model owns a wolf's health base.
            if (species, attr) == (S::Wolf, Attr::MaxHealth)
                && let Tag::Compound(c) = existing
            {
                c.insert("base".into(), Tag::Double(base.to_bits()));
            }
        } else {
            list.push(Tag::compound([
                ("id", string(&attr_id)),
                ("base", Tag::Double(base.to_bits())),
            ]));
        }
        map.insert("attributes".into(), Tag::List(list));
    }
    // Mob.addAdditionalSaveData: the chances that are not the default.
    if let Some(chances) = &m.extra.drop_chances {
        let set: Vec<(&str, Tag)> = crate::mobs::drops::SLOTS
            .iter()
            .zip(chances.iter())
            .filter(|(_, c)| **c != crate::mobs::drops::DEFAULT_DROP_CHANCE)
            .map(|(slot, c)| (*slot, Tag::Float(c.to_bits())))
            .collect();
        if set.is_empty() {
            map.remove(&"drop_chances".into());
        } else {
            map.insert("drop_chances".into(), Tag::compound(set));
        }
    }
    // SpellcasterIllager.addAdditionalSaveData: save the live casting timer,
    // including zero and negative values, rather than stale loaded NBT.
    if matches!(m.species(), S::Evoker | S::Illusioner) {
        map.insert("SpellTicks".into(), Tag::Int(m.extra.counter));
    }
    // Vindicator.addAdditionalSaveData: `Johnny` only while the flag is set
    // (`if (this.isJohnny) output.putBoolean("Johnny", true)`).
    if m.species() == S::Vindicator {
        if m.extra.johnny {
            map.insert("Johnny".into(), Tag::Byte(1));
        } else {
            map.remove(&"Johnny".into());
        }
    }
    crate::mobs::raider::write_nbt(m, map);
    crate::mobs::riders::write_nbt(m, map);
    // LivingEntity equipment: what the mob holds now.
    if !crate::mobs::equine::is_equine(m.species()) {
        let mut equipment = match map.get(&"equipment".into()) {
            Some(Tag::Compound(c)) => c.clone(),
            _ => horizon_world::nbt::Compound::new(),
        };
        for (index, (slot, item)) in [
            ("mainhand", &m.extra.main_hand),
            ("offhand", &m.extra.off_hand),
            ("head", &m.extra.head),
            ("chest", &m.extra.chest),
            ("legs", &m.extra.legs),
            ("feet", &m.extra.feet),
        ]
        .into_iter()
        .enumerate()
        {
            if slot == "mainhand"
                && let Some(stack) = m.extra.main_hand_stack()
            {
                let mut stack = stack.clone();
                if let Some(g) = crate::mobs::gear::gear(m) {
                    stack.data_mut().enchantments = g.slots[index].clone();
                    stack.data_mut().trim = g.trims[index].clone();
                }
                equipment.insert(slot.into(), crate::save::item_to_nbt(&stack));
                continue;
            }
            // A worn piece or held item that kept its damage, name or
            // components is saved with them.
            if slot != "mainhand"
                && item.is_some()
                && let Some(stack) = crate::mobs::finalize::stored_stack(
                    m,
                    [
                        crate::mobs::finalize::MAINHAND,
                        crate::mobs::finalize::OFFHAND,
                        crate::mobs::finalize::HEAD,
                        crate::mobs::finalize::CHEST,
                        crate::mobs::finalize::LEGS,
                        crate::mobs::finalize::FEET,
                    ][index],
                )
            {
                equipment.insert(slot.into(), crate::save::item_to_nbt(&stack));
                continue;
            }
            // Enchanted or trimmed gear is saved with its components.
            if let (Some(id), Some(g)) = (item, crate::mobs::gear::gear(m))
                && (!g.slots[index].is_empty() || g.trims[index].is_some())
            {
                let mut stack = crate::items::ItemStack::new(id, 1);
                stack.data_mut().enchantments = g.slots[index].clone();
                stack.data_mut().trim = g.trims[index].clone();
                equipment.insert(slot.into(), crate::save::item_to_nbt(&stack));
                continue;
            }
            match item {
                Some(id) => {
                    let keep = slot != "mainhand"
                        && matches!(
                            equipment.get(&slot.into()).and_then(|e| e.get("id")),
                            Some(Tag::String(s)) if s.to_string_lossy() == **id
                        );
                    if slot == "head"
                        && &**id == crate::mobs::raider::BANNER
                        && crate::mobs::raider::is_raider(m.species())
                    {
                        // The captain's ominous banner keeps its patterns.
                        let has_patterns = equipment
                            .get(&slot.into())
                            .and_then(|e| e.get("components"))
                            .is_some();
                        if !has_patterns {
                            equipment.insert(
                                slot.into(),
                                crate::save::item_to_nbt(&crate::mobs::raider::ominous_banner()),
                            );
                        }
                    } else if !keep {
                        equipment.insert(
                            slot.into(),
                            Tag::compound([
                                ("id", Tag::String(id.as_ref().into())),
                                ("count", Tag::Int(1)),
                            ]),
                        );
                    }
                }
                None => {
                    equipment.remove(&slot.into());
                }
            }
        }
        // LivingEntity saves the actual saddle equipment even on a mob whose
        // Survival interaction does not offer saddling (for example a husk).
        if let Some(saddle) = m.saddle_item() {
            equipment.insert("saddle".into(), crate::save::item_to_nbt(&saddle));
        }
        // A happy ghast wears its harness in the body slot (and a nautilus
        // its armour, a sulfur cube its swallowed block, a wolf its armor,
        // with the durability it lost).
        if crate::mobs::species::body_slot(m.species())
            && let Some(id) = &m.extra.body
        {
            let mut stack = crate::items::ItemStack::new(id, 1);
            if m.species() == crate::mobs::Species::Wolf {
                stack.damage = m.extra.body_damage.max(0) as u32;
                stack.extra = m.extra.body_extra.clone();
            }
            equipment.insert("body".into(), crate::save::item_to_nbt(&stack));
        }
        // Vanilla saves `equipment` only when something is equipped.
        if equipment.is_empty() {
            map.remove(&"equipment".into());
        } else {
            map.insert("equipment".into(), Tag::Compound(equipment));
        }
    }
    // A mannequin is an `Avatar`, not a `Mob`: none of
    // `Mob.addAdditionalSaveData`'s fields are its, written or loaded.
    if m.species() == crate::mobs::Species::Mannequin
        && let Tag::Compound(map) = &mut tag
    {
        for key in MOB_ONLY_KEYS {
            map.remove(&horizon_world::nbt::JavaString::from(key));
        }
    }
    tag
}

/// `PlayerModelPart` in its order: bit `i` of a hidden-layer mask is part `i`.
const MODEL_PARTS: [&str; 7] = [
    "cape",
    "jacket",
    "left_sleeve",
    "right_sleeve",
    "left_pants_leg",
    "right_pants_leg",
    "hat",
];
/// The keys `Mob.addAdditionalSaveData` writes.
const MOB_ONLY_KEYS: [&str; 10] = [
    "NoAI",
    "PersistenceRequired",
    "CanPickUpLoot",
    "LeftHanded",
    "drop_chances",
    "leash",
    "home_radius",
    "home_pos",
    "DeathLootTable",
    "DeathLootTableSeed",
];
/// `AttributeMap.pack` for the attributes with a base or permanent modifier
/// finalizeSpawn set, over the saved list.
fn put_attributes(map: &mut Compound, m: &Mob, a: &crate::mobs::finalize::Attributes) {
    use crate::mobs::finalize::{self, Attr, Op};
    let mut list = match map.get(&"attributes".into()) {
        Some(Tag::List(l)) => l.clone(),
        _ => Vec::new(),
    };
    for attr in Attr::ALL {
        let modifiers: Vec<Tag> = a
            .modifiers
            .iter()
            .filter(|x| x.attr == attr)
            .map(|x| {
                let op = match x.op {
                    Op::AddValue => "add_value",
                    Op::AddMultipliedBase => "add_multiplied_base",
                    Op::AddMultipliedTotal => "add_multiplied_total",
                };
                Tag::compound([
                    ("id", string(&x.id)),
                    ("amount", Tag::Double(x.amount.to_bits())),
                    ("operation", string(op)),
                ])
            })
            .collect();
        if modifiers.is_empty() && !a.bases.iter().any(|(b, _)| *b == attr) {
            saved_attributes::forget_stale_modifiers(&mut list, attr);
            continue;
        }
        let id = format!("minecraft:{}", attr.name());
        let mut entry = vec![
            ("id", string(&id)),
            ("base", Tag::Double(finalize::base_value(m, attr).to_bits())),
        ];
        if !modifiers.is_empty() {
            entry.push(("modifiers", Tag::List(modifiers)));
        }
        let entry = Tag::compound(entry);
        match list
            .iter_mut()
            .find(|t| text(t.get("id")).as_deref() == Some(id.as_str()))
        {
            Some(t) => *t = entry,
            None => list.push(entry),
        }
    }
    map.insert("attributes".into(), Tag::List(list));
}
/// `AttributeMap.pack` of a sulfur cube's body archetype attributes: the
/// base and permanent modifiers of each instance its own map holds (the
/// archetype's transient modifiers are not saved).
fn put_sulfur_attributes(map: &mut Compound, m: &Mob) {
    use horizon_world::attributes::Operation;
    if m.species() != crate::mobs::Species::SulfurCube {
        return;
    }
    let Some(attributes) = m.extra.armor_attributes.as_deref() else {
        return;
    };
    let mut list = match map.get(&"attributes".into()) {
        Some(Tag::List(l)) => l.clone(),
        _ => Vec::new(),
    };
    let mut changed = false;
    for id in crate::mobs::sulfur_cube::ARCHETYPE_ATTRIBUTES {
        let Some(instance) = attributes
            .catalog()
            .id(id)
            .and_then(|k| attributes.instance(k))
        else {
            continue;
        };
        let packed = instance.pack();
        let mut entry = vec![
            ("id", string(id)),
            ("base", Tag::Double(packed.base.to_bits())),
        ];
        if !packed.modifiers.is_empty() {
            let modifiers = packed
                .modifiers
                .iter()
                .map(|x| {
                    let op = match x.operation() {
                        Operation::AddValue => "add_value",
                        Operation::AddMultipliedBase => "add_multiplied_base",
                        Operation::AddMultipliedTotal => "add_multiplied_total",
                    };
                    Tag::compound([
                        ("id", string(x.id())),
                        ("amount", Tag::Double(x.amount().to_bits())),
                        ("operation", string(op)),
                    ])
                })
                .collect();
            entry.push(("modifiers", Tag::List(modifiers)));
        }
        let entry = Tag::compound(entry);
        match list
            .iter_mut()
            .find(|t| text(t.get("id")).as_deref() == Some(id))
        {
            Some(t) => *t = entry,
            None => list.push(entry),
        }
        changed = true;
    }
    if changed {
        map.insert("attributes".into(), Tag::List(list));
    }
}
/// `Brain.Packed`: the memories finalizeSpawn set, over the saved ones.
/// `CustomName`: a text component saved as a string or a `{text: ..}` compound.
fn custom_name(tag: &Tag) -> Option<String> {
    match tag.get("CustomName")? {
        Tag::String(s) => Some(s.to_string_lossy()),
        t @ Tag::Compound(_) => match t.get("text") {
            Some(Tag::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        },
        _ => None,
    }
}
pub use horizon_client_core::component::name_text;
/// `Mob.readAdditionalSaveData`: negative radii retain the live homePosition.
/// Used after the command adapter restores runtime AI, as well as fresh load.
pub fn read_mob_home(m: &mut Mob, tag: &Tag) {
    let radius = match tag.get("home_radius") {
        Some(Tag::Float(v)) => Some(f32::from_bits(*v).floor() as i32),
        Some(Tag::Double(v)) => Some(f64::from_bits(*v).floor() as i32),
        other => number(other).map(|v| v as i32),
    }
    .unwrap_or(-1);
    let pos = if radius >= 0 {
        home_block_pos(tag.get("home_pos")).unwrap_or([0; 3])
    } else {
        m.ai.home_position()
    };
    m.ai.restore_home(pos, radius);
}
/// `BlockPos.CODEC` through NbtOps.getIntStream: numeric collection entries
/// use Java Number's int conversion (floating entries truncate, unlike getIntOr above).
fn home_block_pos(tag: Option<&Tag>) -> Option<[i32; 3]> {
    match tag? {
        Tag::IntArray(v) => v.as_slice().try_into().ok(),
        Tag::ByteArray(v) if v.len() == 3 => Some([
            i32::from(v[0] as i8),
            i32::from(v[1] as i8),
            i32::from(v[2] as i8),
        ]),
        Tag::LongArray(v) if v.len() == 3 => Some([v[0] as i32, v[1] as i32, v[2] as i32]),
        Tag::List(v) if v.len() == 3 => {
            let int = |t: &Tag| match t {
                Tag::Float(v) => Some(f32::from_bits(*v) as i32),
                Tag::Double(v) => Some(f64::from_bits(*v) as i32),
                other => number(Some(other)).map(|v| v as i32),
            };
            Some([int(&v[0])?, int(&v[1])?, int(&v[2])?])
        }
        _ => None,
    }
}
/// The mob a saved compound describes. `registry` is what reads a field holding
/// a block state (an enderman's `carriedBlockState`); without one, such a field
/// is left out, as it was before it was modelled.
pub fn mob_from_nbt(
    tag: &Tag,
    types: &HashMap<String, Arc<MobType>>,
    id: u32,
    registry: Option<&BlockRegistry>,
) -> Option<Mob> {
    let position = position(tag)?;
    let random = crate::mobs::entity_random(id, position);
    mob_from_nbt_with_random(tag, types, id, registry, random)
}
/// `mob_from_nbt` for a mob with its own random given (`Mob::with_random`).
pub fn mob_from_nbt_with_random(
    tag: &Tag,
    types: &HashMap<String, Arc<MobType>>,
    id: u32,
    registry: Option<&BlockRegistry>,
    random: horizon_core::random::EntityRandom,
) -> Option<Mob> {
    let kind = types.get(&self::kind(tag)?)?.clone();
    let position = position(tag)?;
    let [yaw, pitch] = rotation(tag);
    let mut m = Mob::with_random(id, kind, position, yaw, random);
    m.velocity = motion(tag);
    m.head_pitch = pitch;
    // LivingEntity.readAdditionalSaveData: `setSharedFlag(7, getBooleanOr(FallFlying, false))`.
    m.extra.fall_flying = flag(tag, "FallFlying");
    read_mob_home(&mut m, tag);
    m.on_ground = flag(tag, "OnGround");
    m.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    // LivingEntity.readAdditionalSaveData: the saved attribute bases, then
    // the health, the max health when there is none.
    if let Some(Tag::List(attrs)) = tag.get("attributes") {
        for a in attrs {
            let (Some(id), Some(v)) = (text(a.get("id")), float(a.get("base"))) else {
                continue;
            };
            // A mount's rolled health and speed stay attribute bases, which
            // the server syncs (its hearts over the rider's hotbar).
            let equine = crate::mobs::equine::is_equine(m.species());
            match id.as_str() {
                "minecraft:max_health" if equine => crate::mobs::finalize::set_base(
                    &mut m,
                    crate::mobs::finalize::Attr::MaxHealth,
                    v,
                ),
                "minecraft:movement_speed" if equine => crate::mobs::finalize::set_base(
                    &mut m,
                    crate::mobs::finalize::Attr::MovementSpeed,
                    v,
                ),
                // A saved base other than the type's default is the
                // attribute's base, which `/attribute ... base get` reads.
                "minecraft:max_health"
                    if crate::mobs::finalize::default_value(
                        &m,
                        crate::mobs::finalize::Attr::MaxHealth,
                    ) != Some(v) =>
                {
                    crate::mobs::finalize::set_base(
                        &mut m,
                        crate::mobs::finalize::Attr::MaxHealth,
                        v,
                    )
                }
                "minecraft:max_health" => m.attrs.max_health = v as f32,
                "minecraft:movement_speed"
                    if crate::mobs::finalize::default_value(
                        &m,
                        crate::mobs::finalize::Attr::MovementSpeed,
                    ) != Some(v) =>
                {
                    crate::mobs::finalize::set_base(
                        &mut m,
                        crate::mobs::finalize::Attr::MovementSpeed,
                        v,
                    )
                }
                "minecraft:movement_speed" => m.attrs.speed = v,
                "minecraft:attack_damage"
                    if crate::mobs::finalize::default_value(
                        &m,
                        crate::mobs::finalize::Attr::AttackDamage,
                    ) != Some(v) =>
                {
                    crate::mobs::finalize::set_base(
                        &mut m,
                        crate::mobs::finalize::Attr::AttackDamage,
                        v,
                    )
                }
                "minecraft:attack_damage" => m.attrs.attack_damage = v as f32,
                "minecraft:attack_knockback" => m.attrs.attack_knockback = v,
                "minecraft:flying_speed"
                    if crate::mobs::finalize::default_value(
                        &m,
                        crate::mobs::finalize::Attr::FlyingSpeed,
                    ) != Some(v) =>
                {
                    crate::mobs::finalize::set_base(
                        &mut m,
                        crate::mobs::finalize::Attr::FlyingSpeed,
                        v,
                    )
                }
                _ => {}
            }
        }
    }
    saved_attributes::read_attribute_modifiers(&mut m, tag);
    m.health = float(tag.get("Health")).map_or(m.attrs.max_health, |h| h as f32);
    // LivingEntity.readAdditionalSaveData: `internalSetAbsorptionAmount`
    // stores the value as read; only `setAbsorptionAmount` clamps.
    m.absorption = float(tag.get("AbsorptionAmount")).map_or(0.0, |h| h as f32);
    // LivingEntity.setHealth clamps to the max health: a horse or llama is made
    // with the type's and saved before it drops to the one it rolled. (Only
    // the base is read here, so the clamp would cut a leader zombie's bonus.)
    if crate::mobs::equine::is_equine(m.species()) {
        m.health = m.health.min(m.attrs.max_health);
    }
    m.hurt_time = i32::from(number(tag.get("HurtTime")).unwrap_or(0) as i16);
    // What it holds (equipment.mainhand / offhand), when saved.
    if let Some(eq) = tag.get("equipment") {
        m.extra
            .set_main_hand_stack(eq.get("mainhand").and_then(crate::save::item_from_nbt));
        m.extra.off_hand = eq
            .get("offhand")
            .and_then(|i| text(i.get("id")))
            .map(Into::into);
        if !crate::mobs::equine::is_equine(m.species()) {
            let slot = |key| eq.get(key).and_then(|i| text(i.get("id"))).map(Into::into);
            m.extra.head = slot("head");
            // A happy ghast wears its harness in the body slot (and a
            // nautilus its armour, a sulfur cube its swallowed block).
            if crate::mobs::species::body_slot(m.species()) {
                m.extra.body = slot("body");
            }
            m.extra.chest = slot("chest");
            m.extra.legs = slot("legs");
            m.extra.feet = slot("feet");
        }
        // Their enchantments and trims.
        for (index, key) in crate::mobs::gear::KEYS.iter().enumerate() {
            if let Some(stack) = eq.get(key).and_then(crate::save::item_from_nbt) {
                crate::mobs::gear::set(&mut m, index, stack.enchantments().to_vec());
                let trim = stack.data().and_then(|d| d.trim.clone());
                crate::mobs::gear::set_trim(&mut m, index, trim);
                // The damage, name and components stay with the slot.
                if index != crate::mobs::gear::MAIN {
                    crate::mobs::gear::set_stack(&mut m, index, stack);
                }
            }
        }
    } else {
        // Registration hands out default weapons (finalizeSpawn does in
        // vanilla); a loaded mob holds only what its file lists.
        m.extra.set_main_hand(None);
    }
    m.death_time = i32::from(number(tag.get("DeathTime")).unwrap_or(0) as i16);
    m.impulse_grace_time =
        number(tag.get("current_impulse_context_reset_grace_time")).unwrap_or(0) as i32;
    // Mob.readAdditionalSaveData overrides what the constructor set (copper
    // golems and elder guardians start persistent).
    m.persistent = flag(tag, "PersistenceRequired");
    m.extra.death_loot_table = text(tag.get("DeathLootTable"))
        .and_then(|t| horizon_client_core::identifier::normalize_id(&t));
    m.extra.death_loot_seed = number(tag.get("DeathLootTableSeed")).unwrap_or(0);
    refs::read_last_hurt_by(&mut m, tag);
    // `NeutralMob.readPersistentAngerSaveData`: the anger end time and the
    // target come back with the mob.
    if persistent_anger(m.species()) {
        m.extra.anger_end = number(tag.get("anger_end_time")).unwrap_or(-1);
        // Anger at an entity other than the player is anger at a mob that may
        // not be here (`EntityReference`): it still is not `isAngryAtAllPlayers`.
        m.extra.angry_at = match uuid_from_tag(tag.get("angry_at")) {
            Some(uuid) if uuid == player_uuid() => Some(crate::mobs::Who::Player),
            Some(_) => Some(crate::mobs::Who::Mob(0)),
            None => None,
        };
    }
    // `Mannequin.readAdditionalSaveData`: the profile comes back with the skin, then
    // validated pose, `immovable`, `hide_description` and `description` (`updateDescription`).
    if m.species() == crate::mobs::Species::Mannequin {
        m.profile = tag.get("profile").and_then(profile_from_nbt);
        // Mannequin.POSE_CODEC accepts only these Avatar poses; an invalid
        // or absent value defaults to standing before the live dimensions read it.
        m.extra.avatar_pose = match text(tag.get("pose")).as_deref() {
            Some("crouching") => crate::pose::Pose::Crouching,
            Some("swimming") => crate::pose::Pose::Swimming,
            Some("fall_flying") => crate::pose::Pose::FallFlying,
            Some("sleeping") => crate::pose::Pose::Sleeping,
            _ => crate::pose::Pose::Standing,
        };
        // `HumanoidArm.CODEC`, right when absent or not an arm.
        m.extra.main_hand_left = text(tag.get("main_hand")).as_deref() == Some("left");
        // `LAYERS_CODEC`: every listed part hidden; an entry that is not a
        // part fails the whole list, which reads as `ALL_LAYERS` (none hidden).
        m.extra.hidden_model_parts = match tag.get("hidden_layers") {
            Some(Tag::List(parts)) => parts
                .iter()
                .try_fold(0u8, |mask, part| {
                    let name = text(Some(part))?;
                    let bit = MODEL_PARTS.iter().position(|p| *p == name)?;
                    Some(mask | 1 << bit)
                })
                .unwrap_or(0),
            _ => 0,
        };
        m.extra.immovable = flag(tag, "immovable");
        m.extra.description = if flag(tag, "hide_description") {
            crate::mobs::MannequinDescription::Hidden
        } else {
            crate::mobs::MannequinDescription::from_component(Some(
                &tag.get("description")
                    .cloned()
                    .or_else(|| crate::mobs::MannequinDescription::Default.component())
                    .unwrap_or(Tag::End),
            ))
        };
    }
    // AbstractPiglin.readAdditionalSaveData: getBooleanOr("CanPickUpLoot", true).
    let piglin = matches!(
        m.species(),
        crate::mobs::Species::Piglin | crate::mobs::Species::PiglinBrute
    );
    m.extra.pick_up_loot = tag
        .get("CanPickUpLoot")
        .and_then(int_value)
        .map_or(piglin, |value| value as i8 != 0);
    m.custom_name = custom_name(tag);
    m.custom_name_text = tag.get("CustomName").map(name_text);
    m.extra.left_handed = flag(tag, "LeftHanded");
    if let Some(Tag::Compound(c)) = tag.get("drop_chances") {
        let mut chances = [crate::mobs::drops::DEFAULT_DROP_CHANCE; 8];
        for (chance, slot) in chances.iter_mut().zip(crate::mobs::drops::SLOTS) {
            if let Some(Tag::Float(v)) = c.get(&slot.into()) {
                *chance = f32::from_bits(*v);
            }
        }
        m.extra.drop_chances = Some(Box::new(chances));
    }
    if saves_age(m.species()) {
        m.extra.age = tag.get("Age").and_then(int_value).unwrap_or(0);
        m.extra.forced_age = tag.get("ForcedAge").and_then(int_value).unwrap_or(0);
        m.extra.age_locked = flag(tag, "AgeLocked");
    }
    if is_animal(&m.kind.id) {
        m.extra.in_love = tag.get("InLove").and_then(int_value).unwrap_or(0);
        // Animal.readAdditionalSaveData: the player who fed it into love.
        m.extra.love_cause = crate::save::uuid_from_tag(tag.get("LoveCause"));
    }
    m.name_visible = flag(tag, "CustomNameVisible");
    m.no_ai = flag(tag, "NoAI");
    m.no_gravity = flag(tag, "NoGravity");
    m.glowing = flag(tag, "Glowing");
    m.ticks_frozen = number(tag.get("TicksFrozen")).unwrap_or(0) as i32;
    // LivingEntity.readAdditionalSaveData: the saved `active_effects`, the last
    // entry of an effect winning.
    if let Some(Tag::List(effects)) = tag.get("active_effects") {
        for e in effects {
            let Some(def) = text(e.get("id")).and_then(|id| crate::brewing::effect(&id)) else {
                continue;
            };
            let effect = crate::brewing::namespaced(def.id);
            let duration = number(e.get("duration")).unwrap_or(0) as i32;
            // `MobEffectInstance.Details`: Codec.BYTE uses boxed Number.byteValue,
            // then UNSIGNED_BYTE interprets the resulting byte as 0..255.
            let amplifier = e
                .get("amplifier")
                .and_then(horizon_world::datafix::dynamic::as_number)
                .map_or(0, |n| i32::from(n.byte_value() as u8));
            m.extra.effects.retain(|x| x.0 != effect);
            m.extra.effects.push((effect, duration, amplifier));
            // Details.CODEC: `show_particles` defaults to true.
            m.extra.hidden_effects.retain(|h| *h != effect);
            if number(e.get("show_particles")) == Some(0) {
                m.extra.hidden_effects.push(effect);
            }
            // Details.CODEC: `ambient` defaults to false.
            m.extra.ambient_effects.retain(|h| *h != effect);
            if number(e.get("ambient")).is_some_and(|a| a != 0) {
                m.extra.ambient_effects.push(effect);
            }
            m.set_effect_fallback(
                effect,
                e.get("hidden_effect")
                    .and_then(|h| mob_hidden_effect(h, effect))
                    .map(Box::new),
            );
        }
        if m.extra
            .effects
            .iter()
            .any(|e| e.0 == "minecraft:health_boost")
        {
            crate::mobs::finalize::health_boost_changed(&mut m);
        }
    }
    m.silent = flag(tag, "Silent");
    m.invulnerable_flag = flag(tag, "Invulnerable");
    // Entity.load: `getIntOr("invulnerable_time", 0)`.
    m.invulnerable_time = number(tag.get("invulnerable_time")).unwrap_or(0) as i32;
    if m.species() == crate::mobs::Species::Bat {
        // Bat.readAdditionalSaveData: BatFlags bit 0, resting.
        m.extra.flag = number(tag.get("BatFlags")).unwrap_or(0) & 1 != 0;
    }
    if m.species() == crate::mobs::Species::Creeper {
        m.extra.charged = flag(tag, "powered");
        // Creeper.readAdditionalSaveData: `ignited` lights the fuse.
        m.extra.flag = flag(tag, "ignited");
        m.max_swell = number(tag.get("Fuse")).unwrap_or(30) as i16 as i32;
        // `getByteOr("ExplosionRadius", 3)`: any numeric tag, as a byte.
        m.extra.explosion_radius = number(tag.get("ExplosionRadius")).unwrap_or(3) as i8;
    }
    // InventoryCarrier.readInventoryFromTag, using the existing occupied-stack model.
    let carrier_slots = match m.species() {
        crate::mobs::Species::Pillager => 5,
        crate::mobs::Species::WanderingTrader => 8,
        _ => 0,
    };
    if carrier_slots != 0
        && let Some(Tag::List(items)) = tag.get("Inventory")
    {
        m.extra.inventory = items
            .iter()
            .filter_map(crate::save::item_from_nbt)
            .take(carrier_slots)
            .collect();
    }
    if matches!(
        m.species(),
        crate::mobs::Species::Piglin
            | crate::mobs::Species::PiglinBrute
            | crate::mobs::Species::Hoglin
    ) {
        m.extra.flag2 = flag(tag, "IsImmuneToZombification");
        m.extra.counter2 = number(tag.get("TimeInOverworld")).unwrap_or(0) as i32;
        if m.species() == crate::mobs::Species::Piglin {
            crate::mobs::piglin::read_guarded_anger(&mut m, tag);
        }
        if m.species() == crate::mobs::Species::Hoglin {
            m.extra.cannot_be_hunted = flag(tag, "CannotBeHunted");
        }
        if m.species() == crate::mobs::Species::Piglin
            && let Some(Tag::List(items)) = tag.get("Inventory")
        {
            // InventoryCarrier.readInventoryFromTag -> SimpleContainer
            // .fromItemList (eight slots).
            m.extra.inventory = crate::mobs::villager::load_container(&m, items, 8);
        }
    }
    if matches!(
        m.species(),
        crate::mobs::Species::Pig | crate::mobs::Species::Strider
    ) {
        let saddle = tag
            .get("equipment")
            .and_then(|e| e.get("saddle"))
            .and_then(crate::save::item_from_nbt);
        if saddle.is_some() || flag(tag, "Saddle") {
            m.set_saddle(Some(saddle.unwrap_or_else(|| {
                crate::items::ItemStack::new("minecraft:saddle", 1)
            })));
        }
    }
    if crate::mobs::equine::is_equine(m.species()) {
        m.extra.tame = flag(tag, "Tame");
        // AbstractHorse.readAdditionalSaveData: the saved Int as is, camels included.
        m.extra.equine_temper = tag.get("Temper").and_then(int_value).unwrap_or(0);
        m.extra.bred = flag(tag, "Bred");
        m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
        let v = number(tag.get("Variant")).unwrap_or(0) as i32;
        (m.extra.variant, m.extra.variant2) = crate::mobs::equine::horse_look(v);
        if crate::mobs::llama::is_llama(m.species()) {
            crate::mobs::llama::from_nbt(&mut m, tag);
        }
        m.extra.chested = flag(tag, "ChestedHorse");
        if m.extra.chested {
            m.extra.items = crate::save::decode_items(tag.get("Items"), 15);
        }
        let eq = tag.get("equipment");
        m.set_saddle(
            eq.and_then(|e| e.get("saddle"))
                .and_then(crate::save::item_from_nbt),
        );
        m.extra.body = eq
            .and_then(|e| e.get("body"))
            .and_then(|b| text(b.get("id")))
            .map(Into::into);
        m.extra.head = eq
            .and_then(|e| e.get("head"))
            .and_then(|b| text(b.get("id")))
            .map(Into::into);
        if let Some(Tag::List(attrs)) = tag.get("attributes") {
            for a in attrs {
                let (Some(id), Some(v)) = (text(a.get("id")), float(a.get("base"))) else {
                    continue;
                };
                if id == "minecraft:jump_strength" {
                    m.extra.vector[0] = v;
                }
            }
        }
        if m.extra.vector[0] <= 0.0 {
            m.extra.vector[0] = if crate::mobs::equine::is_chested(m.species()) {
                0.5
            } else {
                0.7
            };
        }
    }
    if crate::mobs::finalize::saves_variant_name(m.species())
        && let Some(v) = text(tag.get("variant"))
    {
        m.extra.variant = crate::mobs::finalize::variant_index(m.species(), &v);
    }
    if m.species() == crate::mobs::Species::Chicken
        && let Some(t) = number(tag.get("EggLayTime"))
    {
        m.extra.egg_time = t as i32;
    }
    if let Some(v) = text(tag.get("sound_variant")) {
        let v = v.trim_start_matches("minecraft:");
        let sounds = crate::mobs::finalize::sound_variants(m.species());
        m.extra.sound_variant = sounds.iter().position(|n| *n == v).map(|i| i as u8);
    }
    match m.species() {
        crate::mobs::Species::Fox => {
            m.extra.variant = u8::from(text(tag.get("Type")).as_deref() == Some("snow"));
            // Fox.readAdditionalSaveData: clearTrusted, then addTrustedEntity for
            // each of "Trusted".
            crate::mobs::fox::clear_trusted(&mut m);
            if let Some(Tag::List(trusted)) = tag.get("Trusted") {
                for uuid in trusted
                    .iter()
                    .filter_map(|t| crate::save::uuid_from_tag(Some(t)))
                {
                    crate::mobs::fox::add_trusted_entity(&mut m, uuid);
                }
            }
            // Fox.readAdditionalSaveData: the sleeping, sitting and crouching flags.
            for (key, bit) in [
                ("Sleeping", crate::mobs::fox::SLEEPING),
                ("Sitting", crate::mobs::fox::SITTING),
                ("Crouching", crate::mobs::fox::CROUCHING),
            ] {
                if flag(tag, key) {
                    m.extra.ints[0] |= bit;
                }
            }
        }
        // MushroomCow.readAdditionalSaveData: a missing or unknown name is
        // Variant.DEFAULT, red; stew_effects decodes like the component's list.
        crate::mobs::Species::Mooshroom => {
            m.extra.variant = u8::from(text(tag.get("Type")).as_deref() == Some("brown"));
            if let Some(Tag::List(list)) = tag.get("stew_effects") {
                m.extra.stew_effects = list
                    .iter()
                    .map(|entry| {
                        Some((
                            Arc::from(text(entry.get("id"))?.as_str()),
                            number(entry.get("duration")).unwrap_or(160) as i32,
                        ))
                    })
                    .collect();
            }
        }
        crate::mobs::Species::Rabbit => {
            // Rabbit.Variant.byId: an unknown id is brown.
            m.extra.variant = match number(tag.get("RabbitType")).unwrap_or(0) {
                v @ (0..=5 | 99) => v as u8,
                _ => 0,
            };
        }
        crate::mobs::Species::Goat => crate::mobs::goat::read_nbt(&mut m, tag),
        _ => {}
    }
    // `Rabbit.readAdditionalSaveData`.
    if m.species() == crate::mobs::Species::Rabbit {
        m.extra.more_carrot_ticks = number(tag.get("MoreCarrotTicks")).unwrap_or(0) as i32;
    }
    // `Ocelot.readAdditionalSaveData`.
    if m.species() == crate::mobs::Species::Ocelot {
        m.extra.trusting = flag(tag, "Trusting");
    }
    if m.species() == crate::mobs::Species::Cat {
        m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
        m.extra.tame = m.extra.owner.is_some();
        crate::mobs::tame::restore_sitting(&mut m, flag(tag, "Sitting"));
        m.extra.color =
            number(tag.get("CollarColor")).map_or(crate::mobs::tame::RED, |c| (c as u8) & 15);
    }
    if m.species() == crate::mobs::Species::Panda {
        let gene = |key| text(tag.get(key)).map_or(0, |n| crate::mobs::panda::gene_from_name(&n));
        m.extra.variant = gene("MainGene");
        m.extra.variant2 = gene("HiddenGene");
    }
    if m.species() == crate::mobs::Species::Parrot {
        m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
        m.extra.tame = m.extra.owner.is_some();
        crate::mobs::tame::restore_sitting(&mut m, flag(tag, "Sitting"));
    }
    if m.species() == crate::mobs::Species::Wolf {
        let health = m.health;
        m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
        let tame = m.extra.owner.is_some();
        crate::mobs::tame::set_tame(&mut m, tame);
        m.health = health;
        crate::mobs::tame::restore_sitting(&mut m, flag(tag, "Sitting"));
        let armor = tag
            .get("equipment")
            .and_then(|e| e.get("body"))
            .and_then(crate::save::item_from_nbt);
        m.extra.body_damage = armor.as_ref().map_or(0, |s| s.damage as i32);
        m.extra.body_extra = armor.and_then(|s| s.extra);
        m.extra.color =
            number(tag.get("CollarColor")).map_or(crate::mobs::tame::RED, |c| (c as u8) & 15);
    }
    if matches!(
        m.species(),
        crate::mobs::Species::Villager | crate::mobs::Species::WanderingTrader
    ) {
        read_villager(&mut m, tag);
    }
    if m.species() == crate::mobs::Species::ZombieVillager {
        read_villager(&mut m, tag);
        let xp = number(tag.get("Xp")).unwrap_or(0) as i32;
        if xp != 0 {
            m.extra.trade.get_or_insert_with(Default::default).xp = xp;
        }
        crate::mobs::zombie_villager::read_nbt(&mut m, tag);
    }
    if m.species() == crate::mobs::Species::Dolphin {
        // Dolphin.readAdditionalSaveData: a save that names neither field is an
        // unfed dolphin at full moistness.
        m.extra.flag = flag(tag, "GotFish");
        m.extra.counter = number(tag.get("Moistness")).unwrap_or(2400) as i32;
    }
    if m.species() == crate::mobs::Species::Axolotl {
        restore_axolotl_memories(&mut m, tag);
    }
    read_variant(&mut m, tag);
    m.fire_ticks = i32::from(number(tag.get("Fire")).unwrap_or(0) as i16);
    // Entity.load: `input.getIntOr("Air", getMaxAirSupply())`, 4800 for a dolphin.
    m.air = number(tag.get("Air")).map_or(m.max_air_supply(), |air| air as i32);
    if m.species() == crate::mobs::Species::Allay {
        if let Some(Tag::List(inv)) = tag.get("Inventory") {
            for stack in inv.iter().filter_map(item_from_nbt) {
                let max = m.kind.inventory_stack_limit(&stack);
                if let Some(st) = m.extra.allay.as_mut() {
                    st.add(stack, max);
                }
            }
        }
        m.extra.egg_time = number(tag.get("DuplicationCooldown")).unwrap_or(0) as i32;
        if let (Some(st), Some(listener)) = (m.extra.allay.as_mut(), tag.get("listener")) {
            st.vibration = crate::sculk::Data::from_nbt(listener);
        }
        let liked = tag
            .get("Brain")
            .and_then(|b| b.get("memories"))
            .and_then(|b| b.get("minecraft:liked_player"));
        m.extra.tame = liked.is_some();
        m.extra.owner = liked
            .and_then(|l| l.get("value"))
            .and_then(|v| crate::save::uuid_from_tag(Some(v)));
        // `Brain.readAdditionalSaveData`: a memory saved with a time to live
        // (`Codec.LONG`, read from any integer tag). `Long.MAX_VALUE` is
        // `MemorySlot.NEVER_EXPIRE`: that memory never expires.
        if let Some(st) = m.extra.allay.as_mut() {
            st.liked_player_ttl = liked
                .and_then(|l| match l.get("ttl")? {
                    Tag::Byte(v) => Some(i64::from(*v)),
                    Tag::Short(v) => Some(i64::from(*v)),
                    Tag::Int(v) => Some(i64::from(*v)),
                    Tag::Long(v) => Some(*v),
                    _ => None,
                })
                .filter(|&ttl| ttl != i64::MAX);
        }
        let memory = |key: &str| {
            tag.get("Brain")
                .and_then(|b| b.get("memories"))
                .and_then(|b| b.get(key))
                .and_then(|l| l.get("value"))
        };
        m.extra.ints[2] =
            number(memory("minecraft:liked_noteblock_cooldown_ticks")).unwrap_or(0) as i32;
        let pickup = number(memory("minecraft:item_pickup_cooldown_ticks")).map(|t| t as i32);
        crate::mobs::allay::set_pickup_cooldown(&mut m, pickup);
        if let Some(st) = m.extra.allay.as_mut()
            && let Some(noteblock) = memory("minecraft:liked_noteblock")
            && let Some(Tag::IntArray(p)) = noteblock.get("pos")
            && p.len() == 3
        {
            st.noteblock = Some(crate::mobs::allay::GlobalPos {
                dimension: crate::mobs::allay::dimension_id(
                    &text(noteblock.get("dimension")).unwrap_or_default(),
                ),
                pos: [p[0], p[1], p[2]],
            });
        }
    }
    if m.species() == crate::mobs::Species::Frog
        && tag
            .get("Brain")
            .and_then(|b| b.get("memories"))
            .and_then(|b| b.get("minecraft:is_pregnant"))
            .is_some()
    {
        crate::mobs::frog::set_pregnant(&mut m, true);
    }
    if crate::mobs::camel::is_camel(m.species()) {
        // AbstractHorse.readAdditionalSaveData: the eating state (Temper is
        // read with the other equines' above).
        m.extra.flag = flag(tag, "EatingHaystack");
        // The tick is a game time: `camel::finish_load` puts it on the mob's
        // clock once the world's time is known.
        let tick = match tag.get("LastPoseTick") {
            Some(Tag::Long(v)) => *v,
            _ => 0,
        };
        crate::mobs::camel::stand_up_instantly(&mut m);
        m.extra.pose_change_tick = tick;
        m.extra.camel_sitting = tick < 0;
        // Camels are always tame (`Camel.register` in Horizon).
        m.extra.tame = true;
    }
    if crate::mobs::nautilus::is_nautilus(m.species()) {
        crate::mobs::nautilus::from_nbt(&mut m, tag);
    }
    if m.species() == crate::mobs::Species::SulfurCube {
        crate::mobs::sulfur_cube::from_nbt(&mut m, tag);
    }
    if m.species() == crate::mobs::Species::Bee {
        crate::mobs::bee::from_nbt(&mut m, tag);
    }
    if m.species() == crate::mobs::Species::Creaking
        && let Some(Tag::IntArray(home)) = tag.get("home_pos")
        && home.len() == 3
    {
        crate::mobs::creaking::set_transient(&mut m, [home[0], home[1], home[2]]);
    }
    if m.species() == crate::mobs::Species::Turtle {
        // Turtle.readAdditionalSaveData: `home_pos` (else where it stands) and `has_egg`.
        m.extra.flag = flag(tag, "has_egg");
        m.extra.home = Some(match tag.get("home_pos") {
            Some(Tag::IntArray(home)) if home.len() == 3 => [home[0], home[1], home[2]],
            _ => [
                m.position.x.floor() as i32,
                m.position.y.floor() as i32,
                m.position.z.floor() as i32,
            ],
        });
    }
    if m.species() == crate::mobs::Species::Wither {
        m.extra.counter = number(tag.get("Invul")).unwrap_or(0) as i32;
    }
    if m.species() == crate::mobs::Species::Sniffer {
        crate::mobs::sniffer::from_nbt(&mut m, tag);
    }
    if m.species() == crate::mobs::Species::CopperGolem {
        // CopperGolem.readAdditionalSaveData: -1 (unset) when missing.
        m.extra.vector[0] = match tag.get("next_weather_age") {
            Some(Tag::Long(v)) => *v as f64,
            _ => -1.0,
        };
        let weather = match tag.get("weather_state") {
            Some(Tag::String(s)) => s.to_string_lossy(),
            _ => String::new(),
        };
        m.extra.variant = crate::mobs::copper_golem::WEATHER
            .iter()
            .position(|w| *w == weather)
            .unwrap_or(0) as u8;
    }
    m.extra.leash_pending = crate::mobs::leash::pending_from_nbt(tag.get("leash"));
    if m.species() == crate::mobs::Species::Warden {
        // The brain starts afresh: a minute before it digs back down.
        crate::mobs::warden::finalize(&mut m, false, &mut Vec::new());
        if let Some(st) = m.extra.warden.as_mut() {
            if let Some(listener) = tag.get("listener") {
                st.vibration = crate::sculk::Data::from_nbt(listener);
            }
            // `Warden.readAdditionalSaveData`: the anger comes back as the list
            // of UUIDs it was saved as. Every suspect starts in the UUID map and
            // the anger ticks move the loaded ones into the live one, since a
            // suspect may be loaded long after the warden.
            if let Some(anger) = tag.get("anger") {
                st.anger =
                    crate::mobs::warden::AngerManagement::from_saved(warden_anger_from_nbt(anger));
            }
        }
        restore_warden_owned_memories(&mut m, tag);
    }
    // Armadillo.readAdditionalSaveData: the saved state through `switchToState`
    // (`onSyncedDataUpdated` then restarts the time in that state), idle for a
    // save that names none; then the time to the next scute, the constructor's
    // when absent.
    if m.species() == crate::mobs::Species::Armadillo {
        crate::mobs::armadillo::set_state(
            &mut m,
            crate::mobs::armadillo::state_by_name(text(tag.get("state")).as_deref()),
        );
        if let Some(t) = number(tag.get("scute_time")) {
            m.extra.egg_time = t as i32;
        } else {
            // Armadillo(): scuteTime = pickNextScuteDropTime().
            m.extra.egg_time = crate::mobs::ai::next_scute_time(&mut m);
        }
    }
    if m.species() == crate::mobs::Species::Ravager {
        m.extra.counter = number(tag.get("AttackTick")).unwrap_or(0) as i32;
        m.extra.counter2 = number(tag.get("StunTick")).unwrap_or(0) as i32;
        m.extra.anim_ticks = number(tag.get("RoarTick")).unwrap_or(0) as i32;
    }
    if m.species() == crate::mobs::Species::Endermite {
        m.extra.counter = number(tag.get("Lifetime")).unwrap_or(0) as i32;
    }
    if m.species() == crate::mobs::Species::Vex {
        crate::mobs::vex::read_nbt(&mut m, tag);
    }
    // Bogged.readAdditionalSaveData: unsheared when absent.
    if m.species() == crate::mobs::Species::Bogged {
        m.extra.sheared = flag(tag, "sheared");
    }
    // Sheep.readAdditionalSaveData: white and unsheared when absent.
    if m.species() == crate::mobs::Species::Sheep {
        m.extra.color = crate::mobs::farm::saved_color(tag.get("Color"));
        m.extra.sheared = flag(tag, "Sheared");
    }
    // GlowSquid.readAdditionalSaveData: not dimmed when absent.
    if m.species() == crate::mobs::Species::GlowSquid {
        crate::mobs::aquatic::set_dark_ticks(
            &mut m,
            number(tag.get("DarkTicksRemaining")).unwrap_or(0) as i32,
        );
    }
    // IronGolem.readAdditionalSaveData: not player created when absent.
    if m.species() == crate::mobs::Species::IronGolem {
        m.extra.player_created = flag(tag, "PlayerCreated");
    }
    // SnowGolem.readAdditionalSaveData: getBooleanOr("Pumpkin", true).
    if m.species() == crate::mobs::Species::SnowGolem {
        m.extra.sheared = !tag
            .get("Pumpkin")
            .and_then(int_value)
            .is_none_or(|value| value as i8 != 0);
    }
    // Pufferfish.readAdditionalSaveData: `getIntOr("PuffState", 0)`, at most 2.
    if m.species() == crate::mobs::Species::Pufferfish {
        m.extra.size = number(tag.get("PuffState")).unwrap_or(0).min(2) as i32;
    }
    // Enderman.readAdditionalSaveData: the block it carried; an air state
    // reads as no block at all.
    if m.species() == crate::mobs::Species::Enderman
        && let Some(registry) = registry
    {
        m.extra.carried = tag
            .get("carriedBlockState")
            .and_then(|t| state_from_nbt(registry, t))
            .filter(|&state| !is_air(registry, state));
    }
    // HappyGhast.readAdditionalSaveData: the still timeout, 0 when absent (a fresh
    // ghast, or one saved by a version that did not write it).
    if m.species() == crate::mobs::Species::HappyGhast {
        m.extra.counter = number(tag.get("still_timeout")).unwrap_or(0) as i32;
    }
    if m.species() == crate::mobs::Species::Shulker {
        m.extra.variant3 = (number(tag.get("AttachFace")).unwrap_or(0) as i8 % 6).unsigned_abs();
        m.extra.counter = number(tag.get("Peek")).map_or(0, |v| v as i8 as i32);
        m.extra.color = number(tag.get("Color")).map_or(16, |v| v as i8 as u8);
        m.anim.v[0] = m.extra.counter as f32 * 0.01;
        m.anim.vo[0] = m.anim.v[0];
        // Shulker.setPos: square on its block.
        m.position = horizon_core::geometry::Vec3::new(
            m.position.x.floor() + 0.5,
            (m.position.y + 0.5).floor(),
            m.position.z.floor() + 0.5,
        );
        m.previous = m.position;
    }
    if matches!(
        m.species(),
        crate::mobs::Species::Slime | crate::mobs::Species::MagmaCube
    ) {
        // AbstractCubeMob.readAdditionalSaveData: the size's attributes, then
        // the saved health, the max health when there is none.
        crate::mobs::cube::set_size(&mut m, number(tag.get("Size")).unwrap_or(0) as i32 + 1);
        if let Some(h) = float(tag.get("Health")) {
            m.health = h as f32;
        }
        m.extra.flag2 = flag(tag, "wasOnGround");
    }
    if m.species() == crate::mobs::Species::Phantom {
        // Phantom.readAdditionalSaveData: setPhantomSize(size), and the circling
        // anchor, absent or unreadable when the save has no anchor at all
        // (`input.read("anchor_pos", BlockPos.CODEC).orElse(null)`).
        crate::mobs::phantom::set_size(&mut m, number(tag.get("size")).unwrap_or(0) as i32);
        m.extra.home = home_block_pos(tag.get("anchor_pos"));
    }
    // Zombie, Piglin and Zoglin.readAdditionalSaveData: setBaby(IsBaby), after the
    // saved modifiers, so it also removes a saved permanent `minecraft:baby`.
    if !crate::mobs::finalize::ageable(m.species()) {
        crate::mobs::finalize::set_baby(&mut m, flag(tag, "IsBaby"));
    }
    // Zombie.readAdditionalSaveData: setCanBreakDoors.
    if crate::mobs::doors::is_zombie(m.species()) {
        crate::mobs::doors::set_can_break_doors(&mut m, flag(tag, "CanBreakDoors"));
    }
    // Zombie and Skeleton.readAdditionalSaveData: `ConversionTracker` reads its
    // affliction time (0 when absent) and starts a conversion from a saved time
    // other than -1, -1 stopping one (`setConverting(false)`).
    let tracker = match m.species() {
        crate::mobs::Species::Zombie | crate::mobs::Species::Husk => {
            Some(("InWaterTime", "DrownedConversionTime"))
        }
        crate::mobs::Species::Skeleton => Some(("FreezingTime", "StrayConversionTime")),
        _ => None,
    };
    if let Some((affliction, conversion)) = tracker {
        m.extra.counter = number(tag.get(affliction)).unwrap_or(0) as i32;
        let time = number(tag.get(conversion)).unwrap_or(-1) as i32;
        // Any other time starts a conversion (`startConversion`), a negative
        // one spent on the next tick.
        m.extra.counter2 = if time == -1 { 0 } else { time.max(0) + 1 };
    }
    // SpellcasterIllager.readAdditionalSaveData: this is the same timer used
    // by casting goals and the existing customServerAiStep countdown.
    if matches!(
        m.species(),
        crate::mobs::Species::Evoker | crate::mobs::Species::Illusioner
    ) {
        m.extra.counter = number(tag.get("SpellTicks")).unwrap_or(0) as i32;
    }
    // Vindicator.readAdditionalSaveData: `Johnny`, false when absent. A name
    // that is Johnny latches the flag again on the next AI step.
    if m.species() == crate::mobs::Species::Vindicator {
        m.extra.johnny = flag(tag, "Johnny");
    }
    crate::mobs::raider::read_nbt(&mut m, tag);
    crate::mobs::riders::read_nbt(&mut m, tag);
    // A mannequin is an `Avatar`, not a `Mob`: it reads none of
    // `Mob.readAdditionalSaveData`'s flags, after the common reads above.
    if m.species() == crate::mobs::Species::Mannequin {
        m.persistent = false;
        m.extra.pick_up_loot = false;
        m.extra.left_handed = false;
        m.no_ai = false;
    }
    // setHealth clamps a negative health to 0; a dead mob keeps its body (tickDeath).
    if m.health < 0.0 {
        m.health = 0.0;
    }
    Some(m)
}

/// `ItemEntity`: Health, Age, PickupDelay, Item, Thrower.
pub fn item_entity_to_nbt(e: &ItemEntity, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    item_entity_to_nbt_saved(e, uuid, raw, item_to_nbt(&e.stack))
}

/// ItemEntity saved with the actual loaded item prototype, including transfers.
pub fn item_entity_to_nbt_with_items(
    e: &ItemEntity,
    uuid: [i32; 4],
    raw: Option<&Tag>,
    items: &crate::items::Items,
) -> Tag {
    item_entity_to_nbt_saved(
        e,
        uuid,
        raw,
        horizon_items::item_nbt::item_to_nbt_with_items(&e.stack, items),
    )
}

fn item_entity_to_nbt_saved(e: &ItemEntity, uuid: [i32; 4], raw: Option<&Tag>, item: Tag) -> Tag {
    let base = Base {
        id: "minecraft:item",
        uuid,
        position: e.position,
        motion: e.velocity,
        rotation: e.rotation,
        on_ground: e.on_ground,
        fall_distance: e.fall_distance,
    };
    let mut tag = base.non_mob_tag(raw);
    for (key, value) in [
        ("Item", item),
        ("Health", Tag::Short(e.health as i16)),
        ("Fire", Tag::Short(e.fire_ticks as i16)),
        (
            "Age",
            Tag::Short(e.age.clamp(i16::MIN as i32, i16::MAX as i32) as i16),
        ),
        (
            "PickupDelay",
            Tag::Short(
                e.pickup_delay
                    .clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16,
            ),
        ),
    ] {
        put(&mut tag, key, value);
    }
    if let Some(thrower) = e.thrower {
        put(&mut tag, "Thrower", Tag::IntArray(thrower.to_vec()));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"Thrower".into());
    }
    if let Some(target) = e.target {
        put(&mut tag, "Owner", Tag::IntArray(target.to_vec()));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"Owner".into());
    }
    tag
}
/// `PrimedTnt.readAdditionalSaveData`: `explosion_power`, 4 by default and
/// clamped to 0..128.
pub fn tnt_power(tag: &Tag) -> f32 {
    // `getFloatOr` accepts any numeric tag, not only a float.
    let power = match tag.get("explosion_power") {
        Some(Tag::Byte(v)) => f32::from(*v),
        Some(Tag::Short(v)) => f32::from(*v),
        Some(Tag::Int(v)) => *v as f32,
        Some(Tag::Long(v)) => *v as f32,
        Some(Tag::Float(bits)) => f32::from_bits(*bits),
        Some(Tag::Double(bits)) => f64::from_bits(*bits) as f32,
        _ => 4.0,
    };
    power.clamp(0.0, 128.0)
}

/// `PrimedTnt.addAdditionalSaveData`: its `fuse` and, when it is not the
/// default, its `explosion_power`, the encoded live block state, and its
/// `owner` (`EntityReference.store`). With no live owner, a loaded `owner`
/// stays as the raw compound has it.
#[allow(clippy::too_many_arguments)]
pub fn tnt_to_nbt(
    position: Vec3,
    motion: Vec3,
    on_ground: bool,
    fall_distance: f64,
    fuse: i32,
    power: f32,
    block_state: Tag,
    rotation: [f32; 2],
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
    raw: Option<&Tag>,
) -> Tag {
    let mut tag = Base {
        id: "minecraft:tnt",
        uuid,
        position,
        motion,
        rotation,
        on_ground,
        fall_distance,
    }
    .tag(raw);
    put(&mut tag, "fuse", Tag::Short(fuse as i16));
    if power != 4.0 {
        put(&mut tag, "explosion_power", Tag::Float(power.to_bits()));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"explosion_power".into());
    }
    put(&mut tag, "block_state", block_state);
    if let Some(owner) = owner {
        put(&mut tag, "owner", Tag::IntArray(owner.to_vec()));
    }
    tag
}
/// `PrimedTnt.readAdditionalSaveData`: `EntityReference.read(input, "owner")`.
pub fn tnt_owner(tag: &Tag) -> Option<[i32; 4]> {
    uuid_from_tag(tag.get("owner"))
}
/// `EvokerFangs.addAdditionalSaveData`: always save the live warmup counter.
/// Known Owner UUIDs use `EntityReference.store` encoding; unresolved
/// loaded Owner data remains in the raw compound.
pub fn evoker_fangs_to_nbt(
    position: Vec3,
    yaw: f32,
    warmup: i32,
    owner: Option<[i32; 4]>,
    uuid: [i32; 4],
    raw: Option<&Tag>,
) -> Tag {
    let mut tag = Base {
        id: "minecraft:evoker_fangs",
        uuid,
        position,
        motion: raw.map(motion).unwrap_or(Vec3::ZERO),
        rotation: [yaw, raw.map(rotation).unwrap_or([0.0; 2])[1]],
        on_ground: raw.is_some_and(|t| flag(t, "OnGround")),
        fall_distance: 0.0,
    }
    .tag(raw);
    put(&mut tag, "Warmup", Tag::Int(warmup));
    if let Some(owner) = owner {
        put(&mut tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
    tag
}

/// `UUIDUtil.CODEC` via `NbtOps.getIntStream`: boxed Number conversions,
/// with the first-four partial value retained by `TagValueInput.read`.
fn item_uuid_from_tag(tag: Option<&Tag>) -> Option<[i32; 4]> {
    let values = horizon_world::datafix::dynamic::as_int_stream(tag)?;
    values.get(..4)?.try_into().ok()
}

/// The `Entity` fields a saved non-mob entity carries (`Base::tag`), for a
/// caller that writes its own kind's fields around them.
pub fn entity_base_tag(
    id: &str,
    position: Vec3,
    motion: Vec3,
    rotation: [f32; 2],
    uuid: [i32; 4],
    raw: Option<&Tag>,
) -> Tag {
    Base {
        id,
        uuid,
        position,
        motion,
        rotation,
        on_ground: raw.is_some_and(|t| flag(t, "OnGround")),
        fall_distance: 0.0,
    }
    .tag(raw)
}

/// `ItemEntity.readAdditionalSaveData`: `getShortOr` (floored, then narrowed) for three;
/// Owner and Thrower use the numeric UUID codec.
pub fn item_entity_from_nbt(tag: &Tag, id: u32, bob_offset: f32) -> Option<ItemEntity> {
    let stack = item_from_nbt(tag.get("Item")?)?;
    let position = position(tag)?;
    let short = |key, default| match tag.get(key) {
        Some(Tag::Float(v)) => f32::from_bits(*v).floor() as i32 as i16,
        Some(Tag::Double(v)) => f64::from_bits(*v).floor() as i32 as i16,
        other => number(other).unwrap_or(default) as i16,
    };
    Some(ItemEntity {
        rotation: rotation(tag),
        join: Default::default(),
        no_gravity: flag(tag, "NoGravity"),
        health: i32::from(short("Health", 5)),
        fire_ticks: i32::from(short("Fire", 0)),
        id,
        stack,
        thrower: item_uuid_from_tag(tag.get("Thrower")),
        target: item_uuid_from_tag(tag.get("Owner")),
        position,
        previous: position,
        velocity: motion(tag),
        on_ground: flag(tag, "OnGround"),
        vertical_collision: false,
        horizontal_collision: false,
        // Entity.load: getDoubleOr, any number widened.
        fall_distance: float(tag.get("fall_distance")).unwrap_or(0.0),
        age: i32::from(short("Age", 0)),
        pickup_delay: i64::from(short("PickupDelay", 0)),
        bob_offset,
        ticks: 0,
        needs_sync: false,
        stuck: Vec3::ZERO,
        was_touching_water: false,
        // The caller gives it the own random that drew its UUID and bobOffs.
        random: horizon_core::random::LegacyRandom::new(0),
    })
}

/// `OminousItemSpawner.addAdditionalSaveData`: its item (left out when
/// empty) and `spawn_item_after_ticks`. Its tick count is the entity's
/// `tickCount`, which isn't saved: a reloaded spawner counts from 0 again.
/// It never moves or turns: it saves the Motion, Rotation, OnGround and
/// fall_distance it was loaded with (`Entity.load`).
pub fn ominous_item_spawner_to_nbt(
    item: &crate::items::ItemStack,
    position: Vec3,
    spawn_item_after: i64,
    uuid: [i32; 4],
    raw: Option<&Tag>,
) -> Tag {
    let base = Base {
        id: "minecraft:ominous_item_spawner",
        uuid,
        position,
        motion: raw.map_or(Vec3::ZERO, loaded_motion),
        rotation: raw.map_or([0.0, 0.0], rotation),
        on_ground: raw.is_some_and(|t| flag(t, "OnGround")),
        fall_distance: float(raw.and_then(|t| t.get("fall_distance"))).unwrap_or(0.0),
    };
    let mut tag = base.tag(raw);
    // `ItemStack.CODEC` can't encode `ItemStack.EMPTY`: an empty item writes no key.
    if let Tag::Compound(map) = &mut tag {
        map.remove(&"item".into());
    }
    if &*item.item != "minecraft:air" {
        put(&mut tag, "item", item_to_nbt(item));
    }
    put(
        &mut tag,
        "spawn_item_after_ticks",
        Tag::Long(spawn_item_after),
    );
    tag
}
/// `OminousItemSpawner.readAdditionalSaveData`: the item, where it floats
/// and `spawn_item_after_ticks` (0 when absent). None without an item:
/// such a spawner would spawn nothing.
pub fn ominous_item_spawner_from_nbt(tag: &Tag) -> Option<(crate::items::ItemStack, Vec3, i64)> {
    // `readAdditionalSaveData`: `ItemStack.EMPTY` when it has none.
    let item = tag
        .get("item")
        .and_then(item_from_nbt)
        .unwrap_or_else(|| crate::items::ItemStack::new("minecraft:air", 1));
    let after = number(tag.get("spawn_item_after_ticks")).unwrap_or(0);
    Some((item, position(tag)?, after))
}

mod orb;
pub use orb::{orb_to_nbt, orbs_from_nbt};

/// `Projectile` + `AbstractArrow.addAdditionalSaveData` of a live arrow;
/// `in_block` is its `inBlockState`, `raw` the compound it was loaded with.
pub fn arrow_to_nbt(
    p: &Projectile,
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
    in_block: Option<Tag>,
    raw: Option<&Tag>,
) -> Option<Tag> {
    let a = p.arrow.as_deref()?;
    let ProjectileKind::Arrow {
        base_damage,
        critical,
        ..
    } = p.kind
    else {
        return None;
    };
    let base = Base {
        // A spectral arrow is its own entity type.
        id: if a.spectral || &*a.pickup_item.item == "minecraft:spectral_arrow" {
            "minecraft:spectral_arrow"
        } else {
            "minecraft:arrow"
        },
        uuid,
        position: p.position,
        motion: p.velocity,
        rotation: [p.yaw, p.pitch],
        on_ground: p.on_ground,
        fall_distance: a.fall_distance,
    };
    let mut tag = base.tag(raw);
    // `SpectralArrow.addAdditionalSaveData` always writes its integer duration.
    if base.id == "minecraft:spectral_arrow" {
        put(&mut tag, "Duration", Tag::Int(a.spectral_duration));
    }
    for (key, value) in [
        ("Fire", Tag::Short(a.fire_ticks as i16)),
        ("life", Tag::Short(a.life as i16)),
        ("shake", Tag::Byte(a.shake as i8)),
        ("inGround", Tag::Byte(i8::from(a.in_ground))),
        ("pickup", Tag::Byte(a.pickup.byte())),
        ("damage", Tag::Double(base_damage.to_bits())),
        ("crit", Tag::Byte(i8::from(critical))),
        ("PierceLevel", Tag::Byte(a.pierce as i8)),
        ("HasBeenShot", Tag::Byte(i8::from(a.has_been_shot))),
        ("item", item_to_nbt(&a.pickup_item)),
    ] {
        put(&mut tag, key, value);
    }
    if let Tag::Compound(map) = &mut tag {
        map.entry("SoundEvent".into())
            .or_insert_with(|| string("minecraft:entity.arrow.hit"));
        // Projectile: LeftOwner only when set; Owner as the UUID.
        map.remove(&"LeftOwner".into());
        map.remove(&"inBlockState".into());
        map.remove(&"weapon".into());
        map.remove(&"can_break".into());
    }
    if let Some(weapon) = &a.fired_from_weapon {
        put(&mut tag, "weapon", item_to_nbt(weapon));
    }
    // `Projectile.addAdditionalSaveData`: the actual decoded predicate,
    // never a stale source compound or the pickup item's component patch.
    if let Some(predicate) = &a.can_break {
        put(&mut tag, "can_break", predicate.clone());
    }
    if p.left_owner {
        put(&mut tag, "LeftOwner", Tag::Byte(1));
    }
    if let Some(state) = in_block {
        put(&mut tag, "inBlockState", state);
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if p.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    }
    if let Some(owner) = owner {
        put(&mut tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
    Some(tag)
}
/// A saved arrow as a live one (`readAdditionalSaveData`), with its
/// `inBlockState` compound for the caller to resolve. `effect` is the tipped
/// arrow's lasting effect from its item.
pub fn arrow_from_nbt(
    tag: &Tag,
    player: Option<[i32; 4]>,
    effect: impl FnOnce(&crate::items::ItemStack) -> Option<(&'static str, i32)>,
) -> Option<(Projectile, Option<Tag>)> {
    let position = position(tag)?;
    let owner = uuid_from_tag(tag.get("Owner"));
    let from_player = player.is_some() && owner == player;
    // `getDefaultPickupItem`: a `SpectralArrow` is picked up as its own item.
    let spectral = kind(tag).as_deref() == Some("minecraft:spectral_arrow");
    let default_item = if spectral {
        "minecraft:spectral_arrow"
    } else {
        "minecraft:arrow"
    };
    let item = tag
        .get("item")
        .and_then(item_from_nbt)
        .unwrap_or_else(|| crate::items::ItemStack::new(default_item, 1));
    let kind = ProjectileKind::Arrow {
        base_damage: float(tag.get("damage")).unwrap_or(2.0),
        critical: flag(tag, "crit"),
        effect: effect(&item),
    };
    let mut p = Projectile::new(kind, position, from_player.then_some(Who::Player));
    p.velocity = motion(tag);
    [p.yaw, p.pitch] = rotation(tag);
    p.left_owner = flag(tag, "LeftOwner");
    p.owner_uuid = owner;
    p.no_gravity = flag(tag, "NoGravity");
    p.on_ground = flag(tag, "OnGround");
    let a = p.arrow.as_mut()?;
    a.life = number(tag.get("life")).unwrap_or(0) as i16 as i32;
    a.shake = (number(tag.get("shake")).unwrap_or(0) as i8 as u8).into();
    a.in_ground = flag(tag, "inGround");
    a.pickup = Pickup::from_byte(number(tag.get("pickup")).unwrap_or(0));
    a.pierce = number(tag.get("PierceLevel")).unwrap_or(0) as u8;
    a.has_been_shot = flag(tag, "HasBeenShot");
    // `Projectile.readAdditionalSaveData`: one block predicate or a nonempty
    // list. Unsupported conjuncts remain saved, but cannot grant permission.
    a.can_break = tag
        .get("can_break")
        .filter(|predicate| match predicate {
            Tag::Compound(_) => true,
            Tag::List(list) => {
                !list.is_empty() && list.iter().all(|p| matches!(p, Tag::Compound(_)))
            }
            _ => false,
        })
        .cloned();
    a.fire_ticks = number(tag.get("Fire")).unwrap_or(0) as i16 as i32;
    a.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    a.spectral = spectral;
    // `SpectralArrow.readAdditionalSaveData`: absent Duration defaults to 200.
    if spectral {
        a.spectral_duration = number(tag.get("Duration")).unwrap_or(200) as i32;
    }
    a.pickup_item = item;
    a.fired_from_weapon = tag.get("weapon").and_then(item_from_nbt);
    p.stuck = a.in_ground;
    Some((p, tag.get("inBlockState").cloned()))
}

/// `LargeFireball`, `SmallFireball` and `WitherSkull`: `AbstractHurtingProjectile`'s
/// `acceleration_power`, `Projectile`'s owner and flags, `ExplosionPower`,
/// and a skull's `dangerous`.
pub fn fireball_to_nbt(
    p: &Projectile,
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
    raw: Option<&Tag>,
) -> Option<Tag> {
    let power = match p.kind {
        ProjectileKind::LargeFireball { power } => Some(power),
        ProjectileKind::SmallFireball
        | ProjectileKind::DragonFireball
        | ProjectileKind::WindCharge
        | ProjectileKind::BreezeWindCharge
        | ProjectileKind::WitherSkull { .. } => None,
        _ => return None,
    };
    let base = Base {
        id: p.kind.id(),
        uuid,
        position: p.position,
        motion: p.velocity,
        rotation: [p.yaw, p.pitch],
        on_ground: p.on_ground,
        fall_distance: p.fall_distance,
    };
    let mut tag = base.tag(raw);
    put(
        &mut tag,
        "acceleration_power",
        Tag::Double(p.acceleration.to_bits()),
    );
    put_projectile(&mut tag, p, owner);
    if projectile_fields::keeps_entity_fields(p) {
        put_projectile_entity_fields(&mut tag, p);
    }
    if let Some(power) = power {
        put(&mut tag, "ExplosionPower", Tag::Byte(power as i8));
    }
    // WitherSkull.addAdditionalSaveData: `dangerous`, false for a black skull.
    if let ProjectileKind::WitherSkull { dangerous } = p.kind {
        put(&mut tag, "dangerous", Tag::Byte(i8::from(dangerous)));
    }
    // Fireball.addAdditionalSaveData: the item `getItem` returns, a fire
    // charge unless `setItem` was given another (kept as a single one).
    if let Some(item) = &p.item {
        put(&mut tag, "Item", item_to_nbt(item));
    }
    Some(tag)
}

/// `Projectile.addAdditionalSaveData`: Owner, LeftOwner only when set, and
/// the explicit HasBeenShot flag.
fn put_projectile(tag: &mut Tag, p: &Projectile, owner: Option<[i32; 4]>) {
    if let Tag::Compound(map) = tag {
        map.remove(&"LeftOwner".into());
    }
    if p.left_owner {
        put(tag, "LeftOwner", Tag::Byte(1));
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if p.no_gravity {
        put(tag, "NoGravity", Tag::Byte(1));
    }
    put(tag, "HasBeenShot", Tag::Byte(i8::from(p.has_been_shot)));
    if let Some(owner) = owner {
        put(tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
}

/// The thrown item projectiles Horizon saves: each entity type's id is also
/// the item it saves when none was loaded (`getDefaultItem`).
const THROWABLE_ITEMS: [&str; 6] = [
    "minecraft:snowball",
    "minecraft:egg",
    "minecraft:ender_pearl",
    "minecraft:splash_potion",
    "minecraft:lingering_potion",
    "minecraft:experience_bottle",
];
/// The item a throwable flies as: its stack (a potion's), the mod type's
/// item, or the item its entity type is named after.
fn throwable_item(kind: &ProjectileKind) -> Option<&str> {
    match kind {
        ProjectileKind::Thrown(t) => Some(t.item),
        ProjectileKind::Potion(stack, _) => Some(&stack.item),
        _ => THROWABLE_ITEMS.iter().find(|id| **id == kind.id()).copied(),
    }
}

/// The thrown kind an entity id names: vanilla's, or a content mod's type.
/// A potion flies as the item its entity type names, which is the item a
/// summoned one has when it names none (`getDefaultItem`).
pub fn throwable_kind(id: &str) -> Option<ProjectileKind> {
    if !THROWABLE_ITEMS.contains(&id) {
        return crate::projectiles::thrown_type(id).map(ProjectileKind::Thrown);
    }
    match id {
        "minecraft:snowball" => Some(ProjectileKind::Snowball),
        "minecraft:egg" => Some(ProjectileKind::Egg),
        "minecraft:ender_pearl" => Some(ProjectileKind::EnderPearl),
        // The entity type is the id; the `Item` read later may be another potion.
        "minecraft:experience_bottle" => Some(ProjectileKind::ExperienceBottle),
        _ => Some(ProjectileKind::Potion(
            Box::new(crate::items::ItemStack::new(id, 1)),
            id == "minecraft:lingering_potion",
        )),
    }
}

/// Whether its chunk saves it: the kinds `arrow_to_nbt`, `fireball_to_nbt`
/// and [`throwable_to_nbt`] write.
pub fn saved_with_chunk(p: &Projectile) -> bool {
    // No wildcard: a kind added to `ProjectileKind` must say here whether its
    // chunk saves it (`Entity.shouldBeSaved` is true for every projectile in
    // vanilla), and a `true` needs a writer in `entity_chunks`.
    match &p.kind {
        ProjectileKind::LargeFireball { .. }
        | ProjectileKind::SmallFireball
        | ProjectileKind::DragonFireball
        | ProjectileKind::WindCharge
        | ProjectileKind::BreezeWindCharge
        | ProjectileKind::WitherSkull { .. }
        | ProjectileKind::Snowball
        | ProjectileKind::Egg
        | ProjectileKind::EnderPearl
        | ProjectileKind::Potion(..)
        | ProjectileKind::ExperienceBottle
        | ProjectileKind::LlamaSpit
        | ProjectileKind::ShulkerBullet { .. }
        | ProjectileKind::Thrown(_) => true,
        ProjectileKind::Arrow { .. } => p.arrow.is_some(),
        // Not written yet: tridents save from their own list. Wither skulls and
        // bullets are written now.
        ProjectileKind::Trident => false,
    }
}

/// `ThrowableItemProjectile.addAdditionalSaveData`: `Projectile`'s keys and
/// the `Item` it flies as.
pub fn throwable_to_nbt(
    p: &Projectile,
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
    raw: Option<&Tag>,
) -> Option<Tag> {
    let item = throwable_item(&p.kind)?;
    let base = Base {
        id: p.kind.id(),
        uuid,
        position: p.position,
        motion: p.velocity,
        rotation: [p.yaw, p.pitch],
        on_ground: p.on_ground,
        fall_distance: p.fall_distance,
    };
    let mut tag = base.tag(raw);
    put_projectile(&mut tag, p, owner);
    put_projectile_entity_fields(&mut tag, p);
    // Entity.saveWithoutId writes NoGravity only when set.
    if p.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    }
    // the loaded item stays (blue egg, potion effects) at `setItem`'s count 1.
    if let Tag::Compound(m) = &mut tag {
        let st = match &p.kind {
            ProjectileKind::Potion(s, _) => (**s).clone(),
            _ => crate::items::ItemStack::new(item, 1),
        };
        if let Tag::Compound(i) = m.entry("Item".into()).or_insert_with(|| item_to_nbt(&st)) {
            i.insert("count".into(), Tag::Int(1));
        }
    }
    Some(tag)
}

/// `ThrowableItemProjectile.readAdditionalSaveData` for the kinds
/// [`throwable_to_nbt`] writes; the owner is the player when it is theirs.
pub fn throwable_from_nbt(tag: &Tag, player: Option<[i32; 4]>) -> Option<Projectile> {
    let mut kind = throwable_kind(&kind(tag)?)?;
    if let ProjectileKind::Potion(stack, _) = &mut kind
        && let Some(item) = tag.get("Item").and_then(item_from_nbt)
    {
        **stack = item;
        stack.count = 1;
    }
    let owner = owner_uuid(tag);
    let from_player = player.is_some() && owner == player;
    let mut p = Projectile::new(kind, position(tag)?, from_player.then_some(Who::Player));
    p.velocity = motion(tag);
    [p.yaw, p.pitch] = rotation(tag);
    p.left_owner = flag(tag, "LeftOwner");
    p.has_been_shot = flag(tag, "HasBeenShot");
    p.owner_uuid = owner;
    p.no_gravity = flag(tag, "NoGravity");
    load_projectile_entity_fields(&mut p, tag);
    p.on_ground = flag(tag, "OnGround");
    Some(p)
}

/// `LlamaSpit` and `ShulkerBullet` as `/summon` or a chunk loads them, with
/// `ShulkerBullet.readAdditionalSaveData`'s `Dir`, `Steps` and per-axis target
/// deltas: `Projectile`'s `Owner`, `LeftOwner`, `NoGravity` and the motion.
/// `target` is the entity the caller resolved from `Target`; a bullet loaded
/// without one falls (`ShulkerBullet.tick`).
pub fn spit_or_bullet_from_nbt(
    tag: &Tag,
    player: Option<[i32; 4]>,
    target: Option<Who>,
) -> Option<Projectile> {
    let at = position(tag)?;
    let kind = match kind(tag)?.as_str() {
        "minecraft:llama_spit" => ProjectileKind::LlamaSpit,
        "minecraft:shulker_bullet" => ProjectileKind::ShulkerBullet {
            target,
            // `Dir`: `Direction.LEGACY_ID_CODEC`, the step the bullet is on,
            // absent while it has not chosen one. A byte read through
            // `Direction.from3DDataValue`, which wraps it: `abs(d % 6)`.
            dir: number(tag.get("Dir")).map(|d| ((d as i8) % 6).unsigned_abs()),
            // `getIntOr("Steps", 0)`: any int, a negative one included.
            steps: number(tag.get("Steps")).unwrap_or(0) as i32,
            delta: [
                float(tag.get("TXD")).unwrap_or(0.0),
                float(tag.get("TYD")).unwrap_or(0.0),
                float(tag.get("TZD")).unwrap_or(0.0),
            ],
            // `Entity.random` is not saved: a loaded bullet gets a stream of
            // its own from where it is, as `Projectile::new` does for an
            // arrow.
            seed: (at.x.to_bits() ^ at.z.to_bits().rotate_left(17)) | 1,
        },
        _ => return None,
    };
    let owner = owner_uuid(tag);
    let from_player = player.is_some() && owner == player;
    let mut p = Projectile::new(kind, at, from_player.then_some(Who::Player));
    p.velocity = motion(tag);
    [p.yaw, p.pitch] = rotation(tag);
    p.owner_uuid = owner;
    projectile_fields::load_projectile_flags_and_entity_fields(&mut p, tag);
    p.on_ground = flag(tag, "OnGround");
    Some(p)
}

/// The save of [`spit_or_bullet_from_nbt`]'s kinds: `Projectile`'s keys, and
/// for a bullet the `Target`, `Dir`, `Steps` and target deltas vanilla writes.
pub fn spit_or_bullet_to_nbt(
    p: &Projectile,
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
    target: Option<[i32; 4]>,
    raw: Option<&Tag>,
) -> Option<Tag> {
    if !matches!(
        p.kind,
        ProjectileKind::LlamaSpit | ProjectileKind::ShulkerBullet { .. }
    ) {
        return None;
    }
    let base = Base {
        id: p.kind.id(),
        uuid,
        position: p.position,
        motion: p.velocity,
        rotation: [p.yaw, p.pitch],
        on_ground: p.on_ground,
        fall_distance: p.fall_distance,
    };
    let mut tag = base.tag(raw);
    put_projectile(&mut tag, p, owner);
    put_projectile_entity_fields(&mut tag, p);
    // `ShulkerBullet.addAdditionalSaveData`: the target it homes on (written
    // only while it has one), the step it is on (omitted while it has not
    // chosen one), the steps left and the per-axis target deltas.
    if let ProjectileKind::ShulkerBullet {
        dir, steps, delta, ..
    } = &p.kind
    {
        if let Tag::Compound(map) = &mut tag {
            map.remove(&"Target".into());
            map.remove(&"Dir".into());
        }
        if let Some(target) = target {
            put(&mut tag, "Target", uuid_tag(target));
        }
        if let Some(dir) = dir {
            put(&mut tag, "Dir", Tag::Byte(*dir as i8));
        }
        put(&mut tag, "Steps", Tag::Int(*steps));
        for (key, v) in [("TXD", delta[0]), ("TYD", delta[1]), ("TZD", delta[2])] {
            put(&mut tag, key, Tag::Double(v.to_bits()));
        }
    }
    Some(tag)
}

/// `ShulkerBullet.readAdditionalSaveData`'s `Target`: the UUID of the entity
/// the bullet homes on, which vanilla keeps even while that entity is away.
pub fn bullet_target(tag: &Tag) -> Option<[i32; 4]> {
    uuid_from_tag(tag.get("Target"))
}

/// `Projectile`'s `Owner`.
pub fn owner_uuid(tag: &Tag) -> Option<[i32; 4]> {
    uuid_from_tag(tag.get("Owner"))
}

/// `Fireball.readAdditionalSaveData` (and, around it, `Projectile`'s Owner and
/// LeftOwner and `AbstractHurtingProjectile`'s acceleration power): a fireball
/// also reads the item it is drawn as through `setItem`, whose default is the
/// fire charge; a wind charge is no `Fireball` and has none.
pub fn fireball_from_nbt(tag: &Tag, player: Option<[i32; 4]>) -> Option<Projectile> {
    let kind = match kind(tag)?.as_str() {
        "minecraft:fireball" => ProjectileKind::LargeFireball {
            power: i32::from(number(tag.get("ExplosionPower")).unwrap_or(1) as i8),
        },
        "minecraft:small_fireball" => ProjectileKind::SmallFireball,
        "minecraft:dragon_fireball" => ProjectileKind::DragonFireball,
        "minecraft:wind_charge" => ProjectileKind::WindCharge,
        "minecraft:breeze_wind_charge" => ProjectileKind::BreezeWindCharge,
        // WitherSkull.readAdditionalSaveData: an absent `dangerous` is a black skull.
        "minecraft:wither_skull" => ProjectileKind::WitherSkull {
            dangerous: flag(tag, "dangerous"),
        },
        _ => return None,
    };
    let owner = uuid_from_tag(tag.get("Owner"));
    let from_player = player.is_some() && owner == player;
    let mut p = Projectile::new(kind, position(tag)?, from_player.then_some(Who::Player));
    p.velocity = motion(tag);
    [p.yaw, p.pitch] = rotation(tag);
    p.left_owner = flag(tag, "LeftOwner");
    p.has_been_shot = flag(tag, "HasBeenShot");
    p.owner_uuid = owner;
    p.no_gravity = flag(tag, "NoGravity");
    if projectile_fields::keeps_entity_fields(&p) {
        load_projectile_entity_fields(&mut p, tag);
    }
    p.on_ground = flag(tag, "OnGround");
    p.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    p.acceleration = float(tag.get("acceleration_power")).unwrap_or(0.1);
    if let Some(item) = tag.get("Item").and_then(item_from_nbt) {
        p.set_item(item);
    }
    Some(p)
}

mod rocket;
pub use rocket::*;
mod trident;
pub use trident::{TRIDENT_HIT_GROUND, TridentNbt, TridentSave, trident_from_nbt, trident_to_nbt};
mod hanging;
pub use hanging::*;

/// `FallingBlockEntity`: BlockState, Time, DropItem, HurtEntities.
#[allow(clippy::too_many_arguments)]
pub fn falling_block_to_nbt(
    state: Tag,
    position: Vec3,
    motion: Vec3,
    time: i32,
    drop_item: bool,
    damage: crate::falling_block::Damage,
    fall_distance: f64,
    on_ground: bool,
    rotation: [f32; 2],
    uuid: [i32; 4],
) -> Tag {
    let base = Base {
        id: "minecraft:falling_block",
        uuid,
        position,
        motion,
        rotation,
        on_ground,
        fall_distance,
    };
    let mut tag = base.tag(None);
    for (key, value) in [
        ("BlockState", state),
        ("Time", Tag::Int(time)),
        ("DropItem", Tag::Byte(i8::from(drop_item))),
    ] {
        put(&mut tag, key, value);
    }
    damage.save(&mut tag);
    tag
}
/// (block state tag, position, motion, time, fall distance)
pub fn falling_block_from_nbt(tag: &Tag) -> Option<(Tag, Vec3, Vec3, i32, f64)> {
    Some((
        tag.get("BlockState")
            .cloned()
            .unwrap_or_else(|| string("minecraft:sand")),
        position(tag)?,
        motion(tag),
        number(tag.get("Time")).unwrap_or(0) as i32,
        float(tag.get("fall_distance")).unwrap_or(0.0),
    ))
}

/// FallingBlockEntity.readAdditionalSaveData uses the modern state codec.
pub fn falling_state_from_nbt(registry: &BlockRegistry, tag: &Tag) -> Option<BlockStateId> {
    BlockState::from_nbt(tag)
        .ok()
        .and_then(|mut state| {
            state.name = horizon_world::identifier::Identifier::parse(&state.name)
                .ok()?
                .to_string();
            registry
                .id(&state)
                .or_else(|| registry.default_state(&state.name))
        })
        .or_else(|| registry.default_state("minecraft:sand"))
}

/// `NoGravity` and `DropItem` (true when absent) of a saved falling block.
pub fn falling_block_flags(tag: &Tag) -> (bool, bool) {
    (flag(tag, "NoGravity"), flag_or(tag, "DropItem", true))
}

/// `Cushion.addAdditionalSaveData`: the block it sits on and its colour.
pub fn cushion_to_nbt(c: &Cushion, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    // A block-attached cushion does not update these: they stay as loaded.
    let (on_ground, fall_distance) = raw.map_or((false, 0.0), ground_from_nbt);
    let base = Base {
        id: "minecraft:cushion",
        uuid,
        position: c.position,
        motion: Vec3::ZERO,
        rotation: [c.yaw, 0.0],
        on_ground,
        fall_distance,
    };
    let mut tag = base.tag(raw);
    put(&mut tag, "block_pos", Tag::IntArray(c.pos.to_vec()));
    put(&mut tag, "color", string(c.color_name()));
    if let Some(name) = &c.custom_name {
        put(&mut tag, "CustomName", name.clone());
    }
    tag
}
/// The position, block position, yaw and colour of a saved cushion.
pub fn cushion_from_nbt(tag: &Tag, id: u32) -> Option<Cushion> {
    let position = position(tag)?;
    let pos = match tag.get("block_pos") {
        Some(Tag::IntArray(v)) if v.len() == 3 => [v[0], v[1], v[2]],
        _ => [
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ],
    };
    let yaw = match tag.get("Rotation") {
        Some(Tag::List(l)) => l
            .first()
            .and_then(|v| crate::save::float(Some(v)))
            .unwrap_or(0.0) as f32,
        _ => 0.0,
    };
    let color =
        crate::entities::dye_index(&text(tag.get("color")).unwrap_or_else(|| "white".into()));
    Some(Cushion {
        id,
        position,
        pos,
        yaw,
        color,
        check_interval: 0,
        custom_name: tag.get("CustomName").cloned(),
    })
}

mod armor_stands;
#[cfg(test)]
#[path = "cushion_tests.rs"]
mod cushion_tests;
pub use armor_stands::*;
/// `AbstractBoat.addAdditionalSaveData` / `ContainerEntity.addChestVehicleSaveData`:
/// only chest boats own container fields; a pending table replaces `Items`.
/// Ordinary slots use `ContainerHelper.saveAllItems` through the existing item encoder.
/// `ground` is the live `OnGround` and `fall_distance` (`Entity.saveWithoutId`).
#[allow(clippy::too_many_arguments)]
pub fn boat_to_nbt(
    id: &str,
    position: Vec3,
    motion: Vec3,
    rotation: [f32; 2],
    ground: (bool, f64),
    uuid: [i32; 4],
    raw: Option<&Tag>,
    items: Option<&[Option<crate::items::ItemStack>]>,
) -> Tag {
    let mut tag = Base {
        id,
        uuid,
        position,
        motion,
        rotation,
        on_ground: ground.0,
        fall_distance: ground.1,
    }
    .non_mob_tag(raw);
    if let Tag::Compound(map) = &mut tag {
        map.remove(&"Items".into());
        if let Some(items) = items {
            let table = crate::save::text(map.get(&"LootTable".into()))
                .and_then(|id| horizon_client_core::identifier::normalize_id(&id));
            if let Some(table) = table {
                map.insert("LootTable".into(), Tag::String(table.as_str().into()));
            } else {
                map.remove(&"LootTable".into());
                map.remove(&"LootTableSeed".into());
                map.insert("Items".into(), crate::save::encode_items(items));
            }
        } else {
            map.remove(&"LootTable".into());
            map.remove(&"LootTableSeed".into());
        }
    }
    tag
}
/// A vehicle's base entity tag (minecarts, the dragon and end crystals), with
/// the live `OnGround` and `fall_distance` in `ground`.
pub fn vehicle_to_nbt(
    id: &str,
    position: Vec3,
    motion: Vec3,
    rotation: [f32; 2],
    ground: (bool, f64),
    uuid: [i32; 4],
    raw: Option<&Tag>,
) -> Tag {
    Base {
        id,
        uuid,
        position,
        motion,
        rotation,
        on_ground: ground.0,
        fall_distance: ground.1,
    }
    .tag(raw)
}
/// `Entity.load`'s `OnGround` (`getBooleanOr`: `byteValue() != 0`) and
/// `fall_distance` (`getDoubleOr`: any number's `doubleValue`), defaulting to
/// false and 0.
pub fn ground_from_nbt(tag: &Tag) -> (bool, f64) {
    (
        flag(tag, "OnGround"),
        float(tag.get("fall_distance")).unwrap_or(0.0),
    )
}

/// `VillagerData`: type, profession and level.
fn villager_data(m: &Mob) -> Tag {
    use crate::mobs::render::{PROFESSIONS, VILLAGER_TYPES};
    Tag::compound([
        (
            "type",
            string(&format!(
                "minecraft:{}",
                VILLAGER_TYPES[m.extra.variant as usize % VILLAGER_TYPES.len()]
            )),
        ),
        (
            "profession",
            string(&format!(
                "minecraft:{}",
                PROFESSIONS[m.extra.variant2 as usize % PROFESSIONS.len()]
            )),
        ),
        ("level", Tag::Int(i32::from(m.extra.variant3.max(1)))),
    ])
}
/// `AbstractVillager` "Offers" (`MerchantOffers.CODEC`).
fn put_offers(map: &mut Compound, m: &Mob) {
    let Some(t) = m.extra.trade.as_ref().filter(|t| !t.unset) else {
        return;
    };
    map.insert("Offers".into(), offers_to_nbt(&t.offers));
}
/// `ItemCost.CODEC`: id, count (always written) and the exact components.
fn item_cost_to_nbt(stack: &crate::items::ItemStack) -> Tag {
    let mut tag = item_to_nbt(stack);
    if let Tag::Compound(c) = &mut tag
        && !c.contains_key(&"count".into())
    {
        c.insert("count".into(), Tag::Int(stack.count as i32));
    }
    tag
}
/// `MerchantOffers.CODEC`: `{Recipes: [...]}`, each offer without the
/// fields at their defaults.
pub fn offers_to_nbt(offers: &[crate::merchant::MerchantOffer]) -> Tag {
    let recipes = offers
        .iter()
        .map(|o| {
            let mut entries = vec![
                ("buy", item_cost_to_nbt(&o.base_cost_a.stack)),
                ("sell", item_to_nbt(&o.result)),
            ];
            // `MerchantOffer.CODEC` omits fields at their defaults.
            if o.uses != 0 {
                entries.push(("uses", Tag::Int(o.uses)));
            }
            if o.max_uses != 4 {
                entries.push(("maxUses", Tag::Int(o.max_uses)));
            }
            if o.price_multiplier != 0.0 {
                entries.push(("priceMultiplier", Tag::Float(o.price_multiplier.to_bits())));
            }
            if o.xp != 1 {
                entries.push(("xp", Tag::Int(o.xp)));
            }
            if !o.reward_exp {
                entries.push(("rewardExp", Tag::Byte(0)));
            }
            if o.special_price_diff != 0 {
                entries.push(("specialPrice", Tag::Int(o.special_price_diff)));
            }
            if o.demand != 0 {
                entries.push(("demand", Tag::Int(o.demand)));
            }
            if let Some(b) = &o.cost_b {
                entries.push(("buyB", item_cost_to_nbt(&b.stack)));
            }
            Tag::compound(entries)
        })
        .collect::<Vec<_>>();
    // An empty list is the field's default: left out.
    if recipes.is_empty() {
        return Tag::Compound(Compound::new());
    }
    Tag::compound([("Recipes", Tag::List(recipes))])
}
/// The offers of a `MerchantOffers.CODEC` tag.
pub fn offers_from_nbt(tag: &Tag) -> Vec<crate::merchant::MerchantOffer> {
    use crate::merchant::{ItemCost, MerchantOffer};
    let Some(Tag::List(recipes)) = tag.get("Recipes") else {
        return Vec::new();
    };
    recipes
        .iter()
        .filter_map(|r| {
            let cost = |key| {
                r.get(key)
                    .and_then(item_from_nbt)
                    .map(|stack| ItemCost { stack })
            };
            Some(MerchantOffer {
                base_cost_a: cost("buy")?,
                cost_b: cost("buyB"),
                result: r.get("sell").and_then(item_from_nbt)?,
                uses: number(r.get("uses")).unwrap_or(0) as i32,
                max_uses: number(r.get("maxUses")).unwrap_or(4) as i32,
                reward_exp: number(r.get("rewardExp")).unwrap_or(1) != 0,
                special_price_diff: number(r.get("specialPrice")).unwrap_or(0) as i32,
                demand: number(r.get("demand")).unwrap_or(0) as i32,
                price_multiplier: float(r.get("priceMultiplier")).unwrap_or(0.0) as f32,
                xp: number(r.get("xp")).unwrap_or(1) as i32,
            })
        })
        .collect()
}
/// The villager brain's `job_site`, `home` and `meeting_point` memories.
fn put_memories(map: &mut Compound, m: &Mob) {
    let mut memories: Vec<(&str, Tag)> = [
        ("minecraft:job_site", m.extra.home),
        ("minecraft:home", m.extra.bed),
        ("minecraft:meeting_point", m.extra.meeting),
    ]
    .into_iter()
    .filter_map(|(key, p)| {
        let pos = Tag::compound([
            ("dimension", string("minecraft:overworld")),
            ("pos", Tag::IntArray(p?.to_vec())),
        ]);
        Some((key, Tag::compound([("value", pos)])))
    })
    .collect();
    crate::mobs::villager::write_memories(m, &mut memories);
    map.insert(
        "Brain".into(),
        Tag::compound([("memories", Tag::compound(memories))]),
    );
}
fn read_villager(m: &mut Mob, tag: &Tag) {
    use crate::{
        merchant::Trader,
        mobs::render::{PROFESSIONS, VILLAGER_TYPES},
    };
    if let Some(data) = tag.get("VillagerData") {
        let name = |key| text(data.get(key)).map(|s| s.trim_start_matches("minecraft:").to_owned());
        if let Some(t) = name("type").and_then(|t| VILLAGER_TYPES.iter().position(|v| *v == t)) {
            m.extra.variant = t as u8;
        }
        m.extra.variant2 = name("profession")
            .and_then(|p| PROFESSIONS.iter().position(|v| *v == p))
            .unwrap_or(0) as u8;
        m.extra.variant3 = number(data.get("level")).unwrap_or(1).clamp(1, 5) as u8;
    }
    let memory = |key: &str| {
        tag.get("Brain")
            .and_then(|b| b.get("memories"))
            .and_then(|b| b.get(key))
            .and_then(|j| j.get("value"))
            .and_then(|v| match v.get("pos") {
                Some(Tag::IntArray(a)) if a.len() == 3 => Some([a[0], a[1], a[2]]),
                _ => None,
            })
    };
    m.extra.home = memory("minecraft:job_site");
    m.extra.bed = memory("minecraft:home");
    m.extra.meeting = memory("minecraft:meeting_point");
    if m.species() == crate::mobs::Species::Villager {
        crate::mobs::villager::read_nbt(m, tag);
    }
    if m.species() == crate::mobs::Species::WanderingTrader {
        // WanderingTrader.readAdditionalSaveData: both default to idle.
        m.extra.ints[5] = number(tag.get("DespawnDelay")).unwrap_or(0) as i32;
        m.extra.point = match tag.get("wander_target") {
            Some(Tag::IntArray(a)) if a.len() == 3 => {
                Some([f64::from(a[0]), f64::from(a[1]), f64::from(a[2])])
            }
            _ => None,
        };
    }
    let offers_tag = tag.get("Offers");
    let xp = number(tag.get("Xp")).unwrap_or(0) as i32;
    if offers_tag.is_none() && xp == 0 && tag.get("LastRestock").is_none() {
        return;
    }
    let offers = offers_tag.map(offers_from_nbt).unwrap_or_default();
    m.extra.trade = Some(Box::new(Trader {
        unset: offers_tag.is_none(),
        xp: number(tag.get("Xp")).unwrap_or(0) as i32,
        offers,
        last_restock: number(tag.get("LastRestock")).unwrap_or(0),
        restocks_today: number(tag.get("RestocksToday")).unwrap_or(0) as i32,
        check_day: 0,
    }));
}

#[cfg(test)]
mod tnt_power_tests {
    use super::*;

    /// `PrimedTnt.readAdditionalSaveData`: `explosion_power` defaults to 4 and
    /// is clamped to 0..128 both ways, so a summoned `explosion_power:200` is
    /// 128 and one that is absent is ordinary TNT.
    #[test]
    fn explosion_power_is_read_clamped_and_defaulted() {
        let with = |power: Option<f32>| {
            let mut map = std::collections::BTreeMap::new();
            if let Some(p) = power {
                map.insert("explosion_power".into(), Tag::Float(p.to_bits()));
            }
            Tag::Compound(map)
        };
        assert_eq!(tnt_power(&with(None)), 4.0);
        assert_eq!(tnt_power(&with(Some(10.0))), 10.0);
        assert_eq!(tnt_power(&with(Some(200.0))), 128.0);
        assert_eq!(tnt_power(&with(Some(-3.0))), 0.0);
        // Any numeric tag type counts, as with `getFloatOr`.
        let numeric = |tag: Tag| {
            let mut map = std::collections::BTreeMap::new();
            map.insert("explosion_power".into(), tag);
            Tag::Compound(map)
        };
        assert_eq!(tnt_power(&numeric(Tag::Int(50))), 50.0);
        assert_eq!(tnt_power(&numeric(Tag::Byte(7))), 7.0);
        assert_eq!(tnt_power(&numeric(Tag::Short(9))), 9.0);
        assert_eq!(tnt_power(&numeric(Tag::Long(500))), 128.0);
        assert_eq!(tnt_power(&numeric(Tag::Double(12.5f64.to_bits()))), 12.5);
        // And it round-trips through the save, which writes it only when it is
        // not the default.
        let saved = tnt_to_nbt(
            Vec3::ZERO,
            Vec3::ZERO,
            false,
            0.0,
            20,
            10.0,
            string("minecraft:tnt"),
            [0.0, 0.0],
            [1, 2, 3, 4],
            None,
            None,
        );
        assert_eq!(tnt_power(&saved), 10.0);
        let plain = tnt_to_nbt(
            Vec3::ZERO,
            Vec3::ZERO,
            false,
            0.0,
            20,
            4.0,
            string("minecraft:tnt"),
            [0.0, 0.0],
            [1, 2, 3, 4],
            None,
            None,
        );
        assert!(plain.get("explosion_power").is_none());
    }

    #[test]
    fn saved_power_uses_the_model_and_omits_stale_defaults() {
        let block = string("minecraft:stone");
        let save = |power, raw| {
            tnt_to_nbt(
                Vec3::new(1.5, 200.0, 4.5),
                Vec3::ZERO,
                false,
                0.0,
                80,
                power,
                block.clone(),
                [0.0; 2],
                [91, 0, 0, 1],
                Some([91, 1, 0, 90]),
                raw,
            )
        };
        let raw_absent = Tag::compound([("Silent", Tag::Byte(1))]);
        let raw_default = Tag::compound([
            ("Silent", Tag::Byte(1)),
            ("explosion_power", Tag::Float(4.0_f32.to_bits())),
        ]);
        let raw_stale = Tag::compound([
            ("Silent", Tag::Byte(1)),
            ("explosion_power", Tag::Float(2.0_f32.to_bits())),
        ]);
        for raw in [
            None,
            Some(&raw_absent),
            Some(&raw_default),
            Some(&raw_stale),
        ] {
            let saved = save(4.0, raw);
            assert!(saved.get("explosion_power").is_none(), "stale raw {raw:?}");
            assert_eq!(saved.get("fuse"), Some(&Tag::Short(80)));
            assert_eq!(saved.get("block_state"), Some(&block));
            assert_eq!(saved.get("owner"), Some(&Tag::IntArray(vec![91, 1, 0, 90])));
            assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(vec![91, 0, 0, 1])));
            if raw.is_some() {
                assert_eq!(saved.get("Silent"), Some(&Tag::Byte(1)));
            }
        }
        for power in [0.0, 2.0, 5.0, 128.0] {
            for raw in [None, Some(&raw_default), Some(&raw_stale)] {
                assert_eq!(
                    save(power, raw).get("explosion_power"),
                    Some(&Tag::Float(power.to_bits()))
                );
            }
        }
    }

    /// `EntityReference.store(owner, output, "owner")`: the igniter's UUID as
    /// an int array, read back by `EntityReference.read`; an owner the save
    /// could not name keeps the loaded one from the raw compound.
    /// A dragon fireball saved with no raw compound: its own id, UUID,
    /// motion, power and owner, no fireball item or explosion power, and no
    /// fire (`DragonFireball.shouldBurn` is false); it loads back as itself.
    #[test]
    fn a_dragon_fireball_saves_and_loads_as_itself_without_raw_data() {
        let mut p = Projectile::new(
            ProjectileKind::DragonFireball,
            Vec3::new(1.5, 70.0, -2.5),
            None,
        );
        p.velocity = Vec3::new(0.1, 0.0, 0.0);
        p.acceleration = 0.1;
        p.age = 3;
        let tag = fireball_to_nbt(&p, [1, 2, 3, 4], Some([5, 6, 7, 8]), None).unwrap();
        assert_eq!(kind(&tag).as_deref(), Some("minecraft:dragon_fireball"));
        assert_eq!(uuid(&tag), Some([1, 2, 3, 4]));
        assert_eq!(uuid_from_tag(tag.get("Owner")), Some([5, 6, 7, 8]));
        assert_eq!(float(tag.get("acceleration_power")), Some(0.1));
        for absent in ["ExplosionPower", "Item"] {
            assert!(tag.get(absent).is_none(), "{absent}");
        }
        // Entity.saveWithoutId always writes Fire; a fireball that does not
        // burn is never ignited (an item fireball saves at least 20).
        assert!(
            number(tag.get("Fire")).unwrap_or(0) <= 0,
            "{:?}",
            tag.get("Fire")
        );
        let back = fireball_from_nbt(&tag, None).unwrap();
        assert_eq!(back.kind, ProjectileKind::DragonFireball);
        assert_eq!(back.velocity, p.velocity);
        assert_eq!(back.owner_uuid, Some([5, 6, 7, 8]));
        assert!(saved_with_chunk(&back));
    }
    #[test]
    fn the_owner_round_trips_as_a_uuid() {
        let save = |owner, raw: Option<&Tag>| {
            let t = string("minecraft:tnt");
            tnt_to_nbt(
                Vec3::ZERO,
                Vec3::ZERO,
                false,
                0.0,
                80,
                4.0,
                t,
                [0.0; 2],
                [1; 4],
                owner,
                raw,
            )
        };
        let saved = save(Some([5, 6, 7, 8]), None);
        assert_eq!(saved.get("owner"), Some(&Tag::IntArray(vec![5, 6, 7, 8])));
        assert_eq!(tnt_owner(&saved), Some([5, 6, 7, 8]));
        assert_eq!(tnt_owner(&save(None, None)), None);
        assert_eq!(tnt_owner(&save(None, Some(&saved))), Some([5, 6, 7, 8]));
        assert_eq!(tnt_owner(&save(Some([9; 4]), Some(&saved))), Some([9; 4]));
    }
}

#[cfg(test)]
mod spell_ticks_nbt_tests;

#[cfg(test)]
mod tests {
    /// `ItemEntity.readAdditionalSaveData` reads `Age` and `PickupDelay` with
    /// `getShortOr`: an int tag is narrowed (`IntTag.shortValue`,
    /// `(short)(value & 65535)`), so 40000 is -25536 and 65536 is 0.
    #[test]
    fn an_item_narrows_its_age_and_pickup_delay_to_a_short() {
        let load = |fields: &str| {
            let snbt =
                format!("{{Pos:[0d,0d,0d],Item:{{id:\"minecraft:stone\",count:1}},{fields}}}");
            let tag = horizon_world::nbt::parse_snbt(&snbt).unwrap().0;
            let item = super::item_entity_from_nbt(&tag, 1, 0.0).unwrap();
            (item.age, item.pickup_delay)
        };
        assert_eq!(load("Age:40000,PickupDelay:65546"), (-25536, 10));
        assert_eq!(load("Age:65536,PickupDelay:40000"), (0, -25536));
        // What vanilla writes itself reads back as it was.
        assert_eq!(load("Age:-32768s,PickupDelay:32767s"), (-32768, 32767));
        assert_eq!(load("Age:5999s,PickupDelay:10s"), (5999, 10));
        // A float or double floors first (`Mth.floor`), as `Health` does.
        assert_eq!(load("Age:-0.5f,PickupDelay:9.9d"), (-1, 9));
        // The sentinels `ItemEntity.tick` tests for (`age != -32768`: never
        // ages; `pickupDelay != 32767`: never counts down), given as ints,
        // and the ints that narrow onto them.
        assert_eq!(load("Age:-32768,PickupDelay:32767"), (-32768, 32767));
        assert_eq!(load("Age:32768,PickupDelay:98303"), (-32768, 32767));
        // `Entity.load` reads the item's `Fire` with `getShortOr` as well.
        let fire = |fields: &str| {
            let snbt =
                format!("{{Pos:[0d,0d,0d],Item:{{id:\"minecraft:stone\",count:1}},{fields}}}");
            let tag = horizon_world::nbt::parse_snbt(&snbt).unwrap().0;
            super::item_entity_from_nbt(&tag, 1, 0.0)
                .unwrap()
                .fire_ticks
        };
        assert_eq!(
            [fire("Fire:40000"), fire("Fire:60s"), fire("Age:1s")],
            [-25536, 60, 0]
        );
        // They save as the shorts vanilla writes.
        let tag = horizon_world::nbt::parse_snbt(
            "{Pos:[0d,0d,0d],Item:{id:\"minecraft:stone\",count:1},Age:32768,PickupDelay:98303}",
        )
        .unwrap()
        .0;
        let item = super::item_entity_from_nbt(&tag, 1, 0.0).unwrap();
        let saved = super::item_entity_to_nbt(&item, [1; 4], None);
        assert_eq!(saved.get("Age"), Some(&super::Tag::Short(i16::MIN)));
        assert_eq!(saved.get("PickupDelay"), Some(&super::Tag::Short(i16::MAX)));
    }

    // Added after the actual old Game proof: live mutation and a fresh absent
    // load cannot inherit a predicate from a contradictory saved compound.
    #[test]
    fn arrow_can_break_mutation_clone_and_absent_load_save_actual_state() {
        let parse = |text: &str| horizon_world::nbt::parse_snbt(text).unwrap().0;
        let raw = parse(
            "{id:\"minecraft:arrow\",Pos:[1d,200d,1d],NoGravity:1b,marker:37,can_break:{blocks:\"minecraft:chorus_flower\"}}",
        );
        let (mut arrow, _) = super::arrow_from_nbt(&raw, None, |_| None).unwrap();
        let original = raw.get("can_break").unwrap().clone();
        assert_eq!(
            arrow.arrow.as_ref().unwrap().can_break.as_ref(),
            Some(&original)
        );
        let replacement = parse("{blocks:\"minecraft:stone\"}");
        arrow.arrow.as_mut().unwrap().can_break = Some(replacement.clone());
        let cloned = arrow.clone();
        for source in [None, Some(&raw)] {
            let saved = super::arrow_to_nbt(&cloned, [197, 0, 0, 1], None, None, source).unwrap();
            assert_eq!(saved.get("can_break"), Some(&replacement));
            assert_eq!(saved.get("NoGravity"), Some(&super::Tag::Byte(1)));
        }
        arrow.arrow.as_mut().unwrap().can_break = None;
        let saved = super::arrow_to_nbt(&arrow, [197, 0, 0, 1], None, None, Some(&raw)).unwrap();
        assert!(saved.get("can_break").is_none());
        assert_eq!(saved.get("marker"), Some(&super::Tag::Int(37)));
        let absent = parse("{id:\"minecraft:arrow\",Pos:[1d,200d,1d]}");
        let (fresh, _) = super::arrow_from_nbt(&absent, None, |_| None).unwrap();
        assert!(fresh.arrow.as_ref().unwrap().can_break.is_none());
        assert!(
            super::arrow_to_nbt(&fresh, [197, 0, 0, 2], None, None, Some(&raw))
                .unwrap()
                .get("can_break")
                .is_none()
        );
    }

    /// `Shulker` reads `AttachFace` as a byte before
    /// `Direction.from3DDataValue` (`abs(data % 6)`): an int is narrowed
    /// first, so 258 is byte 2, not 258 % 6 = 0, and 250 is byte -6, face 0.
    #[test]
    fn a_shulkers_attach_face_is_narrowed_to_a_byte_first() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:shulker".into();
        ty.species = crate::mobs::Species::Shulker;
        let types = [("minecraft:shulker".into(), kind)].into();
        let face = |saved: Tag| {
            let raw = Tag::compound([
                ("id", string("minecraft:shulker")),
                ("Pos", doubles(&[4.5, 220.0, 4.5])),
                ("AttachFace", saved),
            ]);
            mob_from_nbt(&raw, &types, 1, None).unwrap().extra.variant3
        };
        assert_eq!(face(Tag::Byte(3)), 3);
        assert_eq!(face(Tag::Byte(-2)), 2);
        assert_eq!(face(Tag::Int(258)), 2);
        assert_eq!(face(Tag::Int(250)), 0);
    }
    use super::*;
    use crate::items::ItemStack;

    /// Older worlds name their single player by the offline UUID; the migration rewrites
    /// every reference to it (owner, thrower, trusted list, anger, leash) and nothing else.
    #[test]
    fn a_players_offline_uuid_becomes_the_accounts_everywhere_in_an_entity() {
        let old = [1, 2, 3, 4];
        let new = [5, 6, 7, 8];
        let mut tag = Tag::compound([
            ("id", string("minecraft:wolf")),
            ("UUID", Tag::IntArray(vec![9, 9, 9, 9])),
            ("Owner", Tag::IntArray(old.to_vec())),
            ("Thrower", Tag::IntArray(old.to_vec())),
            ("angry_at", Tag::IntArray(old.to_vec())),
            (
                "Trusted",
                Tag::List(vec![
                    Tag::IntArray(old.to_vec()),
                    Tag::IntArray(vec![3, 3, 3, 3]),
                ]),
            ),
            (
                "leash",
                Tag::compound([("UUID", Tag::IntArray(old.to_vec()))]),
            ),
            ("Health", Tag::Float(20.0f32.to_bits())),
        ]);
        migrate_player_uuid(&mut tag, old, new);
        assert_eq!(tag.get("Owner"), Some(&Tag::IntArray(new.to_vec())));
        assert_eq!(tag.get("Thrower"), Some(&Tag::IntArray(new.to_vec())));
        assert_eq!(tag.get("angry_at"), Some(&Tag::IntArray(new.to_vec())));
        assert_eq!(
            tag.get("Trusted"),
            Some(&Tag::List(vec![
                Tag::IntArray(new.to_vec()),
                Tag::IntArray(vec![3, 3, 3, 3]),
            ]))
        );
        assert_eq!(
            tag.get("leash").and_then(|leash| leash.get("UUID")),
            Some(&Tag::IntArray(new.to_vec()))
        );
        // The entity's own UUID is not the player's.
        assert_eq!(tag.get("UUID"), Some(&Tag::IntArray(vec![9, 9, 9, 9])));
    }

    #[test]
    fn empty_saved_tags_are_omitted_without_changing_raw_or_nonempty_tags() {
        let base = Base {
            id: "minecraft:pig",
            uuid: [328, 0, 0, 1],
            position: Vec3::new(4.5, 200.0, 4.5),
            motion: Vec3::ZERO,
            rotation: [0.0; 2],
            on_ground: false,
            fall_distance: 0.0,
        };
        let raw = |tags| {
            Tag::compound([
                ("Tags", Tag::List(tags)),
                ("PortalCooldown", Tag::Int(17)),
                ("Invulnerable", Tag::Byte(1)),
            ])
        };
        let fresh = base.tag(None);
        assert!(fresh.get("Tags").is_none());
        assert_eq!(fresh.get("PortalCooldown"), Some(&Tag::Int(0)));
        for tags in [vec![string("keep")], vec![]] {
            let raw = raw(tags.clone());
            let saved = base.tag(Some(&raw));
            assert_eq!(saved.get("PortalCooldown"), Some(&Tag::Int(17)));
            assert_eq!(saved.get("Invulnerable"), Some(&Tag::Byte(1)));
            assert_eq!(raw.get("Tags"), Some(&Tag::List(tags.clone())));
            assert_eq!(
                saved.get("Tags"),
                if tags.is_empty() {
                    None
                } else {
                    Some(Tag::List(tags))
                }
                .as_ref()
            );
        }
    }

    #[test]
    fn conversion_keeps_only_the_old_mobs_command_tags() {
        let tags = Tag::List(vec![
            Tag::String("subject".into()),
            Tag::String("quest".into()),
        ]);
        let raw = Tag::compound([
            ("id", Tag::String("minecraft:husk".into())),
            ("UUID", Tag::IntArray(vec![1, 2, 3, 4])),
            ("HuskConversionTime", Tag::Int(1)),
            ("Tags", tags.clone()),
        ]);
        let original = raw.clone();
        assert_eq!(
            conversion_tags(Some(&raw)),
            Some(Tag::compound([("Tags", tags)]))
        );
        assert_eq!(raw, original);
        assert_eq!(conversion_tags(None), None);
        assert_eq!(
            conversion_tags(Some(&Tag::compound([("Tags", Tag::List(Vec::new()))]))),
            None
        );
        assert_eq!(
            conversion_tags(Some(&Tag::Compound(Default::default()))),
            None
        );
    }

    #[test]
    fn an_ageable_mob_that_cannot_be_a_baby_saves_no_age() {
        use crate::mobs::Species;
        // AgeableMob.addAdditionalSaveData writes Age only when canBeABaby is
        // true, so every ageable class that overrides it to false writes none
        // of it: a frog and a camel husk keep no raw `Age` either.
        for (species, id) in [
            (Species::Slime, "minecraft:slime"),
            (Species::MagmaCube, "minecraft:magma_cube"),
            (Species::Frog, "minecraft:frog"),
            (Species::CamelHusk, "minecraft:camel_husk"),
            (Species::Pig, "minecraft:pig"),
        ] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let ty = Arc::make_mut(&mut kind);
            ty.species = species;
            ty.id = id.into();
            let mut mob = Mob::new(1, kind, Vec3::ZERO, 0.0);
            mob.extra.age = -37;
            let expected = (species == Species::Pig).then_some(Tag::Int(-37));
            for age in [None, Some(0), Some(-24000), Some(37)] {
                let mut raw = Tag::compound([("h228_control", Tag::Int(42))]);
                if let Some(age) = age
                    && let Tag::Compound(map) = &mut raw
                {
                    map.insert("Age".into(), Tag::Int(age));
                }
                let saved = mob_to_nbt(&mob, [0; 4], Some(&raw), None, |_| None);
                assert_eq!(saved.get("Age"), expected.as_ref(), "{species:?}, {age:?}");
                assert_eq!(saved.get("h228_control"), Some(&Tag::Int(42)));
            }
            let saved = mob_to_nbt(&mob, [0; 4], None, None, |_| None);
            assert_eq!(
                saved.get("Age"),
                expected.as_ref(),
                "{species:?}, no raw tag"
            );
        }
    }

    #[test]
    fn zombie_nautilus_save_omits_ignored_age_fields() {
        use crate::mobs::Species;
        for (id, species) in [
            ("minecraft:zombie_nautilus", Species::ZombieNautilus),
            ("minecraft:nautilus", Species::Nautilus),
        ] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let def = Arc::make_mut(&mut kind);
            def.id = id.into();
            def.species = species;
            let mut mob = Mob::new(1, kind, Vec3::ZERO, 0.0);
            mob.extra.age = -123;
            mob.extra.forced_age = 19;
            mob.extra.age_locked = true;
            let raw = Tag::compound([
                ("Age", Tag::Int(37)),
                ("ForcedAge", Tag::Int(99)),
                ("AgeLocked", Tag::Byte(0)),
                ("audit:unrelated", Tag::Int(7)),
            ]);
            for base in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [0; 4], base, None, |_| None);
                for (key, expected) in [
                    ("Age", Tag::Int(-123)),
                    ("ForcedAge", Tag::Int(19)),
                    ("AgeLocked", Tag::Byte(1)),
                ] {
                    if species == Species::ZombieNautilus {
                        assert!(saved.get(key).is_none(), "{id}: {key}");
                    } else {
                        assert_eq!(saved.get(key), Some(&expected), "{id}: {key}");
                    }
                }
                if base.is_some() {
                    assert_eq!(saved.get("audit:unrelated"), Some(&Tag::Int(7)));
                }
            }
        }
    }

    /// `Horse.getVariant` is `Variant.byId(typeVariant & 255)` and
    /// `getMarkings` `Markings.byId((typeVariant & 0xFF00) >> 8)`; both maps
    /// are `OutOfBoundsStrategy.WRAP` (7 coats, 5 markings), so 9 is coat 2
    /// and markings 6 is 1.
    #[test]
    fn a_horses_unknown_coat_and_markings_wrap() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:horse".into();
        ty.species = crate::mobs::Species::Horse;
        let types = [("minecraft:horse".into(), kind)].into();
        let look = |saved: i32| {
            let raw = Tag::compound([
                ("id", string("minecraft:horse")),
                ("Pos", doubles(&[4.5, 220.0, 4.5])),
                ("Variant", Tag::Int(saved)),
            ]);
            let m = mob_from_nbt(&raw, &types, 1, None).unwrap();
            (m.extra.variant, m.extra.variant2)
        };
        assert_eq!(look(6 | 4 << 8), (6, 4));
        assert_eq!(look(9), (2, 0));
        assert_eq!(look(6 << 8), (0, 1));
        assert_eq!(look(255 | 255 << 8), (3, 0));
        // `Horse.addAdditionalSaveData` writes the loaded int back as it was;
        // a horse whose look changed since saves the two ids.
        let saved = |loaded: i32, change: Option<u8>| {
            let raw = Tag::compound([
                ("id", string("minecraft:horse")),
                ("Pos", doubles(&[4.5, 220.0, 4.5])),
                ("Variant", Tag::Int(loaded)),
            ]);
            let mut m = mob_from_nbt(&raw, &types, 1, None).unwrap();
            if let Some(coat) = change {
                m.extra.variant = coat;
            }
            number(mob_to_nbt(&m, [0; 4], Some(&raw), None, |_| None).get("Variant"))
        };
        assert_eq!(saved(9, None), Some(9));
        assert_eq!(saved(6 << 8 | 0x7_0000, None), Some(6 << 8 | 0x7_0000));
        assert_eq!(saved(9, Some(5)), Some(5));
    }

    /// `LivingEntity.readAdditionalSaveData` reads `HurtTime` and `DeathTime`
    /// with `getShortOr`: an int tag is narrowed (`(short)(value & 65535)`),
    /// so 40000 is -25536 and never counts down.
    #[test]
    fn a_mob_narrows_its_hurt_and_death_time_to_a_short() {
        let kind = crate::mobs::sensor_scan_tests::kind();
        let types = [(kind.id.to_string(), kind.clone())].into();
        let load = |hurt: Tag, death: Tag| {
            let raw = Tag::compound([
                ("id", string(&kind.id)),
                ("Pos", doubles(&[4.5, 220.0, 4.5])),
                ("HurtTime", hurt),
                ("DeathTime", death),
            ]);
            let m = mob_from_nbt(&raw, &types, 1, None).unwrap();
            (m.hurt_time, m.death_time)
        };
        assert_eq!(load(Tag::Int(40000), Tag::Int(65546)), (-25536, 10));
        assert_eq!(load(Tag::Short(7), Tag::Short(19)), (7, 19));
    }

    /// `Axolotl.Variant.BY_ID` is `OutOfBoundsStrategy.ZERO`: an id outside
    /// 0..=4 is lucy (0). `Parrot.Variant.BY_ID` is `CLAMP`: it is the nearest
    /// end.
    #[test]
    fn an_unknown_axolotl_variant_is_lucy_and_a_parrots_is_clamped() {
        let variant = |species, id: &str, saved: i32| {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let ty = Arc::make_mut(&mut kind);
            ty.id = id.into();
            ty.species = species;
            let types = [(id.into(), kind)].into();
            let raw = Tag::compound([
                ("id", string(id)),
                ("Pos", doubles(&[4.5, 220.0, 4.5])),
                ("Variant", Tag::Int(saved)),
            ]);
            mob_from_nbt(&raw, &types, 1, None).unwrap().extra.variant
        };
        let axolotl = |saved| variant(crate::mobs::Species::Axolotl, "minecraft:axolotl", saved);
        let parrot = |saved| variant(crate::mobs::Species::Parrot, "minecraft:parrot", saved);
        assert_eq!([0, 3, 4, 5, 7, -1].map(axolotl), [0, 3, 4, 0, 0, 0]);
        assert_eq!([0, 3, 4, 5, 7, -1].map(parrot), [0, 3, 4, 4, 4, 0]);
    }

    /// A mount keeps its body armour and a helmet (say
    /// `/item replace entity <horse> armor.head with minecraft:iron_helmet`)
    /// in their own slots through a save: vanilla holds both (`equipment.body`
    /// and `equipment.head`), one in each.
    #[test]
    fn a_horses_save_keeps_a_helmet_and_its_body_armour_apart() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:horse".into();
        ty.species = crate::mobs::Species::Horse;
        let types = [("minecraft:horse".into(), kind)].into();
        let raw = Tag::compound([
            ("id", string("minecraft:horse")),
            ("Pos", doubles(&[4.5, 220.0, 4.5])),
            ("Tame", Tag::Byte(1)),
            (
                "equipment",
                Tag::compound([
                    (
                        "body",
                        Tag::compound([
                            ("id", string("minecraft:diamond_horse_armor")),
                            ("count", Tag::Int(1)),
                        ]),
                    ),
                    (
                        "head",
                        Tag::compound([
                            ("id", string("minecraft:iron_helmet")),
                            ("count", Tag::Int(1)),
                        ]),
                    ),
                ]),
            ),
        ]);
        let m = mob_from_nbt(&raw, &types, 1, None).unwrap();
        assert_eq!(
            m.extra.body.as_deref(),
            Some("minecraft:diamond_horse_armor")
        );
        assert_eq!(m.extra.head.as_deref(), Some("minecraft:iron_helmet"));
        let saved = mob_to_nbt(&m, [0; 4], Some(&raw), None, |_| None);
        let eq = saved.get("equipment").unwrap();
        let id = |key: &str| {
            eq.get(key)
                .and_then(|i| text(i.get("id")))
                .unwrap_or_default()
        };
        assert_eq!(id("body"), "minecraft:diamond_horse_armor");
        assert_eq!(id("head"), "minecraft:iron_helmet");
    }

    /// A wolf loads its armor (`equipment.body`) into the body slot with the
    /// durability it lost, and a helmet next to it into the head slot.
    #[test]
    fn a_wolfs_save_keeps_its_armour_and_a_helmet_apart() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:wolf".into();
        ty.species = crate::mobs::Species::Wolf;
        let types = [("minecraft:wolf".into(), kind)].into();
        let armor = Tag::compound([
            ("id", string("minecraft:wolf_armor")),
            ("count", Tag::Int(1)),
            (
                "components",
                Tag::compound([("minecraft:damage", Tag::Int(9))]),
            ),
        ]);
        let raw = Tag::compound([
            ("id", string("minecraft:wolf")),
            ("Pos", doubles(&[4.5, 220.0, 4.5])),
            ("Owner", Tag::IntArray([0; 4].to_vec())),
            (
                "equipment",
                Tag::compound([
                    ("body", armor),
                    (
                        "head",
                        Tag::compound([
                            ("id", string("minecraft:iron_helmet")),
                            ("count", Tag::Int(1)),
                        ]),
                    ),
                ]),
            ),
        ]);
        let m = mob_from_nbt(&raw, &types, 1, None).unwrap();
        assert_eq!(m.extra.body.as_deref(), Some("minecraft:wolf_armor"));
        assert_eq!(m.extra.body_damage, 9);
        assert_eq!(m.extra.head.as_deref(), Some("minecraft:iron_helmet"));
    }

    fn fused_creeper(fuse: Option<i16>) -> (Mob, Tag) {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:creeper".into();
        ty.species = crate::mobs::Species::Creeper;
        let types = [("minecraft:creeper".into(), kind)].into();
        let mut tag = Tag::compound([
            ("id", string("minecraft:creeper")),
            ("Pos", doubles(&[4.5, 220.0, 4.5])),
            ("ignited", Tag::Byte(1)),
            ("NoAI", Tag::Byte(1)),
            ("NoGravity", Tag::Byte(1)),
        ]);
        if let (Some(fuse), Tag::Compound(c)) = (fuse, &mut tag) {
            c.insert("Fuse".into(), Tag::Short(fuse));
        }
        (mob_from_nbt(&tag, &types, 1, None).unwrap(), tag)
    }

    #[test]
    fn creeper_fuse_nbt_controls_explosion_tick_and_saves_runtime_value() {
        for (fuse, boundary) in [
            (Some(1), 1),
            (None, 30),
            (Some(40), 40),
            (Some(0), 1),
            (Some(-1), 1),
        ] {
            let (mut mob, raw) = fused_creeper(fuse);
            let mut events = Vec::new();
            for tick in 1..=boundary {
                crate::mobs::hostile::creeper_tick(&mut mob, &mut events);
                assert_eq!(mob.removed, tick == boundary, "Fuse={fuse:?}, tick={tick}");
            }
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, crate::mobs::MobEvent::Explode(..)))
                    .count(),
                1
            );
            mob.max_swell = 7;
            let saved = mob_to_nbt(&mob, [0; 4], Some(&raw), None, |_| None);
            assert_eq!(saved.get("Fuse"), Some(&Tag::Short(7)));
        }
    }

    /// #11332: `Creeper.readAdditionalSaveData` reads `ExplosionRadius` as a
    /// byte (default 3), `explodeCreeper` blows with it, twice it when powered,
    /// and `addAdditionalSaveData` writes the value back as a byte.
    #[test]
    fn creeper_explosion_radius_nbt_sizes_the_blast_and_saves() {
        let blast = |mob: &mut Mob| -> f32 {
            let mut events = Vec::new();
            mob.max_swell = 1;
            crate::mobs::hostile::creeper_tick(mob, &mut events);
            events
                .iter()
                .find_map(|e| match e {
                    crate::mobs::MobEvent::Explode(_, radius, ..) => Some(*radius),
                    _ => None,
                })
                .expect("the creeper exploded")
        };
        for (radius, powered, expected) in [
            (None, false, 3.0),
            (None, true, 6.0),
            (Some(Tag::Byte(12)), false, 12.0),
            (Some(Tag::Byte(12)), true, 24.0),
            (Some(Tag::Byte(0)), false, 0.0),
            // getByteOr takes any numeric tag, narrowed to a byte.
            (Some(Tag::Long(300)), false, 44.0),
        ] {
            let (mut mob, raw) = fused_creeper(None);
            let mut tag = raw.clone();
            if let Tag::Compound(c) = &mut tag {
                if let Some(radius) = radius.clone() {
                    c.insert("ExplosionRadius".into(), radius);
                }
                c.insert("powered".into(), Tag::Byte(i8::from(powered)));
            }
            let types = [("minecraft:creeper".into(), mob.kind.clone())].into();
            mob = mob_from_nbt(&tag, &types, 1, None).unwrap();
            let saved = mob_to_nbt(&mob, [0; 4], Some(&raw), None, |_| None);
            assert_eq!(
                saved.get("ExplosionRadius"),
                Some(&Tag::Byte(expected as i8 / if powered { 2 } else { 1 })),
                "{radius:?} powered={powered}"
            );
            assert_eq!(blast(&mut mob), expected, "{radius:?} powered={powered}");
        }
    }

    #[test]
    fn creeper_fall_swell_uses_custom_fuse_without_a_zero_floor() {
        for (fuse, expected) in [(Some(1), -4), (None, 25), (Some(40), 35)] {
            let (mut mob, _) = fused_creeper(fuse);
            mob.cause_fall_damage_from(100.0, 0.0, &mut Vec::new());
            assert_eq!(mob.swell, expected, "Fuse={fuse:?}");
        }
    }

    #[test]
    fn loaded_villager_command_names_follow_profession_and_custom_name() {
        use crate::mobs::Species;
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:villager".into();
        def.species = Species::Villager;
        let types = [("minecraft:villager".into(), kind)].into();
        for (profession, key) in [
            ("none", "entity.minecraft.villager.none"),
            ("nitwit", "entity.minecraft.villager.nitwit"),
            ("cleric", "entity.minecraft.villager.cleric"),
            ("farmer", "entity.minecraft.villager.farmer"),
            ("librarian", "entity.minecraft.villager.librarian"),
        ] {
            let data = Tag::compound([
                ("profession", string(&format!("minecraft:{profession}"))),
                ("type", string("minecraft:plains")),
                ("level", Tag::Int(1)),
            ]);
            let mut tag = Tag::compound([
                ("id", string("minecraft:villager")),
                ("Pos", doubles(&[4.5, 220.0, 4.5])),
                ("NoAI", Tag::Byte(1)),
                ("VillagerData", data.clone()),
            ]);
            let mob = mob_from_nbt(&tag, &types, 7, None).unwrap();
            assert_eq!(villager_data(&mob), data);
            assert_eq!(mob.command_name().translation_key(), Some(key));
            put(&mut tag, "CustomName", string("Ada"));
            let named = mob_from_nbt(&tag, &types, 8, None).unwrap();
            assert_eq!(
                named.command_name().component(),
                crate::text::Text::literal("Ada").component()
            );
        }
    }

    #[test]
    fn other_mob_command_names_keep_their_type_and_custom_name() {
        use crate::mobs::Species;
        for (id, species, key) in [
            (
                "minecraft:zombie_villager",
                Species::ZombieVillager,
                "entity.minecraft.zombie_villager",
            ),
            (
                "minecraft:wandering_trader",
                Species::WanderingTrader,
                "entity.minecraft.wandering_trader",
            ),
            ("audit:helper", Species::Pig, "entity.audit.helper"),
        ] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let def = Arc::make_mut(&mut kind);
            def.id = id.into();
            def.species = species;
            let mut mob = Mob::new(1, kind, Vec3::ZERO, 0.0);
            mob.extra.variant2 = 4;
            assert_eq!(mob.command_name().translation_key(), Some(key));
            mob.custom_name = Some("Ada".into());
            assert_eq!(mob.command_name().plain(), "Ada");
        }
    }

    #[test]
    fn mob_loading_accepts_default_namespace_and_preserves_custom_namespaces() {
        let cow = crate::mobs::sensor_scan_tests::kind();
        let types = [
            ("minecraft:cow".into(), cow.clone()),
            ("test:cow".into(), cow),
        ]
        .into();
        let pos = || {
            Tag::List(
                [4.5_f64, 220.0, 6.5]
                    .map(|v| Tag::Double(v.to_bits()))
                    .to_vec(),
            )
        };
        for id in ["cow", ":cow", "minecraft:cow", "test:cow"] {
            let tag = Tag::compound([("id", string(id)), ("Pos", pos()), ("NoAI", Tag::Byte(1))]);
            let mob = mob_from_nbt(&tag, &types, 7, None).expect(id);
            assert_eq!(mob.position, Vec3::new(4.5, 220.0, 6.5));
            assert!(mob.no_ai);
        }
        let unknown = Tag::compound([("id", string("other:cow")), ("Pos", pos())]);
        assert!(mob_from_nbt(&unknown, &types, 8, None).is_none());
    }

    /// A saved rocket and trident come back with their own Life, LifeTime,
    /// flags and position (the keys vanilla's addAdditionalSaveData writes).
    #[test]
    fn rocket_and_trident_round_trip() {
        let item = ItemStack::new("minecraft:firework_rocket", 1);
        let tag = firework_rocket_to_nbt(
            &RocketSave {
                rotation: [90.0, 10.0],
                position: Vec3::new(1.5, 70.0, -3.25),
                motion: Vec3::new(0.0, 0.5, 0.0),
                life: 7,
                lifetime: 31,
                item: &item,
                angled: true,
                owner: None,
                left_owner: false,
                on_ground: true,
                has_been_shot: false,
                fall_distance: 2.5,
            },
            [1, 2, 3, 4],
            None,
        );
        let r = firework_rocket_from_nbt(&tag, None).unwrap();
        assert_eq!(
            (r.life, r.lifetime, r.angled, r.on_ground, r.has_been_shot),
            (7, 31, true, true, false)
        );
        assert_eq!(r.position, Vec3::new(1.5, 70.0, -3.25));
        assert_eq!(r.rotation, [90.0, 10.0]);
        // Entity.saveWithoutId/load keep the fall.
        assert_eq!(r.fall_distance, 2.5);
        let trident = ItemStack::new("minecraft:trident", 1);
        let tag = trident_to_nbt(
            &TridentSave {
                position: Vec3::new(2.0, 64.0, 2.0),
                motion: Vec3::new(0.0, -0.25, 0.0),
                yaw: 10.0,
                pitch: 20.0,
                in_ground: true,
                life: 9,
                shake: 3,
                dealt_damage: true,
                pickup: Pickup::CreativeOnly,
                left_owner: true,
                has_been_shot: true,
                owner: Some([5, 6, 7, 8]),
                no_gravity: true,
                on_ground: true,
                fire_ticks: 0,
                fall_distance: 0.0,
                sound_event: TRIDENT_HIT_GROUND,
                item: &trident,
            },
            [1, 2, 3, 4],
            None,
        );
        let t = trident_from_nbt(&tag).unwrap();
        // `Entity.saveWithoutId` writes NoGravity only when set.
        assert!(t.no_gravity);
        assert!(matches!(tag.get("NoGravity"), Some(Tag::Byte(1))));
        // Entity.load/saveWithoutId: OnGround as it was.
        assert!(t.on_ground);
        assert_eq!(tag.get("OnGround"), Some(&Tag::Byte(1)));
        assert_eq!((t.in_ground, t.life, t.shake), (true, 9, 3));
        assert_eq!((t.dealt_damage, t.pickup), (true, Pickup::CreativeOnly));
        assert!(t.left_owner && t.has_been_shot);
        assert_eq!(t.owner, Some([5, 6, 7, 8]));
        assert_eq!((t.yaw, t.pitch), (10.0, 20.0));
    }

    /// The item, the spawn delay and the fields Horizon doesn't model come
    /// back; a compound without an item is no spawner.
    #[test]
    fn ominous_item_spawner_round_trip() {
        let item = ItemStack {
            item: Arc::from("minecraft:ominous_trial_key"),
            count: 1,
            damage: 0,
            extra: None,
        };
        let raw = Tag::compound([("CustomName", string("kept"))]);
        let at = Vec3::new(10.5, 64.25, -3.5);
        let tag = ominous_item_spawner_to_nbt(&item, at, 97, [1, 2, 3, 4], Some(&raw));
        assert_eq!(
            kind(&tag).as_deref(),
            Some("minecraft:ominous_item_spawner")
        );
        assert_eq!(uuid(&tag), Some([1, 2, 3, 4]));
        assert!(matches!(
            tag.get("spawn_item_after_ticks"),
            Some(Tag::Long(97))
        ));
        assert!(tag.get("CustomName").is_some());
        let (back, pos, after) = ominous_item_spawner_from_nbt(&tag).unwrap();
        assert_eq!(
            (back.item.as_ref(), back.count),
            ("minecraft:ominous_trial_key", 1)
        );
        assert_eq!((pos, after), (at, 97));
        assert!(ominous_item_spawner_from_nbt(&raw).is_none());
        // No item is `ItemStack.EMPTY`, which writes no key back.
        let empty = ominous_item_spawner_to_nbt(
            &crate::items::ItemStack::new("minecraft:air", 1),
            at,
            0,
            [1, 2, 3, 4],
            Some(&tag),
        );
        assert!(empty.get("item").is_none());
        let (back, pos, after) = ominous_item_spawner_from_nbt(&empty).unwrap();
        assert_eq!((back.item.as_ref(), pos, after), ("minecraft:air", at, 0));
    }

    #[test]
    fn armor_stand_names_keep_their_formatting_across_a_save() {
        let tag = Tag::compound([
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(64.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                ]),
            ),
            ("CustomName", string("§6Hello")),
            ("CustomNameVisible", Tag::Byte(1)),
        ]);
        let a = armor_stand_from_nbt(&tag, 7).unwrap();
        let name = a.custom_name.as_ref().unwrap();
        assert_eq!(name.plain(), "Hello");
        assert!(name.spans[0].style.color.is_some());
        assert!(a.name_visible);
        let saved = armor_stand_to_nbt(&a, [0; 4], Some(&tag));
        let restored = armor_stand_from_nbt(&saved, 7).unwrap();
        assert_eq!(restored.custom_name, a.custom_name);
        assert!(restored.name_visible);
    }

    #[test]
    fn armor_stand_keeps_no_gravity_through_a_save() {
        let tag = Tag::compound([
            ("id", string("minecraft:armor_stand")),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(64.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                ]),
            ),
            ("NoGravity", Tag::Byte(1)),
        ]);
        let mut a = armor_stand_from_nbt(&tag, 7).unwrap();
        assert!(a.no_gravity, "summon NBT sets the flag");
        let saved = armor_stand_to_nbt(&a, [0; 4], Some(&tag));
        assert_eq!(saved.get("NoGravity"), Some(&Tag::Byte(1)));
        assert!(armor_stand_from_nbt(&saved, 7).unwrap().no_gravity);
        // `data merge entity @s {NoGravity:0b}` clears it again.
        a.no_gravity = false;
        let cleared = armor_stand_to_nbt(&a, [0; 4], Some(&saved));
        assert!(cleared.get("NoGravity").is_none());
        assert!(!armor_stand_from_nbt(&cleared, 7).unwrap().no_gravity);
    }

    /// `Entity.load`: a Motion component past 10 blocks a tick loads as 0,
    /// the others as saved.
    #[test]
    fn armor_stand_drops_a_motion_component_past_ten() {
        let doubles = |v: [f64; 3]| Tag::List(v.map(|d| Tag::Double(d.to_bits())).to_vec());
        let tag = Tag::compound([
            ("id", string("minecraft:armor_stand")),
            ("Pos", doubles([0.0, 64.0, 0.0])),
            ("Motion", doubles([1.0e10, -0.5, -10.5])),
        ]);
        let a = armor_stand_from_nbt(&tag, 7).unwrap();
        assert_eq!(a.velocity, Vec3::new(0.0, -0.5, 0.0));
        let tag = Tag::compound([
            ("id", string("minecraft:armor_stand")),
            ("Pos", doubles([0.0, 64.0, 0.0])),
            ("Motion", doubles([10.0, 0.0, -10.0])),
        ]);
        let a = armor_stand_from_nbt(&tag, 7).unwrap();
        assert_eq!(a.velocity, Vec3::new(10.0, 0.0, -10.0), "10 itself stays");
    }

    fn falling_decode_registry() -> BlockRegistry {
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
            (
                "states",
                Tag::List(
                    ["minecraft:air", "minecraft:sand", "minecraft:gravel"]
                        .into_iter()
                        .map(string)
                        .collect(),
                ),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:air", Tag::Int(0)),
                    ("minecraft:sand", Tag::Int(1)),
                    ("minecraft:gravel", Tag::Int(2)),
                ]),
            ),
        ]))
        .unwrap()
    }

    fn falling_decode_input() -> Tag {
        horizon_world::nbt::parse_snbt(
            "{Pos:[3.5d,200d,4.5d],Motion:[0d,0d,0d],Time:10,DropItem:0b,NoGravity:1b}",
        )
        .unwrap()
        .0
    }

    fn falling_decode_save(registry: &BlockRegistry, input: &Tag) -> Tag {
        let (state, pos, motion, time, distance) =
            falling_block_from_nbt(input).expect("absent BlockState is valid input");
        let actual = falling_state_from_nbt(registry, &state).unwrap();
        assert_eq!(falling_block_flags(input), (true, false));
        // Save only the decoded concrete model; no original source compound.
        let saved = falling_block_to_nbt(
            state_to_nbt(registry, actual),
            pos,
            motion,
            time,
            false,
            crate::falling_block::Damage::default(),
            distance,
            false,
            [0.0, 0.0],
            [96, 0, 0, 1],
        );
        assert_eq!(saved.get("Pos"), input.get("Pos"));
        assert_eq!(saved.get("Motion"), input.get("Motion"));
        assert_eq!(saved.get("Time"), Some(&Tag::Int(10)));
        assert_eq!(saved.get("DropItem"), Some(&Tag::Byte(0)));
        saved
    }

    #[test]
    fn falling_decode_absent_state_saves_actual_sand_without_raw() {
        let registry = falling_decode_registry();
        let saved = falling_decode_save(&registry, &falling_decode_input());
        assert_eq!(saved.get("BlockState"), Some(&string("minecraft:sand")));
    }

    #[test]
    fn falling_decode_modern_boundary_keeps_shared_legacy_reader() {
        let registry = falling_decode_registry();
        let legacy = Tag::compound([("Name", string("minecraft:gravel"))]);
        assert_eq!(
            state_from_nbt(&registry, &legacy),
            registry.default_state("minecraft:gravel")
        );
        for (state, expected) in [
            (string("minecraft:sand"), "minecraft:sand"),
            (
                Tag::compound([("id", string("minecraft:gravel"))]),
                "minecraft:gravel",
            ),
            (string("minecraft:air"), "minecraft:air"),
            (legacy, "minecraft:sand"),
            (Tag::Compound(Default::default()), "minecraft:sand"),
        ] {
            let mut input = falling_decode_input();
            put(&mut input, "BlockState", state);
            let saved = falling_decode_save(&registry, &input);
            assert_eq!(saved.get("BlockState"), Some(&string(expected)));
        }
    }

    #[test]
    fn falling_block_round_trip_keeps_impact_distance_and_settings() {
        let state = Tag::compound([("Name", string("minecraft:damaged_anvil"))]);
        let position = Vec3::new(3.5, 100.0, -7.5);
        let velocity = Vec3::new(0.0, -1.25, 0.0);
        let damage = crate::falling_block::Damage {
            cancel_drop: true,
            ..crate::falling_block::Damage::anvil()
        };
        let tag = falling_block_to_nbt(
            state.clone(),
            position,
            velocity,
            30,
            false,
            damage,
            19.75,
            true,
            [0.0, 0.0],
            [1, 2, 3, 4],
        );
        let (loaded_state, pos, motion, age, distance) = falling_block_from_nbt(&tag).unwrap();
        assert_eq!(loaded_state, state);
        assert_eq!(
            (pos, motion, age, distance),
            (position, velocity, 30, 19.75)
        );
        assert_eq!(falling_block_flags(&tag), (false, false));
        assert_eq!(crate::falling_block::Damage::from_nbt(&tag, true), damage);
        assert_eq!(uuid(&tag), Some([1, 2, 3, 4]));
    }

    #[test]
    fn loaded_item_lifetime_counters_tick_with_signed_sentinels() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            });
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let registry = assets.registry;
        let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
        let collision = crate::collisions::load(&root, registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let mut world =
            crate::world::World::new(registry.default_state("minecraft:air").unwrap(), 0);
        world
            .chunks
            .insert((0, 0), Arc::new(crate::chunk::Chunk::empty(0, 0)));
        let tick = |item: &mut ItemEntity| {
            item.tick(
                &world,
                registry,
                &facts,
                &collision,
                false,
                &fluids,
                &[],
                None,
                None,
            )
        };
        let fresh = Tag::compound([
            ("id", string("minecraft:item")),
            ("Pos", doubles(&[4.5, 210.0, 4.5])),
            ("NoGravity", Tag::Byte(1)),
            (
                "Item",
                Tag::compound([("id", string("minecraft:stone")), ("count", Tag::Int(1))]),
            ),
        ]);
        for delay in [32767_i16, -1, 0, 1, 2] {
            for age in [-32768_i16, -1, 0, 5998, 5999, 6000, 32767] {
                let mut item = item_entity_from_nbt(&fresh, 1, 0.0).unwrap();
                for _ in 0..3 {
                    assert!(tick(&mut item));
                }
                assert_eq!(item.age, 3);
                let mut data = item_entity_to_nbt(&item, [0, 0, 0, 1], None);
                put(&mut data, "Age", Tag::Short(age));
                put(&mut data, "PickupDelay", Tag::Short(delay));
                let mut item = item_entity_from_nbt(&data, 1, 0.0).unwrap();
                let mut alive = true;
                for step in 0..=3 {
                    if step > 0 && alive {
                        alive = tick(&mut item);
                    }
                    let expected_age = if age == i16::MIN {
                        i32::from(age)
                    } else {
                        i32::from(age) + step
                    };
                    let expected_alive = step == 0 || expected_age < 6000;
                    assert_eq!(
                        alive, expected_alive,
                        "age={age}, delay={delay}, tick={step}"
                    );
                    if !alive {
                        continue;
                    }
                    let data = item_entity_to_nbt(&item, [0, 0, 0, 1], None);
                    let expected_delay = if delay > 0 && delay != i16::MAX {
                        (i32::from(delay) - step).max(0)
                    } else {
                        i32::from(delay)
                    };
                    assert_eq!(
                        number(data.get("Age")),
                        Some(i64::from(expected_age)),
                        "age={age}, tick={step}"
                    );
                    assert_eq!(
                        number(data.get("PickupDelay")),
                        Some(i64::from(expected_delay)),
                        "age={age}, delay={delay}, tick={step}"
                    );
                }
            }
        }
        let mut legacy = item_entity_from_nbt(&fresh, 1, 0.0).unwrap();
        legacy.pickup_delay = i64::from(u32::MAX);
        assert!(tick(&mut legacy));
        assert_eq!(legacy.pickup_delay, i64::from(u32::MAX));
    }

    #[test]
    fn item_health_uses_floor_then_short_narrowing() {
        let cases = [
            (None, 5),
            (Some(Tag::Short(5)), 5),
            (Some(Tag::Int(5)), 5),
            (Some(Tag::Long(5)), 5),
            (Some(Tag::Byte(5)), 5),
            (Some(Tag::Float((-1.5_f32).to_bits())), -2),
            (Some(Tag::Double((-1.5_f64).to_bits())), -2),
            (Some(Tag::Float(1.5_f32.to_bits())), 1),
            (Some(Tag::Double(1.5_f64.to_bits())), 1),
            (Some(Tag::Double((-40000.5_f64).to_bits())), 25535),
            (Some(Tag::Int(32767)), 32767),
            (Some(Tag::Int(32768)), -32768),
            (Some(Tag::Int(-32768)), -32768),
            (Some(Tag::Int(-32769)), 32767),
            (Some(Tag::Long(65535)), -1),
            (Some(Tag::Long(65536)), 0),
            (Some(Tag::Long(-65537)), -1),
            (Some(Tag::Long(i64::MAX)), -1),
            (Some(string("not_numeric")), 5),
            (Some(Tag::Compound(Compound::new())), 5),
        ];
        for (input, expected) in cases {
            let mut tag = Tag::compound([
                ("id", string("minecraft:item")),
                ("Pos", doubles(&[4.5, 200.0, 4.5])),
                ("NoGravity", Tag::Byte(1)),
                (
                    "Item",
                    Tag::compound([("id", string("minecraft:diamond")), ("count", Tag::Int(1))]),
                ),
            ]);
            if let Some(input) = input {
                put(&mut tag, "Health", input);
            }
            let item = item_entity_from_nbt(&tag, 9, 0.0).unwrap();
            assert_eq!(item.health, expected, "input {tag:?}");
            let saved = item_entity_to_nbt(&item, [1, 2, 3, 4], None);
            assert_eq!(saved.get("Health"), Some(&Tag::Short(expected as i16)));
        }
    }

    #[test]
    fn item_numeric_uuid_codec_adopts_numeric_streams_before_save() {
        let cases = [
            (Tag::IntArray(vec![54, 1, -2, 3]), [54, 1, -2, 3]),
            (
                Tag::LongArray(vec![
                    54,
                    4_294_967_297,
                    -4_294_967_298,
                    9_007_199_254_740_993,
                ]),
                [54, 1, -2, 1],
            ),
            (
                Tag::List(vec![
                    Tag::Float(54.9_f32.to_bits()),
                    Tag::Float(1.75_f32.to_bits()),
                    Tag::Float((-2.75_f32).to_bits()),
                    Tag::Float((-0.5_f32).to_bits()),
                ]),
                [54, 1, -2, 0],
            ),
            (
                Tag::List(vec![
                    Tag::Double(54.9_f64.to_bits()),
                    Tag::Double(1.75_f64.to_bits()),
                    Tag::Double((-2.75_f64).to_bits()),
                    Tag::Double((-0.5_f64).to_bits()),
                ]),
                [54, 1, -2, 0],
            ),
            (
                Tag::List(vec![
                    Tag::Long(54),
                    Tag::Long(4_294_967_297),
                    Tag::Long(-4_294_967_298),
                    Tag::Long(9_007_199_254_740_993),
                ]),
                [54, 1, -2, 1],
            ),
            (
                Tag::ByteArray(vec![54, 1, (-2_i8) as u8, 3]),
                [54, 1, -2, 3],
            ),
            // Util.fixedSize returns a partial first-four result for overlong streams.
            (Tag::IntArray(vec![54, 1, -2, 3, 4]), [54, 1, -2, 3]),
        ];
        for field in ["Owner", "Thrower"] {
            for (value, expected) in &cases {
                let raw = Tag::compound([
                    ("Pos", doubles(&[2.5, 200.0, 2.5])),
                    (
                        "Item",
                        Tag::compound([("id", string("minecraft:stone")), ("count", Tag::Int(1))]),
                    ),
                    ("CustomName", string("kept")),
                    (field, value.clone()),
                ]);
                let mut item = item_entity_from_nbt(&raw, 7, 0.0).unwrap();
                assert_eq!(
                    if field == "Owner" {
                        item.target
                    } else {
                        item.thrower
                    },
                    Some(*expected),
                    "{field}: {value:?}"
                );
                assert_eq!(
                    if field == "Owner" {
                        item.thrower
                    } else {
                        item.target
                    },
                    None
                );
                for source in [None, Some(&raw)] {
                    let saved = item_entity_to_nbt(&item, [54, 0, 0, 1], source);
                    assert_eq!(saved.get(field), Some(&Tag::IntArray(expected.to_vec())));
                    assert_eq!(saved.get("Item").unwrap().get("count"), Some(&Tag::Int(1)));
                }
                let saved = item_entity_to_nbt(&item, [54, 0, 0, 1], Some(&raw));
                assert_eq!(saved.get("CustomName"), raw.get("CustomName"));
                item.target = None;
                item.thrower = None;
                let cleared = item_entity_to_nbt(&item, [54, 0, 0, 1], Some(&saved));
                assert!(cleared.get("Owner").is_none());
                assert!(cleared.get("Thrower").is_none());
            }
        }
    }

    #[test]
    fn item_numeric_uuid_codec_rejects_nonstreams_and_short_inputs() {
        let cases = [
            None,
            Some(string("00000036-0000-0001-ffff-fffe00000003")),
            Some(Tag::IntArray(vec![])),
            Some(Tag::IntArray(vec![54, 1, -2])),
            Some(Tag::Int(7)),
            Some(Tag::compound([("value", Tag::Int(7))])),
            Some(Tag::List(vec![
                Tag::Int(54),
                Tag::Int(1),
                Tag::Int(-2),
                string("3"),
            ])),
        ];
        for field in ["Owner", "Thrower"] {
            for value in &cases {
                let mut raw = Tag::compound([
                    ("Pos", doubles(&[2.5, 200.0, 2.5])),
                    (
                        "Item",
                        Tag::compound([("id", string("minecraft:stone")), ("count", Tag::Int(1))]),
                    ),
                ]);
                if let Some(value) = value {
                    put(&mut raw, field, value.clone());
                }
                let mut item = item_entity_from_nbt(&raw, 7, 0.0).unwrap();
                assert_eq!(
                    (item.target, item.thrower),
                    (None, None),
                    "{field}: {value:?}"
                );
                for source in [None, Some(&raw)] {
                    let saved = item_entity_to_nbt(&item, [54, 0, 0, 1], source);
                    assert!(saved.get("Owner").is_none());
                    assert!(saved.get("Thrower").is_none());
                }
                item.target = Some([54, 1, -2, 3]);
                item.thrower = Some([54, 4, 5, 6]);
                let saved = item_entity_to_nbt(&item, [54, 0, 0, 1], Some(&raw));
                assert_eq!(saved.get("Owner"), Some(&Tag::IntArray(vec![54, 1, -2, 3])));
                assert_eq!(
                    saved.get("Thrower"),
                    Some(&Tag::IntArray(vec![54, 4, 5, 6]))
                );
            }
        }
    }

    #[test]
    fn item_entities_round_trip() {
        let e = ItemEntity {
            health: 2,
            fire_ticks: 73,
            thrower: Some([5, 6, 7, 8]),
            previous: Vec3::ZERO,
            on_ground: true,
            age: 120,
            pickup_delay: 10,
            ..ItemEntity::new(
                7,
                ItemStack {
                    item: Arc::from("minecraft:diamond_pickaxe"),
                    count: 1,
                    damage: 5,
                    extra: None,
                },
                Vec3::new(100.5, 64.0, -31.5),
                Vec3::new(0.0, -0.04, 0.0),
                0.0,
            )
        };
        let tag = item_entity_to_nbt(&e, [1, 2, 3, 4], None);
        assert_eq!(kind(&tag).as_deref(), Some("minecraft:item"));
        assert_eq!(uuid(&tag), Some([1, 2, 3, 4]));
        let back = item_entity_from_nbt(&tag, 9, 0.0).unwrap();
        assert_eq!(back.stack.damage, 5);
        assert_eq!((back.health, back.fire_ticks), (2, 73));
        assert_eq!(
            (back.age, back.pickup_delay, back.on_ground),
            (120, 10, true)
        );
        assert_eq!(back.position.x, 100.5);
        assert_eq!(back.thrower, e.thrower);
        let mut cleared = back;
        cleared.thrower = None;
        let cleared = item_entity_to_nbt(&cleared, [1, 2, 3, 4], Some(&tag));
        assert!(cleared.get("Thrower").is_none());
    }
    #[test]
    fn nbt_numeric_flags_decode_and_save_modeled_values_without_raw() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:pig".into();
        def.species = crate::mobs::Species::Pig;
        let types = [("minecraft:pig".into(), kind)].into();
        for (input, expected) in [
            (None, false),
            (Some(Tag::Byte(0)), false),
            (Some(Tag::Byte(1)), true),
            (Some(Tag::Byte(-1)), true),
            (Some(Tag::Short(256)), false),
            (Some(Tag::Int(256)), false),
            (Some(Tag::Int(257)), true),
            (Some(Tag::Int(-256)), false),
            (Some(Tag::Long(256)), false),
            (Some(Tag::Long(257)), true),
            (Some(Tag::Long(4294967296)), false),
            (Some(Tag::Long(9007199254740993)), true),
            (Some(Tag::Float(0.5f32.to_bits())), false),
            (Some(Tag::Float((-0.5f32).to_bits())), true),
            (Some(Tag::Float(256.5f32.to_bits())), false),
            (Some(Tag::Float((-255.5f32).to_bits())), false),
            (Some(Tag::Double(0.5f64.to_bits())), false),
            (Some(Tag::Double((-0.5f64).to_bits())), true),
            (Some(Tag::Double(256.5f64.to_bits())), false),
            (Some(Tag::Double((-255.5f64).to_bits())), false),
            (Some(Tag::Float((-1e20f32).to_bits())), false),
            (Some(Tag::Double((-1e20f64).to_bits())), false),
            (Some(Tag::Double(1e20f64.to_bits())), true),
            (Some(string("1")), false),
            (Some(string("true")), false),
            (Some(Tag::compound([("a", Tag::Int(1))])), false),
        ] {
            let mut raw = Tag::compound([
                ("id", string("minecraft:pig")),
                ("Pos", doubles(&[4.5, 200.0, 4.5])),
                ("Health", Tag::Float(10f32.to_bits())),
                ("NoGravity", Tag::Byte(1)),
                ("Unrelated", string("keep")),
            ]);
            if let Some(value) = input.as_ref() {
                for key in ["NoAI", "CanPickUpLoot"] {
                    put(&mut raw, key, value.clone());
                }
            }
            let loaded = mob_from_nbt(&raw, &types, 7, None).unwrap();
            assert_eq!(loaded.no_ai, expected, "NoAI {input:?}");
            assert_eq!(loaded.can_pick_up_loot(), expected, "pickup {input:?}");
            assert!(loaded.no_gravity);
            for source in [None, Some(&raw)] {
                let saved = mob_to_nbt(&loaded, [45, 0, 0, 1], source, None, |_| None);
                assert_eq!(
                    saved.get("NoAI"),
                    expected.then_some(&Tag::Byte(1)),
                    "NoAI {input:?}"
                );
                assert_eq!(
                    saved.get("CanPickUpLoot"),
                    Some(&Tag::Byte(i8::from(expected))),
                    "pickup {input:?}"
                );
                if source.is_some() {
                    assert_eq!(saved.get("Unrelated"), Some(&string("keep")));
                }
            }
        }
    }

    #[test]
    fn non_mob_saves_drop_mob_flags_without_losing_entity_fields() {
        let uuid = [104, 0, 0, 1];
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:pig".into();
        def.species = crate::mobs::Species::Pig;
        let types = [("minecraft:pig".into(), kind)].into();
        for input in [None, Some(0), Some(1)] {
            for id in [
                "minecraft:oak_boat",
                "minecraft:oak_chest_boat",
                "minecraft:armor_stand",
                "minecraft:item",
                "minecraft:pig",
            ] {
                let mut raw = Tag::compound([
                    ("id", string(id)),
                    ("Pos", doubles(&[4.5, 200.0, 4.5])),
                    ("Motion", doubles(&[0.0, 0.0, 0.0])),
                    ("Silent", Tag::Byte(1)),
                    ("NoGravity", Tag::Byte(1)),
                    ("Invulnerable", Tag::Byte(1)),
                    ("PortalCooldown", Tag::Int(17)),
                    ("Tags", Tag::List(vec![string("audit")])),
                    ("Unrelated", string("keep")),
                    (
                        "Item",
                        item_to_nbt(&crate::items::ItemStack::new("minecraft:stone", 1)),
                    ),
                ]);
                if let Some(value) = input {
                    put(&mut raw, "NoAI", Tag::Byte(value));
                    put(&mut raw, "PersistenceRequired", Tag::Byte(value));
                }
                let stand = armor_stand_from_nbt(&raw, 1).unwrap();
                let item = item_entity_from_nbt(&raw, 1, 0.0).unwrap();
                let boat = boat_from_nbt(&raw).unwrap();
                for source in [None, Some(&raw)] {
                    let saved = match id {
                        "minecraft:armor_stand" => armor_stand_to_nbt(&stand, uuid, source),
                        "minecraft:item" => item_entity_to_nbt(&item, uuid, source),
                        "minecraft:pig" => {
                            let mut mob = mob_from_nbt(&raw, &types, 1, None).unwrap();
                            // The writer must use live state, even opposite stale raw values.
                            mob.no_ai = input != Some(1);
                            mob.persistent = input != Some(1);
                            mob_to_nbt(&mob, uuid, source, None, |_| None)
                        }
                        _ => boat_to_nbt(
                            id,
                            boat.0,
                            boat.1,
                            [boat.2, 0.0],
                            (false, 0.0),
                            uuid,
                            source,
                            (id == "minecraft:oak_chest_boat").then_some(&[]),
                        ),
                    };
                    if id == "minecraft:pig" {
                        let live = input != Some(1);
                        assert_eq!(saved.get("NoAI"), live.then_some(&Tag::Byte(1)));
                        assert_eq!(
                            saved.get("PersistenceRequired"),
                            Some(&Tag::Byte(i8::from(live)))
                        );
                    } else {
                        assert!(saved.get("NoAI").is_none(), "{id} {input:?}");
                        assert!(saved.get("PersistenceRequired").is_none(), "{id} {input:?}");
                    }
                    assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
                    assert_eq!(saved.get("Pos"), raw.get("Pos"));
                    if source.is_some() {
                        for key in [
                            "Silent",
                            "NoGravity",
                            "Invulnerable",
                            "PortalCooldown",
                            "Tags",
                            "Unrelated",
                        ] {
                            assert_eq!(saved.get(key), raw.get(key), "{id} {key}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn animal_integer_loads_save_modeled_values_without_raw_source() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:pig".into();
        def.species = crate::mobs::Species::Pig;
        let types = [("minecraft:pig".into(), kind)].into();
        for (input, expected) in [
            (None, 0),
            (Some(Tag::Int(0)), 0),
            (Some(Tag::Byte(-7)), -7),
            (Some(Tag::Short(13)), 13),
            (Some(Tag::Int(i32::MIN)), i32::MIN),
            (Some(Tag::Int(i32::MAX)), i32::MAX),
            (Some(Tag::Long(2147483648)), i32::MIN),
            (Some(Tag::Long(-2147483649)), i32::MAX),
            (Some(Tag::Long(4294967296)), 0),
            (Some(Tag::Long(i64::MAX)), -1),
            (Some(Tag::Long(i64::MIN)), 0),
            (Some(Tag::Float(2.5f32.to_bits())), 2),
            (Some(Tag::Float((-2.5f32).to_bits())), -3),
            (Some(Tag::Double(2.5f64.to_bits())), 2),
            (Some(Tag::Double((-2.5f64).to_bits())), -3),
            (Some(Tag::Float(2147483520f32.to_bits())), 2147483520),
            (Some(Tag::Float(2147483648f32.to_bits())), i32::MAX),
            (Some(Tag::Float((-2147483648f32).to_bits())), i32::MIN),
            (Some(Tag::Float((-2147483904f32).to_bits())), i32::MIN),
            (Some(Tag::Double(2147483647.75f64.to_bits())), i32::MAX),
            (Some(Tag::Double(2147483648f64.to_bits())), i32::MAX),
            (Some(Tag::Double((-2147483647.5f64).to_bits())), i32::MIN),
            (Some(Tag::Double((-2147483648.5f64).to_bits())), i32::MIN),
            (Some(Tag::Double(1e20f64.to_bits())), i32::MAX),
            (Some(Tag::Double((-1e20f64).to_bits())), i32::MIN),
            (Some(Tag::Float(1e20f32.to_bits())), i32::MAX),
            (Some(Tag::Float((-1e20f32).to_bits())), i32::MIN),
        ] {
            let mut raw = Tag::compound([
                ("id", string("minecraft:pig")),
                ("Pos", doubles(&[4.5, 200.0, 4.5])),
                ("NoAI", Tag::Byte(1)),
                ("NoGravity", Tag::Byte(1)),
                ("AgeLocked", Tag::Byte(1)),
                ("Unrelated", string("keep")),
            ]);
            if let Some(value) = input.as_ref() {
                for key in ["Age", "ForcedAge", "InLove"] {
                    put(&mut raw, key, value.clone());
                }
            }
            let mob = mob_from_nbt(&raw, &types, 1, None).unwrap();
            assert_eq!(
                (mob.extra.age, mob.extra.forced_age, mob.extra.in_love),
                (expected, expected, expected),
                "{input:?}"
            );
            assert!(mob.no_ai && mob.no_gravity && mob.extra.age_locked);
            for source in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [0, 0, 0, 55], source, None, |_| None);
                for key in ["Age", "ForcedAge", "InLove"] {
                    assert_eq!(
                        saved.get(key),
                        Some(&Tag::Int(expected)),
                        "{key}: {input:?}"
                    );
                }
                assert_eq!(saved.get("NoAI"), Some(&Tag::Byte(1)));
                assert_eq!(saved.get("AgeLocked"), Some(&Tag::Byte(1)));
                assert_eq!(
                    saved.get("Unrelated"),
                    source.and_then(|tag| tag.get("Unrelated"))
                );
            }
        }
    }

    #[test]
    fn animal_nbt_preserves_signed_integer_in_love_and_defaults_after_removal() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let def = Arc::make_mut(&mut kind);
        def.id = "minecraft:pig".into();
        def.species = crate::mobs::Species::Pig;
        let types = [("minecraft:pig".into(), kind)].into();
        for (input, expected) in [
            (Tag::Int(-32769), -32769),
            (Tag::Int(-32768), -32768),
            (Tag::Int(-129), -129),
            (Tag::Int(-128), -128),
            (Tag::Int(-1), -1),
            (Tag::Byte(-1), -1),
            (Tag::Byte(-128), -128),
            (Tag::Int(0), 0),
            (Tag::Int(1), 1),
            (Tag::Int(37), 37),
        ] {
            let raw = Tag::compound([
                ("id", string("minecraft:pig")),
                ("Pos", doubles(&[4.0, 200.0, 4.0])),
                ("NoAI", Tag::Byte(1)),
                ("NoGravity", Tag::Byte(1)),
                ("InLove", input),
            ]);
            let mob = mob_from_nbt(&raw, &types, 1, None).unwrap();
            assert_eq!(mob.extra.in_love, expected);
            // Query snapshots and saves both write the live field as an int,
            // regardless of whether there is an old compound to preserve.
            for old in [None, Some(&raw)] {
                let mut saved = mob_to_nbt(&mob, [0, 0, 0, 1], old, None, |_| None);
                assert_eq!(saved.get("InLove"), Some(&Tag::Int(expected)));
                if let Tag::Compound(map) = &mut saved {
                    map.remove(&"InLove".into());
                }
                let cleared = mob_from_nbt(&saved, &types, 2, None).unwrap();
                assert_eq!(cleared.extra.in_love, 0);
                let saved = mob_to_nbt(&cleared, [0, 0, 0, 1], Some(&saved), None, |_| None);
                assert_eq!(saved.get("InLove"), Some(&Tag::Int(0)));
            }
        }
    }

    #[test]
    fn unknown_fields_survive() {
        let raw = Tag::compound([
            ("id", string("minecraft:item")),
            ("CustomName", string("x")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
            (
                "Item",
                Tag::compound([("id", string("minecraft:stone")), ("count", Tag::Int(3))]),
            ),
        ]);
        let e = item_entity_from_nbt(&raw, 1, 0.0).unwrap();
        let tag = item_entity_to_nbt(&e, [0; 4], Some(&raw));
        assert_eq!(text(tag.get("CustomName")).as_deref(), Some("x"));
        assert_eq!(number(tag.get("Health")), Some(5));
    }
    #[test]
    fn saved_rotation_is_the_entitys_own() {
        // `/tp <entity> x y z 90 10` sets the entity's yaw and pitch; the save
        // (`/data get ... Rotation`) writes them, not [0, 0].
        let tnt = tnt_to_nbt(
            Vec3::ZERO,
            Vec3::ZERO,
            false,
            0.0,
            20,
            4.0,
            string("minecraft:tnt"),
            [90.0, 10.0],
            [1; 4],
            None,
            None,
        );
        assert_eq!(rotation(&tnt), [90.0, 10.0]);
        let mut stand = ArmorStand::new(1, Vec3::ZERO, 0.0);
        stand.yaw = 90.0;
        stand.pitch = 10.0;
        assert_eq!(
            rotation(&armor_stand_to_nbt(&stand, [1; 4], None)),
            [90.0, 10.0]
        );
        let cart = vehicle_to_nbt(
            "minecraft:minecart",
            Vec3::ZERO,
            Vec3::ZERO,
            [90.0, 10.0],
            (false, 0.0),
            [1; 4],
            None,
        );
        assert_eq!(rotation(&cart), [90.0, 10.0]);
    }

    #[test]
    fn entities_without_fire_save_zero_and_a_mob_saves_its_speed_instance() {
        // Entity.remainingFireTicks starts at 0; LivingEntity.removeFrost makes
        // the movement_speed instance AttributeMap.pack then saves.
        let orb = orbs_from_nbt(&Tag::compound([("Pos", doubles(&[0.0; 3]))])).remove(0);
        let tag = orb_to_nbt(&orb, [0; 4]);
        assert_eq!(tag.get("Fire"), Some(&Tag::Short(0)));
        // A stand summoned this tick has no instance yet; its first tick makes
        // it, and a save that lists it (`/data merge`, a reload) keeps it.
        let attributes =
            |stand: &ArmorStand| match armor_stand_to_nbt(stand, [0; 4], None).get("attributes") {
                Some(Tag::List(attrs)) => attrs
                    .iter()
                    .map(|a| (text(a.get("id")).unwrap(), a.get("base").cloned()))
                    .collect::<Vec<_>>(),
                other => panic!("attributes: {other:?}"),
            };
        let mut stand = ArmorStand::new(1, Vec3::ZERO, 0.0);
        assert_eq!(attributes(&stand), []);
        stand.speed_instance = true;
        let speed = vec![(
            "minecraft:movement_speed".to_owned(),
            Some(Tag::Double(0.7f64.to_bits())),
        )];
        assert_eq!(attributes(&stand), speed);
        let saved = armor_stand_to_nbt(&stand, [0; 4], None);
        assert_eq!(attributes(&armor_stand_from_nbt(&saved, 1).unwrap()), speed);
    }
    #[test]
    fn a_loaded_stand_gains_the_speed_instance_its_first_tick_makes() {
        // Vanilla 26.3: `/summon armor_stand ~ ~ ~ {attributes:[]}` saves `[]` until the
        // first tick, whose removeFrost makes movement_speed (base 0.7d); a saved
        // instance of another attribute stays in front of it.
        let ids = |stand: &ArmorStand, raw: &Tag| match armor_stand_to_nbt(stand, [0; 4], Some(raw))
            .get("attributes")
        {
            Some(Tag::List(attrs)) => attrs
                .iter()
                .map(|a| (text(a.get("id")).unwrap(), a.get("base").cloned()))
                .collect::<Vec<_>>(),
            other => panic!("attributes: {other:?}"),
        };
        let health = (
            "minecraft:max_health".to_owned(),
            Some(Tag::Double(20f64.to_bits())),
        );
        let speed = (
            "minecraft:movement_speed".to_owned(),
            Some(Tag::Double(0.7f64.to_bits())),
        );
        let empty = Tag::compound([("attributes", Tag::List(Vec::new()))]);
        let saved = Tag::compound([(
            "attributes",
            Tag::List(vec![Tag::compound([
                ("id", string("minecraft:max_health")),
                ("base", Tag::Double(20f64.to_bits())),
            ])]),
        )]);
        let mut stand = ArmorStand::new(1, Vec3::ZERO, 0.0);
        assert_eq!(ids(&stand, &empty), []);
        assert_eq!(ids(&stand, &saved), std::slice::from_ref(&health));
        stand.speed_instance = true;
        assert_eq!(ids(&stand, &empty), std::slice::from_ref(&speed));
        assert_eq!(ids(&stand, &saved), [health, speed]);
    }
    #[test]
    fn summoned_trident_defaults_are_not_pickable_or_already_shot() {
        let raw = Tag::compound([
            ("Pos", doubles(&[0.0, 0.0, 0.0])),
            ("LeftOwner", Tag::Byte(0)),
        ]);
        let loaded = trident_from_nbt(&raw).unwrap();
        assert_eq!(loaded.pickup, Pickup::Disallowed);
        assert!(!loaded.left_owner && !loaded.has_been_shot);
        let trident = ItemStack::new("minecraft:trident", 1);
        let tag = trident_to_nbt(
            &TridentSave {
                position: Vec3::ZERO,
                motion: Vec3::ZERO,
                yaw: 0.0,
                pitch: 0.0,
                in_ground: false,
                life: 0,
                shake: 0,
                dealt_damage: false,
                pickup: loaded.pickup,
                left_owner: loaded.left_owner,
                has_been_shot: loaded.has_been_shot,
                owner: loaded.owner,
                no_gravity: false,
                on_ground: false,
                fire_ticks: 0,
                fall_distance: 0.0,
                sound_event: TRIDENT_HIT_GROUND,
                item: &trident,
            },
            [0; 4],
            Some(&raw),
        );
        assert!(tag.get("LeftOwner").is_none());
        assert_eq!(tag.get("HasBeenShot"), Some(&Tag::Byte(0)));
        assert_eq!(tag.get("pickup"), Some(&Tag::Byte(0)));
    }
    #[test]
    fn orbs_keep_their_count() {
        let at = ("Pos", doubles(&[1.0, 2.0, 3.0]));
        let loaded = [at, ("Value", Tag::Short(7)), ("Age", Tag::Short(3))];
        let o = orbs_from_nbt(&Tag::compound(
            loaded
                .into_iter()
                .chain([("Count", Tag::Int(3)), ("Air", Tag::Short(32767))]),
        ))
        .remove(0);
        let tag = orb_to_nbt(&o, [0; 4]);
        let back = orbs_from_nbt(&tag);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].value, 7);
        assert_eq!(back[0].count, 3);
        // Entity.load keeps the saved Air; the orb writes it back unchanged.
        assert_eq!(tag.get("Air"), Some(&Tag::Short(32767)));
    }
}

/// `Brain`'s two measured Warden Unit memories, using actual existing slots.
fn write_warden_owned_memories(m: &Mob, map: &mut Compound) {
    let brain = map
        .entry("Brain".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    let Tag::Compound(brain) = brain else { return };
    let memories = brain
        .entry("memories".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    let Tag::Compound(memories) = memories else {
        return;
    };
    let mem = m.extra.warden.as_ref().map(|st| &st.brain.mem);
    for (key, slot) in [
        ("minecraft:is_emerging", mem.map(|mem| &mem.is_emerging)),
        ("minecraft:dig_cooldown", mem.map(|mem| &mem.dig_cooldown)),
    ] {
        if let Some(slot) = slot.filter(|slot| slot.present()) {
            let mut entry = Tag::compound([("value", Tag::Compound(Compound::new()))]);
            if slot.ttl() != i64::MAX {
                put(&mut entry, "ttl", Tag::Long(slot.ttl()));
            }
            memories.insert(key.into(), entry);
        } else {
            memories.remove(&key.into());
        }
    }
}
/// `Brain` / `ExpirableValue`: admitted Unit values with absent or Long TTL.
fn restore_warden_owned_memories(m: &mut Mob, tag: &Tag) {
    let memories = tag.get("Brain").and_then(|brain| brain.get("memories"));
    let Some(st) = m.extra.warden.as_mut() else {
        return;
    };
    for (key, slot) in [
        ("minecraft:is_emerging", &mut st.brain.mem.is_emerging),
        ("minecraft:dig_cooldown", &mut st.brain.mem.dig_cooldown),
    ] {
        let Some(entry) = memories.and_then(|memories| memories.get(key)) else {
            // Fresh Java read/load: absent entries do not retain finalize's slot.
            slot.clear();
            continue;
        };
        if !matches!(entry.get("value"), Some(Tag::Compound(value)) if value.is_empty()) {
            continue; // Unmeasured malformed shapes retain existing initialization.
        }
        match entry.get("ttl") {
            None => slot.set(()),
            Some(Tag::Long(ttl)) => slot.set_ttl((), *ttl),
            _ => {} // No new numeric/malformed TTL policy.
        }
    }
}

fn write_axolotl_memories(m: &Mob, map: &mut Compound) {
    let brain = map
        .entry("Brain".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if !matches!(brain, Tag::Compound(_)) {
        *brain = Tag::Compound(Compound::new());
    }
    let Tag::Compound(brain) = brain else { return };
    let memories = brain
        .entry("memories".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if !matches!(memories, Tag::Compound(_)) {
        *memories = Tag::Compound(Compound::new());
    }
    let Tag::Compound(memories) = memories else {
        return;
    };
    let mm = m.extra.animal_brain.as_deref().map(|b| &b.mem);
    let hunting = mm.and_then(|mem| {
        let slot = &mem.hunting_cooldown;
        slot.value().map(|value| {
            let mut entry = Tag::compound([("value", Tag::Byte(i8::from(value)))]);
            if slot.ttl() != i64::MAX {
                put(&mut entry, "ttl", Tag::Long(slot.ttl()));
            }
            entry
        })
    });
    // ExpirableValue.codec: integer value and optional finite long TTL.
    let integer_memory = |slot: &crate::mobs::animal_brain::Slot<i32>| {
        slot.value().map(|v| {
            let mut entry = Tag::compound([("value", Tag::Int(v))]);
            if slot.ttl() != i64::MAX {
                put(&mut entry, "ttl", Tag::Long(slot.ttl()));
            }
            entry
        })
    };
    let play_dead = mm.and_then(|m| integer_memory(&m.play_dead_ticks));
    let temptation = mm.and_then(|m| integer_memory(&m.temptation_cooldown_ticks));
    for (key, value) in [
        ("minecraft:has_hunting_cooldown", hunting),
        ("minecraft:play_dead_ticks", play_dead),
        ("minecraft:temptation_cooldown_ticks", temptation),
    ] {
        if let Some(value) = value {
            memories.insert(key.into(), value);
        } else {
            memories.remove(&key.into());
        }
    }
}
/// Saved variants and fish/axolotl bucket flags. Missing variants use a
/// medium salmon or index 0; a missing bucket flag is false.
fn read_variant(m: &mut Mob, tag: &Tag) {
    use crate::mobs::Species as S;
    // AbstractFish/Axolotl: false when absent. Tadpole.setFromBucket ignores the input.
    if matches!(
        m.species(),
        S::Cod | S::Salmon | S::Pufferfish | S::TropicalFish | S::Axolotl
    ) {
        m.extra.from_bucket = flag(tag, "FromBucket");
    }
    let int = |key| number(tag.get(key)).unwrap_or(0) as i32;
    match m.species() {
        S::Parrot => m.extra.variant = int("Variant").clamp(0, 4) as u8, // CLAMP
        S::Axolotl => m.extra.variant = ((int("Variant") as u32).min(5) % 5) as u8, // ZERO: lucy
        // Rabbit.Variant.LEGACY_CODEC: an unknown id is brown.
        S::Rabbit => {
            m.extra.variant = match int("RabbitType") {
                v @ (0..=5 | 99) => v as u8,
                _ => 0,
            }
        }
        S::Salmon => {
            m.extra.variant = text(tag.get("type"))
                .and_then(|t| SALMON_SIZES.iter().position(|v| *v == t))
                .unwrap_or(1) as u8;
        }
        S::TropicalFish => {
            let packed = int("Variant");
            let (size, index) = (packed & 0xFF, (packed >> 8) & 0xFF);
            // TropicalFish.Pattern.byId: an unknown id is a kob.
            m.extra.variant = if size <= 1 && index < 6 {
                (size * 6 + index) as u8
            } else {
                0
            };
            // TropicalFish.getBaseColor/getPatternColor use DyeColor.byId:
            // read the full byte, then default invalid color ids to white.
            let color = |shift| match (packed >> shift) & 0xFF {
                value @ 0..=15 => value as u8,
                _ => 0,
            };
            m.extra.variant2 = color(16);
            m.extra.variant3 = color(24);
        }
        _ => {}
    }
}
fn restore_axolotl_memories(m: &mut Mob, tag: &Tag) {
    let memories = tag.get("Brain").and_then(|b| b.get("memories"));
    // Initialize/clear the slot without applying the runtime setter's positive-TTL rule to saved data.
    crate::mobs::animal_brain::set_hunting_cooldown(m, 0);
    if let Some(b) = m.extra.animal_brain.as_mut() {
        if let Some(entry) = memories.and_then(|v| v.get("minecraft:has_hunting_cooldown"))
            && let Some(value) = float(entry.get("value"))
        {
            // NbtOps boolean numbers use doubleValue()!=0; ExpirableValue's optional TTL is lenient.
            let ttl = number(entry.get("ttl")).unwrap_or(i64::MAX);
            b.mem.hunting_cooldown.set_ttl(value != 0.0, ttl);
        }
        for (key, slot) in [
            ("minecraft:play_dead_ticks", &mut b.mem.play_dead_ticks),
            (
                "minecraft:temptation_cooldown_ticks",
                &mut b.mem.temptation_cooldown_ticks,
            ),
        ] {
            if let Some(entry) = memories.and_then(|b| b.get(key))
                && let Some(value) = number(entry.get("value"))
            {
                // ExpirableValue.codec's lenient optional TTL defaults to permanent.
                slot.set_ttl(value as i32, number(entry.get("ttl")).unwrap_or(i64::MAX));
            }
        }
    }
}
const AXOLOTL_VARIANTS: [&str; 5] = ["lucy", "wild", "gold", "cyan", "blue"];
/// `Salmon.Variant` names, by id.
const SALMON_SIZES: [&str; 3] = ["small", "medium", "large"];
/// `TropicalFish.Pattern` names in enum order: the six small patterns, then
/// the six large ones (the order of `extra.variant`).
const TROPICAL_PATTERNS: [&str; 12] = [
    "kob",
    "sunstreak",
    "snooper",
    "dasher",
    "brinely",
    "spotty",
    "flopper",
    "stripey",
    "glitter",
    "blockfish",
    "betty",
    "clayfish",
];
/// `Bucketable.saveDefaultDataToBucketTag`, with the age an axolotl and a
/// tadpole add (`saveToBucketTag`).
pub fn bucket_data(m: &Mob) -> Tag {
    let mut data = Tag::compound([("Health", Tag::Float(m.health.to_bits()))]);
    if matches!(
        m.species(),
        crate::mobs::Species::Axolotl | crate::mobs::Species::Tadpole
    ) {
        put(&mut data, "Age", Tag::Int(m.extra.age));
        put(
            &mut data,
            "AgeLocked",
            Tag::Byte(i8::from(m.extra.age_locked)),
        );
    }
    for (key, on) in [
        ("NoAI", m.no_ai),
        ("Silent", m.silent),
        ("NoGravity", m.no_gravity),
        ("Glowing", m.glowing),
        ("Invulnerable", m.invulnerable_flag),
        ("PersistenceRequired", m.persistent),
    ] {
        if on {
            put(&mut data, key, Tag::Byte(1));
        }
    }
    if let Some(ttl) = crate::mobs::animal_brain::hunting_cooldown(m) {
        put(&mut data, "HuntingCooldown", Tag::Long(ttl));
    }
    // SulfurCube.saveToBucketTag: after the default data, its age.
    if m.species() == crate::mobs::Species::SulfurCube {
        put(&mut data, "age", Tag::Int(m.extra.age));
        put(
            &mut data,
            "age_locked",
            Tag::Byte(i8::from(m.extra.age_locked)),
        );
    }
    data
}
pub fn bucket_components(m: &Mob) -> crate::item_data::StackData {
    let mut data = crate::item_data::StackData {
        custom_name: m
            .custom_name
            .as_ref()
            .map(|name| Tag::String(name.as_str().into())),
        ..Default::default()
    };
    data.set_other("minecraft:bucket_entity_data", Some(bucket_data(m)));
    // SulfurCube.saveToBucketTag: the body block, when it has one.
    if m.species() == crate::mobs::Species::SulfurCube
        && let Some(body) = &m.extra.body
    {
        data.set_other(
            "minecraft:sulfur_cube_content",
            Some(Tag::compound([("id", string(body))])),
        );
    }
    if m.species() == crate::mobs::Species::Axolotl {
        let variant = AXOLOTL_VARIANTS
            .get(usize::from(m.extra.variant))
            .unwrap_or(&"lucy");
        data.set_other("minecraft:axolotl/variant", Some(string(variant)));
    }
    // Salmon.saveToBucketTag: the size.
    if m.species() == crate::mobs::Species::Salmon {
        let size = SALMON_SIZES
            .get(usize::from(m.extra.variant))
            .unwrap_or(&"medium");
        data.set_other("minecraft:salmon/size", Some(string(size)));
    }
    // TropicalFish.saveToBucketTag: the pattern and both colours.
    if m.species() == crate::mobs::Species::TropicalFish {
        let dye = |i: u8| crate::mobs::render::DYE_NAMES[usize::from(i) & 15];
        let pattern = TROPICAL_PATTERNS
            .get(usize::from(m.extra.variant))
            .unwrap_or(&"kob");
        data.set_other("minecraft:tropical_fish/pattern", Some(string(pattern)));
        data.set_other(
            "minecraft:tropical_fish/base_color",
            Some(string(dye(m.extra.variant2))),
        );
        data.set_other(
            "minecraft:tropical_fish/pattern_color",
            Some(string(dye(m.extra.variant3))),
        );
    }
    data
}
/// `Entity.applyComponentsFromItemStack` → the spawned mob's
/// `applyImplicitComponents`: a bucket's contents (`MobBucketItem.spawn`) and
/// a spawn egg (`PostSpawnProcessor.apply`) both land here. Vanilla applies
/// these after `finalizeSpawn`, so a component wins over the biome variant.
pub fn apply_item_components(m: &mut Mob, stack: &crate::items::ItemStack) {
    let Some(components) = stack.data() else {
        return;
    };
    if let Some(name) = stack.custom_name() {
        m.custom_name = Some(name);
    }
    if let Some(data) = components.other("minecraft:bucket_entity_data") {
        if let Some(v) = number(data.get("Age")) {
            m.extra.age = v as i32;
        }
        if let Some(v) = number(data.get("AgeLocked")) {
            m.extra.age_locked = v != 0;
        }
        // Keep old Horizon buckets readable; the modern component below takes precedence.
        if let Some(v) = number(data.get("Variant")) {
            m.extra.variant = v as u8;
        }
        if let Some(v) = float(data.get("Health")) {
            m.health = v as f32;
        }
        for (key, flag) in [
            ("NoAI", &mut m.no_ai),
            ("Silent", &mut m.silent),
            ("NoGravity", &mut m.no_gravity),
            ("Glowing", &mut m.glowing),
            ("Invulnerable", &mut m.invulnerable_flag),
        ] {
            if let Some(v) = number(data.get(key)) {
                *flag = v != 0;
            }
        }
        m.persistent |= flag(data, "PersistenceRequired");
        if m.species() == crate::mobs::Species::Axolotl {
            crate::mobs::animal_brain::set_hunting_cooldown(
                m,
                number(data.get("HuntingCooldown")).unwrap_or(0),
            );
        }
    }
    if m.species() == crate::mobs::Species::Axolotl
        && let Some(name) = text(components.other("minecraft:axolotl/variant"))
        && let Some(id) = AXOLOTL_VARIANTS
            .iter()
            .position(|&v| v == name.trim_start_matches("minecraft:"))
    {
        m.extra.variant = id as u8;
    }
    // Salmon.applyImplicitComponents: the size.
    if m.species() == crate::mobs::Species::Salmon
        && let Some(name) = text(components.other("minecraft:salmon/size"))
        && let Some(id) = SALMON_SIZES
            .iter()
            .position(|&v| v == name.trim_start_matches("minecraft:"))
    {
        m.extra.variant = id as u8;
    }
    // TropicalFish.applyImplicitComponent: the pattern and both colours.
    if m.species() == crate::mobs::Species::TropicalFish {
        let name = |key| text(components.other(key));
        let dye = |key| {
            name(key).and_then(|n| {
                crate::mobs::render::DYE_NAMES
                    .iter()
                    .position(|&v| v == n.trim_start_matches("minecraft:"))
            })
        };
        if let Some(id) = name("minecraft:tropical_fish/pattern").and_then(|n| {
            TROPICAL_PATTERNS
                .iter()
                .position(|&v| v == n.trim_start_matches("minecraft:"))
        }) {
            m.extra.variant = id as u8;
        }
        if let Some(id) = dye("minecraft:tropical_fish/base_color") {
            m.extra.variant2 = id as u8;
        }
        if let Some(id) = dye("minecraft:tropical_fish/pattern_color") {
            m.extra.variant3 = id as u8;
        }
    }
    // Chicken, Cow and Pig.applyImplicitComponents: their variant and sound
    // variant components, both registry names (`minecraft:warm`,
    // `minecraft:picky`).
    let species = m.species();
    let (variant, sound) = match species {
        crate::mobs::Species::Chicken => (
            Some("minecraft:chicken/variant"),
            Some("minecraft:chicken/sound_variant"),
        ),
        crate::mobs::Species::Cow => (
            Some("minecraft:cow/variant"),
            Some("minecraft:cow/sound_variant"),
        ),
        crate::mobs::Species::Pig => (
            Some("minecraft:pig/variant"),
            Some("minecraft:pig/sound_variant"),
        ),
        // ZombieNautilus.applyImplicitComponents: the variant only.
        crate::mobs::Species::ZombieNautilus => (Some("minecraft:zombie_nautilus/variant"), None),
        // Fox and Frog.applyImplicitComponent: `setVariant` from the component
        // alone, so a spawn egg carrying one hatches that variant.
        crate::mobs::Species::Fox => (Some("minecraft:fox/variant"), None),
        crate::mobs::Species::Frog => (Some("minecraft:frog/variant"), None),
        // Horse.applyImplicitComponent: `setVariant` sets the coat colour and
        // keeps the markings the spawn picked (`getTypeVariant() & -256`),
        // which Horizon keeps in `extra.variant2` and leaves alone here.
        crate::mobs::Species::Horse => (Some("minecraft:horse/variant"), None),
        // Llama.applyImplicitComponent, which a trader llama inherits.
        crate::mobs::Species::Llama | crate::mobs::Species::TraderLlama => {
            (Some("minecraft:llama/variant"), None)
        }
        _ => (None, None),
    };
    if let Some(name) = variant.and_then(|id| text(components.other(id))) {
        m.extra.variant = crate::mobs::finalize::variant_index(species, &name);
    }
    if let Some(name) = sound.and_then(|id| text(components.other(id)))
        && let Some(i) = crate::mobs::finalize::sound_variants(species)
            .iter()
            .position(|v| *v == name.trim_start_matches("minecraft:"))
    {
        m.extra.sound_variant = Some(i as u8);
    }
    // MushroomCow.applyImplicitComponents: `Variant.DEFAULT` is red, as the
    // save's `Type` reads it.
    if species == crate::mobs::Species::Mooshroom
        && let Some(name) = text(components.other("minecraft:mooshroom/variant"))
    {
        m.extra.variant = u8::from(name.trim_start_matches("minecraft:") == "brown");
    }
    // Sheep.applyImplicitComponents: the wool colour, a DyeColor id.
    if species == crate::mobs::Species::Sheep
        && let Some(name) = text(components.other("minecraft:sheep/color"))
        && let Some(i) = crate::mobs::render::DYE_NAMES
            .iter()
            .position(|c| *c == name.trim_start_matches("minecraft:"))
    {
        m.extra.color = i as u8;
    }
}

#[cfg(test)]
mod axolotl_nbt_tests {
    use super::*;
    #[test]
    fn axolotl_boolean_memory_preserves_values_and_saved_lifetimes() {
        let key = "minecraft:has_hunting_cooldown";
        for (value, expected) in [
            (Some(Tag::Byte(0)), Some(false)),
            (Some(Tag::Byte(1)), Some(true)),
            (Some(Tag::Int(256)), Some(true)),
            (Some(Tag::Float(0.5f32.to_bits())), Some(true)),
            (Some(Tag::Double((-0.5f64).to_bits())), Some(true)),
            (Some(string("true")), None),
            (Some(Tag::Compound(Compound::new())), None),
            (None, None),
        ] {
            for ttl in [None, Some(3), Some(0), Some(-1), Some(i64::MAX)] {
                let mut entry = Tag::Compound(Compound::new());
                if let Some(value) = &value {
                    put(&mut entry, "value", value.clone());
                }
                if let Some(ttl) = ttl {
                    put(&mut entry, "ttl", Tag::Long(ttl));
                }
                let raw = Tag::compound([(
                    "Brain",
                    Tag::compound([("memories", Tag::compound([(key, entry)]))]),
                )]);
                let mut kind = crate::mobs::sensor_scan_tests::kind();
                let ty = Arc::make_mut(&mut kind);
                ty.id = "minecraft:axolotl".into();
                ty.species = crate::mobs::Species::Axolotl;
                let mut mob = Mob::new(1, kind, Vec3::ZERO, 0.0);
                restore_axolotl_memories(&mut mob, &raw);
                let saved = mob_to_nbt(&mob, [0; 4], Some(&raw), None, |_| None);
                let entry = saved
                    .get("Brain")
                    .and_then(|b| b.get("memories"))
                    .and_then(|m| m.get(key));
                match expected {
                    Some(value) => {
                        let entry = entry.expect("false and nonpositive lifetimes remain present");
                        assert_eq!(entry.get("value"), Some(&Tag::Byte(i8::from(value))));
                        let expected_ttl = ttl.filter(|v| *v != i64::MAX).map(Tag::Long);
                        assert_eq!(entry.get("ttl"), expected_ttl.as_ref());
                    }
                    None => assert!(entry.is_none(), "invalid/missing boolean must be dropped"),
                }
            }
        }
    }
    #[test]
    fn aquatic_from_bucket_saves_defaults_and_tadpole_constant() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let types: HashMap<_, _> = crate::mobs::MobTypes::load(&root)
            .unwrap()
            .types
            .into_iter()
            .map(|(id, kind)| (id, Arc::new(kind)))
            .collect();
        for kind in [
            "cod",
            "salmon",
            "pufferfish",
            "tropical_fish",
            "axolotl",
            "tadpole",
        ] {
            for value in [None, Some(false), Some(true)] {
                let mut input = Tag::compound([
                    ("id", string(&format!("minecraft:{kind}"))),
                    ("Pos", doubles(&[10.5, 220.0, 10.5])),
                ]);
                if let (Some(value), Tag::Compound(map)) = (value, &mut input) {
                    map.insert("FromBucket".into(), Tag::Byte(i8::from(value)));
                }
                let m = mob_from_nbt(&input, &types, 1, None).unwrap();
                let expected = kind == "tadpole" || value.unwrap_or(false);
                for raw in [None, Some(&input)] {
                    let saved = mob_to_nbt(&m, [1; 4], raw, None, |_| None);
                    assert_eq!(
                        saved.get("FromBucket"),
                        Some(&Tag::Byte(i8::from(expected))),
                        "{kind}, input={value:?}, raw={}",
                        raw.is_some()
                    );
                }
            }
        }
        let pig = Mob::new(1, types["minecraft:pig"].clone(), Vec3::ZERO, 0.0);
        assert!(
            mob_to_nbt(&pig, [1; 4], None, None, |_| None)
                .get("FromBucket")
                .is_none()
        );
    }

    /// `HappyGhast.addAdditionalSaveData` / `readAdditionalSaveData`: the still timeout
    /// (`serverStillTimeout`) goes with the world. A ghast saved while it was held still
    /// comes back still, which is what keeps the box a dropped item rests on
    /// (`EntityGetter.getEntityCollisions`) and the control a rider has
    /// (`getControllingPassenger` reads `isOnStillTimeout`).
    #[test]
    fn a_happy_ghast_keeps_its_still_timeout() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<_, _> = data
            .types
            .into_iter()
            .map(|(id, kind)| (id, Arc::new(kind)))
            .collect();
        let kind = types["minecraft:happy_ghast"].clone();
        let mut m = Mob::new(1, kind, Vec3::new(0.5, 80.0, 0.5), 0.0);
        // Past `HappyGhast.tick`'s 60-tick gate, so the timer counts down from here.
        m.extra.age = 100;
        for timeout in [0, 7, 10] {
            m.extra.counter = timeout;
            let saved = mob_to_nbt(&m, [1; 4], None, None, |_| None);
            assert_eq!(
                number(saved.get("still_timeout")),
                Some(i64::from(timeout)),
                "{timeout}"
            );
            let back = mob_from_nbt(&saved, &types, 2, None).unwrap();
            assert_eq!(back.extra.counter, timeout, "{timeout}");
            assert_eq!(
                crate::mobs::happy_ghast::on_still_timeout(&back),
                timeout > 0,
                "{timeout}"
            );
        }
    }

    #[test]
    fn bucket_cooldown_and_brain_memories_survive_and_expired_cooldown_is_removed() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let kind = Arc::new(data.types.get("minecraft:axolotl").unwrap().clone());
        let mut m = Mob::new(1, kind.clone(), Vec3::new(0.0, 4.0, 0.0), 0.0);
        crate::mobs::animal_brain::set_hunting_cooldown(&mut m, 123);
        m.extra.variant = 4;
        m.extra.age = -24000;
        m.extra.age_locked = true;
        m.no_ai = true;
        m.silent = true;
        m.no_gravity = true;
        m.glowing = true;
        m.invulnerable_flag = true;
        m.persistent = true;
        m.custom_name = Some("Bucket friend".into());
        m.health = 5.0;

        {
            let mm = &mut m.extra.animal_brain.as_mut().unwrap().mem;
            mm.play_dead_ticks.set(78);
            mm.temptation_cooldown_ticks.set(42);
        }
        let raw = Tag::compound([(
            "Brain",
            Tag::compound([(
                "memories",
                Tag::compound([("test:unrelated", Tag::compound([("value", Tag::Int(9))]))]),
            )]),
        )]);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], Some(&raw), None, |_| None);
        let memories = tag.get("Brain").unwrap().get("memories").unwrap();
        assert!(memories.get("test:unrelated").is_some());
        assert_eq!(
            number(
                memories
                    .get("minecraft:play_dead_ticks")
                    .unwrap()
                    .get("value")
            ),
            Some(78)
        );
        assert_eq!(
            number(
                memories
                    .get("minecraft:temptation_cooldown_ticks")
                    .unwrap()
                    .get("value")
            ),
            Some(42)
        );
        let mut stack = crate::items::ItemStack::new("minecraft:axolotl_bucket", 1);
        stack.extra = Some(Box::new(bucket_components(&m)));
        assert_eq!(
            text(stack.data().unwrap().other("minecraft:axolotl/variant")),
            Some("blue".into())
        );
        assert!(bucket_data(&m).get("Variant").is_none());
        let stack = item_from_nbt(&item_to_nbt(&stack)).unwrap();
        let mut restored = Mob::new(2, kind, Vec3::new(0.0, 4.0, 0.0), 0.0);
        apply_item_components(&mut restored, &stack);
        assert_eq!(
            crate::mobs::animal_brain::hunting_cooldown(&restored),
            Some(123)
        );
        assert_eq!(
            (
                restored.extra.variant,
                restored.extra.age,
                restored.extra.age_locked
            ),
            (4, -24000, true)
        );
        assert!(
            restored.no_ai
                && restored.silent
                && restored.no_gravity
                && restored.glowing
                && restored.invulnerable_flag
                && restored.persistent
        );
        assert_eq!(restored.custom_name.as_deref(), Some("Bucket friend"));
        assert_eq!(restored.health, 5.0);
        m.glowing = false;
        m.add_effect("glowing", 200, 0);
        assert!(m.has_effect("minecraft:glowing"));
        assert!(bucket_data(&m).get("Glowing").is_none());
        m.glowing = true;
        assert_eq!(number(bucket_data(&m).get("Glowing")), Some(1));
        for (persistent, expected) in [(None, false), (Some(0), false), (Some(1), true)] {
            let mut data = Tag::Compound(Compound::new());
            if let Some(value) = persistent {
                put(&mut data, "PersistenceRequired", Tag::Byte(value));
            }
            let mut bucket = crate::items::ItemStack::new("minecraft:axolotl_bucket", 1);
            bucket
                .data_mut()
                .set_other("minecraft:bucket_entity_data", Some(data));
            restored.persistent = false;
            apply_item_components(&mut restored, &bucket);
            assert_eq!(restored.persistent, expected);
            restored.persistent = true;
            apply_item_components(&mut restored, &bucket);
            assert!(restored.persistent, "bucket data cannot clear persistence");
        }
        restored.ticks_frozen = 140;
        restored.ignite_for_ticks(160);
        assert_eq!(restored.ticks_frozen, 0);
        restored.ticks_frozen = 140;
        restored.ignite_for_ticks(1);
        assert_eq!(
            restored.ticks_frozen, 0,
            "even an ignition that does not extend burning clears freeze"
        );
        for (id, name) in AXOLOTL_VARIANTS.iter().enumerate() {
            let mut vanilla = crate::items::ItemStack::new("minecraft:axolotl_bucket", 1);
            vanilla.extra = Some(Box::new(crate::item_data::StackData {
                other: vec![("minecraft:axolotl/variant".into(), string(name))],
                ..Default::default()
            }));
            apply_item_components(&mut restored, &vanilla);
            assert_eq!(
                usize::from(restored.extra.variant),
                id,
                "component-only vanilla buckets work"
            );
        }
        let mut legacy = crate::items::ItemStack::new("minecraft:axolotl_bucket", 1);
        legacy.extra = Some(Box::new(crate::item_data::StackData {
            other: vec![(
                "minecraft:bucket_entity_data".into(),
                Tag::compound([("Variant", Tag::Int(3))]),
            )],
            ..Default::default()
        }));
        apply_item_components(&mut restored, &legacy);
        assert_eq!(restored.extra.variant, 3);
        legacy
            .data_mut()
            .set_other("minecraft:axolotl/variant", Some(string("gold")));
        apply_item_components(&mut restored, &legacy);
        assert_eq!(
            restored.extra.variant, 2,
            "component wins over old Horizon data"
        );

        crate::mobs::animal_brain::set_hunting_cooldown(&mut m, 0);
        assert!(bucket_data(&m).get("HuntingCooldown").is_none());
        let saved = mob_to_nbt(&m, [1, 2, 3, 4], Some(&tag), None, |_| None);
        assert!(
            saved
                .get("Brain")
                .unwrap()
                .get("memories")
                .unwrap()
                .get("minecraft:has_hunting_cooldown")
                .is_none()
        );
        restore_axolotl_memories(&mut restored, &saved);
        let mm = &restored.extra.animal_brain.as_ref().unwrap().mem;
        assert_eq!(mm.play_dead_ticks.value(), Some(78));
        assert_eq!(mm.temptation_cooldown_ticks.value(), Some(42));
    }
}

#[cfg(test)]
mod sheep_nbt_tests {
    use super::*;
    #[test]
    fn a_sheep_keeps_its_dyed_and_sheared_wool() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let mut m = Mob::new(
            1,
            Arc::new(data.types["minecraft:sheep"].clone()),
            Vec3::new(0.0, 4.0, 0.0),
            0.0,
        );
        // Summoned white, then dyed red and sheared: the save has it now.
        let raw = Tag::compound([("Color", Tag::Byte(0)), ("Sheared", Tag::Byte(0))]);
        (m.extra.color, m.extra.sheared) = (14, true);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], Some(&raw), None, |_| None);
        assert_eq!(
            (number(tag.get("Color")), number(tag.get("Sheared"))),
            (Some(14), Some(1))
        );
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!((back.extra.color, back.extra.sheared), (14, true));
    }

    #[test]
    fn a_snow_golem_keeps_its_pumpkin_state() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let saved = |pumpkin: Option<i8>| {
            let mut tag = Tag::compound([
                ("id", string("minecraft:snow_golem")),
                ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ]);
            if let Some(value) = pumpkin {
                put(&mut tag, "Pumpkin", Tag::Byte(value));
            }
            tag
        };
        // SnowGolem.readAdditionalSaveData: getBooleanOr("Pumpkin", true).
        for (pumpkin, sheared) in [(None, false), (Some(1), false), (Some(0), true)] {
            let m = mob_from_nbt(&saved(pumpkin), &types, 2, None).unwrap();
            assert_eq!(m.extra.sheared, sheared, "{pumpkin:?}");
            // addAdditionalSaveData always writes it, over the loaded value.
            let tag = mob_to_nbt(&m, [1, 2, 3, 4], Some(&saved(pumpkin)), None, |_| None);
            assert_eq!(number(tag.get("Pumpkin")), Some(i64::from(!sheared)));
        }
        // Sheared in game: the save and the next load keep it off.
        let mut m = mob_from_nbt(&saved(None), &types, 3, None).unwrap();
        m.extra.sheared = true;
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert!(mob_from_nbt(&tag, &types, 4, None).unwrap().extra.sheared);
    }

    /// `Hoglin.readAdditionalSaveData` and `addAdditionalSaveData`:
    /// `CannotBeHunted` (a bastion's hoglins) stays with the hoglin and is always
    /// written, and such a hoglin is no piglin's prey (`Hoglin.canBeHunted`).
    #[test]
    fn a_hoglin_keeps_cannot_be_hunted_and_is_no_prey() {
        use crate::mobs::Species;
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.species = Species::Hoglin;
        ty.id = "minecraft:hoglin".into();
        let types = HashMap::from([("minecraft:hoglin".to_owned(), kind)]);
        let hoglin = |cannot: Option<i8>| {
            let mut tag = Tag::compound([
                ("id", string("minecraft:hoglin")),
                ("Pos", doubles(&[0.0, 64.0, 0.0])),
            ]);
            if let (Some(b), Tag::Compound(map)) = (cannot, &mut tag) {
                map.insert("CannotBeHunted".into(), Tag::Byte(b));
            }
            tag
        };
        let raw = hoglin(Some(1));
        let protected = mob_from_nbt(&raw, &types, 1, None).unwrap();
        assert!(protected.extra.cannot_be_hunted);
        assert!(!protected.seen(None).huntable);
        let saved = mob_to_nbt(&protected, [0; 4], Some(&raw), None, |_| None);
        assert_eq!(saved.get("CannotBeHunted"), Some(&Tag::Byte(1)));
        for raw in [hoglin(None), hoglin(Some(0))] {
            let prey = mob_from_nbt(&raw, &types, 2, None).unwrap();
            assert!(!prey.extra.cannot_be_hunted);
            assert!(prey.seen(None).huntable);
            let saved = mob_to_nbt(&prey, [0; 4], Some(&raw), None, |_| None);
            assert_eq!(saved.get("CannotBeHunted"), Some(&Tag::Byte(0)));
        }
    }

    /// `Mob.addAdditionalSaveData` and `readAdditionalSaveData`:
    /// `DeathLootTable` and a set `DeathLootTableSeed` stay with the mob.
    #[test]
    fn a_death_loot_table_round_trips() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let summoned = Tag::compound([
            ("id", string("minecraft:zombie")),
            ("Pos", doubles(&[0.0, 64.0, 0.0])),
            ("DeathLootTable", string("mymod:entities/boss")),
            ("DeathLootTableSeed", Tag::Long(42)),
        ]);
        let m = mob_from_nbt(&summoned, &types, 2, None).unwrap();
        assert_eq!(
            m.extra.death_loot_table.as_deref(),
            Some("mymod:entities/boss")
        );
        assert_eq!(m.extra.death_loot_seed, 42);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(
            text(tag.get("DeathLootTable")).as_deref(),
            Some("mymod:entities/boss")
        );
        assert_eq!(number(tag.get("DeathLootTableSeed")), Some(42));
        // None set: neither is written.
        let plain = Tag::compound([
            ("id", string("minecraft:zombie")),
            ("Pos", doubles(&[0.0, 64.0, 0.0])),
        ]);
        let plain = mob_from_nbt(&plain, &types, 3, None).unwrap();
        let tag = mob_to_nbt(&plain, [1, 2, 3, 4], None, None, |_| None);
        assert!(tag.get("DeathLootTable").is_none() && tag.get("DeathLootTableSeed").is_none());
    }

    /// `Rabbit.addAdditionalSaveData` and `readAdditionalSaveData`: `MoreCarrotTicks`.
    #[test]
    fn a_rabbit_keeps_how_full_of_carrots_it_is() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let mut m = Mob::new(
            1,
            Arc::new(data.types["minecraft:rabbit"].clone()),
            Vec3::new(0.0, 4.0, 0.0),
            0.0,
        );
        m.extra.more_carrot_ticks = 33;
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("MoreCarrotTicks")), Some(33));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(back.extra.more_carrot_ticks, 33);
    }

    /// `Ocelot.addAdditionalSaveData` and `readAdditionalSaveData`: `Trusting`.
    #[test]
    fn an_ocelot_keeps_its_trust() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let mut m = Mob::new(
            1,
            Arc::new(data.types["minecraft:ocelot"].clone()),
            Vec3::new(0.0, 4.0, 0.0),
            0.0,
        );
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("Trusting")), Some(0));
        m.extra.trusting = true;
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("Trusting")), Some(1));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert!(back.extra.trusting);
    }

    /// `Mannequin.addAdditionalSaveData` and `readAdditionalSaveData`: `immovable`, and the
    /// description (the default is not written, `hide_description` hides it, a component stays).
    #[test]
    fn a_mannequin_keeps_its_immovable_flag_and_description() {
        use crate::mobs::MannequinDescription as D;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let mut m = Mob::new(
            1,
            Arc::new(data.types["minecraft:mannequin"].clone()),
            Vec3::new(0.0, 4.0, 0.0),
            0.0,
        );
        let save = |m: &Mob| mob_to_nbt(m, [1, 2, 3, 4], None, None, |_| None);
        // The default description is not written; it comes back.
        let tag = save(&m);
        assert!(tag.get("description").is_none() && tag.get("hide_description").is_none());
        assert_eq!(number(tag.get("immovable")), Some(0));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(
            (back.extra.immovable, back.extra.description),
            (false, D::Default)
        );
        // Hidden, and immovable.
        m.extra.immovable = true;
        m.extra.description = D::Hidden;
        let tag = save(&m);
        assert_eq!(number(tag.get("hide_description")), Some(1));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(
            (back.extra.immovable, back.extra.description),
            (true, D::Hidden)
        );
        // A component of the world's own.
        let own = Tag::compound([("text", string("Guard")), ("color", string("red"))]);
        m.extra.description = D::Custom(own.clone());
        let tag = save(&m);
        assert_eq!(tag.get("description"), Some(&own));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(back.extra.description, D::Custom(own));
    }

    /// The only lib test that registers mod effects (once per process).
    #[test]
    fn a_mob_keeps_a_mod_effect() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let types: HashMap<String, Arc<MobType>> = crate::mobs::MobTypes::load(&root)
            .unwrap()
            .types
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect();
        crate::brewing::register_mod_effects([(
            "testfx:glow".to_owned(),
            1,
            crate::brewing::Category::Harmful,
        )]);
        let mut m = Mob::new(1, types["minecraft:pig"].clone(), Vec3::ZERO, 0.0);
        m.add_effect(crate::brewing::effect("testfx:glow").unwrap().id, 200, 2);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        let Some(Tag::List(saved)) = tag.get("active_effects") else {
            panic!("no active_effects");
        };
        assert_eq!(text(saved[0].get("id")).as_deref(), Some("testfx:glow"));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(back.extra.effects, [("testfx:glow", 200, 2)]);
    }

    #[test]
    fn an_amplifier_of_zero_is_left_out_of_active_effects() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let types: HashMap<String, Arc<MobType>> = crate::mobs::MobTypes::load(&root)
            .unwrap()
            .types
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect();
        let mut m = Mob::new(1, types["minecraft:pig"].clone(), Vec3::ZERO, 0.0);
        let speed = crate::brewing::effect("minecraft:speed").unwrap().id;
        m.add_effect(speed, 200, 0);
        m.add_effect(
            crate::brewing::effect("minecraft:slowness").unwrap().id,
            200,
            2,
        );
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        let Some(Tag::List(saved)) = tag.get("active_effects") else {
            panic!("no active_effects");
        };
        assert!(saved[0].get("amplifier").is_none());
        assert!(saved[1].get("amplifier").is_some());
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(back.extra.effects[0].2, 0);
        assert_eq!(back.extra.effects[1].2, 2);
    }

    #[test]
    fn farm_animals_keep_their_variant_and_sound_variant() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let mut m = Mob::new(
            1,
            types["minecraft:pig"].clone(),
            Vec3::new(0.0, 4.0, 0.0),
            0.0,
        );
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(
            text(tag.get("variant")).as_deref(),
            Some("minecraft:temperate")
        );
        assert_eq!(
            text(tag.get("sound_variant")).as_deref(),
            Some("minecraft:classic")
        );
        // Spawned in a warm biome with the mini sounds.
        (m.extra.variant, m.extra.sound_variant) = (1, Some(2));
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(text(tag.get("variant")).as_deref(), Some("minecraft:warm"));
        assert_eq!(
            text(tag.get("sound_variant")).as_deref(),
            Some("minecraft:mini")
        );
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!((back.extra.variant, back.extra.sound_variant), (1, Some(2)));
        let mut hen = Mob::new(
            3,
            types["minecraft:chicken"].clone(),
            Vec3::new(0.0, 4.0, 0.0),
            0.0,
        );
        (hen.extra.variant, hen.extra.egg_time) = (2, 7795);
        let tag = mob_to_nbt(&hen, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(text(tag.get("variant")).as_deref(), Some("minecraft:cold"));
        let back = mob_from_nbt(&tag, &types, 4, None).unwrap();
        assert_eq!((back.extra.variant, back.extra.egg_time), (2, 7795));
        // The sound variants are indices into the registries finalizeSpawn draws from.
        use crate::mobs::{Species, finalize};
        let registries = finalize::SpawnRegistries::load(&root);
        for (species, registry) in [
            (Species::Pig, &registries.pig_sound),
            (Species::Cow, &registries.cow_sound),
            (Species::Chicken, &registries.chicken_sound),
            (Species::Cat, &registries.cat_sound),
            (Species::Wolf, &registries.wolf_sound),
        ] {
            assert_eq!(
                finalize::sound_variants(species),
                registry.names,
                "{species:?}"
            );
        }
    }

    #[test]
    fn parrots_omit_raw_age_and_pigs_keep_modeled_age() {
        use crate::mobs::Species;
        for species in [Species::Parrot, Species::Pig] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let ty = Arc::make_mut(&mut kind);
            ty.species = species;
            ty.id = if species == Species::Parrot {
                "minecraft:parrot"
            } else {
                "minecraft:pig"
            }
            .into();
            let mut mob = Mob::new(1, kind, Vec3::ZERO, 0.0);
            let expected = if species == Species::Pig {
                mob.extra.age = -37;
                Some(Tag::Int(-37))
            } else {
                None
            };
            for raw in [
                None,
                Some(Tag::Compound(Default::default())),
                Some(Tag::compound([("Age", Tag::Int(0))])),
                Some(Tag::compound([("Age", Tag::Int(-100))])),
                Some(Tag::compound([("Age", Tag::Int(100))])),
            ] {
                let saved = mob_to_nbt(&mob, [0; 4], raw.as_ref(), None, |_| None);
                assert_eq!(
                    saved.get("Age"),
                    expected.as_ref(),
                    "{species:?}, raw={raw:?}"
                );
            }
        }
    }

    #[test]
    fn a_baby_villager_keeps_its_age() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let saved = Tag::compound([
            ("id", string("minecraft:villager")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ("Age", Tag::Int(-21359)),
        ]);
        let m = mob_from_nbt(&saved, &types, 1, None).unwrap();
        assert_eq!(m.extra.age, -21359);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("Age")), Some(-21359));
        assert_eq!(number(tag.get("AgeLocked")), Some(0));
        assert_eq!(number(tag.get("VillagerDataFinalized")), Some(1));
    }

    /// `Dolphin.addAdditionalSaveData`: a fed dolphin stays fed and a wet one
    /// stays wet across a reload; a save with neither field reads as an unfed
    /// dolphin at full moistness.
    #[test]
    fn a_dolphins_moistness_and_fish_survive_a_save() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        // A fresh dolphin starts full, as `Dolphin`'s field does.
        let mut m = Mob::new(
            1,
            types["minecraft:dolphin"].clone(),
            Vec3::new(0.5, 64.0, 0.5),
            0.0,
        );
        assert_eq!(m.extra.counter, 2400);
        m.extra.flag = true;
        m.extra.counter = 123;
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("Moistness")), Some(123));
        assert_eq!(number(tag.get("GotFish")), Some(1));
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert!(back.extra.flag, "fed");
        assert_eq!(back.extra.counter, 123);
        // A save with neither field: unfed, full.
        let bare = Tag::compound([
            ("id", string("minecraft:dolphin")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
        ]);
        let back = mob_from_nbt(&bare, &types, 3, None).unwrap();
        assert!(!back.extra.flag);
        assert_eq!(back.extra.counter, 2400);
    }

    /// `WanderingTrader.addAdditionalSaveData`: the visit's countdown and the
    /// place it is walking to (a `BlockPos` codec, so an int array), which the
    /// despawn and stroll goals read back after a reload.
    #[test]
    fn a_wandering_traders_countdown_and_target_survive_a_save() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let mut m = Mob::new(
            1,
            types["minecraft:wandering_trader"].clone(),
            Vec3::new(0.5, 64.0, 0.5),
            0.0,
        );
        m.extra.ints[5] = 48000;
        m.extra.point = Some([12.0, 65.0, -30.0]);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("DespawnDelay")), Some(48000));
        let Some(Tag::IntArray(target)) = tag.get("wander_target") else {
            panic!("wander_target missing");
        };
        assert_eq!(target, &vec![12, 65, -30]);
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(back.extra.ints[5], 48000);
        assert_eq!(back.extra.point, Some([12.0, 65.0, -30.0]));
        // A save with neither field: idle, and nothing to walk to.
        let bare = Tag::compound([
            ("id", string("minecraft:wandering_trader")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
        ]);
        let back = mob_from_nbt(&bare, &types, 3, None).unwrap();
        assert_eq!((back.extra.ints[5], back.extra.point), (0, None));
    }

    #[test]
    fn mobs_keep_their_variant_puff_state_carried_block_and_conversion_times() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let registry = horizon_world::registry::BlockRegistry::from_facts_json(
            std::fs::File::open(root.join("facts.json")).unwrap(),
        )
        .unwrap();
        let new = |kind: &str| Mob::new(1, types[kind].clone(), Vec3::new(0.0, 4.0, 0.0), 0.0);
        let round_trip = |m: &Mob| {
            let tag = mob_to_nbt(m, [1, 2, 3, 4], None, Some(&registry), |_| None);
            (mob_from_nbt(&tag, &types, 2, Some(&registry)).unwrap(), tag)
        };
        // A save with nothing but where the mob stands.
        let empty =
            |kind: &str| Tag::compound([("id", string(kind)), ("Pos", doubles(&[0.5, 64.0, 0.5]))]);
        let mut fox = new("minecraft:fox");
        fox.extra.variant = 1;
        let (back, tag) = round_trip(&fox);
        assert_eq!(text(tag.get("Type")).as_deref(), Some("snow"));
        assert_eq!(number(tag.get("Sleeping")), Some(0));
        assert_eq!(back.extra.variant, 1);
        // MushroomCow: a brown mooshroom stays brown, and a save without (or
        // with an unknown) Type reads as red.
        let mut mooshroom = new("minecraft:mooshroom");
        mooshroom.extra.variant = 1;
        let (back, tag) = round_trip(&mooshroom);
        assert_eq!(text(tag.get("Type")).as_deref(), Some("brown"));
        assert_eq!(back.extra.variant, 1);
        mooshroom.extra.variant = 0;
        let (back, tag) = round_trip(&mooshroom);
        assert_eq!(text(tag.get("Type")).as_deref(), Some("red"));
        assert_eq!(back.extra.variant, 0);
        // Its stew effects round-trip as the component's bare {id, duration}
        // entries, and a cleared list is not written at all.
        mooshroom.extra.variant = 1;
        mooshroom.extra.stew_effects = Some(vec![(Arc::from("minecraft:night_vision"), 100)]);
        let (back, tag) = round_trip(&mooshroom);
        assert_eq!(back.extra.stew_effects, mooshroom.extra.stew_effects);
        let entry = match tag.get("stew_effects") {
            Some(Tag::List(list)) => list.first().expect("one effect"),
            other => panic!("stew_effects: {other:?}"),
        };
        assert_eq!(
            (text(entry.get("id")), number(entry.get("duration"))),
            (Some("minecraft:night_vision".into()), Some(100))
        );
        mooshroom.extra.stew_effects = None;
        let (back, tag) = round_trip(&mooshroom);
        assert_eq!(back.extra.stew_effects, None);
        assert!(tag.get("stew_effects").is_none());
        let mut rabbit = new("minecraft:rabbit");
        rabbit.extra.variant = 4;
        let (back, tag) = round_trip(&rabbit);
        assert_eq!(number(tag.get("RabbitType")), Some(4));
        assert_eq!(back.extra.variant, 4);
        let mut goat = new("minecraft:goat");
        (goat.extra.screaming, goat.extra.lost_horns) = (true, [false, true]);
        let (back, tag) = round_trip(&goat);
        assert_eq!(number(tag.get("HasRightHorn")), Some(0));
        assert_eq!(
            (back.extra.screaming, back.extra.lost_horns),
            (true, [false, true])
        );
        // Pufferfish.addAdditionalSaveData: the puff state, 0 when the save
        // has none and at most 2 when it has more.
        let mut pufferfish = new("minecraft:pufferfish");
        pufferfish.extra.size = 2;
        let (back, tag) = round_trip(&pufferfish);
        assert_eq!(number(tag.get("PuffState")), Some(2));
        assert_eq!(back.extra.size, 2);
        assert_eq!(
            mob_from_nbt(&empty("minecraft:pufferfish"), &types, 3, Some(&registry))
                .unwrap()
                .extra
                .size,
            0
        );
        let puffed = Tag::compound([
            ("id", string("minecraft:pufferfish")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ("PuffState", Tag::Int(5)),
        ]);
        assert_eq!(
            mob_from_nbt(&puffed, &types, 3, Some(&registry))
                .unwrap()
                .extra
                .size,
            2
        );
        // Enderman.addAdditionalSaveData: the block it carries, a block name
        // for the block's default state and `{id, properties}` otherwise.
        let mut enderman = new("minecraft:enderman");
        let grass = registry.default_state("minecraft:grass_block");
        enderman.extra.carried = grass;
        let (back, tag) = round_trip(&enderman);
        assert_eq!(
            text(tag.get("carriedBlockState")).as_deref(),
            Some("minecraft:grass_block")
        );
        assert_eq!(back.extra.carried, grass);
        let log = registry
            .id(&horizon_world::chunk::BlockState {
                name: "minecraft:oak_log".into(),
                properties: [("axis".to_owned(), "x".to_owned())].into(),
            })
            .unwrap();
        enderman.extra.carried = Some(log);
        let (back, tag) = round_trip(&enderman);
        assert!(matches!(
            tag.get("carriedBlockState"),
            Some(Tag::Compound(_))
        ));
        assert_eq!(back.extra.carried, Some(log));
        // Nothing carried: no key, and none read back; an air state reads as
        // no block at all.
        enderman.extra.carried = None;
        let (back, tag) = round_trip(&enderman);
        assert_eq!(tag.get("carriedBlockState"), None);
        assert_eq!(back.extra.carried, None);
        let air = Tag::compound([
            ("id", string("minecraft:enderman")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ("carriedBlockState", string("minecraft:air")),
        ]);
        assert_eq!(
            mob_from_nbt(&air, &types, 3, Some(&registry))
                .unwrap()
                .extra
                .carried,
            None
        );
        // A goat saved without horn flags has both, as vanilla reads it.
        assert_eq!(
            mob_from_nbt(&empty("minecraft:goat"), &types, 3, None)
                .unwrap()
                .extra
                .lost_horns,
            [false; 2]
        );
        // Zombie and Skeleton.addAdditionalSaveData: the `ConversionTracker`
        // tags, -1 for a counter that is not running, and both come back into
        // the counters the tick uses.
        let mut zombie = new("minecraft:zombie");
        // Positive affliction counters represent a zombie with immersed eyes.
        zombie.fluids.eye_in_water = true;
        zombie.extra.counter = 300;
        let (back, tag) = round_trip(&zombie);
        assert_eq!(number(tag.get("InWaterTime")), Some(300));
        assert_eq!(number(tag.get("DrownedConversionTime")), Some(-1));
        assert_eq!((back.extra.counter, back.extra.counter2), (300, 0));
        (zombie.extra.counter, zombie.extra.counter2) = (600, 301);
        let (back, tag) = round_trip(&zombie);
        assert_eq!(number(tag.get("InWaterTime")), Some(600));
        assert_eq!(number(tag.get("DrownedConversionTime")), Some(300));
        assert_eq!((back.extra.counter, back.extra.counter2), (600, 301));
        // A skeleton outside powder snow is not afflicted: -1 whatever the
        // counter holds, as with a fresh or NoAI one.
        let (_, tag) = round_trip(&new("minecraft:skeleton"));
        assert_eq!(number(tag.get("FreezingTime")), Some(-1));
        assert_eq!(number(tag.get("StrayConversionTime")), Some(-1));
        let mut skeleton = new("minecraft:skeleton");
        skeleton.in_powder_snow = true;
        skeleton.extra.counter = 70;
        let (back, tag) = round_trip(&skeleton);
        assert_eq!(number(tag.get("FreezingTime")), Some(70));
        assert_eq!(number(tag.get("StrayConversionTime")), Some(-1));
        assert_eq!((back.extra.counter, back.extra.counter2), (70, 0));
        (skeleton.extra.counter, skeleton.extra.counter2) = (140, 301);
        let (back, tag) = round_trip(&skeleton);
        assert_eq!(number(tag.get("FreezingTime")), Some(140));
        assert_eq!(number(tag.get("StrayConversionTime")), Some(300));
        assert_eq!((back.extra.counter, back.extra.counter2), (140, 301));
        // Not converting: -1 in both tags, and nothing starts on load.
        for (kind, affliction, conversion) in [
            ("minecraft:zombie", "InWaterTime", "DrownedConversionTime"),
            ("minecraft:skeleton", "FreezingTime", "StrayConversionTime"),
        ] {
            let mut m = new(kind);
            (m.extra.counter, m.extra.counter2) = (-1, 0);
            let (back, tag) = round_trip(&m);
            assert_eq!(number(tag.get(affliction)), Some(-1));
            assert_eq!(number(tag.get(conversion)), Some(-1));
            assert_eq!((back.extra.counter, back.extra.counter2), (-1, 0));
        }
        // Zombie villagers and zombified piglins keep Zombie's tracker tags.
        for kind in ["minecraft:zombie_villager", "minecraft:zombified_piglin"] {
            let (_, tag) = round_trip(&new(kind));
            assert_eq!(number(tag.get("DrownedConversionTime")), Some(-1), "{kind}");
            assert_eq!(number(tag.get("InWaterTime")), Some(-1), "{kind}");
        }
        // A ticking one that left the snow or the water saves -1 over the times
        // it was loaded with (ConversionTracker.tick: setConverting(false)).
        for (kind, conversion, affliction) in [
            ("minecraft:skeleton", "StrayConversionTime", "FreezingTime"),
            ("minecraft:zombie", "DrownedConversionTime", "InWaterTime"),
        ] {
            let loaded = Tag::compound([
                ("id", string(kind)),
                ("Pos", doubles(&[0.5, 64.0, 0.5])),
                (conversion, Tag::Int(200)),
                (affliction, Tag::Int(5)),
            ]);
            let mut m = new(kind);
            (m.extra.counter, m.extra.counter2) = (-1, 0);
            let tag = mob_to_nbt(&m, [1, 2, 3, 4], Some(&loaded), Some(&registry), |_| None);
            assert_eq!(number(tag.get(conversion)), Some(-1), "{kind}");
            assert_eq!(number(tag.get(affliction)), Some(-1), "{kind}");
        }
        // AbstractPiglin: a piglin or brute saved without CanPickUpLoot picks up
        // loot. Piglin always writes CannotHunt, the brute never does.
        for (kind, hunt) in [
            ("minecraft:piglin", Some(0)),
            ("minecraft:piglin_brute", None),
        ] {
            let back = mob_from_nbt(&empty(kind), &types, 5, None).unwrap();
            assert!(back.can_pick_up_loot(), "{kind}");
            let tag = mob_to_nbt(&back, [1, 2, 3, 4], None, Some(&registry), |_| None);
            assert_eq!(number(tag.get("CannotHunt")), hunt, "{kind}");
        }
        let mut saved = empty("minecraft:piglin");
        put(&mut saved, "CanPickUpLoot", Tag::Byte(0));
        let back = mob_from_nbt(&saved, &types, 6, None).unwrap();
        assert!(!back.can_pick_up_loot());
        // Bogged: whether it was sheared comes back, and a fresh one saves false.
        let (_, tag) = round_trip(&new("minecraft:bogged"));
        assert_eq!(number(tag.get("sheared")), Some(0));
        let mut bogged = new("minecraft:bogged");
        bogged.extra.sheared = true;
        let (back, tag) = round_trip(&bogged);
        assert_eq!(number(tag.get("sheared")), Some(1));
        assert!(back.extra.sheared);
        // A saved conversion time starts one: `startConversion`.
        let saved = Tag::compound([
            ("id", string("minecraft:zombie")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ("InWaterTime", Tag::Int(600)),
            ("DrownedConversionTime", Tag::Int(300)),
        ]);
        let back = mob_from_nbt(&saved, &types, 4, None).unwrap();
        assert_eq!((back.extra.counter, back.extra.counter2), (600, 301));
    }
}

#[cfg(test)]
mod item_component_tests {
    use super::*;
    #[test]
    fn a_spawn_egg_beats_the_biome_variant() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let spawn = |kind: &str, id: u32| {
            let mut m = Mob::new(
                id,
                Arc::new(data.types[kind].clone()),
                Vec3::new(0.0, 4.0, 0.0),
                0.0,
            );
            // What `finalizeSpawn` leaves in a temperate biome.
            m.extra.variant = 0;
            m
        };
        let mut egg = crate::items::ItemStack::new("minecraft:chicken_spawn_egg", 1);
        egg.data_mut()
            .set_other("minecraft:chicken/variant", Some(string("minecraft:warm")));
        let mut hen = spawn("minecraft:chicken", 1);
        assert_eq!(hen.extra.variant, 0);
        apply_item_components(&mut hen, &egg);
        assert_eq!(hen.extra.variant, 1, "minecraft:warm, not the biome's");
        // MushroomCow.applyImplicitComponents: the egg's variant, and the
        // component the mob reports back.
        let mut egg = crate::items::ItemStack::new("minecraft:mooshroom_spawn_egg", 1);
        egg.data_mut().set_other(
            "minecraft:mooshroom/variant",
            Some(string("minecraft:brown")),
        );
        let mut mooshroom = spawn("minecraft:mooshroom", 3);
        apply_item_components(&mut mooshroom, &egg);
        let (id, variant) = crate::mobs::finalize::variant_component(&mooshroom).unwrap();
        assert_eq!(
            (id, variant.as_str()),
            ("minecraft:mooshroom/variant", "minecraft:brown")
        );
        // Sheep.applyImplicitComponents: the egg's colour over the mob's.
        let mut egg = crate::items::ItemStack::new("minecraft:sheep_spawn_egg", 1);
        egg.data_mut()
            .set_other("minecraft:sheep/color", Some(string("lime")));
        let mut sheep = spawn("minecraft:sheep", 2);
        apply_item_components(&mut sheep, &egg);
        assert_eq!(sheep.extra.color, 5, "lime, not the white it started as");
        // ZombieNautilus.applyImplicitComponents: the egg carries the coral
        // variant over the temperate one the biome picked.
        let mut egg = crate::items::ItemStack::new("minecraft:zombie_nautilus_spawn_egg", 1);
        egg.data_mut().set_other(
            "minecraft:zombie_nautilus/variant",
            Some(string("minecraft:warm")),
        );
        let mut nautilus = spawn("minecraft:zombie_nautilus", 4);
        apply_item_components(&mut nautilus, &egg);
        assert_eq!(nautilus.extra.variant, 1, "minecraft:warm, not temperate");
    }

    /// `Fox.applyImplicitComponent` and `Frog.applyImplicitComponent`: the
    /// variant component alone decides the mob, `setVariant` from the egg.
    #[test]
    fn a_fox_or_frog_spawn_egg_picks_its_variant_component() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let spawn = |kind: &str, id: u32| {
            Mob::new(
                id,
                Arc::new(data.types[kind].clone()),
                Vec3::new(0.0, 4.0, 0.0),
                0.0,
            )
        };
        // A red fox, as a spawn in a forest leaves it.
        let mut fox = spawn("minecraft:fox", 1);
        assert_eq!(fox.extra.variant, 0);
        let mut egg = crate::items::ItemStack::new("minecraft:fox_spawn_egg", 1);
        egg.data_mut()
            .set_other("minecraft:fox/variant", Some(string("minecraft:snow")));
        apply_item_components(&mut fox, &egg);
        assert_eq!(fox.extra.variant, 1, "Fox.Variant.SNOW, not red");
        // A temperate frog, as a swamp spawn leaves it.
        let mut frog = spawn("minecraft:frog", 2);
        assert_eq!(frog.extra.variant, 0);
        let mut egg = crate::items::ItemStack::new("minecraft:frog_spawn_egg", 1);
        egg.data_mut()
            .set_other("minecraft:frog/variant", Some(string("minecraft:warm")));
        apply_item_components(&mut frog, &egg);
        assert_eq!(frog.extra.variant, 1, "minecraft:warm, not temperate");
        // An unknown name falls back to the codec's default, red.
        let mut egg = crate::items::ItemStack::new("minecraft:fox_spawn_egg", 1);
        egg.data_mut()
            .set_other("minecraft:fox/variant", Some(string("minecraft:temperate")));
        apply_item_components(&mut fox, &egg);
        assert_eq!(fox.extra.variant, 0, "unknown names read as red");
    }

    /// `Horse.applyImplicitComponent` and `Llama.applyImplicitComponent`: the
    /// `.../variant` component sets the coat colour. A horse's markings are
    /// the other half of the packed variant its spawn picked and stay.
    #[test]
    fn a_horse_or_llama_spawn_egg_picks_its_variant_component() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let spawn = |kind: &str, id: u32| {
            Mob::new(
                id,
                Arc::new(data.types[kind].clone()),
                Vec3::new(0.0, 4.0, 0.0),
                0.0,
            )
        };
        // A black horse (Horse.Variant.BLACK, 4) with a white field (markings
        // 2), as a spawn in a plains leaves it.
        let mut horse = spawn("minecraft:horse", 1);
        (horse.extra.variant, horse.extra.variant2) = (4, 2);
        let mut egg = crate::items::ItemStack::new("minecraft:horse_spawn_egg", 1);
        egg.data_mut().set_other(
            "minecraft:horse/variant",
            Some(string("minecraft:dark_brown")),
        );
        apply_item_components(&mut horse, &egg);
        assert_eq!(horse.extra.variant, 6, "Horse.Variant.DARK_BROWN");
        assert_eq!(
            horse.extra.variant2, 2,
            "the markings the spawn picked stay"
        );
        // `Horse.get(DataComponents.HORSE_VARIANT)` reports it back.
        let (id, variant) = crate::mobs::finalize::variant_component(&horse).unwrap();
        assert_eq!(
            (id, variant.as_str()),
            ("minecraft:horse/variant", "minecraft:dark_brown")
        );
        // The save keeps the packed `Variant` int and gains no `variant` name,
        // which only `VariantUtils.writeVariant` species store.
        let tag = mob_to_nbt(&horse, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(tag.get("variant"), None);
        assert!(
            matches!(tag.get("Variant"), Some(Tag::Int(v)) if *v == 6 | 2 << 8),
            "colour 6 and markings 2 packed"
        );
        // A llama, as a spawn in a plains leaves it: creamy.
        let mut egg = crate::items::ItemStack::new("minecraft:llama_spawn_egg", 1);
        egg.data_mut()
            .set_other("minecraft:llama/variant", Some(string("minecraft:gray")));
        let mut llama = spawn("minecraft:llama", 2);
        assert_eq!(llama.extra.variant, 0);
        apply_item_components(&mut llama, &egg);
        assert_eq!(llama.extra.variant, 3, "Llama.Variant.GRAY");
        // TraderLlama inherits the llama's arm.
        let mut trader = spawn("minecraft:trader_llama", 3);
        apply_item_components(&mut trader, &egg);
        assert_eq!(trader.extra.variant, 3, "the trader llama reads it too");
    }

    /// `EntityType.updateCustomEntityTag`: the egg's `entity_data` on the mob it
    /// spawned, when the type is its own — the mob's compound, the data merged
    /// over it and loaded back, as the spawn egg site does it.
    #[test]
    fn a_spawn_eggs_entity_data_reaches_the_mob_it_spawned() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let egg = |entity_data: Option<Tag>| {
            let mut egg = crate::items::ItemStack::new("minecraft:zombie_spawn_egg", 1);
            if let Some(data) = entity_data {
                egg.data_mut()
                    .set_other("minecraft:entity_data", Some(data));
            }
            egg
        };
        let spawn = |stack: &crate::items::ItemStack| {
            let mut m = Mob::new(
                2,
                types["minecraft:zombie"].clone(),
                Vec3::new(0.0, 4.0, 0.0),
                0.0,
            );
            apply_item_components(&mut m, stack);
            let Some(data) =
                crate::block_entity_item::spawn_egg_entity_data(stack, &m.kind.id, false)
            else {
                return m;
            };
            let tag =
                crate::data_cmd::merged(&mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None), data);
            mob_from_nbt(&tag, &types, 2, None).unwrap()
        };
        // `entity_data={NoAI:1b}`: no id, so the egg's own type holds the data.
        assert!(spawn(&egg(Some(Tag::compound([("NoAI", Tag::Byte(1))])))).no_ai);
        // `entity_data.id` names another type: the egg's own type, data skipped.
        let other = spawn(&egg(Some(Tag::compound([
            ("id", string("minecraft:pig")),
            ("NoAI", Tag::Byte(1)),
        ]))));
        assert_eq!(other.kind.id.as_str(), "minecraft:zombie");
        assert!(!other.no_ai);
        // A plain egg is unchanged.
        assert!(!spawn(&egg(None)).no_ai);
    }
}

#[cfg(test)]
mod fireball_nbt_tests {
    use super::*;

    /// `WitherSkull`: a skull in flight is saved with its chunk, `dangerous`
    /// is a byte (absent means a black skull), and it never saves a fire
    /// because `WitherSkull.shouldBurn` is false.
    #[test]
    fn a_wither_skull_keeps_whether_it_is_dangerous() {
        let blue = Tag::compound([
            ("id", Tag::String("minecraft:wither_skull".into())),
            ("Pos", doubles(&[3.0, 64.0, 3.0])),
            ("Motion", doubles(&[0.0, 0.0, 0.0])),
            ("dangerous", Tag::Byte(1)),
        ]);
        let mut skull = fireball_from_nbt(&blue, None).unwrap();
        assert_eq!(skull.kind, ProjectileKind::WitherSkull { dangerous: true });
        assert!(saved_with_chunk(&skull));
        skull.age = 5;
        let back = fireball_to_nbt(&skull, [1, 2, 3, 4], None, None).unwrap();
        assert_eq!(kind(&back).as_deref(), Some("minecraft:wither_skull"));
        assert_eq!(number(back.get("dangerous")), Some(1));
        // Entity saves Fire every time; a skull's stays 0 (a fireball's is 20 once ticked).
        assert_eq!(number(back.get("Fire")), Some(0));

        let black = Tag::compound([
            ("id", Tag::String("minecraft:wither_skull".into())),
            ("Pos", doubles(&[0.0, 0.0, 0.0])),
        ]);
        let skull = fireball_from_nbt(&black, None).unwrap();
        assert_eq!(skull.kind, ProjectileKind::WitherSkull { dangerous: false });
        let back = fireball_to_nbt(&skull, [1, 2, 3, 4], None, None).unwrap();
        assert_eq!(number(back.get("dangerous")), Some(0));
    }

    #[test]
    fn a_fireball_round_trips_its_flight() {
        let raw = Tag::compound([
            ("id", Tag::String("minecraft:fireball".into())),
            ("Pos", doubles(&[1.0, 70.0, -2.5])),
            ("Motion", doubles(&[0.0, -0.5, 1.0])),
            ("ExplosionPower", Tag::Byte(4)),
            ("acceleration_power", Tag::Double(0.0f64.to_bits())),
        ]);
        let p = fireball_from_nbt(&raw, None).unwrap();
        assert_eq!(p.kind, ProjectileKind::LargeFireball { power: 4 });
        assert_eq!((p.velocity.z, p.acceleration), (1.0, 0.0));
        let back = fireball_to_nbt(&p, [1, 2, 3, 4], None, None).unwrap();
        assert_eq!(number(back.get("ExplosionPower")), Some(4));
        assert_eq!(float(back.get("acceleration_power")), Some(0.0));
        // Projectile.addAdditionalSaveData: no LeftOwner unless set, HasBeenShot always.
        assert!(back.get("LeftOwner").is_none());
        assert_eq!(number(back.get("HasBeenShot")), Some(0));
        assert_eq!(
            read_doubles::<3>(back.get("Motion")),
            Some([0.0, -0.5, 1.0])
        );
        // A default fireball keeps vanilla's power and acceleration.
        let plain = Tag::compound([
            ("id", Tag::String("minecraft:fireball".into())),
            ("Pos", doubles(&[0.0, 0.0, 0.0])),
        ]);
        let p = fireball_from_nbt(&plain, None).unwrap();
        assert_eq!(
            (p.kind, p.acceleration),
            (ProjectileKind::LargeFireball { power: 1 }, 0.1)
        );
        // `/summon` and a saved world give back both wind charges.
        for (id, kind) in [
            ("minecraft:wind_charge", ProjectileKind::WindCharge),
            (
                "minecraft:breeze_wind_charge",
                ProjectileKind::BreezeWindCharge,
            ),
        ] {
            let tag = Tag::compound([
                ("id", Tag::String(id.into())),
                ("Pos", doubles(&[0.0, 0.0, 0.0])),
            ]);
            assert_eq!(fireball_from_nbt(&tag, None).unwrap().kind, kind);
        }
    }

    /// `Fireball.addAdditionalSaveData` / `readAdditionalSaveData`: both fireballs save
    /// `Item`, a fire charge by default, and a loaded one stays, as one item.
    #[test]
    fn a_fireball_saves_its_item() {
        let item_of = |tag: &Tag| {
            let item = tag.get("Item").and_then(item_from_nbt).unwrap();
            (item.item.to_string(), item.count)
        };
        for id in ["minecraft:fireball", "minecraft:small_fireball"] {
            let plain = Tag::compound([
                ("id", Tag::String(id.into())),
                ("Pos", doubles(&[0.0, 0.0, 0.0])),
            ]);
            let p = fireball_from_nbt(&plain, None).unwrap();
            let back = fireball_to_nbt(&p, [1, 2, 3, 4], None, Some(&plain)).unwrap();
            assert_eq!(
                item_of(&back),
                ("minecraft:fire_charge".to_owned(), 1),
                "{id}"
            );
            // A loaded item stays, as a single one, on the entity itself.
            let mut loaded = plain.clone();
            put(
                &mut loaded,
                "Item",
                item_to_nbt(&crate::items::ItemStack::new("minecraft:diamond", 5)),
            );
            let p = fireball_from_nbt(&loaded, None).unwrap();
            let held = p.item.as_ref().unwrap();
            assert_eq!((&*held.item, held.count), ("minecraft:diamond", 1), "{id}");
            // `addAdditionalSaveData` writes `getItem()`: the item in the
            // compound it was loaded from does not replace the one it holds,
            // which is what `/data get` and the chunk save then report.
            let mut stale = plain.clone();
            put(
                &mut stale,
                "Item",
                item_to_nbt(&crate::items::ItemStack::new("minecraft:stone", 8)),
            );
            let back = fireball_to_nbt(&p, [1, 2, 3, 4], None, Some(&stale)).unwrap();
            assert_eq!(item_of(&back), ("minecraft:diamond".to_owned(), 1), "{id}");
            // An empty one falls back to the fire charge.
            let mut empty = plain.clone();
            put(
                &mut empty,
                "Item",
                item_to_nbt(&crate::items::ItemStack::new("minecraft:air", 1)),
            );
            let p = fireball_from_nbt(&empty, None).unwrap();
            let back = fireball_to_nbt(&p, [1, 2, 3, 4], None, Some(&empty)).unwrap();
            assert_eq!(
                item_of(&back),
                ("minecraft:fire_charge".to_owned(), 1),
                "{id}"
            );
        }
        // The wind charges are not `Fireball`s: they save no item.
        let wind = Tag::compound([
            ("id", Tag::String("minecraft:wind_charge".into())),
            ("Pos", doubles(&[0.0, 0.0, 0.0])),
        ]);
        let p = fireball_from_nbt(&wind, None).unwrap();
        let back = fireball_to_nbt(&p, [1, 2, 3, 4], None, Some(&wind)).unwrap();
        assert!(back.get("Item").is_none());
    }

    #[test]
    fn a_summoned_potion_flies_as_the_item_its_compound_names() {
        // `ThrowableItemProjectile.getDefaultItem`: the item its entity type is
        // named after, so a bare summon is a splash potion with no effects and
        // the chunk saves it (ThrowableProjectile is not transient).
        for id in ["minecraft:splash_potion", "minecraft:lingering_potion"] {
            let bare = Tag::compound([
                ("id", Tag::String(id.into())),
                ("Pos", doubles(&[24.5, 216.0, 24.5])),
            ]);
            let p = throwable_from_nbt(&bare, None).unwrap();
            assert_eq!(p.kind.id(), id);
            assert_eq!(
                p.kind,
                ProjectileKind::Potion(
                    Box::new(crate::items::ItemStack::new(id, 1)),
                    id == "minecraft:lingering_potion"
                )
            );
            assert!(saved_with_chunk(&p));
            let tag = throwable_to_nbt(&p, [1; 4], None, None).unwrap();
            let item = tag.get("Item").and_then(item_from_nbt).unwrap();
            assert_eq!((&*item.item, item.count), (id, 1));
            assert_eq!(item.potion(), None);
        }
        // `readAdditionalSaveData`: the `Item` it flies as, `potion_contents`
        // and all, so what it splashes is the potion's.
        let harming = Tag::compound([
            ("id", Tag::String("minecraft:splash_potion".into())),
            ("Pos", doubles(&[24.5, 216.0, 24.5])),
            (
                "Item",
                item_to_nbt(
                    &crate::items::ItemStack::new("minecraft:splash_potion", 1)
                        .with_potion("minecraft:harming"),
                ),
            ),
        ]);
        let p = throwable_from_nbt(&harming, None).unwrap();
        let ProjectileKind::Potion(stack, _) = &p.kind else {
            panic!("a potion");
        };
        assert_eq!(stack.potion(), Some("minecraft:harming"));
        // The stack stays in the tag it was loaded from.
        let tag = throwable_to_nbt(&p, [1; 4], None, Some(&harming)).unwrap();
        let item = tag.get("Item").and_then(item_from_nbt).unwrap();
        assert_eq!(item.potion(), Some("minecraft:harming"));
        // A lingering potion is its own entity type.
        let lingering = Tag::compound([
            ("id", Tag::String("minecraft:lingering_potion".into())),
            ("Pos", doubles(&[0.0, 64.0, 0.0])),
            (
                "Item",
                item_to_nbt(
                    &crate::items::ItemStack::new("minecraft:lingering_potion", 1)
                        .with_potion("minecraft:poison"),
                ),
            ),
        ]);
        let p = throwable_from_nbt(&lingering, None).unwrap();
        assert_eq!(p.kind.id(), "minecraft:lingering_potion");
    }

    #[test]
    fn wind_charges_in_flight_are_saved_and_read_back() {
        // vanilla 26.3 saves them (`Entity.shouldBeSaved` is true; the round
        // trip of a vanilla world lost all four before this).
        for id in ["minecraft:wind_charge", "minecraft:breeze_wind_charge"] {
            let saved = Tag::compound([
                ("id", Tag::String(id.into())),
                ("Pos", doubles(&[4.5, 80.0, 10.5])),
                ("NoGravity", Tag::Byte(1)),
                ("acceleration_power", Tag::Double(0.1_f64.to_bits())),
            ]);
            let p = fireball_from_nbt(&saved, None).unwrap();
            assert_eq!(p.kind.id(), id);
            assert!(saved_with_chunk(&p), "{id}");
            let tag = fireball_to_nbt(&p, [1; 4], None, Some(&saved)).unwrap();
            assert_eq!(kind(&tag).as_deref(), Some(id));
            assert_eq!(tag.get("NoGravity"), Some(&Tag::Byte(1)));
        }
    }

    #[test]
    fn a_lingering_potion_keeps_its_type_whatever_item_it_flies_as() {
        // `/summon minecraft:lingering_potion ~ ~ ~ {Item:{id:"minecraft:splash_potion"}}`
        // is a `ThrownLingeringPotion` in vanilla and stays one when saved.
        let tag = Tag::compound([
            ("id", Tag::String("minecraft:lingering_potion".into())),
            ("Pos", doubles(&[24.5, 80.0, 8.5])),
            (
                "Item",
                item_to_nbt(&crate::items::ItemStack::new("minecraft:splash_potion", 1)),
            ),
        ]);
        let p = throwable_from_nbt(&tag, None).unwrap();
        assert_eq!(p.kind.id(), "minecraft:lingering_potion");
        let back = throwable_to_nbt(&p, [1; 4], None, Some(&tag)).unwrap();
        assert_eq!(kind(&back).as_deref(), Some("minecraft:lingering_potion"));
        let item = back.get("Item").and_then(item_from_nbt).unwrap();
        assert_eq!(&*item.item, "minecraft:splash_potion");
    }

    #[test]
    fn a_painting_keeps_its_saved_pitch_and_takes_its_yaw_from_the_facing() {
        // Entity.load sets both angles; Painting.readAdditionalSaveData's
        // setDirection only rewrites the yaw. `/summon painting ...
        // {Rotation:[45.0f,10.0f]}` reads back `Rotation: [0.0f, 10.0f]`.
        let painting_at = |rotation: Option<Tag>| {
            let mut tag = Tag::compound([
                ("id", Tag::String("minecraft:painting".into())),
                ("Pos", doubles(&[28.5, 80.0, 6.5])),
                ("variant", Tag::String("minecraft:kebab".into())),
                ("facing", Tag::Byte(3)),
            ]);
            if let (Tag::Compound(map), Some(rotation)) = (&mut tag, rotation) {
                map.insert("Rotation".into(), rotation);
            }
            tag
        };
        let saved = |raw: &Tag| {
            let (pos, facing, variant) = painting_from_nbt(raw).unwrap();
            let painting = crate::entities::Painting {
                id: 1,
                pos,
                facing,
                variant: variant.unwrap().into(),
                width: 1,
                height: 1,
                check_interval: 0,
            };
            rotation(&painting_to_nbt(&painting, [1; 4], Some(raw)))
        };
        assert_eq!(
            saved(&painting_at(Some(floats(&[45.0, 10.0])))),
            [270.0, 10.0]
        );
        // setXRot: `xRot % 360` clamped to +-90.
        assert_eq!(
            saved(&painting_at(Some(floats(&[0.0, -400.0])))),
            [270.0, -40.0]
        );
        assert_eq!(
            saved(&painting_at(Some(floats(&[0.0, 135.0])))),
            [270.0, 90.0]
        );
        // A non-finite pitch is ignored, as is no Rotation at all.
        assert_eq!(
            saved(&painting_at(Some(floats(&[0.0, f32::NAN])))),
            [270.0, 0.0]
        );
        assert_eq!(saved(&painting_at(None)), [270.0, 0.0]);
    }

    #[test]
    fn a_summoned_painting_or_frame_has_no_block_pos_and_sits_at_its_position() {
        // BlockAttachedEntity.setPos: the block holding the summon position.
        let painting = Tag::compound([
            ("id", Tag::String("minecraft:painting".into())),
            ("Pos", doubles(&[28.5, 80.0, 6.5])),
            ("variant", Tag::String("minecraft:kebab".into())),
            ("facing", Tag::Byte(3)),
        ]);
        assert_eq!(
            painting_from_nbt(&painting),
            Some(([28, 80, 6], 3, Some("minecraft:kebab".to_string())))
        );
        // No variant saved (a bare summon): the position and facing still load.
        let mut bare = painting.clone();
        if let Tag::Compound(m) = &mut bare {
            m.remove(&"variant".into());
        }
        assert_eq!(painting_from_nbt(&bare), Some(([28, 80, 6], 3, None)));
        let frame = Tag::compound([
            ("id", Tag::String("minecraft:item_frame".into())),
            ("Pos", doubles(&[-0.5, 80.0, 6.5])),
            ("Facing", Tag::Byte(1)),
        ]);
        assert_eq!(item_frame_from_nbt(&frame, 1).unwrap().pos, [-1, 80, 6]);
    }

    #[test]
    fn frame_missing_facing_loads_down_and_valid_directions_round_trip() {
        // ItemFrame.readAdditionalSaveData: LEGACY_ID_CODEC, orElse(DOWN).
        for kind in ["minecraft:item_frame", "minecraft:glow_item_frame"] {
            // Explicit controls run before the missing-field assertion.
            for facing in [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), None] {
                let mut tag = Tag::compound([
                    ("id", string(kind)),
                    ("block_pos", Tag::IntArray(vec![4, 102, 6])),
                    ("Fixed", Tag::Byte(1)),
                    ("ItemRotation", Tag::Byte(5)),
                    ("ItemDropChance", Tag::Float(0.25_f32.to_bits())),
                    (
                        "Item",
                        item_to_nbt(&crate::items::ItemStack::new("minecraft:diamond", 1)),
                    ),
                ]);
                if let Some(direction) = facing {
                    put(&mut tag, "Facing", Tag::Byte(direction));
                }
                let frame = item_frame_from_nbt(&tag, 17).unwrap();
                assert_eq!(frame.kind(), kind);
                assert_eq!(frame.id, 17);
                assert!(frame.fixed);
                assert_eq!(
                    frame.item.as_ref().unwrap().item.as_ref(),
                    "minecraft:diamond"
                );
                for raw in [None, Some(&tag)] {
                    let saved = item_frame_to_nbt(&frame, [1, 2, 3, 4], raw);
                    assert_eq!(
                        saved.get("Facing"),
                        Some(&Tag::Byte(facing.unwrap_or(0))),
                        "{kind}"
                    );
                    assert_eq!(saved.get("Item"), tag.get("Item"));
                    assert_eq!(saved.get("ItemRotation"), Some(&Tag::Byte(5)));
                    assert_eq!(saved.get("ItemDropChance"), tag.get("ItemDropChance"));
                    assert_eq!(saved.get("Fixed"), Some(&Tag::Byte(1)));
                    assert_eq!(saved.get("block_pos"), tag.get("block_pos"));
                    if facing.is_none() {
                        assert_eq!(saved.get("Pos"), Some(&doubles(&[4.5, 102.96875, 6.5])));
                    }
                }
            }
        }
    }

    #[test]
    fn frame_rotation_uses_signed_byte_value_and_round_trips() {
        // `ItemFrame.readAdditionalSaveData`: getByteOr, then setRotation's `% 8`.
        for kind in ["minecraft:item_frame", "minecraft:glow_item_frame"] {
            for (rotation, expected) in [
                (Some(Tag::Byte(-1)), -1),
                (Some(Tag::Byte(-9)), -1),
                (Some(Tag::Byte(-128)), 0),
                (Some(Tag::Byte(0)), 0),
                (Some(Tag::Byte(7)), 7),
                (Some(Tag::Byte(8)), 0),
                (Some(Tag::Byte(127)), 7),
                (Some(Tag::Short(255)), -1),
                (Some(Tag::Int(129)), -7),
                (Some(Tag::Int(-129)), 7),
                (Some(Tag::Long(255)), -1),
                (Some(Tag::Float((-0.5_f32).to_bits())), -1),
                (Some(Tag::Double((-9.5_f64).to_bits())), -2),
                (Some(string("-1")), 0),
                (None, 0),
            ] {
                let mut tag = Tag::compound([
                    ("id", string(kind)),
                    ("block_pos", Tag::IntArray(vec![4, 102, 6])),
                ]);
                if let Some(rotation) = rotation {
                    put(&mut tag, "ItemRotation", rotation);
                }
                let frame = item_frame_from_nbt(&tag, 17).unwrap();
                assert_eq!(frame.rotation, expected, "{kind}: {tag:?}");
                for raw in [None, Some(&tag)] {
                    let saved = item_frame_to_nbt(&frame, [1, 2, 3, 4], raw);
                    assert_eq!(saved.get("ItemRotation"), Some(&Tag::Byte(expected)));
                    assert_eq!(item_frame_from_nbt(&saved, 17).unwrap().rotation, expected);
                }
            }
        }
    }

    #[test]
    fn a_named_frame_drops_with_its_custom_name() {
        // `ItemFrame.getFrameItemStackWithData`: the frame item, then
        // `DataComponents.CUSTOM_NAME` from `getCustomName`. Glow frames
        // share it through `GlowItemFrame.getFrameItemStack`.
        for id in ["minecraft:item_frame", "minecraft:glow_item_frame"] {
            let tag = Tag::compound([
                ("id", string(id)),
                ("Pos", doubles(&[0.5, 64.0, 0.5])),
                ("Facing", Tag::Byte(2)),
                ("CustomName", string("Gallery")),
            ]);
            let frame = item_frame_from_nbt(&tag, 1).unwrap();
            let stack = frame.item_stack_with_data();
            assert_eq!(&*stack.item, id);
            assert_eq!(stack.count, 1);
            assert_eq!(stack.custom_name().as_deref(), Some("Gallery"));
            let saved = item_frame_to_nbt(&frame, [1, 2, 3, 4], None);
            assert_eq!(text(saved.get("CustomName")).as_deref(), Some("Gallery"));
        }
        let bare = Tag::compound([
            ("id", string("minecraft:item_frame")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
            ("Facing", Tag::Byte(3)),
        ]);
        let frame = item_frame_from_nbt(&bare, 2).unwrap();
        assert!(frame.custom_name.is_none());
        assert!(frame.item_stack_with_data().custom_name().is_none());
        assert!(
            item_frame_to_nbt(&frame, [1, 2, 3, 4], Some(&bare))
                .get("CustomName")
                .is_none()
        );
    }

    #[test]
    fn projectile_shot_flag_round_trips_independently_of_age() {
        for id in [
            "minecraft:snowball",
            "minecraft:fireball",
            "minecraft:llama_spit",
            "minecraft:shulker_bullet",
        ] {
            for shot in [0, 1] {
                let raw = Tag::compound([
                    ("id", Tag::String(id.into())),
                    ("Pos", doubles(&[1.0, 70.0, 2.0])),
                    ("HasBeenShot", Tag::Byte(shot)),
                ]);
                let mut p = throwable_from_nbt(&raw, None)
                    .or_else(|| fireball_from_nbt(&raw, None))
                    .or_else(|| spit_or_bullet_from_nbt(&raw, None, None))
                    .unwrap();
                assert_eq!(p.has_been_shot, shot != 0);
                for age in [0, 12] {
                    p.age = age;
                    let tag = throwable_to_nbt(&p, [1, 2, 3, 4], None, Some(&raw))
                        .or_else(|| fireball_to_nbt(&p, [1, 2, 3, 4], None, Some(&raw)))
                        .or_else(|| spit_or_bullet_to_nbt(&p, [1, 2, 3, 4], None, None, Some(&raw)))
                        .unwrap();
                    assert_eq!(number(tag.get("HasBeenShot")), Some(i64::from(shot)));
                }
            }
        }
    }

    #[test]
    fn thrown_item_projectiles_round_trip_their_flight_item_and_owner() {
        let player = Some([5, 6, 7, 8]);
        for (kind, id) in [
            (ProjectileKind::Snowball, "minecraft:snowball"),
            (ProjectileKind::Egg, "minecraft:egg"),
            (ProjectileKind::EnderPearl, "minecraft:ender_pearl"),
        ] {
            let mut p =
                Projectile::new(kind.clone(), Vec3::new(1.5, 70.0, -2.5), Some(Who::Player));
            (p.velocity, p.left_owner, p.age) = (Vec3::new(0.3, -0.1, 1.2), true, 4);
            p.has_been_shot = true;
            [p.yaw, p.pitch] = [30.0, -10.0];
            let tag = throwable_to_nbt(&p, [1, 2, 3, 4], player, None).unwrap();
            assert_eq!(super::kind(&tag).as_deref(), Some(id));
            // ThrowableItemProjectile: the item it flies as, by its default.
            let item = tag.get("Item").and_then(item_from_nbt).unwrap();
            assert_eq!((&*item.item, item.count), (id, 1));
            assert_eq!(number(tag.get("HasBeenShot")), Some(1));
            assert_eq!(uuid_from_tag(tag.get("Owner")), player);
            let back = throwable_from_nbt(&tag, player).unwrap();
            assert_eq!((back.kind, back.owner), (kind, Some(Who::Player)));
            assert_eq!((back.position, back.velocity), (p.position, p.velocity));
            assert_eq!(
                ([back.yaw, back.pitch], back.left_owner),
                ([30.0, -10.0], true)
            );
            // an owner who is not here keeps their UUID through a moved resave.
            let mut away = throwable_from_nbt(&tag, Some([9; 4])).unwrap();
            assert_eq!((away.owner, away.owner_uuid), (None, player));
            away.position.y += 3.0;
            let resaved = throwable_to_nbt(&away, [1; 4], away.owner_uuid, None).unwrap();
            assert_eq!(uuid_from_tag(resaved.get("Owner")), player);
        }
        // the chunk saves arrows, fireballs, wind charges and these, and so takes them with it.
        let kinds = [
            (ProjectileKind::SmallFireball, true),
            (ProjectileKind::Snowball, true),
            (ProjectileKind::WindCharge, true),
            (ProjectileKind::BreezeWindCharge, true),
            (ProjectileKind::Trident, false),
        ];
        for (kind, saved) in kinds {
            let p = Projectile::new(kind, Vec3::ZERO, None);
            assert_eq!(saved_with_chunk(&p), saved, "{:?}", p.kind);
        }
        // a loaded item stays, and a new projectile leaves LeftOwner out.
        let raw = Tag::compound([
            ("id", Tag::String("minecraft:egg".into())),
            ("Pos", doubles(&[0.0, 64.0, 0.0])),
            (
                "Item",
                item_to_nbt(&crate::items::ItemStack::new("minecraft:blue_egg", 1)),
            ),
        ]);
        let egg = throwable_from_nbt(&raw, None).unwrap();
        let tag = throwable_to_nbt(&egg, [1; 4], None, Some(&raw)).unwrap();
        let item = tag.get("Item").and_then(item_from_nbt).unwrap();
        assert_eq!(&*item.item, "minecraft:blue_egg");
        assert!(tag.get("LeftOwner").is_none() && tag.get("Owner").is_none());
        assert_eq!(number(tag.get("HasBeenShot")), Some(0));
        let wind = Projectile::new(ProjectileKind::WindCharge, Vec3::ZERO, None);
        assert!(throwable_to_nbt(&wind, [1; 4], None, None).is_none());
        // NoGravity is loaded and saved again (a summoned pearl that floats).
        let floating = Tag::compound([
            ("id", Tag::String("minecraft:ender_pearl".into())),
            ("Pos", doubles(&[0.0, 64.0, 0.0])),
            ("NoGravity", Tag::Byte(1)),
        ]);
        let pearl = throwable_from_nbt(&floating, None).unwrap();
        assert!(pearl.no_gravity);
        let tag = throwable_to_nbt(&pearl, [1; 4], None, None).unwrap();
        assert_eq!(number(tag.get("NoGravity")), Some(1));
        assert!(
            throwable_to_nbt(&egg, [1; 4], None, None)
                .unwrap()
                .get("NoGravity")
                .is_none()
        );
    }
}

#[cfg(test)]
mod neutral_anger_tests {
    use super::*;

    /// #2784: a neutral mob's anger survives the save: the anger ends at an
    /// absolute game time and the target comes back with it
    /// (`NeutralMob.addPersistentAngerSaveData` / `readPersistentAngerSaveData`).
    #[test]
    fn a_neutral_mobs_anger_survives_the_save() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<String, Arc<MobType>> = data
            .types
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let mut m = Mob::new(
            1,
            types["minecraft:wolf"].clone(),
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
        );
        m.extra.anger = 400;
        m.extra.anger_end = 3000;
        m.extra.angry_at = Some(crate::mobs::Who::Player);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(number(tag.get("anger_end_time")), Some(3000));
        assert!(tag.get("angry_at").is_some());
        let back = mob_from_nbt(&tag, &types, 2, None).unwrap();
        assert_eq!(back.extra.anger_end, 3000);
        assert_eq!(back.extra.angry_at, Some(crate::mobs::Who::Player));
        // An iron golem angry at a monster: the target's UUID comes back as
        // anger at a mob (not at every player under universal_anger), and is
        // written back as it was read.
        let mut golem = Mob::new(4, types["minecraft:iron_golem"].clone(), Vec3::ZERO, 0.0);
        golem.extra.anger = 400;
        golem.extra.anger_end = 3000;
        let mut raw = mob_to_nbt(&golem, [1, 2, 3, 6], None, None, |_| None);
        if let Tag::Compound(map) = &mut raw {
            map.insert("angry_at".into(), Tag::IntArray(vec![9, 8, 7, 6]));
        }
        let back = mob_from_nbt(&raw, &types, 5, None).unwrap();
        assert_eq!(back.extra.angry_at, Some(crate::mobs::Who::Mob(0)));
        let again = mob_to_nbt(&back, [1, 2, 3, 6], Some(&raw), None, |_| None);
        assert_eq!(
            crate::save::uuid_from_tag(again.get("angry_at")),
            Some([9, 8, 7, 6])
        );
        // Once the anger is over (`stopBeingAngry`), the loaded target goes.
        let mut calm = back.clone();
        calm.extra.angry_at = None;
        let again = mob_to_nbt(&calm, [1, 2, 3, 6], Some(&raw), None, |_| None);
        assert!(again.get("angry_at").is_none());
        // A mob that is no NeutralMob writes no anger.
        let pig = Mob::new(3, types["minecraft:pig"].clone(), Vec3::ZERO, 0.0);
        let tag = mob_to_nbt(&pig, [1, 2, 3, 5], None, None, |_| None);
        assert!(tag.get("anger_end_time").is_none());
        // The anger that is left when the mob comes back.
        assert_eq!(crate::mobs::anger_left(3000, 1000), 2000);
        assert_eq!(crate::mobs::anger_left(3000, 4000), 0);
    }
}

#[cfg(test)]
mod uuid_bridge_tests {
    use super::*;
    use crate::mobs::{Species, sensor_scan_tests::kind};

    /// A wolf, whose anger is `NeutralMob`'s, so its save carries `angry_at`.
    /// No resources are needed: only the species decides that.
    fn wolf(id: u32) -> Mob {
        let mut kind = (*kind()).clone();
        kind.id = "minecraft:wolf".into();
        kind.species = Species::Wolf;
        let mut m = Mob::new(id, Arc::new(kind), Vec3::ZERO, 0.0);
        m.extra.anger = 400;
        m.extra.anger_end = 3000;
        m
    }

    /// A mob's anger at another mob is saved as that mob's UUID, which the game
    /// layer answers for (`Who` itself carries none).
    #[test]
    fn a_mob_anger_target_is_saved_as_its_uuid() {
        let mut m = wolf(1);
        m.extra.angry_at = Some(Who::Mob(7));
        let target = [7, 0, 0, 3];
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |id| {
            (id == 7).then_some(target)
        });
        assert_eq!(tag.get("angry_at"), Some(&Tag::IntArray(target.to_vec())));
        assert_eq!(number(tag.get("anger_end_time")), Some(3000));
    }

    /// A target the map does not know (a mob that left the level) is not named:
    /// nothing is written rather than a made-up UUID no entity answers for.
    #[test]
    fn an_unknown_anger_target_writes_no_angry_at() {
        let mut m = wolf(1);
        m.extra.angry_at = Some(Who::Mob(7));
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(tag.get("angry_at"), None);
        assert_eq!(number(tag.get("anger_end_time")), Some(3000));
    }

    /// The player is not an id: its UUID needs no map from the caller.
    #[test]
    fn a_player_anger_target_needs_no_map() {
        let mut m = wolf(1);
        m.extra.angry_at = Some(Who::Player);
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        assert_eq!(
            tag.get("angry_at"),
            Some(&Tag::IntArray(player_uuid().to_vec()))
        );
    }

    /// The round trip both deferred fixes need: a mob's UUID written through the
    /// save seam reads back as that mob, not as another one.
    #[test]
    fn a_saved_mob_uuid_resolves_back_to_it() {
        let mut wolf = wolf(1);
        let pig = {
            let mut kind = (*kind()).clone();
            kind.id = "minecraft:pig".into();
            kind.species = Species::Pig;
            Mob::new(9, Arc::new(kind), Vec3::ZERO, 0.0)
        };
        let (wolf_uuid, pig_uuid) = ([1, 2, 3, 4], [9, 0, 0, 1]);
        let uuids: HashMap<u32, [i32; 4]> = HashMap::from([(1, wolf_uuid), (9, pig_uuid)]);
        wolf.extra.angry_at = Some(Who::Mob(pig.id));
        let tag = mob_to_nbt(&wolf, wolf_uuid, None, None, |id| uuids.get(&id).copied());
        let Some(Tag::IntArray(stored)) = tag.get("angry_at") else {
            panic!("anger at a mob is saved as that mob's UUID");
        };
        assert_eq!(stored, &pig_uuid.to_vec());
        let mobs = [wolf, pig];
        assert_eq!(
            who_by_uuid(&uuids, &mobs, None, pig_uuid),
            Some(Who::Mob(9))
        );
        // The other loaded mob is not mistaken for it.
        assert_eq!(
            who_by_uuid(&uuids, &mobs, None, wolf_uuid),
            Some(Who::Mob(1))
        );
        // The player is the other half of `Who`, kept apart from the id map.
        let player = [5, 5, 5, 5];
        assert_eq!(
            who_by_uuid(&uuids, &mobs, Some(player), player),
            Some(Who::Player)
        );
    }

    /// A UUID nothing loaded has is no entity at all: not another mob, and not
    /// the projectile whose id shares the map.
    #[test]
    fn a_uuid_no_loaded_mob_has_resolves_to_nothing() {
        let wolf = wolf(1);
        let uuids: HashMap<u32, [i32; 4]> =
            HashMap::from([(1, [1, 2, 3, 4]), (1000, [5, 0, 0, 1])]);
        let mobs = [wolf];
        // Never loaded (it left, or the save is another world's).
        assert_eq!(who_by_uuid(&uuids, &mobs, None, [9, 9, 9, 9]), None);
        // Loaded, but not a mob: `Who::Mob` would mean an arrow's id.
        assert_eq!(who_by_uuid(&uuids, &mobs, None, [5, 0, 0, 1]), None);
        // Nothing loaded at all resolves to nothing either.
        assert_eq!(who_by_uuid(&uuids, &[], None, [1, 2, 3, 4]), None);
        assert_eq!(
            who_by_uuid(&uuids, &mobs, None, [1, 2, 3, 4]),
            Some(Who::Mob(1))
        );
    }
}

#[cfg(test)]
mod warden_anger_tests {
    use super::*;
    use crate::mobs::{Species, sensor_scan_tests::kind, warden::AngerManagement};
    use std::cell::Cell;

    /// A warden, with everything its save reads. No resources are needed: only
    /// the species decides which fields it has.
    fn warden(id: u32) -> Mob {
        let mut kind = (*kind()).clone();
        kind.id = "minecraft:warden".into();
        kind.species = Species::Warden;
        Mob::new(id, Arc::new(kind), Vec3::new(0.0, 64.0, 0.0), 0.0)
    }

    fn types() -> HashMap<String, Arc<MobType>> {
        let mut warden = (*kind()).clone();
        warden.id = "minecraft:warden".into();
        warden.species = Species::Warden;
        HashMap::from([("minecraft:warden".to_string(), Arc::new(warden))])
    }

    /// `Warden.addAdditionalSaveData` stores the anger as `AngerManagement`'s
    /// codec does - one `{uuid, anger}` pair per suspect, the ones in the level
    /// by each entity's UUID and then the ones that are away - and the per
    /// suspect anger comes back with the mob.
    #[test]
    fn a_wardens_anger_round_trips_as_suspect_uuid_pairs() {
        let mut m = warden(1);
        let away = [4, 0, 0, 42];
        let mut anger = AngerManagement::from_saved([(away, 12)]);
        anger.increase(Who::Player, 95);
        anger.increase(Who::Mob(7), 41);
        m.extra.warden.as_mut().unwrap().anger = anger;
        let pig_uuid = [0, 0, 0, 7];
        let tag = mob_to_nbt(&m, [9, 9, 9, 9], None, None, |id| {
            (id == 7).then_some(pig_uuid)
        });
        // A warden is no `NeutralMob`: neither of its keys is written.
        assert!(tag.get("angry_at").is_none());
        assert!(tag.get("anger_end_time").is_none());
        let stored = tag.get("anger").and_then(|t| t.get("suspects"));
        let Some(Tag::List(pairs)) = stored else {
            panic!("the anger is saved as a suspects list");
        };
        let pair = |t: &Tag| {
            (
                uuid_from_tag(t.get("uuid")).unwrap(),
                number(t.get("anger")),
            )
        };
        assert_eq!(
            pairs.iter().map(pair).collect::<Vec<_>>(),
            // The angry suspect first (`AngerManagement.Sorter`), then the
            // other one in the level, then the one that is away.
            vec![
                (player_uuid(), Some(95)),
                (pig_uuid, Some(41)),
                (away, Some(12)),
            ]
        );

        // Loading puts every suspect in the UUID map: a warden may load long
        // before the mobs it is angry at do.
        let back = mob_from_nbt(&tag, &types(), 2, None).unwrap();
        let st = back.extra.warden.as_ref().unwrap();
        assert!(st.anger.suspects().is_empty());
        assert_eq!(
            st.anger.unloaded().collect::<Vec<_>>(),
            vec![(player_uuid(), 95), (pig_uuid, 41), (away, 12)]
        );
    }

    /// Anger at a suspect that is still away outlives the session: it decays
    /// while nothing answers for its UUID, and the next save writes the pair
    /// again, so a warden's anger at a mob in an unloaded chunk is not lost by
    /// saving and loading twice.
    #[test]
    fn an_away_suspect_survives_another_save() {
        let away = [4, 0, 0, 42];
        let mut m = warden(1);
        m.extra.warden.as_mut().unwrap().anger = AngerManagement::from_saved([(away, 12)]);
        let tag = mob_to_nbt(&m, [9, 9, 9, 9], None, None, |_| None);
        let mut back = mob_from_nbt(&tag, &types(), 2, None).unwrap();
        let st = back.extra.warden.as_mut().unwrap();
        for _ in 0..4 {
            st.anger.tick(|_| true, |_| None);
        }
        let saved = mob_to_nbt(&back, [9, 9, 9, 9], None, None, |_| None);
        let Some(Tag::List(pairs)) = saved.get("anger").and_then(|t| t.get("suspects")) else {
            panic!("the away suspect is still saved");
        };
        assert_eq!(pairs.len(), 1);
        assert_eq!(uuid_from_tag(pairs[0].get("uuid")), Some(away));
        assert_eq!(number(pairs[0].get("anger")), Some(8));
    }

    /// A warden with no suspect writes the codec's default: an `anger` compound
    /// with no `suspects` key, which is what vanilla's `lenientOptionalFieldOf`
    /// leaves out.
    #[test]
    fn a_warden_with_no_anger_writes_no_suspects() {
        let m = warden(1);
        let tag = mob_to_nbt(&m, [9, 9, 9, 9], None, None, |_| None);
        assert_eq!(
            tag.get("anger"),
            Some(&Tag::compound(Vec::<(&str, Tag)>::new()))
        );
        let back = mob_from_nbt(&tag, &types(), 2, None).unwrap();
        let st = back.extra.warden.as_ref().unwrap();
        assert_eq!(st.anger, AngerManagement::default());
    }

    /// A suspect whose anger is below zero, or whose UUID is not four ints,
    /// does not decode: the whole list is dropped, as
    /// `lenientOptionalFieldOf` does with a value that fails.
    #[test]
    fn a_suspects_list_that_does_not_decode_is_ignored() {
        let pair =
            |uuid: Tag, anger: i32| Tag::compound([("uuid", uuid), ("anger", Tag::Int(anger))]);
        let anger = |pairs: Vec<Tag>| Tag::compound([("suspects", Tag::List(pairs))]);
        let four = || Tag::IntArray(vec![1, 2, 3, 4]);
        assert_eq!(
            warden_anger_from_nbt(&anger(vec![pair(four(), 30)])),
            vec![([1, 2, 3, 4], 30)]
        );
        // `ExtraCodecs.NON_NEGATIVE_INT` refuses a negative anger.
        assert!(warden_anger_from_nbt(&anger(vec![pair(four(), -1)])).is_empty());
        // `UUIDUtil.CODEC` needs exactly four ints.
        assert!(
            warden_anger_from_nbt(&anger(vec![pair(Tag::IntArray(vec![1, 2, 3]), 30)])).is_empty()
        );
        // One bad pair takes the good ones with it.
        assert!(warden_anger_from_nbt(&anger(vec![pair(four(), 30), pair(four(), -7)])).is_empty());
        // No list at all (and an `anger` that is no compound) is no suspect.
        assert!(warden_anger_from_nbt(&Tag::compound(Vec::<(&str, Tag)>::new())).is_empty());
        assert!(warden_anger_from_nbt(&Tag::Int(1)).is_empty());
    }

    /// An away suspect calms down by one an anger tick, like the ones in the
    /// level, and is forgotten at one point - and its UUID keeps the anger until
    /// it does.
    #[test]
    fn an_away_suspect_calms_down_one_a_tick() {
        let mut a = AngerManagement::from_saved([([1, 0, 0, 1], 30), ([2, 0, 0, 2], 1)]);
        for _ in 0..5 {
            a.tick(|_| true, |_| None);
        }
        // Five ticks, five points: the one at one point was dropped on the first.
        assert_eq!(a.unloaded().collect::<Vec<_>>(), vec![([1, 0, 0, 1], 25)]);
        for _ in 0..24 {
            a.tick(|_| true, |_| None);
        }
        assert_eq!(a.unloaded().collect::<Vec<_>>(), vec![([1, 0, 0, 1], 1)]);
        a.tick(|_| true, |_| None);
        assert!(a.unloaded().next().is_none());
        // Nothing in the level sees that anger while it is away.
        assert!(a.suspects().is_empty());
        assert_eq!(a.active_anger(None), 0);
    }

    /// The reconversion: an away suspect's UUID becomes a live `Who` once the
    /// mob is loaded, on the first anger tick that is due a lookup
    /// (`conversionDelay` counts two of them).
    #[test]
    fn an_away_suspect_rejoins_once_its_mob_is_loaded() {
        let uuid = [7, 0, 0, 3];
        let mut a = AngerManagement::from_saved([(uuid, 60)]);
        // The mob is not in the level for the first lookups.
        let loaded = Cell::new(false);
        let by_uuid = |u: [i32; 4]| (loaded.get() && u == uuid).then_some(Who::Mob(3));
        // A fresh anger converts on its first tick, then every second one.
        a.tick(|_| true, by_uuid);
        assert_eq!(a.unloaded().collect::<Vec<_>>(), vec![(uuid, 59)]);
        assert!(a.suspects().is_empty());
        // Now the mob loads. This tick is no lookup one: nothing happens yet.
        loaded.set(true);
        a.tick(|_| true, by_uuid);
        assert_eq!(a.unloaded().collect::<Vec<_>>(), vec![(uuid, 58)]);
        assert!(a.suspects().is_empty());
        // The lookup is due: it rejoins with the anger it kept.
        a.tick(|_| true, by_uuid);
        assert!(a.unloaded().next().is_none());
        assert_eq!(a.suspects(), &[Who::Mob(3)]);
        // 60, less one for each of the three ticks: the two it was away for and
        // the one it rejoined on, which calms it like any suspect in the level.
        assert_eq!(a.anger_of(Who::Mob(3)), 57);
        assert_eq!(a.active_anger(None), 57);
    }

    /// Anger gained at a suspect that has not rejoined yet: vanilla's
    /// `increaseAnger` takes the anger kept under its UUID into the new total
    /// (min(150, 35) + 20 = 55), so the lookup adds it rather than dropping it or
    /// letting it overwrite the new points; one anger tick then calms it to 54.
    #[test]
    fn a_suspect_that_has_not_rejoined_keeps_both_angers() {
        let mut a = AngerManagement::from_saved([([3, 0, 0, 3], 20)]);
        assert_eq!(a.increase(Who::Mob(3), 35), 35);
        a.tick(|_| true, |_| Some(Who::Mob(3)));
        assert_eq!(a.anger_of(Who::Mob(3)), 54);
        assert!(a.unloaded().next().is_none());
    }
}

#[cfg(test)]
mod held_item_name_tests {
    use super::*;

    #[test]
    fn a_worn_piece_keeps_its_damage_and_name_through_the_save() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<_, _> = data
            .types
            .into_iter()
            .map(|(id, kind)| (id, Arc::new(kind)))
            .collect();
        let mut mob = Mob::new(1, Arc::clone(&types["minecraft:zombie"]), Vec3::ZERO, 0.0);
        let mut helmet = crate::items::ItemStack::new("minecraft:iron_helmet", 1);
        helmet.damage = 40;
        helmet.data_mut().custom_name = Some(horizon_world::nbt::Tag::String("Hat".into()));
        crate::mobs::finalize::set_item_stack(&mut mob, crate::mobs::finalize::HEAD, helmet);
        let raw = mob_to_nbt(&mob, [1, 2, 3, 4], None, None, |_| None);
        let loaded = mob_from_nbt(&raw, &types, 2, None).unwrap();
        let worn = crate::mobs::finalize::item_stack(&loaded, crate::mobs::finalize::HEAD).unwrap();
        assert_eq!(worn.damage, 40);
        assert_eq!(worn.custom_name().as_deref(), Some("Hat"));
        // Replacing the item forgets the old stack.
        let mut loaded = loaded;
        crate::mobs::finalize::set_item(
            &mut loaded,
            crate::mobs::finalize::HEAD,
            "minecraft:iron_helmet",
        );
        let fresh =
            crate::mobs::finalize::item_stack(&loaded, crate::mobs::finalize::HEAD).unwrap();
        assert_eq!(fresh.damage, 0);
        assert!(fresh.custom_name().is_none());
    }

    #[test]
    fn replacing_a_named_item_does_not_restore_its_old_name_on_save() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let data = crate::mobs::MobTypes::load(&root).unwrap();
        let types: HashMap<_, _> = data
            .types
            .into_iter()
            .map(|(id, kind)| (id, Arc::new(kind)))
            .collect();
        let mut mob = Mob::new(1, Arc::clone(&types["minecraft:zombie"]), Vec3::ZERO, 0.0);
        let mut sword = crate::items::ItemStack::new("minecraft:diamond_sword", 1);
        sword.data_mut().custom_name = Some(horizon_world::nbt::Tag::String("Old Sword".into()));
        mob.extra.set_main_hand_stack(Some(sword));
        let raw = mob_to_nbt(&mob, [1, 2, 3, 4], None, None, |_| None);
        let mut loaded = mob_from_nbt(&raw, &types, 2, None).unwrap();
        assert_eq!(
            loaded
                .extra
                .main_hand_stack()
                .unwrap()
                .custom_name()
                .as_deref(),
            Some("Old Sword")
        );
        loaded
            .extra
            .set_main_hand(Some("minecraft:diamond_sword".into()));
        let saved = mob_to_nbt(&loaded, [1, 2, 3, 4], Some(&raw), None, |_| None);
        let restored = mob_from_nbt(&saved, &types, 3, None).unwrap();
        assert_eq!(
            restored.extra.main_hand.as_deref(),
            Some("minecraft:diamond_sword")
        );
        assert!(
            restored
                .extra
                .main_hand_stack()
                .unwrap()
                .custom_name()
                .is_none()
        );
    }
}

#[cfg(test)]
mod shulker_bullet_nbt_tests {
    use super::*;

    /// The level's side of a bullet's `Target`: the id -> UUID map
    /// (`EntityMemory.uuids`), the mobs it currently holds, and the player.
    /// `save_who_uuid` and `bullet_target` are the game layer's two helpers,
    /// kept here as the same two calls so both halves of the bridge are what the
    /// round trips below go through.
    struct Level {
        uuids: HashMap<u32, [i32; 4]>,
        mobs: Vec<Mob>,
        player: Option<[i32; 4]>,
    }
    impl Level {
        /// `Game::save_who_uuid`.
        fn save_who_uuid(&self, who: Who) -> Option<[i32; 4]> {
            who_uuid(who, &mut |id| self.uuids.get(&id).copied())
        }
        /// `Game::bullet_target`: the `Who` a saved `Target` names, while that
        /// entity is in the level.
        fn bullet_target(&self, tag: &Tag) -> Option<Who> {
            who_by_uuid(&self.uuids, &self.mobs, self.player, bullet_target(tag)?)
        }
        /// A shulker in the level: its id and the UUID it is saved with.
        fn with_mob(mut self, id: u32, uuid: [i32; 4]) -> Self {
            self.uuids.insert(id, uuid);
            self.mobs.push(Mob::new(
                id,
                crate::mobs::sensor_scan_tests::kind(),
                Vec3::ZERO,
                0.0,
            ));
            self
        }
    }
    fn no_entities() -> Level {
        Level {
            uuids: HashMap::new(),
            mobs: Vec::new(),
            player: None,
        }
    }
    /// The bullet the tests start from: fired by the player at mob 31 along +X.
    fn homing(target: Option<Who>) -> Projectile {
        Projectile::new(
            ProjectileKind::ShulkerBullet {
                target,
                dir: Some(5),
                steps: 23,
                delta: [0.15, -0.15, 0.075],
                seed: 7,
            },
            Vec3::new(4.5, 80.0, 10.5),
            Some(Who::Player),
        )
    }
    fn bullet_kind(p: &Projectile) -> (Option<Who>, Option<u8>, i32, [f64; 3]) {
        let ProjectileKind::ShulkerBullet {
            target,
            dir,
            steps,
            delta,
            ..
        } = p.kind
        else {
            panic!("not a bullet: {:?}", p.kind);
        };
        (target, dir, steps, delta)
    }

    /// `ShulkerBullet.addAdditionalSaveData` then `readAdditionalSaveData`, both
    /// over the UUID bridge: a bullet in flight keeps the target it homes on, the
    /// step it is on, the steps left and its per-axis target deltas, and the
    /// target comes back as the `Who::Mob` of the shulker it was fired at.
    ///
    /// Horizon wrote none of it and `saved_with_chunk` turned the kind away while
    /// it had a target, so every bullet a shulker fired was dropped by the chunk
    /// save.
    #[test]
    fn a_homing_bullet_round_trips_its_target_step_and_deltas() {
        let shulker = [9, 8, 7, 6];
        let level = no_entities().with_mob(31, shulker);
        let p = homing(Some(Who::Mob(31)));
        assert!(saved_with_chunk(&p), "a homing bullet is saved now");
        // The writer `Game::save_projectile` calls, given the UUID
        // `Game::save_who_uuid` answers for the target.
        let target = level.save_who_uuid(Who::Mob(31));
        assert_eq!(target, Some(shulker));
        let tag = spit_or_bullet_to_nbt(&p, [1; 4], None, target, None).unwrap();
        assert_eq!(kind(&tag).as_deref(), Some("minecraft:shulker_bullet"));
        assert_eq!(
            bullet_target(&tag),
            Some(shulker),
            "`Target` is the shulker's UUID"
        );
        assert_eq!(number(tag.get("Dir")), Some(5));
        assert_eq!(number(tag.get("Steps")), Some(23));
        assert_eq!(float(tag.get("TXD")), Some(0.15));
        assert_eq!(float(tag.get("TYD")), Some(-0.15));
        assert_eq!(float(tag.get("TZD")), Some(0.075));
        // Read back the way `Game::load_spit_or_bullet` does: the compound's
        // `Target` resolved against the level, then the bullet's own fields.
        let target = level.bullet_target(&tag);
        assert_eq!(target, Some(Who::Mob(31)), "back on the same shulker");
        let p = spit_or_bullet_from_nbt(&tag, None, target).unwrap();
        assert_eq!(
            bullet_kind(&p),
            (Some(Who::Mob(31)), Some(5), 23, [0.15, -0.15, 0.075])
        );
    }

    /// `ShulkerBullet.readAdditionalSaveData`: `Dir` goes through
    /// `Direction.from3DDataValue`, which wraps any byte, and `Steps` is read
    /// as saved, negative or not.
    #[test]
    fn a_loaded_dir_wraps_and_steps_keep_their_sign() {
        // `Direction.from3DDataValue(b)` is `BY_3D_DATA[abs(b % 6)]`, and
        // `getIntOr` keeps a negative step count.
        for (dir, steps, expected) in [(7, -3, (1, -3)), (-2, 4, (2, 4)), (5, 0, (5, 0))] {
            let tag = Tag::compound([
                ("id", Tag::String("minecraft:shulker_bullet".into())),
                ("Pos", doubles(&[4.5, 80.0, 10.5])),
                ("Dir", Tag::Byte(dir)),
                ("Steps", Tag::Int(steps)),
            ]);
            let p = spit_or_bullet_from_nbt(&tag, None, None).unwrap();
            let (_, d, s, _) = bullet_kind(&p);
            assert_eq!(
                (d, s),
                (Some(expected.0), expected.1),
                "Dir {dir}, Steps {steps}"
            );
        }
    }

    /// A bullet with no target — a `/summon`ed one, or one loaded without its
    /// target — writes no `Target` key at all, as vanilla's
    /// `if (this.finalTarget != null)` does, and no `Dir` until it has chosen a
    /// step. `Steps` and the deltas are always written, zero or not.
    #[test]
    fn a_bullet_without_a_target_writes_no_target_key() {
        let bare = Tag::compound([
            ("id", Tag::String("minecraft:shulker_bullet".into())),
            ("Pos", doubles(&[4.5, 80.0, 10.5])),
        ]);
        let level = no_entities().with_mob(31, [9, 8, 7, 6]);
        let p = spit_or_bullet_from_nbt(&bare, None, level.bullet_target(&bare)).unwrap();
        assert_eq!(
            bullet_kind(&p),
            (None, None, 0, [0.0; 3]),
            "a bare summon has no target and no step"
        );
        let saved = spit_or_bullet_to_nbt(&p, [1; 4], None, None, None).unwrap();
        assert!(saved.get("Target").is_none(), "no `Target`");
        assert!(saved.get("Dir").is_none(), "no `Dir` before a step");
        assert_eq!(number(saved.get("Steps")), Some(0));
        assert_eq!(float(saved.get("TXD")), Some(0.0));
        // The same for a bullet whose target the level has no UUID for: the
        // bridge writes nothing rather than a made-up UUID, and a re-read
        // bullet has no target.
        let p = homing(Some(Who::Mob(31)));
        let nowhere = no_entities();
        assert_eq!(nowhere.save_who_uuid(Who::Mob(31)), None);
        let saved = spit_or_bullet_to_nbt(&p, [1; 4], None, None, None).unwrap();
        assert!(saved.get("Target").is_none());
        assert_eq!(nowhere.bullet_target(&saved), None);
    }

    /// The stored `Target` names a mob that is no longer loaded: vanilla keeps
    /// the UUID and re-resolves it every tick, and Horizon's `Who` can only name
    /// an entity in the level, so the bullet comes back with no target and falls
    /// rather than homing on whatever the id now means. Here the id the mob had
    /// is reused by a projectile the same save holds, which is the case a bare
    /// id would get wrong.
    #[test]
    fn a_target_that_is_not_loaded_comes_back_with_none() {
        let shulker = [9, 8, 7, 6];
        let p = homing(Some(Who::Mob(31)));
        let tag = spit_or_bullet_to_nbt(&p, [1; 4], None, Some(shulker), None).unwrap();
        // id 31 is a projectile now, and 32 is the only mob left.
        let mut uuids = HashMap::from([(31, shulker), (32, [1, 1, 1, 1])]);
        let mut mobs = vec![Mob::new(
            32,
            crate::mobs::sensor_scan_tests::kind(),
            Vec3::ZERO,
            0.0,
        )];
        let gone = Level {
            uuids: uuids.clone(),
            mobs: mobs.clone(),
            player: None,
        };
        assert_eq!(gone.bullet_target(&tag), None, "not the id-reusing entity");
        let p = spit_or_bullet_from_nbt(&tag, None, gone.bullet_target(&tag)).unwrap();
        let (target, dir, steps, delta) = bullet_kind(&p);
        assert_eq!(target, None, "targetless, and it falls");
        assert_eq!(
            (dir, steps, delta),
            (Some(5), 23, [0.15, -0.15, 0.075]),
            "the rest of the bullet is still its own"
        );
        // The mob that is here still resolves, so the `None` above is the mob
        // being gone and not the read half refusing every mob.
        uuids.insert(33, [2, 2, 2, 2]);
        mobs.push(Mob::new(
            33,
            crate::mobs::sensor_scan_tests::kind(),
            Vec3::ZERO,
            0.0,
        ));
        let here = Level {
            uuids,
            mobs,
            player: None,
        };
        assert_eq!(here.bullet_target(&tag), None);
        let mut moved = tag.clone();
        put(&mut moved, "Target", uuid_tag([2, 2, 2, 2]));
        assert_eq!(here.bullet_target(&moved), Some(Who::Mob(33)));
    }

    /// The player is the other `Who` a `Target` can name: the same bridge, the
    /// player's own UUID.
    #[test]
    fn a_target_naming_the_player_comes_back_as_the_player() {
        let player = player_uuid();
        let level = Level {
            uuids: HashMap::new(),
            mobs: Vec::new(),
            player: Some(player),
        };
        let p = homing(Some(Who::Player));
        let target = level.save_who_uuid(Who::Player);
        assert_eq!(target, Some(player));
        let tag = spit_or_bullet_to_nbt(&p, [1; 4], None, target, None).unwrap();
        assert_eq!(level.bullet_target(&tag), Some(Who::Player));
        // With no player in the level the UUID names nobody.
        let empty = Level {
            uuids: HashMap::new(),
            mobs: Vec::new(),
            player: None,
        };
        assert_eq!(empty.bullet_target(&tag), None);
    }
}

#[cfg(test)]
#[path = "spectral_duration_nbt_tests.rs"]
mod spectral_duration_nbt_tests;

#[cfg(test)]
mod mod_thrown_nbt_tests {
    use super::*;

    #[test]
    fn a_mod_thrown_type_saves_its_id_and_item_and_loads_back() {
        crate::projectiles::register_test_types();
        let bolt = crate::projectiles::thrown_type("ruby:bolt").unwrap();
        let mut p = Projectile::new(
            ProjectileKind::Thrown(bolt),
            Vec3::new(1.0, 80.0, 2.0),
            None,
        );
        p.velocity = Vec3::new(0.0, 0.5, 1.0);
        assert!(saved_with_chunk(&p));
        let tag = throwable_to_nbt(&p, [1; 4], None, None).unwrap();
        assert_eq!(super::kind(&tag).as_deref(), Some("ruby:bolt"));
        let item = tag.get("Item").and_then(item_from_nbt).unwrap();
        assert_eq!(&*item.item, "ruby:gem");
        let back = throwable_from_nbt(&tag, None).unwrap();
        assert_eq!((&back.kind, back.velocity), (&p.kind, p.velocity));
        // NoGravity comes back too, as for vanilla's (a summoned bolt that floats).
        p.no_gravity = true;
        let tag = throwable_to_nbt(&p, [1; 4], None, None).unwrap();
        assert!(throwable_from_nbt(&tag, None).unwrap().no_gravity);
        // a type no mod declares now is not a thrown kind.
        assert!(throwable_kind("gone:bolt").is_none());
    }
    fn summoned(id: &str, extra: &[(&str, Tag)]) -> Tag {
        let mut c = vec![
            ("id", Tag::String(id.into())),
            ("Pos", doubles(&[0.5, 110.0, 0.5])),
            ("Motion", doubles(&[0.0, 0.0, 0.0])),
        ];
        c.extend(extra.iter().cloned());
        Tag::compound(c)
    }

    /// Four summoned kinds had no loader and stayed inert (a spectral arrow,
    /// a shulker bullet, a llama's spit and an experience bottle never fell).
    #[test]
    fn summoned_projectiles_of_every_kind_load_and_honour_no_gravity() {
        use crate::entity_rules::EntityRules;
        let flag = ("NoGravity", Tag::Byte(1));
        let arrow = arrow_from_nbt(&summoned("minecraft:spectral_arrow", &[]), None, |_| None)
            .unwrap()
            .0;
        assert!(arrow.is_spectral_arrow() && !arrow.no_gravity);
        let held = arrow_from_nbt(
            &summoned("minecraft:spectral_arrow", std::slice::from_ref(&flag)),
            None,
            |_| None,
        )
        .unwrap()
        .0;
        assert!(held.no_gravity);
        // ...and it is saved as the entity type it is.
        let saved = arrow_to_nbt(&arrow, [1; 4], None, None, None).unwrap();
        assert_eq!(kind(&saved).as_deref(), Some("minecraft:spectral_arrow"));
        let plain = arrow_from_nbt(&summoned("minecraft:arrow", &[]), None, |_| None)
            .unwrap()
            .0;
        let saved = arrow_to_nbt(&plain, [1; 4], None, None, None).unwrap();
        assert_eq!(kind(&saved).as_deref(), Some("minecraft:arrow"));

        for id in ["minecraft:llama_spit", "minecraft:shulker_bullet"] {
            let p = spit_or_bullet_from_nbt(&summoned(id, &[]), None, None).unwrap();
            assert_eq!(p.kind.id(), id);
            assert!(!p.no_gravity && p.position == Vec3::new(0.5, 110.0, 0.5));
            let held =
                spit_or_bullet_from_nbt(&summoned(id, std::slice::from_ref(&flag)), None, None)
                    .unwrap();
            assert!(held.no_gravity);
        }
        assert!(spit_or_bullet_from_nbt(&summoned("minecraft:arrow", &[]), None, None).is_none());
        // a spit and a target-less bullet are saved and selectable (`/data`, `@e`).
        let spit =
            spit_or_bullet_from_nbt(&summoned("minecraft:llama_spit", &[]), None, None).unwrap();
        assert!(saved_with_chunk(&spit));
        let saved = spit_or_bullet_to_nbt(&spit, [1; 4], None, None, None).unwrap();
        assert_eq!(kind(&saved).as_deref(), Some("minecraft:llama_spit"));

        // A summoned experience bottle flies as the bottle item, with a bottle's gravity.
        let bottle =
            throwable_from_nbt(&summoned("minecraft:experience_bottle", &[]), None).unwrap();
        assert_eq!(bottle.kind.id(), "minecraft:experience_bottle");
        assert!(!bottle.no_gravity && (bottle.get_gravity() - 0.07).abs() < 1e-12);
        let held =
            throwable_from_nbt(&summoned("minecraft:experience_bottle", &[flag]), None).unwrap();
        assert!(held.no_gravity && held.get_gravity() == 0.0);
        let saved = throwable_to_nbt(&bottle, [1; 4], None, None).unwrap();
        assert_eq!(kind(&saved).as_deref(), Some("minecraft:experience_bottle"));
        let item = saved.get("Item").and_then(item_from_nbt).unwrap();
        assert_eq!(&*item.item, "minecraft:experience_bottle");
    }

    #[test]
    fn no_gravity_survives_save_and_load_for_orbs() {
        // Entity.saveWithoutId / load: NoGravity round-trips on a kind that
        // fall by their own tick (orb, item, arrow, fireball).
        let mut orb = orbs_from_nbt(&Tag::compound([
            ("Pos", doubles(&[0.0, 10.0, 0.0])),
            ("NoGravity", Tag::Byte(1)),
        ]))
        .remove(0);
        assert!(orb.no_gravity);
        let back = orbs_from_nbt(&orb_to_nbt(&orb, [1; 4])).remove(0);
        assert!(back.no_gravity);
        orb.no_gravity = false;
        assert!(orb_to_nbt(&orb, [1; 4]).get("NoGravity").is_none());
    }
}

#[cfg(test)]
mod axolotl_memory_ttl_tests {
    use super::*;

    fn memory<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
        tag.get("Brain")?.get("memories")?.get(key)
    }

    #[test]
    fn integer_memory_saves_keep_finite_ttl_and_drop_it_after_a_permanent_write() {
        for key in [
            "minecraft:play_dead_ticks",
            "minecraft:temptation_cooldown_ticks",
        ] {
            for ttl in [None, Some(0), Some(1), Some(5), Some(-1), Some(i64::MAX)] {
                let mut entry = Tag::compound([("value", Tag::Int(100))]);
                if let Some(ttl) = ttl {
                    put(&mut entry, "ttl", Tag::Long(ttl));
                }
                let raw = Tag::compound([(
                    "Brain",
                    Tag::compound([("memories", Tag::compound([(key, entry)]))]),
                )]);
                let mut kind = crate::mobs::sensor_scan_tests::kind();
                let ty = Arc::make_mut(&mut kind);
                ty.id = "minecraft:axolotl".into();
                ty.species = crate::mobs::Species::Axolotl;
                let mut mob = Mob::new(1, kind, Vec3::ZERO, 0.0);
                restore_axolotl_memories(&mut mob, &raw);
                let saved = mob_to_nbt(&mob, [0; 4], Some(&raw), None, |_| None);
                let entry = memory(&saved, key).expect("loaded value must be present");
                assert_eq!(entry.get("value"), Some(&Tag::Int(100)));
                let expected = ttl.filter(|v| *v != i64::MAX).map(Tag::Long);
                assert_eq!(entry.get("ttl"), expected.as_ref(), "{key}: {ttl:?}");

                // CountDownCooldownTicks/ValidatePlayDead write ordinary permanent memories.
                let brain = mob.extra.animal_brain.as_mut().unwrap();
                if key == "minecraft:play_dead_ticks" {
                    brain.mem.play_dead_ticks.set(99);
                } else {
                    brain.mem.temptation_cooldown_ticks.set(99);
                }
                let saved = mob_to_nbt(&mob, [0; 4], Some(&raw), None, |_| None);
                let entry = memory(&saved, key).unwrap();
                assert_eq!(entry.get("value"), Some(&Tag::Int(99)));
                assert!(entry.get("ttl").is_none(), "stale raw TTL must not return");
            }
        }
    }
}

#[cfg(test)]
mod generic_home_nbt_tests {
    use super::*;
    use crate::mobs::Species;

    fn mob(species: Species, id: &str) -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.species = species;
        ty.id = id.into();
        Mob::new(1, kind, Vec3::new(3.5, 201.0, 0.5), 0.0)
    }
    fn stale() -> Tag {
        Tag::compound([
            ("home_radius", Tag::Int(99)),
            ("home_pos", Tag::IntArray(vec![90, 91, 92])),
            ("audit:unrelated", Tag::Int(7)),
        ])
    }

    #[test]
    fn spawned_trader_home_restricts_and_survives_save() {
        let mut m = mob(Species::WanderingTrader, "minecraft:wandering_trader");
        crate::mobs::trader::start_visit(&mut m, [4, 201, 0]);
        m.position = Vec3::new(19.5, 201.0, 0.5);
        assert!(crate::mobs::trader::within_home(&m));
        m.position = Vec3::new(20.5, 201.0, 0.5);
        assert!(!crate::mobs::trader::within_home(&m));
        let saved = mob_to_nbt(&m, [0; 4], None, None, |_| None);
        assert_eq!(saved.get("home_radius"), Some(&Tag::Int(16)));
        assert_eq!(saved.get("home_pos"), Some(&Tag::IntArray(vec![4, 201, 0])));
        let mut loaded = mob(Species::WanderingTrader, "minecraft:wandering_trader");
        loaded.position = m.position;
        read_mob_home(&mut loaded, &saved);
        assert!(!crate::mobs::trader::within_home(&loaded));
    }

    #[test]
    fn generic_home_output_uses_model_and_removes_stale_raw_fields() {
        let mut m = mob(Species::Pig, "minecraft:pig");
        let raw = stale();
        for raw in [None, Some(&raw)] {
            for radius in [0, 5, -2, -1] {
                m.ai.home = Some(([4, 5, 6], radius));
                let saved = mob_to_nbt(&m, [0; 4], raw, None, |_| None);
                if radius == -1 {
                    assert!(saved.get("home_radius").is_none());
                    assert!(saved.get("home_pos").is_none());
                } else {
                    assert_eq!(saved.get("home_radius"), Some(&Tag::Int(radius)));
                    assert_eq!(saved.get("home_pos"), Some(&Tag::IntArray(vec![4, 5, 6])));
                }
            }
            m.ai.home = None;
            let saved = mob_to_nbt(&m, [0; 4], raw, None, |_| None);
            assert!(saved.get("home_radius").is_none());
            assert!(saved.get("home_pos").is_none());
            if raw.is_some() {
                assert_eq!(saved.get("audit:unrelated"), Some(&Tag::Int(7)));
            }
        }
    }

    #[test]
    fn negative_home_updates_retain_position_without_replacing_runtime_ai() {
        let mut m = mob(Species::Pig, "minecraft:pig");
        m.ai.home = Some(([4, 5, 6], 5));
        m.ai.jump = true;
        m.ai.speed = 0.37;
        m.ai.no_action_time = 29;
        for radius in [-2, -1, -2] {
            read_mob_home(&mut m, &Tag::compound([("home_radius", Tag::Int(radius))]));
            assert_eq!(m.ai.home, (radius != -1).then_some(([4, 5, 6], radius)));
            assert_eq!(m.ai.home_position(), [4, 5, 6]);
            assert!(m.ai.jump);
            assert_eq!((m.ai.speed, m.ai.no_action_time), (0.37, 29));
        }
        // Runtime setters still assign ai.home directly. clearHome must retain
        // that latest live position, rather than a previous load's position.
        m.ai.home = Some(([7, 8, 9], 5));
        m.ai.clear_home();
        assert!(m.ai.home.is_none());
        read_mob_home(&mut m, &Tag::compound([("home_radius", Tag::Int(-2))]));
        assert_eq!(m.ai.home, Some(([7, 8, 9], -2)));
    }

    #[test]
    fn fresh_home_loads_apply_radius_and_position_codec_defaults() {
        let m = mob(Species::Pig, "minecraft:pig");
        let types = [(m.kind.id.clone(), m.kind.clone())].into();
        for radius in [0, 5, -1, -2] {
            for pos in [
                None,
                Some(Tag::IntArray(vec![4, 5, 6])),
                Some(Tag::IntArray(vec![4, 5])),
            ] {
                let mut raw = Tag::compound([
                    ("id", string("minecraft:pig")),
                    ("Pos", doubles(&[3.5, 201.0, 0.5])),
                    ("home_radius", Tag::Int(radius)),
                ]);
                if let Some(pos) = pos.clone() {
                    put(&mut raw, "home_pos", pos);
                }
                let loaded = mob_from_nbt(&raw, &types, 1, None).unwrap();
                let expected_pos = if radius >= 0 && pos == Some(Tag::IntArray(vec![4, 5, 6])) {
                    [4, 5, 6]
                } else {
                    [0; 3]
                };
                assert_eq!(
                    loaded.ai.home,
                    (radius != -1).then_some((expected_pos, radius))
                );
            }
        }
        let mut loaded = m;
        read_mob_home(
            &mut loaded,
            &Tag::compound([
                ("home_radius", Tag::Float((-2.5_f32).to_bits())),
                ("home_pos", Tag::IntArray(vec![4, 5, 6])),
            ]),
        );
        assert_eq!(loaded.ai.home, Some(([0; 3], -3)));
        read_mob_home(
            &mut loaded,
            &Tag::compound([
                ("home_radius", Tag::Int(5)),
                ("home_pos", doubles(&[-2.5, 5.5, 6.5])),
            ]),
        );
        assert_eq!(loaded.ai.home, Some(([-2, 5, 6], 5)));
    }

    #[test]
    fn turtle_and_creaking_keep_their_own_home_position_writer() {
        let raw = stale();
        for (species, id) in [
            (Species::Turtle, "minecraft:turtle"),
            (Species::Creaking, "minecraft:creaking"),
        ] {
            let mut m = mob(species, id);
            m.extra.home = Some([4, 5, 6]);
            for raw in [None, Some(&raw)] {
                for home in [None, Some(([7, 8, 9], 5))] {
                    m.ai.home = home;
                    let saved = mob_to_nbt(&m, [0; 4], raw, None, |_| None);
                    assert_eq!(saved.get("home_pos"), Some(&Tag::IntArray(vec![4, 5, 6])));
                    assert_eq!(
                        saved.get("home_radius"),
                        home.map(|(_, r)| Tag::Int(r)).as_ref()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod piglin_live_anger_memory_tests {
    use super::*;

    fn mob() -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:piglin".into();
        ty.species = crate::mobs::Species::Piglin;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }
    fn raw() -> Tag {
        Tag::compound([(
            "Brain",
            Tag::compound([
                ("audit:root", Tag::Int(8)),
                (
                    "memories",
                    Tag::compound([
                        ("audit:keep", Tag::compound([("value", Tag::Int(7))])),
                        (
                            "minecraft:angry_at",
                            Tag::compound([
                                ("value", Tag::IntArray(vec![90, 91, 92, 93])),
                                ("ttl", Tag::Long(99)),
                            ]),
                        ),
                    ]),
                ),
            ]),
        )])
    }
    fn memory(tag: &Tag) -> Option<&Tag> {
        tag.get("Brain")?.get("memories")?.get("minecraft:angry_at")
    }
    fn assert_unrelated(saved: &Tag) {
        let brain = saved.get("Brain").unwrap();
        assert_eq!(brain.get("audit:root"), Some(&Tag::Int(8)));
        assert_eq!(
            brain.get("memories").unwrap().get("audit:keep"),
            Some(&Tag::compound([("value", Tag::Int(7))]))
        );
    }

    #[test]
    fn guarded_event_saves_actual_uuid_and_remaining_long_ttl() {
        let raw = raw();
        for actor in [[1, 2, 3, 4], [-5, 6, 7, 8]] {
            let mut m = mob();
            crate::mobs::piglin::set_guarded_anger_target(&mut m, Some(actor));
            assert_eq!(m.extra.anger, 600);
            assert_eq!(m.ai.target, Some(Who::Player));
            for remaining in [600, 520] {
                m.extra.anger = remaining;
                for base in [None, Some(&raw)] {
                    let saved = mob_to_nbt(&m, [42, 43, 44, 45], base, None, |_| None);
                    assert_eq!(
                        memory(&saved),
                        Some(&Tag::compound([
                            ("value", Tag::IntArray(actor.to_vec())),
                            ("ttl", Tag::Long(i64::from(remaining))),
                        ]))
                    );
                    if base.is_some() {
                        assert_unrelated(&saved);
                    }
                }
            }
        }
    }

    /// `Piglin.readAdditionalSaveData` reads the brain's `ANGRY_AT` memory
    /// back: the actor's UUID and the ticks left (`ExpirableValue`'s TTL).
    /// Horizon wrote the memory but never read it, so a piglin that a player
    /// had angered came back calm after its chunk reloaded.
    #[test]
    fn a_saved_guarded_anger_loads_its_uuid_and_ticks() {
        // A whole saved piglin: its id, a position and the brain's memory.
        let saved = |memory: Tag| {
            Tag::compound([
                ("id", Tag::String("minecraft:piglin".into())),
                (
                    "Pos",
                    Tag::List(vec![
                        Tag::Double(0.0f64.to_bits()),
                        Tag::Double(64.0f64.to_bits()),
                        Tag::Double(0.0f64.to_bits()),
                    ]),
                ),
                (
                    "Brain",
                    Tag::compound([("memories", Tag::compound([("minecraft:angry_at", memory)]))]),
                ),
            ])
        };
        let template = mob();
        let types = [(template.kind.id.clone(), template.kind.clone())].into();
        let uuid = Tag::IntArray(vec![90, 91, 92, 93]);
        let ticking = saved(Tag::compound([
            ("value", uuid.clone()),
            ("ttl", Tag::Long(99)),
        ]));
        let loaded = mob_from_nbt(&ticking, &types, 2, None).unwrap();
        assert_eq!(loaded.extra.anger, 99);
        assert_eq!(
            loaded.extra.piglin.guarded_angry_at,
            Some(Some([90, 91, 92, 93]))
        );
        // `ExpirableValue` leaves the TTL out when it never expires, and a
        // long past the counter's range holds as long as it can: the counter
        // keeps the longest it can while the goal ticks it down.
        for memory in [
            Tag::compound([("value", uuid.clone())]),
            Tag::compound([("value", uuid.clone()), ("ttl", Tag::Long(1 << 40))]),
        ] {
            let loaded = mob_from_nbt(&saved(memory), &types, 3, None).unwrap();
            assert_eq!(loaded.extra.anger, i32::MAX);
            assert_eq!(
                loaded.extra.piglin.guarded_angry_at,
                Some(Some([90, 91, 92, 93]))
            );
        }
    }

    #[test]
    fn expired_owned_memory_cannot_resurrect_stale_raw_anger() {
        let mut m = mob();
        crate::mobs::piglin::set_guarded_anger_target(&mut m, Some([1, 2, 3, 4]));
        m.extra.anger = 0;
        m.ai.target = None;
        let saved = mob_to_nbt(&m, [42, 43, 44, 45], Some(&raw()), None, |_| None);
        assert!(memory(&saved).is_none());
        assert_unrelated(&saved);
    }

    #[test]
    fn calm_loaded_and_missing_identity_states_are_distinct() {
        let mut m = mob();
        assert!(memory(&mob_to_nbt(&m, [42, 43, 44, 45], None, None, |_| None)).is_none());
        let raw = raw();
        // Loaded Brain semantics stay outside this live event projection.
        let untouched = mob_to_nbt(&m, [42, 43, 44, 45], Some(&raw), None, |_| None);
        assert_eq!(memory(&untouched), memory(&raw));
        crate::mobs::piglin::set_guarded_anger_target(&mut m, None);
        assert_eq!(m.extra.anger, 600);
        assert_eq!(m.ai.target, Some(Who::Player));
        let saved = mob_to_nbt(&m, [42, 43, 44, 45], Some(&raw), None, |_| None);
        assert!(memory(&saved).is_none());
        assert_unrelated(&saved);
    }
}

#[cfg(test)]
mod pickup_loot_save_tests {
    use super::*;
    use crate::mobs::Species;

    #[test]
    fn pickup_save_uses_live_state_instead_of_raw_and_preserves_overrides() {
        for (species, id) in [
            (Species::Pig, "minecraft:pig"),
            (Species::Allay, "minecraft:allay"),
            (Species::SulfurCube, "minecraft:sulfur_cube"),
        ] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let ty = Arc::make_mut(&mut kind);
            ty.id = id.into();
            ty.species = species;
            let types = HashMap::from([(id.to_string(), kind)]);
            for supplied in [None, Some(false), Some(true)] {
                let mut raw = Tag::compound([
                    ("id", string(id)),
                    ("Pos", doubles(&[4.5, 200.0, 4.5])),
                    ("audit:unrelated", Tag::Int(7)),
                ]);
                if let Some(on) = supplied {
                    put(&mut raw, "CanPickUpLoot", Tag::Byte(i8::from(on)));
                }
                let mut mob = mob_from_nbt(&raw, &types, 1, None).unwrap();
                for live in [false, true] {
                    mob.extra.pick_up_loot = live;
                    mob.extra.main_hand = live.then(|| "minecraft:stone".into());
                    mob.extra.body = live.then(|| "minecraft:stone".into());
                    let expected = if species == Species::SulfurCube {
                        !live
                    } else {
                        live
                    };
                    for input in [None, Some(&raw)] {
                        let saved = mob_to_nbt(&mob, [46, 0, 0, 1], input, None, |_| None);
                        assert_eq!(
                            saved.get("CanPickUpLoot"),
                            Some(&Tag::Byte(i8::from(expected))),
                            "{id} supplied={supplied:?} live={live} raw={}",
                            input.is_some()
                        );
                        if input.is_some() {
                            assert_eq!(saved.get("audit:unrelated"), Some(&Tag::Int(7)));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod stand_fall_save_tests {
    use super::*;

    /// `Entity.load` reads `Fire` with `getShortOr`: an int tag is narrowed
    /// (`(short)(value & 65535)`), so 40000 is -25536 and the stand or mob
    /// does not burn.
    #[test]
    fn a_stand_and_a_mob_narrow_a_loaded_fire_to_a_short() {
        let kind = crate::mobs::sensor_scan_tests::kind();
        let types = [(kind.id.to_string(), kind.clone())].into();
        for (fire, expected) in [
            (Tag::Int(40000), -25536),
            (Tag::Int(65546), 10),
            (Tag::Short(60), 60),
        ] {
            let input = Tag::compound([
                ("id", string(&kind.id)),
                ("Pos", doubles(&[4.5, 205.0, 4.5])),
                ("Fire", fire),
            ]);
            assert_eq!(
                armor_stand_from_nbt(&input, 1).unwrap().fire_ticks,
                expected
            );
            assert_eq!(
                mob_from_nbt(&input, &types, 1, None).unwrap().fire_ticks,
                expected
            );
        }
    }

    /// `LargeFireball.readAdditionalSaveData` reads `ExplosionPower` with
    /// `getByteOr`: an int tag is narrowed (`(byte)(value & 255)`), so 300 is
    /// 44 and 200 is -56.
    #[test]
    fn a_fireball_narrows_its_explosion_power_to_a_byte() {
        for (saved, expected) in [(Tag::Int(300), 44), (Tag::Int(200), -56), (Tag::Byte(4), 4)] {
            let input = Tag::compound([
                ("id", string("minecraft:fireball")),
                ("Pos", doubles(&[4.5, 205.0, 4.5])),
                ("ExplosionPower", saved),
            ]);
            let fireball = fireball_from_nbt(&input, None).unwrap();
            assert!(
                matches!(fireball.kind, ProjectileKind::LargeFireball { power } if power == expected),
                "{expected}"
            );
        }
    }

    #[test]
    fn decoded_stand_fall_distance_survives_save_without_raw_or_with_stale_raw() {
        let stale = Tag::compound([("fall_distance", Tag::Double(99.0_f64.to_bits()))]);
        for no_gravity in [false, true] {
            for distance in [0.0_f64, 1.25, 17.5] {
                let input = Tag::compound([
                    ("Pos", doubles(&[4.5, 205.0, 4.5])),
                    ("NoGravity", Tag::Byte(i8::from(no_gravity))),
                    ("fall_distance", Tag::Double(distance.to_bits())),
                ]);
                let stand = armor_stand_from_nbt(&input, 1).unwrap();
                for raw in [None, Some(&stale)] {
                    let saved = armor_stand_to_nbt(&stand, [28, 0, 0, 1], raw);
                    assert_eq!(
                        saved.get("fall_distance"),
                        Some(&Tag::Double(distance.to_bits())),
                        "distance={distance} no_gravity={no_gravity} raw={}",
                        raw.is_some()
                    );
                    assert_eq!(saved.get("Pos"), input.get("Pos"));
                    assert_eq!(stand.no_gravity, no_gravity);
                }
            }
        }
    }

    /// `Entity.saveWithoutId`'s common fields are filled in when a raw compound
    /// (a marker's) lacks them, and explicit values win.
    #[test]
    fn marker_common_fields_default_and_keep_explicit_values() {
        let mut tag = Tag::compound([("Air", Tag::Short(10)), ("OnGround", Tag::Byte(1))]);
        fill_common_defaults(&mut tag);
        assert_eq!(tag.get("Air"), Some(&Tag::Short(10)));
        assert_eq!(tag.get("OnGround"), Some(&Tag::Byte(1)));
        assert_eq!(tag.get("Invulnerable"), Some(&Tag::Byte(0)));
        assert_eq!(tag.get("PortalCooldown"), Some(&Tag::Int(0)));
        assert_eq!(
            tag.get("fall_distance"),
            Some(&Tag::Double(0.0f64.to_bits()))
        );
        assert_eq!(tag.get("Motion"), Some(&doubles(&[0.0, 0.0, 0.0])));
        // Anything but a compound is left untouched.
        let mut plain = Tag::String("x".into());
        fill_common_defaults(&mut plain);
        assert_eq!(plain, Tag::String("x".into()));
    }
}

#[cfg(test)]
mod animal_love_cause_tests {
    use super::*;
    use crate::mobs::Species;

    #[test]
    fn animal_save_uses_the_parsed_love_cause_instead_of_stale_raw() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:pig".into();
        ty.species = Species::Pig;
        let types = HashMap::from([("minecraft:pig".to_string(), kind)]);
        let raw = |cause: Option<Tag>| {
            let mut tag = Tag::compound([
                ("id", string("minecraft:pig")),
                ("Pos", doubles(&[1.5, 200.0, 4.5])),
                ("InLove", Tag::Int(123)),
                ("CustomName", string("kept")),
                ("Tags", Tag::List(vec![string("kept")])),
            ]);
            if let Some(cause) = cause {
                put(&mut tag, "LoveCause", cause);
            }
            tag
        };
        let valid = [57, 1, 2, 3];
        for cause in [
            None,
            Some(Tag::IntArray(vec![])),
            Some(Tag::IntArray(vec![57, 1, 2])),
            Some(Tag::IntArray(vec![57, 1, 2, 3, 4])),
            Some(Tag::Int(7)),
            Some(Tag::compound([("value", Tag::Int(7))])),
        ] {
            let raw = raw(cause);
            let mut mob = mob_from_nbt(&raw, &types, 1, None).unwrap();
            assert_eq!(mob.extra.love_cause, None);
            for input in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [57, 0, 0, 1], input, None, |_| None);
                assert!(saved.get("LoveCause").is_none(), "invalid raw {raw:?}");
                assert_eq!(saved.get("InLove"), Some(&Tag::Int(123)));
                if input.is_some() {
                    assert_eq!(saved.get("CustomName"), raw.get("CustomName"));
                    assert_eq!(saved.get("Tags"), raw.get("Tags"));
                }
                mob.extra.love_cause = Some(valid);
                let saved = mob_to_nbt(&mob, [57, 0, 0, 1], input, None, |_| None);
                assert_eq!(saved.get("LoveCause"), Some(&Tag::IntArray(valid.to_vec())));
                mob.extra.love_cause = None;
            }
        }
        let loaded_raw = raw(Some(Tag::IntArray(valid.to_vec())));
        let mut mob = mob_from_nbt(&loaded_raw, &types, 2, None).unwrap();
        assert_eq!(mob.extra.love_cause, Some(valid));
        assert_eq!(
            mob_to_nbt(&mob, [57, 0, 0, 2], None, None, |_| None).get("LoveCause"),
            Some(&Tag::IntArray(valid.to_vec()))
        );
        mob.extra.love_cause = None;
        assert!(
            mob_to_nbt(&mob, [57, 0, 0, 2], Some(&loaded_raw), None, |_| None)
                .get("LoveCause")
                .is_none()
        );
        // This cleanup belongs only to Animal's writer, not generic raw data.
        Arc::make_mut(&mut mob.kind).id = "minecraft:zombie".into();
        Arc::make_mut(&mut mob.kind).species = Species::Zombie;
        assert_eq!(
            mob_to_nbt(&mob, [57, 0, 0, 2], Some(&loaded_raw), None, |_| None).get("LoveCause"),
            loaded_raw.get("LoveCause")
        );
    }
}

#[cfg(test)]
mod carrier_inventory_tests {
    use super::*;
    use crate::mobs::Species;

    #[test]
    fn carrier_inventory_is_adopted_and_saved_from_the_model() {
        let stack = Tag::compound([
            ("id", string("minecraft:stone")),
            ("count", Tag::Int(3)),
            (
                "components",
                Tag::compound([(
                    "minecraft:custom_data",
                    Tag::compound([("audit", Tag::Int(137))]),
                )]),
            ),
        ]);
        let offers = Tag::compound([(
            "Recipes",
            Tag::List(vec![Tag::compound([
                (
                    "buy",
                    Tag::compound([("id", string("minecraft:emerald")), ("count", Tag::Int(1))]),
                ),
                (
                    "sell",
                    Tag::compound([("id", string("minecraft:dirt")), ("count", Tag::Int(2))]),
                ),
            ])]),
        )]);
        for (species, id) in [
            (Species::Pillager, "minecraft:pillager"),
            (Species::WanderingTrader, "minecraft:wandering_trader"),
            (Species::Piglin, "minecraft:piglin"),
        ] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            Arc::make_mut(&mut kind).id = id.into();
            Arc::make_mut(&mut kind).species = species;
            let types = HashMap::from([(id.to_string(), kind)]);
            for input in [
                None,
                Some(Tag::List(vec![])),
                Some(Tag::List(vec![stack.clone()])),
            ] {
                let mut raw = Tag::compound([
                    ("id", string(id)),
                    ("Pos", doubles(&[1.5, 200.0, 4.5])),
                    ("CustomName", string("kept")),
                    ("Tags", Tag::List(vec![string("kept")])),
                ]);
                if let Some(input) = input.clone() {
                    put(&mut raw, "Inventory", input);
                }
                if species == Species::WanderingTrader {
                    put(&mut raw, "Offers", offers.clone());
                    put(&mut raw, "DespawnDelay", Tag::Int(48000));
                }
                if species == Species::Pillager {
                    put(&mut raw, "PatrolLeader", Tag::Byte(1));
                    put(&mut raw, "CanJoinRaid", Tag::Byte(0));
                }
                let mut mob = mob_from_nbt(&raw, &types, 1, None).unwrap();
                let has_stack = matches!(&input, Some(Tag::List(v)) if !v.is_empty());
                assert_eq!(
                    mob.extra.inventory.len(),
                    usize::from(has_stack),
                    "{id} adoption"
                );
                let expected = Tag::List(if has_stack {
                    vec![stack.clone()]
                } else {
                    vec![]
                });
                let mut stale = raw.clone();
                put(
                    &mut stale,
                    "Inventory",
                    Tag::List(vec![Tag::compound([
                        ("id", string("minecraft:dirt")),
                        ("count", Tag::Int(2)),
                    ])]),
                );
                for base in [None, Some(&raw), Some(&stale)] {
                    let saved = mob_to_nbt(&mob, [137, 0, 0, 1], base, None, |_| None);
                    assert_eq!(saved.get("Inventory"), Some(&expected), "{id} save");
                    if base.is_some() {
                        assert_eq!(saved.get("CustomName"), raw.get("CustomName"));
                        assert_eq!(saved.get("Tags"), raw.get("Tags"));
                    }
                    if species == Species::WanderingTrader {
                        assert_eq!(saved.get("Offers"), Some(&offers));
                        assert_eq!(saved.get("DespawnDelay"), Some(&Tag::Int(48000)));
                    }
                    if species == Species::Pillager {
                        assert_eq!(saved.get("PatrolLeader"), Some(&Tag::Byte(1)));
                        assert_eq!(saved.get("CanJoinRaid"), Some(&Tag::Byte(0)));
                    }
                }
                if has_stack {
                    mob.extra.inventory[0].count = 4;
                    let mut changed = stack.clone();
                    put(&mut changed, "count", Tag::Int(4));
                    assert_eq!(
                        mob_to_nbt(&mob, [137, 0, 0, 1], Some(&raw), None, |_| None)
                            .get("Inventory"),
                        Some(&Tag::List(vec![changed]))
                    );
                }
                mob.extra.inventory.clear();
                assert_eq!(
                    mob_to_nbt(&mob, [137, 0, 0, 1], Some(&stale), None, |_| None).get("Inventory"),
                    Some(&Tag::List(vec![]))
                );
            }
        }
        // This InventoryCarrier projection does not add a pig inventory.
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        Arc::make_mut(&mut kind).id = "minecraft:pig".into();
        Arc::make_mut(&mut kind).species = Species::Pig;
        let mut pig = Mob::new(2, kind, Vec3::ZERO, 0.0);
        pig.extra
            .inventory
            .push(crate::save::item_from_nbt(&stack).unwrap());
        assert!(
            mob_to_nbt(&pig, [137, 0, 0, 2], None, None, |_| None)
                .get("Inventory")
                .is_none()
        );
    }
}

/// Added controls (H192): the sulfur cube's saved archetype attributes are
/// its map's base and permanent modifiers, replacing a stale entry of the
/// same id once, never its transient modifiers.
#[cfg(test)]
mod sulfur_attribute_save_tests {
    use super::*;
    use horizon_world::attributes::{Modifier, Operation};

    #[test]
    fn the_save_writes_base_and_permanent_modifiers_and_replaces_stale_entries() {
        let mut m = crate::mobs::sulfur_cube::adopt_body_tests::cube(None);
        let map = m.extra.armor_attributes.as_deref_mut().unwrap();
        let kr = map.catalog().id("minecraft:knockback_resistance").unwrap();
        let friction = map.catalog().id("minecraft:friction_modifier").unwrap();
        map.edit(kr, |i, o| {
            i.set_base_value(0.5, o).unwrap();
            i.add_permanent(
                Modifier::new(
                    "minecraft:bouncy_add_knockback_resistance",
                    0.25,
                    Operation::AddValue,
                )
                .unwrap(),
                o,
            )
            .unwrap();
            // The archetype's transient modifier of the same id shadows it.
            i.add_or_update_transient(
                Modifier::new(
                    "minecraft:bouncy_add_knockback_resistance",
                    -2.0,
                    Operation::AddValue,
                )
                .unwrap(),
                o,
            )
            .unwrap();
        });
        map.edit(friction, |i, o| {
            i.add_or_update_transient(
                Modifier::new(
                    "minecraft:bouncy_mul_friction_modifier",
                    -0.7,
                    Operation::AddMultipliedTotal,
                )
                .unwrap(),
                o,
            )
            .unwrap();
        });
        let stale =
            |id: &str| Tag::compound([("id", string(id)), ("base", Tag::Double(9.0f64.to_bits()))]);
        let mut nbt = Compound::new();
        nbt.insert(
            "attributes".into(),
            Tag::List(vec![
                stale("minecraft:max_health"),
                stale("minecraft:knockback_resistance"),
            ]),
        );
        put_sulfur_attributes(&mut nbt, &m);
        let Some(Tag::List(list)) = nbt.get(&"attributes".into()) else {
            panic!("no attributes");
        };
        let ids: Vec<_> = list.iter().map(|t| text(t.get("id")).unwrap()).collect();
        assert_eq!(
            ids,
            [
                "minecraft:max_health",
                "minecraft:knockback_resistance",
                "minecraft:friction_modifier"
            ]
        );
        let base = |t: &Tag| match t.get("base") {
            Some(Tag::Double(b)) => f64::from_bits(*b),
            other => panic!("base {other:?}"),
        };
        assert_eq!(base(&list[0]), 9.0);
        assert_eq!(base(&list[1]), 0.5);
        let Some(Tag::List(modifiers)) = list[1].get("modifiers") else {
            panic!("no modifiers");
        };
        assert_eq!(modifiers.len(), 1);
        assert_eq!(
            modifiers[0].get("amount"),
            Some(&Tag::Double(0.25f64.to_bits()))
        );
        assert_eq!(
            text(modifiers[0].get("operation")).as_deref(),
            Some("add_value")
        );
        // Friction holds only a transient modifier: its base, no modifiers.
        assert_eq!(base(&list[2]), 1.0);
        assert!(list[2].get("modifiers").is_none());
    }

    #[test]
    fn another_species_map_is_not_saved_by_the_bridge() {
        let mut m = crate::mobs::sulfur_cube::adopt_body_tests::cube(None);
        std::sync::Arc::make_mut(&mut m.kind).species = crate::mobs::Species::Slime;
        let map = m.extra.armor_attributes.as_deref_mut().unwrap();
        let kr = map.catalog().id("minecraft:knockback_resistance").unwrap();
        map.edit(kr, |i, o| i.set_base_value(0.5, o).unwrap());
        let mut nbt = Compound::new();
        put_sulfur_attributes(&mut nbt, &m);
        assert!(!nbt.contains_key(&"attributes".into()));
    }
}

#[cfg(test)]
mod mob_hidden_speed_tests {
    use super::*;
    use crate::mobs::{Species, sensor_scan_tests::kind};
    fn types() -> HashMap<String, Arc<MobType>> {
        let mut pig = (*kind()).clone();
        pig.id = "minecraft:pig".into();
        pig.species = Species::Pig;
        HashMap::from([("minecraft:pig".to_owned(), Arc::new(pig))])
    }
    fn saved(m: &Mob) -> Tag {
        mob_to_nbt(m, [202, 0, 0, 1], None, None, |_| None)
    }
    #[test]
    fn loaded_speed_fallback_survives_a_no_raw_save() {
        let input = horizon_world::nbt::parse_snbt(r#"{id:"minecraft:pig",UUID:[I;202,0,0,1],Pos:[2.5d,200d,4.5d],Health:10f,NoAI:1b,NoGravity:1b,active_effects:[{id:"minecraft:speed",amplifier:1b,duration:40,show_icon:1b,hidden_effect:{duration:200,ambient:1b,show_particles:0b,show_icon:0b}}]}"#).unwrap().0;
        let m = mob_from_nbt(&input, &types(), 7, None).unwrap();
        assert_eq!(m.id, 7);
        assert_eq!(m.kind.id, "minecraft:pig");
        assert!(m.no_ai && m.no_gravity);
        assert_eq!(m.health, 10.0);
        assert_eq!(m.extra.effects, [("minecraft:speed", 40, 1)]);
        let tag = saved(&m);
        let hidden = tag
            .get("active_effects")
            .and_then(|t| match t {
                Tag::List(v) => v.first(),
                _ => None,
            })
            .and_then(|e| e.get("hidden_effect"));
        assert_eq!(
            hidden.and_then(|t| t.get("duration")),
            Some(&Tag::Int(200)),
            "real decoder must retain weaker duration without raw input"
        );
    }
    // Added after the frozen old proof: modeled mutation and stale raw controls.
    #[test]
    fn fallback_model_wins_over_stale_raw_and_clears_without_resurrection() {
        let input = horizon_world::nbt::parse_snbt(r#"{id:"minecraft:pig",Pos:[2.5d,200d,4.5d],Tags:["untouched"],active_effects:[{id:"minecraft:speed",amplifier:1b,duration:40,hidden_effect:{duration:200,ambient:1b,show_particles:0b,show_icon:0b}}]}"#).unwrap().0;
        let mut m = mob_from_nbt(&input, &types(), 7, None).unwrap();
        let h = &m.extra.effect_fallbacks[0];
        assert_eq!(
            (
                h.id,
                h.duration,
                h.amplifier,
                h.ambient,
                h.visible,
                h.show_icon
            ),
            ("minecraft:speed", 200, 0, true, false, false)
        );
        m.extra.effect_fallbacks[0].duration = 123;
        m.extra.effects[0].1 = 30;
        let actual = mob_to_nbt(&m, [202, 0, 0, 1], Some(&input), None, |_| None);
        let first = |t: &Tag| match t.get("active_effects").unwrap() {
            Tag::List(v) => v[0].clone(),
            _ => panic!(),
        };
        assert_eq!(first(&actual).get("duration"), Some(&Tag::Int(30)));
        assert_eq!(
            first(&actual).get("hidden_effect").unwrap().get("duration"),
            Some(&Tag::Int(123))
        );
        assert_eq!(actual.get("Tags"), input.get("Tags"));
        m.extra.effect_fallbacks.clear();
        for raw in [None, Some(&input)] {
            let actual = mob_to_nbt(&m, [202, 0, 0, 1], raw, None, |_| None);
            assert!(first(&actual).get("hidden_effect").is_none());
        }
    }
}

#[cfg(test)]
mod mob_effect_amplifier_codec {
    use super::*;

    fn types() -> HashMap<String, Arc<MobType>> {
        let kind = crate::mobs::sensor_scan_tests::kind();
        HashMap::from([(kind.id.clone(), kind)])
    }

    fn input(amplifier: Option<Tag>) -> Tag {
        let mut effect = Tag::compound([
            ("id", string("minecraft:night_vision")),
            ("duration", Tag::Int(200)),
            ("ambient", Tag::Byte(1)),
            ("show_particles", Tag::Byte(0)),
            ("show_icon", Tag::Byte(0)),
        ]);
        if let Some(value) = amplifier {
            put(&mut effect, "amplifier", value);
        }
        Tag::compound([
            ("id", string("minecraft:cow")),
            ("Pos", doubles(&[6.5, 200.0, 6.5])),
            ("Health", Tag::Float(10.0f32.to_bits())),
            ("NoAI", Tag::Byte(1)),
            ("NoGravity", Tag::Byte(1)),
            ("Tags", Tag::List(vec![string("amplifier-control")])),
            ("active_effects", Tag::List(vec![effect])),
        ])
    }

    fn saved_effect(tag: &Tag) -> &Tag {
        let Some(Tag::List(effects)) = tag.get("active_effects") else {
            panic!("actual effect missing: {tag:?}");
        };
        assert_eq!(effects.len(), 1);
        &effects[0]
    }

    #[test]
    fn decoded_numeric_amplifiers_roundtrip_without_raw_input() {
        let cases = [
            (None, 0),
            (Some(Tag::Byte(0)), 0),
            (Some(Tag::Int(1)), 1),
            (Some(Tag::Byte(127)), 127),
            (Some(Tag::Byte(-128)), 128),
            (Some(Tag::Byte(-1)), 255),
            (Some(Tag::Int(256)), 0),
            (Some(Tag::Int(-1)), 255),
            (Some(Tag::Long(9007199254740993)), 1),
            (Some(Tag::Long(-4294967297)), 255),
            (Some(Tag::Float(1.75f32.to_bits())), 1),
            (Some(Tag::Float((-0.5f32).to_bits())), 0),
            (Some(Tag::Double((-1.75f64).to_bits())), 255),
            (Some(Tag::Double(256.75f64.to_bits())), 0),
            (Some(Tag::Double(1e20f64.to_bits())), 255),
            (Some(Tag::Double((-1e20f64).to_bits())), 0),
        ];
        let types = types();
        for (value, expected) in cases {
            let raw = input(value.clone());
            let mob = mob_from_nbt(&raw, &types, 7, None).unwrap();
            assert_eq!(
                mob.extra.effects,
                [("minecraft:night_vision", 200, expected)],
                "{value:?}"
            );
            for source in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [158, 0, 0, 1], source, None, |_| None);
                let effect = saved_effect(&saved);
                let expected_tag = (expected != 0).then_some(Tag::Byte(expected as u8 as i8));
                assert_eq!(effect.get("amplifier"), expected_tag.as_ref(), "{value:?}");
                assert_eq!(effect.get("duration"), Some(&Tag::Int(200)));
                assert_eq!(effect.get("ambient"), Some(&Tag::Byte(1)));
                assert_eq!(effect.get("show_particles"), Some(&Tag::Byte(0)));
                assert_eq!(effect.get("show_icon"), Some(&Tag::Byte(0)));
                let back = mob_from_nbt(&saved, &types, 8, None).unwrap();
                assert_eq!(back.extra.effects, mob.extra.effects);
            }
        }
    }

    #[test]
    fn modeled_amplifier_mutation_overrides_stale_raw_and_omits_zero() {
        let raw = input(Some(Tag::Byte(12)));
        let mut mob = mob_from_nbt(&raw, &types(), 7, None).unwrap();
        for amplifier in [0, 128, 255, 1] {
            mob.extra.effects[0].2 = amplifier;
            for source in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [158, 0, 0, 2], source, None, |_| None);
                let expected = (amplifier != 0).then_some(Tag::Byte(amplifier as u8 as i8));
                assert_eq!(saved_effect(&saved).get("amplifier"), expected.as_ref());
                assert_eq!(saved.get("Health"), Some(&Tag::Float(10.0f32.to_bits())));
                assert_eq!(saved.get("NoAI"), Some(&Tag::Byte(1)));
                if source.is_some() {
                    assert_eq!(saved.get("Tags"), raw.get("Tags"));
                }
            }
        }
    }
}

#[cfg(test)]
mod camel_inherited_saved_tests {
    use super::*;
    use crate::mobs::Species;

    fn types(species: Species, id: &str) -> HashMap<String, Arc<MobType>> {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = id.into();
        ty.species = species;
        HashMap::from([(id.to_owned(), kind)])
    }

    fn input(id: &str, temper: Option<i32>) -> Tag {
        let mut raw = Tag::compound([
            ("id", string(id)),
            ("Pos", doubles(&[1.5, 200.0, 4.5])),
            ("Health", Tag::Float(10.0_f32.to_bits())),
            ("NoAI", Tag::Byte(1)),
            ("NoGravity", Tag::Byte(1)),
            ("Silent", Tag::Byte(1)),
            ("Invulnerable", Tag::Byte(1)),
            ("LastPoseTick", Tag::Long(1)),
            ("CustomName", string("kept")),
            ("Tags", Tag::List(vec![string("kept")])),
            ("audit:kept", Tag::Int(17)),
        ]);
        if let Some(temper) = temper {
            put(&mut raw, "Tame", Tag::Byte(0));
            put(&mut raw, "Temper", Tag::Int(temper));
            put(&mut raw, "Bred", Tag::Byte(1));
            put(&mut raw, "EatingHaystack", Tag::Byte(1));
        }
        raw
    }

    #[test]
    fn camel_inherited_fields_decode_and_save_without_raw_or_with_stale_raw() {
        for (species, id) in [
            (Species::Camel, "minecraft:camel"),
            (Species::CamelHusk, "minecraft:camel_husk"),
        ] {
            let types = types(species, id);
            for temper in [None, Some(0), Some(50), Some(-1), Some(256), Some(i32::MAX)] {
                let raw = input(id, temper);
                let mob = mob_from_nbt(&raw, &types, 92, None).unwrap();
                assert!(
                    mob.extra.tame,
                    "Camel.isTamed remains true even with Tame:0b"
                );
                assert_eq!(mob.extra.bred, temper.is_some());
                assert_eq!(
                    mob.extra.flag,
                    temper.is_some(),
                    "actual loaded eating state"
                );
                assert!(mob.no_ai && mob.no_gravity && mob.silent && mob.invulnerable_flag);
                assert!(!mob.extra.camel_sitting && !mob.extra.flag2);
                assert_eq!(mob.extra.pose_change_tick, 1);
                let mut stale = raw.clone();
                for (field, value) in [
                    ("Tame", Tag::Byte(0)),
                    ("Temper", Tag::Long(9999)),
                    ("Bred", Tag::Byte(i8::from(temper.is_none()))),
                    ("EatingHaystack", Tag::Byte(i8::from(temper.is_none()))),
                ] {
                    put(&mut stale, field, value);
                }
                for source in [None, Some(&raw), Some(&stale)] {
                    let saved = mob_to_nbt(&mob, [92, 0, 0, 1], source, None, |_| None);
                    assert_eq!(saved.get("Tame"), Some(&Tag::Byte(1)), "{id}, {temper:?}");
                    assert_eq!(saved.get("Temper"), Some(&Tag::Int(temper.unwrap_or(0))));
                    for field in ["Bred", "EatingHaystack"] {
                        assert_eq!(
                            saved.get(field),
                            Some(&Tag::Byte(i8::from(temper.is_some())))
                        );
                    }
                    for field in ["NoAI", "NoGravity", "Silent", "Invulnerable"] {
                        assert_eq!(saved.get(field), Some(&Tag::Byte(1)));
                    }
                    assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(vec![92, 0, 0, 1])));
                    if source.is_some() {
                        for field in ["CustomName", "Tags", "audit:kept"] {
                            assert_eq!(saved.get(field), raw.get(field));
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn camel_signed_state_and_mutations_override_contradictory_raw() {
        for (species, id) in [
            (Species::Camel, "minecraft:camel"),
            (Species::CamelHusk, "minecraft:camel_husk"),
        ] {
            let types = types(species, id);
            for (tag, value) in [
                (Tag::Int(-1), -1),
                (Tag::Int(256), 256),
                (Tag::Int(i32::MAX), i32::MAX),
                (Tag::Float((-1.5_f32).to_bits()), -2),
                (Tag::Long(4_294_967_297), 1),
                (string("bad"), 0),
            ] {
                let mut raw = input(id, Some(50));
                put(&mut raw, "Temper", tag);
                let mob = mob_from_nbt(&raw, &types, 92, None).unwrap();
                assert_eq!(mob.extra.equine_temper, value, "actual signed model state");
                assert!(mob.extra.tame && mob.extra.bred && mob.extra.flag);
                assert_eq!(
                    mob_to_nbt(&mob, [92, 0, 0, 1], None, None, |_| None).get("Temper"),
                    Some(&Tag::Int(value))
                );
            }
            let raw = input(id, Some(50));
            let mut mob = mob_from_nbt(&raw, &types, 92, None).unwrap();
            crate::mobs::camel::finish_load(&mut mob, 1024);
            let unchanged = (
                mob.id,
                mob.position,
                mob.velocity,
                mob.health,
                mob.extra.variant3,
                mob.extra.pose_change_tick,
                mob.extra.egg_time,
                mob.extra.camel_sitting,
                mob.extra.flag2,
            );
            mob.extra.equine_temper = -77;
            mob.extra.bred = false;
            mob.extra.flag = false;
            for source in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [92, 0, 0, 1], source, None, |_| None);
                assert_eq!(saved.get("Temper"), Some(&Tag::Int(-77)));
                assert_eq!(saved.get("Tame"), Some(&Tag::Byte(1)));
                assert_eq!(saved.get("Bred"), Some(&Tag::Byte(0)));
                assert_eq!(saved.get("EatingHaystack"), Some(&Tag::Byte(0)));
            }
            assert_eq!(
                (
                    mob.id,
                    mob.position,
                    mob.velocity,
                    mob.health,
                    mob.extra.variant3,
                    mob.extra.pose_change_tick,
                    mob.extra.egg_time,
                    mob.extra.camel_sitting,
                    mob.extra.flag2
                ),
                unchanged
            );
        }
    }

    #[test]
    fn camel_saved_carry_leaves_the_seven_equine_paths_unchanged() {
        for (species, id) in [
            (Species::Horse, "minecraft:horse"),
            (Species::Donkey, "minecraft:donkey"),
            (Species::Mule, "minecraft:mule"),
            (Species::SkeletonHorse, "minecraft:skeleton_horse"),
            (Species::ZombieHorse, "minecraft:zombie_horse"),
            (Species::Llama, "minecraft:llama"),
            (Species::TraderLlama, "minecraft:trader_llama"),
        ] {
            let mut raw = input(id, Some(12));
            put(&mut raw, "Tame", Tag::Byte(1));
            let mob = mob_from_nbt(&raw, &types(species, id), 92, None).unwrap();
            assert_eq!(mob.extra.equine_temper, 12);
            assert!(!mob.extra.flag, "the other species' load path is unchanged");
            for source in [None, Some(&raw)] {
                let saved = mob_to_nbt(&mob, [92, 0, 0, 1], source, None, |_| None);
                assert_eq!(saved.get("Temper"), Some(&Tag::Int(12)));
                assert_eq!(saved.get("Tame"), Some(&Tag::Byte(1)));
                assert_eq!(saved.get("Bred"), Some(&Tag::Byte(1)));
                // Preserve the existing raw/default behavior outside Camel;
                // this control does not claim the other species' eating codec.
                assert_eq!(
                    saved.get("EatingHaystack"),
                    Some(&Tag::Byte(i8::from(source.is_some())))
                );
            }
        }
    }
}

#[cfg(test)]
mod equine_temper_preservation_tests {
    use super::*;
    use crate::mobs::Species;

    #[test]
    fn temper_preservation_decodes_and_saves_without_raw_input() {
        for (name, species) in [
            ("llama", Species::Llama),
            ("trader_llama", Species::TraderLlama),
            ("horse", Species::Horse),
        ] {
            let mut kind = crate::mobs::sensor_scan_tests::kind();
            let def = Arc::make_mut(&mut kind);
            def.id = format!("minecraft:{name}");
            def.species = species;
            let types = [(def.id.clone(), kind.clone())].into();
            for value in [50, 0, 30, 100, -7, 1000, i32::MIN, i32::MAX] {
                let raw = Tag::compound([
                    ("id", string(&format!("minecraft:{name}"))),
                    ("Pos", doubles(&[1.5, 200.0, 1.5])),
                    ("Temper", Tag::Int(value)),
                    ("Tame", Tag::Byte(1)),
                    ("NoAI", Tag::Byte(1)),
                    ("Strength", Tag::Int(3)),
                    ("ChestedHorse", Tag::Byte(1)),
                ]);
                let m = mob_from_nbt(&raw, &types, 7, None).unwrap();
                assert_eq!(m.species(), species);
                assert!(m.extra.tame && m.no_ai);
                let stale =
                    Tag::compound([("Temper", Tag::Int(17)), ("audit:unrelated", Tag::Int(9))]);
                for base in [None, Some(&raw), Some(&stale)] {
                    let saved = mob_to_nbt(&m, [93, 0, 0, 1], base, None, |_| None);
                    assert_eq!(
                        saved.get("Temper"),
                        Some(&Tag::Int(value)),
                        "{name}: loaded temper is not a gameplay limit"
                    );
                    assert_eq!(saved.get("Tame"), Some(&Tag::Byte(1)));
                    if species != Species::Horse {
                        assert_eq!(saved.get("Strength"), Some(&Tag::Int(3)));
                        assert_eq!(saved.get("ChestedHorse"), Some(&Tag::Byte(1)));
                    }
                    if base == Some(&stale) {
                        assert_eq!(saved.get("audit:unrelated"), Some(&Tag::Int(9)));
                    }
                }
            }
            let fresh = Mob::new(7, kind, Vec3::ZERO, 0.0);
            let saved = mob_to_nbt(&fresh, [93, 0, 0, 1], None, None, |_| None);
            assert_eq!(saved.get("Temper"), Some(&Tag::Int(0)));
            let absent = Tag::compound([
                ("id", string(&format!("minecraft:{name}"))),
                ("Pos", doubles(&[1.5, 200.0, 1.5])),
            ]);
            let decoded = mob_from_nbt(&absent, &types, 7, None).unwrap();
            assert_eq!(
                mob_to_nbt(&decoded, [0; 4], None, None, |_| None).get("Temper"),
                Some(&Tag::Int(0))
            );
        }
    }

    #[test]
    fn equine_and_ravager_attributes_parity() {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        Arc::make_mut(&mut kind).species = Species::Donkey;
        let types = [("minecraft:donkey".into(), kind.clone())].into();
        let donkey = mob_from_nbt(
            &Tag::compound([
                ("id", string("minecraft:donkey")),
                ("Pos", doubles(&[0.0, 0.0, 0.0])),
            ]),
            &types,
            1,
            None,
        )
        .unwrap();
        assert_eq!(donkey.extra.vector[0], 0.5);
        assert_eq!(
            mob_to_nbt(&donkey, [0; 4], None, None, |_| None).get("attributes"),
            Some(&Tag::List(vec![]))
        );
        Arc::make_mut(&mut kind).species = Species::Ravager;
        let types = [("minecraft:ravager".into(), kind)].into();
        // Before its first aiStep a roaring ravager has no movement_speed
        // instance; a saved base (mid-ramp after a roar) loads back as is.
        let speed = Tag::List(vec![Tag::compound([
            ("id", string("minecraft:movement_speed")),
            ("base", Tag::Double(0.10317f64.to_bits())),
        ])]);
        for (key, value, saved) in [
            ("RoarTick", Tag::Int(10), Tag::List(vec![])),
            ("attributes", speed.clone(), speed),
        ] {
            let pos = ("Pos", doubles(&[0.0, 0.0, 0.0]));
            let tag = Tag::compound([("id", string("minecraft:ravager")), pos, (key, value)]);
            let ravager = mob_from_nbt(&tag, &types, 2, None).unwrap();
            let nbt = mob_to_nbt(&ravager, [0; 4], None, None, |_| None);
            assert_eq!(nbt.get("attributes"), Some(&saved));
        }
    }
}
