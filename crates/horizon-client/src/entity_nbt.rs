//! Vanilla 26.3 entity NBT (`Entity.saveWithoutId` and the subclasses'
//! `addAdditionalSaveData`) for the entities Horizon simulates: mobs, item
//! entities, experience orbs, arrows and falling blocks. Encoding starts from
//! the compound the entity was loaded with, when there is one, so fields
//! Horizon does not model (variants, attributes, equipment, custom names)
//! survive a Horizon save.
use crate::{
    arrows::Pickup,
    entities::{ArmorStand, Cushion, ExperienceOrb, ItemEntity, ItemFrame, Painting},
    mobs::{Mob, MobType, Who},
    nbt_path::int_value,
    projectiles::{Projectile, ProjectileKind},
    save::{
        doubles, float, floats, item_from_nbt, item_to_nbt, number, put, read_doubles, text,
        uuid_from_tag,
    },
};
use horizon_core::geometry::Vec3;
use horizon_world::{
    chunk::BlockState,
    nbt::{Compound, Tag},
    registry::{BlockRegistry, BlockStateId},
};
use std::{collections::HashMap, sync::Arc};

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
pub fn rotation(tag: &Tag) -> [f32; 2] {
    read_doubles::<2>(tag.get("Rotation")).map_or([0.0; 2], |r| [r[0] as f32, r[1] as f32])
}
/// `NumericTag.byteValue`'s nonzero test for entity NBT, defaulting to false.
pub fn flag(tag: &Tag, key: &str) -> bool {
    // NBT floats floor before int narrowing; the final byte keeps the low bits.
    tag.get(key)
        .and_then(int_value)
        .is_some_and(|value| value as i8 != 0)
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
    use crate::mobs::{Species, drops::can_be_baby, finalize::ageable};
    s == Species::Tadpole || (ageable(s) && can_be_baby(s))
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
/// The `NeutralMob`s whose anger is `Extra::anger`: they write and read
/// `anger_end_time` and `angry_at`. The bee keeps its own anger, and a
/// piglin's is a brain memory, not `NeutralMob`'s.
fn persistent_anger(s: crate::mobs::Species) -> bool {
    use crate::mobs::Species as S;
    matches!(
        s,
        S::Wolf | S::IronGolem | S::PolarBear | S::Enderman | S::ZombifiedPiglin
    )
}
/// The single player's UUID (`offline_uuid("Player")`): the entity the
/// neutral mobs are angry at.
fn player_uuid() -> [i32; 4] {
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
fn who_uuid(who: Who, uuid_of: &mut impl FnMut(u32) -> Option<[i32; 4]>) -> Option<[i32; 4]> {
    match who {
        Who::Player => Some(player_uuid()),
        Who::Mob(id) => uuid_of(id),
    }
}
/// The entity a stored UUID names, while it is in the level: `player` (the
/// player's own UUID, when one is in the level), or the mob `uuids` gives that
/// UUID to. `uuids` is the game layer's id -> UUID map, so this is the read half
/// of the pair [`who_uuid`] and `mob_to_nbt`'s `uuid_of` form. A UUID nothing
/// loaded has, or one naming a kind `Who` cannot carry (an item, a projectile, a
/// guest), is `None` - never some other entity as a stand-in.
pub fn who_by_uuid(
    uuids: &HashMap<u32, [i32; 4]>,
    mobs: &[Mob],
    player: Option<[i32; 4]>,
    uuid: [i32; 4],
) -> Option<Who> {
    if player == Some(uuid) {
        return Some(Who::Player);
    }
    // Only a loaded mob: the map also holds projectiles, vehicles and items,
    // whose ids mean nothing to `Who`.
    uuids
        .iter()
        .find(|(id, u)| **u == uuid && mobs.iter().any(|m| m.id == **id))
        .map(|(id, _)| Who::Mob(*id))
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
        (
            "HurtTime",
            Tag::Short(m.hurt_time.clamp(0, i32::from(i16::MAX)) as i16),
        ),
        (
            "DeathTime",
            Tag::Short(m.death_time.clamp(0, i32::from(i16::MAX)) as i16),
        ),
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
            Tag::Long(if m.extra.anger_end > 0 {
                m.extra.anger_end
            } else {
                -1
            }),
        );
        if let Some(uuid) = m.extra.angry_at.and_then(|who| who_uuid(who, &mut uuid_of)) {
            put(&mut tag, "angry_at", Tag::IntArray(uuid.to_vec()));
        }
    }
    // `Mannequin.addAdditionalSaveData`: the profile whose skin it wears, whether it
    // is `immovable`, and its description (the default is not written; none is `hide_description`).
    if m.species() == crate::mobs::Species::Mannequin {
        if let Some(profile) = &m.profile {
            put(&mut tag, "profile", profile_nbt(profile));
        }
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
    // any (MobEffectInstance.CODEC; a mob's effects are never ambient, and one
    // given with `hideParticles` shows neither particles nor its icon).
    if m.extra.effects.is_empty() {
        map.remove(&horizon_world::nbt::JavaString::from("active_effects"));
    } else {
        let effects = m
            .extra
            .effects
            .iter()
            .map(|&(effect, duration, amplifier)| {
                let shown = !m.extra.hidden_effects.contains(&effect);
                let ambient = m.extra.ambient_effects.contains(&effect);
                let mut e = vec![("id", string(crate::brewing::namespaced(effect)))];
                // The codec leaves out an amplifier of 0 (its default).
                if amplifier != 0 {
                    e.push(("amplifier", Tag::Byte(amplifier.clamp(0, 255) as u8 as i8)));
                }
                e.push(("duration", Tag::Int(duration)));
                if ambient {
                    e.push(("ambient", Tag::Byte(1)));
                }
                if !shown {
                    e.push(("show_particles", Tag::Byte(0)));
                }
                e.push(("show_icon", Tag::Byte(i8::from(shown))));
                Tag::compound(e)
            });
        map.insert("active_effects".into(), Tag::List(effects.collect()));
    }
    map.insert(
        "Invulnerable".into(),
        Tag::Byte(i8::from(m.invulnerable_flag)),
    );
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
    for (key, value) in [
        ("AbsorptionAmount", Tag::Float(0)),
        ("FallFlying", Tag::Byte(0)),
        ("current_impulse_context_reset_grace_time", Tag::Int(0)),
        (
            "Brain",
            Tag::compound([("memories", Tag::Compound(Compound::new()))]),
        ),
    ] {
        map.entry(key.into()).or_insert(value);
    }
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
    } else if matches!(
        species,
        crate::mobs::Species::Parrot
            | crate::mobs::Species::Slime
            | crate::mobs::Species::MagmaCube
            | crate::mobs::Species::ZombieNautilus
    ) {
        // AgeableMob.addAdditionalSaveData writes nothing when canBeABaby is
        // false: drop the ignored values a raw input compound carried.
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
        "minecraft:goat" => {
            map.insert(
                "IsScreamingGoat".into(),
                Tag::Byte(i8::from(m.extra.screaming)),
            );
            let [left, right] = m.extra.lost_horns;
            map.insert("HasLeftHorn".into(), Tag::Byte(i8::from(!left)));
            map.insert("HasRightHorn".into(), Tag::Byte(i8::from(!right)));
        }
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
            // Only Piglin.addAdditionalSaveData writes CannotHunt.
            if id == "minecraft:piglin_brute" {
                map.remove(&"CannotHunt".into());
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
                write_guarded_piglin_anger(m, map);
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
            map.insert("Temper".into(), Tag::Int(i32::from(m.extra.variant3)));
            map.insert("Bred".into(), Tag::Byte(i8::from(m.extra.bred)));
            // AbstractHorse.addAdditionalSaveData; Horizon does not model the
            // eating haystack yet.
            map.entry("EatingHaystack".into()).or_insert(Tag::Byte(0));
            if let Some(owner) = m.extra.owner {
                map.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
            }
            if id == "minecraft:horse" {
                let v = i32::from(m.extra.variant) | i32::from(m.extra.variant2) << 8;
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
            let modelled = [
                ("minecraft:max_health", f64::from(m.attrs.max_health)),
                ("minecraft:movement_speed", m.attrs.speed),
                ("minecraft:jump_strength", m.extra.vector[0]),
            ];
            // AttributeMap.pack writes the instances that exist: the ones the
            // entity was loaded with, or all three randomized by finalizeSpawn.
            let attributes = match map.get(&"attributes".into()) {
                Some(Tag::List(list)) => list
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
                    .collect(),
                _ => modelled
                    .iter()
                    .map(|(id, v)| {
                        Tag::compound([("id", string(id)), ("base", Tag::Double(v.to_bits()))])
                    })
                    .collect(),
            };
            map.insert("attributes".into(), Tag::List(attributes));
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
            map.insert("VillagerDataFinalized".into(), Tag::Byte(1));
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
                memories.push((
                    "minecraft:liked_player",
                    Tag::compound([(
                        "value",
                        Tag::IntArray(m.extra.owner.unwrap_or([0, 0, 0, 1]).to_vec()),
                    )]),
                ));
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
            // Always written, so a forgotten note block does not come back
            // from the compound the allay was loaded with.
            map.insert(
                "Brain".into(),
                Tag::compound([("memories", Tag::compound(memories))]),
            );
        }
        "minecraft:camel" | "minecraft:camel_husk" => {
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
        "minecraft:bee" => crate::mobs::bee::to_nbt(m, map),
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
        "minecraft:warden" => {
            if let Some(st) = &m.extra.warden {
                map.insert("listener".into(), st.vibration.to_nbt());
                map.insert("anger".into(), Tag::compound(Vec::<(&str, Tag)>::new()));
            }
        }
        // Armadillo.addAdditionalSaveData: the time left to the next scute.
        "minecraft:armadillo" => {
            map.insert("scute_time".into(), Tag::Int(m.extra.egg_time));
        }
        "minecraft:ravager" => {
            map.insert("AttackTick".into(), Tag::Int(m.extra.counter));
            map.insert("StunTick".into(), Tag::Int(m.extra.counter2));
            map.insert("RoarTick".into(), Tag::Int(m.extra.anim_ticks));
        }
        "minecraft:vex" => {
            if let Some(h) = m.extra.home {
                map.insert("bound_pos".into(), Tag::IntArray(h.to_vec()));
            }
            if m.extra.egg_time > 0 {
                map.insert("life_ticks".into(), Tag::Int(m.extra.egg_time));
            }
        }
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
        // Phantom.addAdditionalSaveData.
        "minecraft:phantom" => {
            map.insert("size".into(), Tag::Int(m.extra.size));
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
        "minecraft:zombie" | "minecraft:husk" => {
            for (key, value) in [
                ("DrownedConversionTime", conversion_time(m)),
                ("InWaterTime", m.extra.counter),
            ] {
                map.entry(key.into()).or_insert(Tag::Int(value));
            }
        }
        "minecraft:skeleton" => {
            for (key, value) in [
                ("StrayConversionTime", conversion_time(m)),
                ("FreezingTime", m.extra.counter),
            ] {
                map.entry(key.into()).or_insert(Tag::Int(value));
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
    // The attribute their load makes, which AttributeMap.pack then saves:
    // Zombie and Piglin.setBaby (even for an adult) the speed, Rabbit.setVariant
    // the attack damage.
    use crate::mobs::finalize::Attr;
    // Every other living entity's first tick makes the movement_speed instance
    // (LivingEntity.removeFrost reads it through getAttribute).
    let made = if species == crate::mobs::Species::Rabbit {
        vec![Attr::MovementSpeed, Attr::AttackDamage]
    } else {
        vec![Attr::MovementSpeed]
    };
    for attr in made {
        let base = crate::mobs::finalize::base_value(m, attr);
        let mut list = match map.get(&"attributes".into()) {
            Some(Tag::List(l)) => l.clone(),
            _ => Vec::new(),
        };
        let attr_id = format!("minecraft:{}", attr.name());
        if !list
            .iter()
            .any(|t| text(t.get("id")).as_deref() == Some(attr_id.as_str()))
        {
            list.push(Tag::compound([
                ("id", string(&attr_id)),
                ("base", Tag::Double(base.to_bits())),
            ]));
            map.insert("attributes".into(), Tag::List(list));
        }
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
    tag
}
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
/// `Brain.Packed`: the memories finalizeSpawn set, over the saved ones.
pub fn put_brain(map: &mut Compound, m: &Mob, dimension: &str) {
    if m.extra.memories.is_empty() {
        return;
    }
    let mut memories = match map.get(&"Brain".into()).and_then(|b| b.get("memories")) {
        Some(Tag::Compound(c)) => c.clone(),
        _ => Compound::new(),
    };
    for &(name, value, ttl) in &m.extra.memories {
        let value = match name {
            "hunted_recently" => Tag::Byte(value as i8),
            // `Unit` memories save an empty compound.
            "is_pregnant" => Tag::Compound(Compound::new()),
            "home" => match m.extra.home {
                Some(p) => Tag::compound([
                    ("dimension", string(dimension)),
                    ("pos", Tag::IntArray(p.to_vec())),
                ]),
                None => continue,
            },
            _ => Tag::Int(value as i32),
        };
        let mut memory = vec![("value", value)];
        if ttl != i64::MAX {
            memory.push(("ttl", Tag::Long(ttl)));
        }
        memories.insert(
            format!("minecraft:{name}").as_str().into(),
            Tag::compound(memory),
        );
    }
    map.insert(
        "Brain".into(),
        Tag::compound([("memories", Tag::Compound(memories))]),
    );
}
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
/// Mob.finalizeSpawn's follow range bonus in the saved attributes.
pub(crate) fn spawn_bonus(tag: &Tag) -> Option<f64> {
    let Some(Tag::List(attrs)) = tag.get("attributes") else {
        return None;
    };
    let range = attrs
        .iter()
        .find(|a| text(a.get("id")).as_deref() == Some("minecraft:follow_range"))?;
    let Some(Tag::List(modifiers)) = range.get("modifiers") else {
        return None;
    };
    let bonus = modifiers
        .iter()
        .find(|x| text(x.get("id")).as_deref() == Some("minecraft:random_spawn_bonus"))?;
    float(bonus.get("amount"))
}
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
    let kind = types.get(&self::kind(tag)?)?.clone();
    let position = position(tag)?;
    let [yaw, pitch] = rotation(tag);
    let mut m = Mob::new(id, kind, position, yaw);
    m.velocity = motion(tag);
    m.head_pitch = pitch;
    read_mob_home(&mut m, tag);
    m.on_ground = flag(tag, "OnGround");
    m.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    // LivingEntity.readAdditionalSaveData: the saved attribute bases, then
    // the health, the max health when there is none.
    if let Some(Tag::List(attrs)) = tag.get("attributes") {
        for a in attrs {
            let (Some(id), Some(Tag::Double(bits))) = (text(a.get("id")), a.get("base")) else {
                continue;
            };
            let v = f64::from_bits(*bits);
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
    if let Some(bonus) = spawn_bonus(tag) {
        use crate::mobs::finalize::{Attr, Op, add_modifier};
        let id = "minecraft:random_spawn_bonus";
        add_modifier(&mut m, Attr::FollowRange, id, bonus, Op::AddMultipliedBase);
    }
    m.health = float(tag.get("Health")).map_or(m.attrs.max_health, |h| h as f32);
    m.absorption = float(tag.get("AbsorptionAmount"))
        .map_or(0.0, |h| h as f32)
        .max(0.0);
    // LivingEntity.setHealth clamps to the max health: a horse or llama is made
    // with the type's and saved before it drops to the one it rolled. (Only
    // the base is read here, so the clamp would cut a leader zombie's bonus.)
    if crate::mobs::equine::is_equine(m.species()) {
        m.health = m.health.min(m.attrs.max_health);
    }
    m.hurt_time = number(tag.get("HurtTime")).unwrap_or(0) as i32;
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
    m.death_time = number(tag.get("DeathTime")).unwrap_or(0) as i32;
    // Mob.readAdditionalSaveData overrides what the constructor set (copper
    // golems and elder guardians start persistent).
    m.persistent = flag(tag, "PersistenceRequired");
    m.extra.death_loot_table = text(tag.get("DeathLootTable"))
        .and_then(|t| horizon_client_core::identifier::normalize_id(&t));
    m.extra.death_loot_seed = number(tag.get("DeathLootTableSeed")).unwrap_or(0);
    // `NeutralMob.readPersistentAngerSaveData`: the anger end time and the
    // target come back with the mob.
    if persistent_anger(m.species()) {
        m.extra.anger_end = number(tag.get("anger_end_time")).unwrap_or(-1);
        if tag
            .get("angry_at")
            .is_some_and(|t| uuid_from_tag(Some(t)) == Some(player_uuid()))
        {
            m.extra.angry_at = Some(crate::mobs::Who::Player);
        }
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
    m.extra.pick_up_loot = flag(tag, "CanPickUpLoot");
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
            let amplifier = number(e.get("amplifier")).unwrap_or(0) as i32;
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
        }
    }
    m.silent = flag(tag, "Silent");
    m.invulnerable_flag = flag(tag, "Invulnerable");
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
        if m.species() == crate::mobs::Species::Hoglin {
            m.extra.cannot_be_hunted = flag(tag, "CannotBeHunted");
        }
        if m.species() == crate::mobs::Species::Piglin
            && let Some(Tag::List(items)) = tag.get("Inventory")
        {
            // InventoryCarrier.readInventoryFromTag (eight slots).
            m.extra.inventory = items
                .iter()
                .filter_map(crate::save::item_from_nbt)
                .take(8)
                .collect();
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
        m.extra.variant3 = number(tag.get("Temper")).unwrap_or(0).clamp(0, 100) as u8;
        m.extra.bred = flag(tag, "Bred");
        m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
        let v = number(tag.get("Variant")).unwrap_or(0) as i32;
        m.extra.variant = (v & 0xFF).clamp(0, 6) as u8;
        m.extra.variant2 = ((v >> 8) & 0xFF).clamp(0, 4) as u8;
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
                let (Some(id), Some(Tag::Double(bits))) = (text(a.get("id")), a.get("base")) else {
                    continue;
                };
                if id == "minecraft:jump_strength" {
                    m.extra.vector[0] = f64::from_bits(*bits);
                }
            }
        }
        if m.extra.vector[0] <= 0.0 {
            m.extra.vector[0] = 0.7;
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
        crate::mobs::Species::Goat => {
            m.extra.screaming = flag(tag, "IsScreamingGoat");
            let horn = |key| number(tag.get(key)).is_some_and(|v| v == 0);
            m.extra.lost_horns = [horn("HasLeftHorn"), horn("HasRightHorn")];
        }
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
        m.extra.sitting = flag(tag, "Sitting");
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
        m.extra.sitting = flag(tag, "Sitting");
    }
    if m.species() == crate::mobs::Species::Wolf {
        let health = m.health;
        m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
        let tame = m.extra.owner.is_some();
        crate::mobs::tame::set_tame(&mut m, tame);
        m.health = health;
        m.extra.sitting = flag(tag, "Sitting");
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
    m.fire_ticks = number(tag.get("Fire")).unwrap_or(0) as i32;
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
        let memory = |key: &str| {
            tag.get("Brain")
                .and_then(|b| b.get("memories"))
                .and_then(|b| b.get(key))
                .and_then(|l| l.get("value"))
        };
        m.extra.ints[2] =
            number(memory("minecraft:liked_noteblock_cooldown_ticks")).unwrap_or(0) as i32;
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
        if let (Some(st), Some(listener)) = (m.extra.warden.as_mut(), tag.get("listener")) {
            st.vibration = crate::sculk::Data::from_nbt(listener);
        }
    }
    // Armadillo.readAdditionalSaveData: keeps the constructor's time when absent.
    if m.species() == crate::mobs::Species::Armadillo
        && let Some(t) = number(tag.get("scute_time"))
    {
        m.extra.egg_time = t as i32;
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
        if let Some(Tag::IntArray(v)) = tag.get("bound_pos")
            && v.len() == 3
        {
            m.extra.home = Some([v[0], v[1], v[2]]);
        }
        m.extra.egg_time = number(tag.get("life_ticks")).map_or(-1, |v| v as i32);
    }
    // Sheep.readAdditionalSaveData: white and unsheared when absent.
    if m.species() == crate::mobs::Species::Sheep {
        m.extra.color = number(tag.get("Color")).map_or(0, |c| (c as u8) & 15);
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
        m.extra.variant3 = number(tag.get("AttachFace")).unwrap_or(0).clamp(0, 5) as u8;
        m.extra.counter = number(tag.get("Peek")).unwrap_or(0).clamp(0, 100) as i32;
        m.extra.color = number(tag.get("Color")).map_or(16, |c| c.clamp(0, 16) as u8);
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
        // Phantom.readAdditionalSaveData: setPhantomSize(size).
        crate::mobs::phantom::set_size(&mut m, number(tag.get("size")).unwrap_or(0) as i32);
    }
    // Zombie, Piglin and Zoglin.readAdditionalSaveData: IsBaby (the baby
    // zombies of trial spawners).
    if !crate::mobs::finalize::ageable(m.species()) && flag(tag, "IsBaby") {
        crate::mobs::finalize::set_baby(&mut m, true);
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
    crate::mobs::raider::read_nbt(&mut m, tag);
    crate::mobs::riders::read_nbt(&mut m, tag);
    // A mob saved mid-death finishes dying; one saved dead is dropped.
    if m.health <= 0.0 && m.death_time == 0 {
        return None;
    }
    // DeathTime only counts while dead.
    if m.health > 0.0 {
        m.death_time = 0;
    }
    Some(m)
}

/// `ItemEntity`: Health, Age, PickupDelay, Item, Thrower.
pub fn item_entity_to_nbt(e: &ItemEntity, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let base = Base {
        id: "minecraft:item",
        uuid,
        position: e.position,
        motion: e.velocity,
        rotation: e.rotation,
        on_ground: e.on_ground,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    for (key, value) in [
        ("Item", item_to_nbt(&e.stack)),
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
    match tag.get("explosion_power") {
        Some(Tag::Float(bits)) => f32::from_bits(*bits).clamp(0.0, 128.0),
        _ => 4.0,
    }
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
        fall_distance: 0.0,
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

/// `ItemEntity.readAdditionalSaveData`: Owner and Thrower use the numeric UUID codec.
pub fn item_entity_from_nbt(tag: &Tag, id: u32, bob_offset: f32) -> Option<ItemEntity> {
    let stack = item_from_nbt(tag.get("Item")?)?;
    let position = position(tag)?;
    Some(ItemEntity {
        rotation: rotation(tag),
        join: Default::default(),
        no_gravity: flag(tag, "NoGravity"),
        health: number(tag.get("Health")).unwrap_or(5) as i32,
        fire_ticks: number(tag.get("Fire")).unwrap_or(0) as i32,
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
        age: number(tag.get("Age")).unwrap_or(0) as i32,
        pickup_delay: number(tag.get("PickupDelay")).unwrap_or(0),
        bob_offset,
        ticks: 0,
        needs_sync: false,
        stuck: Vec3::ZERO,
        was_touching_water: false,
    })
}

/// `OminousItemSpawner.addAdditionalSaveData`: its item (left out when
/// empty) and `spawn_item_after_ticks`. Its tick count is the entity's
/// `tickCount`, which isn't saved: a reloaded spawner counts from 0 again.
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
        motion: Vec3::ZERO,
        rotation: [0.0, 0.0],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    // `ItemStack.CODEC` can't encode `ItemStack.EMPTY`, so an empty item
    // writes no key.
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

/// `ExperienceOrb`: Health, Age, Value, Count.
pub fn orb_to_nbt(o: &ExperienceOrb, uuid: [i32; 4]) -> Tag {
    let base = Base {
        id: "minecraft:experience_orb",
        uuid,
        position: o.position,
        motion: o.velocity,
        rotation: o.rotation,
        on_ground: o.on_ground,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(None);
    for (key, value) in [
        ("Health", Tag::Short(o.health as i16)),
        ("Age", Tag::Short(o.age.min(i16::MAX as u32) as i16)),
        ("Value", Tag::Short(o.value.min(i16::MAX as u32) as i16)),
        ("Count", Tag::Int(o.count.min(i32::MAX as u32) as i32)),
    ] {
        put(&mut tag, key, value);
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if o.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    }
    tag
}
/// One orb with its merged `Count`.
pub fn orbs_from_nbt(tag: &Tag) -> Vec<ExperienceOrb> {
    let Some(position) = position(tag) else {
        return Vec::new();
    };
    let count = number(tag.get("Count")).unwrap_or(1).max(1) as u32;
    let orb = ExperienceOrb {
        rotation: rotation(tag),
        join: Default::default(),
        no_gravity: flag(tag, "NoGravity"),
        id: 0,
        position,
        previous: position,
        velocity: motion(tag),
        value: number(tag.get("Value")).unwrap_or(1).max(1) as u32,
        age: number(tag.get("Age")).unwrap_or(0).max(0) as u32,
        on_ground: flag(tag, "OnGround"),
        count,
        ticks: 0,
        health: number(tag.get("Health")).unwrap_or(5) as i32,
    };
    vec![orb]
}

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
        on_ground: false,
        fall_distance: a.fall_distance,
    };
    let mut tag = base.tag(raw);
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
    }
    if let Some(weapon) = &a.fired_from_weapon {
        put(&mut tag, "weapon", item_to_nbt(weapon));
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
    let a = p.arrow.as_mut()?;
    a.life = number(tag.get("life")).unwrap_or(0) as i16 as i32;
    a.shake = (number(tag.get("shake")).unwrap_or(0) as i8 as u8).into();
    a.in_ground = flag(tag, "inGround");
    a.pickup = Pickup::from_byte(number(tag.get("pickup")).unwrap_or(0));
    a.pierce = number(tag.get("PierceLevel")).unwrap_or(0) as u8;
    a.has_been_shot = flag(tag, "HasBeenShot");
    a.fire_ticks = number(tag.get("Fire")).unwrap_or(0) as i16 as i32;
    a.fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    a.spectral = spectral;
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
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    put(
        &mut tag,
        "acceleration_power",
        Tag::Double(p.acceleration.to_bits()),
    );
    put_projectile(&mut tag, p, owner);
    // AbstractHurtingProjectile.tick: shouldBurn is true for both fireballs, so
    // once ticked they igniteForSeconds(1) every tick. WitherSkull.shouldBurn is
    // false, so a skull never saves a fire.
    if p.age > 0 && p.kind.is_item_fireball() {
        let fire = number(tag.get("Fire")).unwrap_or(0).max(20);
        put(&mut tag, "Fire", Tag::Short(fire as i16));
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
        | ProjectileKind::WindCharge
        | ProjectileKind::BreezeWindCharge
        | ProjectileKind::WitherSkull { .. }
        | ProjectileKind::Snowball
        | ProjectileKind::Egg
        | ProjectileKind::EnderPearl
        | ProjectileKind::Potion(..)
        | ProjectileKind::ExperienceBottle
        | ProjectileKind::LlamaSpit
        | ProjectileKind::ShulkerBullet { target: None, .. }
        | ProjectileKind::Thrown(_) => true,
        ProjectileKind::Arrow { .. } => p.arrow.is_some(),
        // Not written yet: tridents save from their own list, and a bullet
        // with a target would lose it.
        ProjectileKind::Trident
        | ProjectileKind::ShulkerBullet {
            target: Some(_), ..
        } => false,
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
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    put_projectile(&mut tag, p, owner);
    // Entity.saveWithoutId writes NoGravity only when set.
    if p.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    }
    // the loaded item stays (a blue egg's, a renamed one's, a potion's
    // effects); when there is none, `getItem`'s default is written.
    if tag.get("Item").is_none() {
        let stack = match &p.kind {
            ProjectileKind::Potion(stack, _) => (**stack).clone(),
            _ => crate::items::ItemStack::new(item, 1),
        };
        put(&mut tag, "Item", item_to_nbt(&stack));
    }
    Some(tag)
}

/// `ThrowableItemProjectile.readAdditionalSaveData` for the kinds
/// [`throwable_to_nbt`] writes; the owner is the player when it is theirs.
pub fn throwable_from_nbt(tag: &Tag, player: Option<[i32; 4]>) -> Option<Projectile> {
    let mut kind = throwable_kind(&kind(tag)?)?;
    // `readAdditionalSaveData` sets the `Item` it flies as, potion effects
    // and all; a tag without one keeps `getItem`'s default.
    if let ProjectileKind::Potion(stack, _) = &mut kind
        && let Some(item) = tag.get("Item").and_then(item_from_nbt)
    {
        **stack = item;
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
    Some(p)
}

/// `LlamaSpit` and `ShulkerBullet` as `/summon` or a chunk loads them:
/// `Projectile`'s `Owner`, `LeftOwner`, `NoGravity` and the motion. A bullet
/// loaded without a `Target` has none, so it falls (`ShulkerBullet.tick`).
pub fn spit_or_bullet_from_nbt(tag: &Tag, player: Option<[i32; 4]>) -> Option<Projectile> {
    let kind = match kind(tag)?.as_str() {
        "minecraft:llama_spit" => ProjectileKind::LlamaSpit,
        "minecraft:shulker_bullet" => ProjectileKind::ShulkerBullet {
            target: None,
            dir: None,
            steps: 0,
            delta: [0.0; 3],
            seed: 0,
        },
        _ => return None,
    };
    let owner = owner_uuid(tag);
    let from_player = player.is_some() && owner == player;
    let mut p = Projectile::new(kind, position(tag)?, from_player.then_some(Who::Player));
    p.velocity = motion(tag);
    [p.yaw, p.pitch] = rotation(tag);
    p.left_owner = flag(tag, "LeftOwner");
    p.has_been_shot = flag(tag, "HasBeenShot");
    p.owner_uuid = owner;
    p.no_gravity = flag(tag, "NoGravity");
    Some(p)
}

/// The save of [`spit_or_bullet_from_nbt`]'s kinds: `Projectile`'s keys.
pub fn spit_or_bullet_to_nbt(
    p: &Projectile,
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
    raw: Option<&Tag>,
) -> Option<Tag> {
    if !matches!(
        p.kind,
        ProjectileKind::LlamaSpit | ProjectileKind::ShulkerBullet { target: None, .. }
    ) {
        return None;
    }
    let base = Base {
        id: p.kind.id(),
        uuid,
        position: p.position,
        motion: p.velocity,
        rotation: [p.yaw, p.pitch],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    put_projectile(&mut tag, p, owner);
    Some(tag)
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
            power: number(tag.get("ExplosionPower")).unwrap_or(1) as i32,
        },
        "minecraft:small_fireball" => ProjectileKind::SmallFireball,
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
    p.acceleration = float(tag.get("acceleration_power")).unwrap_or(0.1);
    if let Some(item) = tag.get("Item").and_then(item_from_nbt) {
        p.set_item(item);
    }
    Some(p)
}

/// `FireworkRocketEntity.readAdditionalSaveData`: Life, LifeTime (0 when
/// absent: a bare `/summon` explodes at once), FireworksItem and
/// ShotAtAngle, with `Projectile`'s Owner and LeftOwner.
pub struct RocketNbt {
    pub position: Vec3,
    pub motion: Vec3,
    pub life: i32,
    pub lifetime: i32,
    pub item: Option<crate::items::ItemStack>,
    pub angled: bool,
    pub owner: Option<Who>,
    pub left_owner: bool,
}
pub fn firework_rocket_from_nbt(tag: &Tag, player: Option<[i32; 4]>) -> Option<RocketNbt> {
    let owner = uuid_from_tag(tag.get("Owner"));
    Some(RocketNbt {
        position: position(tag)?,
        motion: motion(tag),
        life: number(tag.get("Life")).unwrap_or(0) as i32,
        lifetime: number(tag.get("LifeTime")).unwrap_or(0) as i32,
        item: tag.get("FireworksItem").and_then(item_from_nbt),
        angled: flag(tag, "ShotAtAngle"),
        owner: (player.is_some() && owner == player).then_some(Who::Player),
        left_owner: flag(tag, "LeftOwner"),
    })
}

/// What `FireworkRocketEntity.addAdditionalSaveData` writes, over `Projectile`'s
/// Owner and LeftOwner.
pub struct RocketSave<'a> {
    pub rotation: [f32; 2],
    pub position: Vec3,
    pub motion: Vec3,
    pub life: i32,
    pub lifetime: i32,
    pub item: &'a crate::items::ItemStack,
    pub angled: bool,
    pub owner: Option<[i32; 4]>,
    pub left_owner: bool,
}
pub fn firework_rocket_to_nbt(r: &RocketSave, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let mut tag = Base {
        id: "minecraft:firework_rocket",
        uuid,
        position: r.position,
        motion: r.motion,
        rotation: r.rotation,
        on_ground: false,
        fall_distance: 0.0,
    }
    .tag(raw);
    for (key, value) in [
        ("Life", Tag::Int(r.life)),
        ("LifeTime", Tag::Int(r.lifetime)),
        ("FireworksItem", item_to_nbt(r.item)),
        ("ShotAtAngle", Tag::Byte(i8::from(r.angled))),
        ("HasBeenShot", Tag::Byte(1)),
    ] {
        put(&mut tag, key, value);
    }
    if r.left_owner {
        put(&mut tag, "LeftOwner", Tag::Byte(1));
    }
    if let Some(owner) = r.owner {
        put(&mut tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
    tag
}

/// A thrown trident as `AbstractArrow` + `ThrownTrident` save it.
pub struct TridentSave<'a> {
    pub position: Vec3,
    pub motion: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub in_ground: bool,
    pub life: i32,
    pub shake: u32,
    pub dealt_damage: bool,
    pub creative_only: bool,
    pub no_gravity: bool,
    pub fire_ticks: i32,
    pub item: &'a crate::items::ItemStack,
}
pub fn trident_to_nbt(t: &TridentSave, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let mut tag = Base {
        id: "minecraft:trident",
        uuid,
        position: t.position,
        motion: t.motion,
        rotation: [t.yaw, t.pitch],
        on_ground: false,
        fall_distance: 0.0,
    }
    .tag(raw);
    // Projectile.addAdditionalSaveData: LeftOwner once set. A trident keeps no
    // owner, and an ownerless projectile has left it on its first tick.
    if let Tag::Compound(map) = &mut tag {
        map.entry("LeftOwner".into()).or_insert(Tag::Byte(1));
    }
    for (key, value) in [
        ("life", Tag::Short(t.life as i16)),
        ("shake", Tag::Byte(t.shake as i8)),
        ("inGround", Tag::Byte(i8::from(t.in_ground))),
        // AbstractArrow.Pickup: 0 disallowed, 1 allowed, 2 creative only.
        ("pickup", Tag::Byte(if t.creative_only { 2 } else { 1 })),
        ("damage", Tag::Double(2.0_f64.to_bits())),
        ("HasBeenShot", Tag::Byte(1)),
        ("crit", Tag::Byte(0)),
        ("PierceLevel", Tag::Byte(0)),
        ("SoundEvent", string("minecraft:item.trident.hit_ground")),
        ("item", item_to_nbt(t.item)),
        ("DealtDamage", Tag::Byte(i8::from(t.dealt_damage))),
        ("Fire", Tag::Short(t.fire_ticks as i16)),
    ] {
        put(&mut tag, key, value);
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if t.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"NoGravity".into());
    }
    tag
}
/// (position, motion, yaw, pitch, in ground, life, shake, dealt damage,
/// creative only, the trident item)
pub struct TridentNbt {
    pub position: Vec3,
    pub motion: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub in_ground: bool,
    pub life: i32,
    pub shake: u32,
    pub dealt_damage: bool,
    pub creative_only: bool,
    pub no_gravity: bool,
    pub fire_ticks: i32,
    pub item: crate::items::ItemStack,
}
pub fn trident_from_nbt(tag: &Tag) -> Option<TridentNbt> {
    let [yaw, pitch] = rotation(tag);
    Some(TridentNbt {
        position: position(tag)?,
        motion: motion(tag),
        yaw,
        pitch,
        in_ground: flag(tag, "inGround"),
        life: number(tag.get("life")).unwrap_or(0) as i32,
        shake: number(tag.get("shake")).unwrap_or(0).max(0) as u32,
        dealt_damage: flag(tag, "DealtDamage"),
        creative_only: number(tag.get("pickup")) == Some(2),
        no_gravity: flag(tag, "NoGravity"),
        fire_ticks: number(tag.get("Fire")).unwrap_or(0) as i32,
        item: tag
            .get("item")
            .and_then(item_from_nbt)
            .unwrap_or_else(|| crate::items::ItemStack::new("minecraft:trident", 1)),
    })
}

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
    rotation: [f32; 2],
    uuid: [i32; 4],
) -> Tag {
    let base = Base {
        id: "minecraft:falling_block",
        uuid,
        position,
        motion,
        rotation,
        on_ground: false,
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
        tag.get("BlockState")?.clone(),
        position(tag)?,
        motion(tag),
        number(tag.get("Time")).unwrap_or(0) as i32,
        float(tag.get("fall_distance")).unwrap_or(0.0),
    ))
}

/// `NoGravity` and `DropItem` (true when absent) of a saved falling block.
pub fn falling_block_flags(tag: &Tag) -> (bool, bool) {
    (
        flag(tag, "NoGravity"),
        number(tag.get("DropItem")).is_none_or(|v| v != 0),
    )
}

/// Painting.addAdditionalSaveData: `facing`, `variant` and `block_pos`.
pub fn painting_to_nbt(p: &Painting, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let (lo, hi) = p.bounds();
    let base = Base {
        id: "minecraft:painting",
        uuid,
        position: Vec3::new(
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ),
        motion: Vec3::ZERO,
        rotation: [f32::from(p.facing) * 90.0, 0.0],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    put(&mut tag, "facing", Tag::Byte(p.facing as i8));
    put(&mut tag, "variant", string(&p.variant));
    put(&mut tag, "block_pos", Tag::IntArray(p.pos.to_vec()));
    tag
}
/// `BlockAttachedEntity.readAdditionalSaveData`: `block_pos` when saved; a
/// summoned one has none, and `setPos` puts its block at the block holding
/// the summon position.
fn attached_block_pos(tag: &Tag) -> Option<[i32; 3]> {
    match tag.get("block_pos") {
        Some(Tag::IntArray(v)) if v.len() == 3 => Some([v[0], v[1], v[2]]),
        _ => {
            let at = position(tag)?;
            Some([
                at.x.floor() as i32,
                at.y.floor() as i32,
                at.z.floor() as i32,
            ])
        }
    }
}
/// The block position, facing and variant of a saved painting.
pub fn painting_from_nbt(tag: &Tag) -> Option<([i32; 3], u8, String)> {
    let pos = attached_block_pos(tag)?;
    let facing = (number(tag.get("facing")).unwrap_or(0) & 3) as u8;
    Some((pos, facing, text(tag.get("variant"))?))
}
/// Cushion.addAdditionalSaveData: the block it sits on and its colour.
pub fn cushion_to_nbt(c: &Cushion, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let base = Base {
        id: "minecraft:cushion",
        uuid,
        position: c.position,
        motion: Vec3::ZERO,
        rotation: [c.yaw, 0.0],
        on_ground: false,
        fall_distance: 0.0,
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

#[cfg(test)]
mod cushion_tests {
    use super::*;

    #[test]
    fn cushion_save_preserves_color_anchor_rotation_and_unknown_data() {
        let cushion = Cushion {
            id: 7,
            position: Vec3::new(-1.5, 64.5, 3.5),
            pos: [-2, 64, 3],
            yaw: 270.0,
            color: 14,
            check_interval: 55,
            custom_name: None,
        };
        let raw = Tag::compound([("CustomName", string("Seat"))]);
        let tag = cushion_to_nbt(&cushion, [1, 2, 3, 4], Some(&raw));
        assert_eq!(text(tag.get("CustomName")).as_deref(), Some("Seat"));
        let loaded = cushion_from_nbt(&tag, 8).unwrap();
        assert_eq!(loaded.id, 8);
        assert_eq!(loaded.position, cushion.position);
        assert_eq!(loaded.pos, cushion.pos);
        assert_eq!(loaded.yaw, cushion.yaw);
        assert_eq!(loaded.color_name(), "red");
        assert_eq!(loaded.check_interval, 0);
        assert_eq!(loaded.bounds(), cushion.bounds());
    }
}
/// ItemFrame.addAdditionalSaveData.
pub fn item_frame_to_nbt(f: &ItemFrame, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let (lo, hi) = f.bounds();
    let base = Base {
        id: f.kind(),
        uuid,
        position: Vec3::new(
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ),
        motion: Vec3::ZERO,
        rotation: [0.0, 0.0],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(raw);
    // `Entity.saveWithoutId`: `CustomName` before the frame's own fields.
    if let Some(name) = &f.custom_name {
        put(&mut tag, "CustomName", name.clone());
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"CustomName".into());
    }
    put(&mut tag, "Facing", Tag::Byte(f.facing as i8));
    put(&mut tag, "block_pos", Tag::IntArray(f.pos.to_vec()));
    put(&mut tag, "ItemRotation", Tag::Byte(f.rotation as i8));
    put(
        &mut tag,
        "ItemDropChance",
        Tag::Float(f.drop_chance.to_bits()),
    );
    put(&mut tag, "Invisible", Tag::Byte(i8::from(f.invisible)));
    put(&mut tag, "Fixed", Tag::Byte(i8::from(f.fixed)));
    if let Tag::Compound(map) = &mut tag {
        map.remove(&horizon_world::nbt::JavaString::from("Item"));
    }
    if let Some(item) = &f.item {
        put(&mut tag, "Item", item_to_nbt(item));
    }
    tag
}
/// `ItemFrame.readAdditionalSaveData`: an absent Facing defaults to Direction.DOWN.
pub fn item_frame_from_nbt(tag: &Tag, id: u32) -> Option<ItemFrame> {
    let pos = attached_block_pos(tag)?;
    Some(ItemFrame {
        id,
        pos,
        facing: number(tag.get("Facing")).unwrap_or(0).clamp(0, 5) as u8,
        glow: kind(tag).as_deref() == Some("minecraft:glow_item_frame"),
        item: tag
            .get("Item")
            .and_then(item_from_nbt)
            .map(|s| crate::items::ItemStack { count: 1, ..s }),
        rotation: (number(tag.get("ItemRotation")).unwrap_or(0) & 7) as u8,
        drop_chance: float(tag.get("ItemDropChance")).map_or(1.0, |v| v as f32),
        fixed: flag(tag, "Fixed"),
        invisible: flag(tag, "Invisible"),
        check_interval: 0,
        custom_name: tag.get("CustomName").cloned(),
    })
}
/// ArmorStand.addAdditionalSaveData with LivingEntity's `equipment`.
pub fn armor_stand_to_nbt(a: &ArmorStand, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
    let base = Base {
        id: "minecraft:armor_stand",
        uuid,
        position: a.position,
        motion: a.velocity,
        rotation: [a.yaw, a.pitch],
        on_ground: a.on_ground,
        fall_distance: a.enchantment_fall_distance,
    };
    let mut tag = base.tag(raw);
    let equipment: Vec<(&str, Tag)> = ArmorStand::SLOTS
        .iter()
        .zip(&a.equipment)
        .filter_map(|(slot, stack)| Some((*slot, item_to_nbt(stack.as_ref()?))))
        .collect();
    // `equipment` is saved only when something is worn, `Marker` only when set.
    if equipment.is_empty() {
        if let Tag::Compound(map) = &mut tag {
            map.remove(&"equipment".into());
        }
    } else {
        put(&mut tag, "equipment", Tag::compound(equipment));
    }
    put(&mut tag, "DisabledSlots", Tag::Int(a.disabled_slots));
    for (key, value) in [
        ("Small", a.small),
        ("ShowArms", a.show_arms),
        ("NoBasePlate", a.no_base_plate),
        ("Invisible", a.invisible),
    ] {
        put(&mut tag, key, Tag::Byte(i8::from(value)));
    }
    if a.marker {
        put(&mut tag, "Marker", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"Marker".into());
    }
    // Entity.saveWithoutId writes NoGravity only when set.
    if a.no_gravity {
        put(&mut tag, "NoGravity", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"NoGravity".into());
    }
    if a.name_visible {
        put(&mut tag, "CustomNameVisible", Tag::Byte(1));
    } else if let Tag::Compound(map) = &mut tag {
        map.remove(&"CustomNameVisible".into());
    }
    // A name from the save keeps its own shape unless it changed.
    if let Some(name) = &a.custom_name
        && !tag.get("CustomName").is_some_and(|t| name_text(t) == *name)
    {
        put(
            &mut tag,
            "CustomName",
            crate::scoreboard::json_to_tag(&name.component()),
        );
    }
    put(&mut tag, "Health", Tag::Float(a.health.to_bits()));
    put(&mut tag, "Fire", Tag::Short(a.fire_ticks as i16));
    // ArmorStand.addAdditionalSaveData: the `Pose` parts that are not default.
    let pose: Vec<(&str, Tag)> = ArmorStand::POSE_KEYS
        .into_iter()
        .zip(a.pose)
        .zip(ArmorStand::DEFAULT_POSE)
        .filter(|((_, rotation), default)| rotation != default)
        .map(|((key, rotation), _)| (key, floats(&rotation)))
        .collect();
    put(&mut tag, "Pose", Tag::compound(pose));
    // LivingEntity.addAdditionalSaveData and ArmorStand's own fields.
    if let Tag::Compound(map) = &mut tag {
        for (key, value) in [
            ("HurtTime", Tag::Short(0)),
            ("DeathTime", Tag::Short(0)),
            ("AbsorptionAmount", Tag::Float(0)),
            ("FallFlying", Tag::Byte(0)),
            ("current_impulse_context_reset_grace_time", Tag::Int(0)),
            // AttributeMap.pack: the movement_speed instance the stand's first
            // tick makes (removeFrost), at the registry default.
            (
                "attributes",
                Tag::List(if a.speed_instance {
                    vec![Tag::compound([
                        ("id", string(SPEED_ATTRIBUTE)),
                        ("base", Tag::Double(0.7f64.to_bits())),
                    ])]
                } else {
                    Vec::new()
                }),
            ),
            (
                "Brain",
                Tag::compound([("memories", Tag::Compound(Compound::new()))]),
            ),
            ("DisabledSlots", Tag::Int(0)),
            ("Pose", Tag::Compound(Compound::new())),
        ] {
            map.entry(key.into()).or_insert(value);
        }
    }
    tag
}
/// The attribute every living entity's first tick makes (`removeFrost`).
const SPEED_ATTRIBUTE: &str = "minecraft:movement_speed";
pub fn armor_stand_from_nbt(tag: &Tag, id: u32) -> Option<ArmorStand> {
    let mut a = ArmorStand::new(id, position(tag)?, rotation(tag)[0]);
    a.enchantment_fall_distance = float(tag.get("fall_distance")).unwrap_or(0.0);
    // AttributeMap.load makes an instance for each saved attribute.
    a.speed_instance = matches!(tag.get("attributes"), Some(Tag::List(list))
        if list.iter().any(|attr| text(attr.get("id")).as_deref() == Some(SPEED_ATTRIBUTE)));
    a.velocity = motion(tag);
    a.health = float(tag.get("Health")).unwrap_or(20.0) as f32;
    a.fire_ticks = number(tag.get("Fire")).unwrap_or(0) as i32;
    a.custom_name = tag.get("CustomName").map(name_text);
    a.name_visible = flag(tag, "CustomNameVisible");
    if let Some(eq) = tag.get("equipment") {
        for (i, slot) in ArmorStand::SLOTS.iter().enumerate() {
            a.equipment[i] = eq.get(slot).and_then(item_from_nbt);
        }
    }
    a.small = flag(tag, "Small");
    a.show_arms = flag(tag, "ShowArms");
    a.disabled_slots = number(tag.get("DisabledSlots")).unwrap_or(0) as i32;
    a.no_base_plate = flag(tag, "NoBasePlate");
    a.invisible = flag(tag, "Invisible");
    a.marker = flag(tag, "Marker");
    a.no_gravity = flag(tag, "NoGravity");
    a.on_ground = flag(tag, "OnGround");
    // ArmorStand.readAdditionalSaveData: each part of `Pose` that is a list of
    // three numbers, the others staying at their defaults.
    if let Some(pose) = tag.get("Pose") {
        for (rotation, key) in a.pose.iter_mut().zip(ArmorStand::POSE_KEYS) {
            if let Some(Tag::List(values)) = pose.get(key)
                && values.len() == 3
            {
                for (r, v) in rotation.iter_mut().zip(values) {
                    *r = float(Some(v)).unwrap_or(0.0) as f32;
                }
            }
        }
    }
    Some(a)
}
/// A boat entity (`minecraft:<wood>_boat`, `minecraft:bamboo_chest_raft`...):
/// chest boats keep their slots in `Items` (`ContainerHelper.saveAllItems`).
pub fn boat_to_nbt(
    id: &str,
    position: Vec3,
    motion: Vec3,
    rotation: [f32; 2],
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
        on_ground: false,
        fall_distance: 0.0,
    }
    .tag(raw);
    if let (Some(items), Tag::Compound(map)) = (items, &mut tag) {
        map.insert("Items".into(), crate::save::encode_items(items));
    }
    tag
}
/// A vehicle's base entity tag (boats and minecarts).
pub fn vehicle_to_nbt(
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
        on_ground: false,
        fall_distance: 0.0,
    }
    .tag(raw)
}
pub fn boat_from_nbt(tag: &Tag) -> Option<(Vec3, Vec3, f32)> {
    Some((position(tag)?, motion(tag), rotation(tag)[0]))
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
        // And it round-trips through the save, which writes it only when it is
        // not the default.
        let saved = tnt_to_nbt(
            Vec3::ZERO,
            Vec3::ZERO,
            false,
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
    #[test]
    fn the_owner_round_trips_as_a_uuid() {
        let save = |owner, raw: Option<&Tag>| {
            let t = string("minecraft:tnt");
            tnt_to_nbt(
                Vec3::ZERO,
                Vec3::ZERO,
                false,
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
mod spell_ticks_nbt_tests {
    use super::*;
    use crate::mobs::Species;

    fn caster(species: Species, name: &str) -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.species = species;
        ty.id = format!("minecraft:{name}");
        Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0)
    }

    /// `SpellcasterIllager.readAdditionalSaveData`: bind loaded values to the
    /// counter already used by casting goals and customServerAiStep.
    #[test]
    fn spell_ticks_load_into_the_live_counter() {
        for (species, name) in [
            (Species::Evoker, "evoker"),
            (Species::Illusioner, "illusioner"),
        ] {
            let m = caster(species, name);
            let types = HashMap::from([(m.kind.id.clone(), m.kind.clone())]);
            for value in [None, Some(30), Some(0), Some(-2)] {
                let mut raw = Tag::compound([
                    ("id", string(&m.kind.id)),
                    ("Pos", doubles(&[0.5, 64.0, 0.5])),
                ]);
                if let Some(value) = value {
                    put(&mut raw, "SpellTicks", Tag::Int(value));
                }
                let loaded = mob_from_nbt(&raw, &types, 2, None).unwrap();
                assert_eq!(
                    loaded.extra.counter,
                    value.unwrap_or(0),
                    "{name}: {value:?}"
                );
                let saved = mob_to_nbt(&loaded, [1, 2, 3, 4], None, None, |_| None);
                assert_eq!(saved.get("SpellTicks"), Some(&Tag::Int(value.unwrap_or(0))));
            }
        }
    }

    /// `SpellcasterIllager.addAdditionalSaveData`: live state wins even when
    /// no raw input exists or the original raw counter has become stale.
    #[test]
    fn spell_ticks_save_the_mutated_model_without_a_raw_fallback() {
        for (species, name) in [
            (Species::Evoker, "evoker"),
            (Species::Illusioner, "illusioner"),
        ] {
            let mut m = caster(species, name);
            let raw = Tag::compound([
                ("SpellTicks", Tag::Int(30)),
                ("audit:unrelated", Tag::Int(7)),
            ]);
            for value in [29, 27, 20, 0, -2] {
                m.extra.counter = value;
                for raw in [None, Some(&raw)] {
                    let saved = mob_to_nbt(&m, [1, 2, 3, 4], raw, None, |_| None);
                    assert_eq!(saved.get("SpellTicks"), Some(&Tag::Int(value)), "{name}");
                    if raw.is_some() {
                        assert_eq!(saved.get("audit:unrelated"), Some(&Tag::Int(7)));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemStack;

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
    fn cubes_omit_supplied_age_and_pigs_keep_modeled_age() {
        use crate::mobs::Species;
        for (species, id) in [
            (Species::Slime, "minecraft:slime"),
            (Species::MagmaCube, "minecraft:magma_cube"),
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
                    crate::mobs::MobEvent::Explode(_, radius, _) => Some(*radius),
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
                rotation: [0.0, 0.0],
                position: Vec3::new(1.5, 70.0, -3.25),
                motion: Vec3::new(0.0, 0.5, 0.0),
                life: 7,
                lifetime: 31,
                item: &item,
                angled: true,
                owner: None,
                left_owner: false,
            },
            [1, 2, 3, 4],
            None,
        );
        let r = firework_rocket_from_nbt(&tag, None).unwrap();
        assert_eq!((r.life, r.lifetime, r.angled), (7, 31, true));
        assert_eq!(r.position, Vec3::new(1.5, 70.0, -3.25));
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
                creative_only: true,
                no_gravity: true,
                fire_ticks: 0,
                item: &trident,
            },
            [1, 2, 3, 4],
            None,
        );
        let t = trident_from_nbt(&tag).unwrap();
        // `Entity.saveWithoutId` writes NoGravity only when set.
        assert!(t.no_gravity);
        assert!(matches!(tag.get("NoGravity"), Some(Tag::Byte(1))));
        assert_eq!((t.in_ground, t.life, t.shake), (true, 9, 3));
        assert_eq!((t.dealt_damage, t.creative_only), (true, true));
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
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            health: 2,
            fire_ticks: 73,
            thrower: Some([5, 6, 7, 8]),
            target: None,
            id: 7,
            stack: ItemStack {
                item: Arc::from("minecraft:diamond_pickaxe"),
                count: 1,
                damage: 5,
                extra: None,
            },
            position: Vec3::new(100.5, 64.0, -31.5),
            previous: Vec3::ZERO,
            velocity: Vec3::new(0.0, -0.04, 0.0),
            on_ground: true,
            vertical_collision: false,
            horizontal_collision: false,
            age: 120,
            pickup_delay: 10,
            ticks: 0,
            bob_offset: 0.0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
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
            [1; 4],
            None,
        );
        assert_eq!(rotation(&cart), [90.0, 10.0]);
    }

    #[test]
    fn entities_without_fire_save_zero_and_a_mob_saves_its_speed_instance() {
        // Entity.remainingFireTicks starts at 0; LivingEntity.removeFrost makes
        // the movement_speed instance AttributeMap.pack then saves.
        let orb = ExperienceOrb {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: Vec3::ZERO,
            previous: Vec3::ZERO,
            velocity: Vec3::ZERO,
            value: 1,
            age: 0,
            on_ground: false,
            count: 1,
            ticks: 0,
            health: 5,
        };
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
    fn ghast_and_trident_save_their_always_present_keys() {
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
                creative_only: false,
                no_gravity: false,
                fire_ticks: 0,
                item: &trident,
            },
            [0; 4],
            None,
        );
        assert_eq!(tag.get("LeftOwner"), Some(&Tag::Byte(1)));
    }
    #[test]
    fn orbs_keep_their_count() {
        let o = ExperienceOrb {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: Vec3::new(1.0, 2.0, 3.0),
            previous: Vec3::ZERO,
            velocity: Vec3::ZERO,
            value: 7,
            age: 3,
            on_ground: false,
            count: 3,
            ticks: 0,
            health: 5,
        };
        let tag = orb_to_nbt(&o, [0; 4]);
        let back = orbs_from_nbt(&tag);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].value, 7);
        assert_eq!(back[0].count, 3);
    }
}

/// `Brain.Packed`: the guarded-block live ANGRY_AT entry. The existing
/// anger counter supplies its remaining long TTL, not an absolute timestamp.
fn write_guarded_piglin_anger(m: &Mob, map: &mut Compound) {
    let Some(actor) = m.extra.piglin.guarded_angry_at else {
        return;
    };
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
    if let Some(uuid) = actor.filter(|_| m.extra.anger > 0) {
        // ExpirableValue.codec: the UUID value and optional remaining Long TTL.
        memories.insert(
            "minecraft:angry_at".into(),
            Tag::compound([
                ("value", Tag::IntArray(uuid.to_vec())),
                ("ttl", Tag::Long(i64::from(m.extra.anger))),
            ]),
        );
    } else {
        memories.remove(&"minecraft:angry_at".into());
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
    // AbstractFish/Axolotl.readAdditionalSaveData: false when absent.
    // Tadpole.setFromBucket ignores the input.
    if matches!(
        m.species(),
        S::Cod | S::Salmon | S::Pufferfish | S::TropicalFish | S::Axolotl
    ) {
        m.extra.from_bucket = flag(tag, "FromBucket");
    }
    let int = |key| number(tag.get(key)).unwrap_or(0) as i32;
    match m.species() {
        S::Axolotl | S::Parrot => m.extra.variant = int("Variant").clamp(0, 4) as u8,
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
        (zombie.extra.counter, zombie.extra.counter2) = (600, 300);
        let (back, tag) = round_trip(&zombie);
        assert_eq!(number(tag.get("InWaterTime")), Some(600));
        assert_eq!(number(tag.get("DrownedConversionTime")), Some(299));
        assert_eq!((back.extra.counter, back.extra.counter2), (600, 300));
        let mut skeleton = new("minecraft:skeleton");
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
            Some(([28, 80, 6], 3, "minecraft:kebab".to_string()))
        );
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
                    .or_else(|| spit_or_bullet_from_nbt(&raw, None))
                    .unwrap();
                assert_eq!(p.has_been_shot, shot != 0);
                for age in [0, 12] {
                    p.age = age;
                    let tag = throwable_to_nbt(&p, [1, 2, 3, 4], None, Some(&raw))
                        .or_else(|| fireball_to_nbt(&p, [1, 2, 3, 4], None, Some(&raw)))
                        .or_else(|| spit_or_bullet_to_nbt(&p, [1, 2, 3, 4], None, Some(&raw)))
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
            let p = spit_or_bullet_from_nbt(&summoned(id, &[]), None).unwrap();
            assert_eq!(p.kind.id(), id);
            assert!(!p.no_gravity && p.position == Vec3::new(0.5, 110.0, 0.5));
            let held =
                spit_or_bullet_from_nbt(&summoned(id, std::slice::from_ref(&flag)), None).unwrap();
            assert!(held.no_gravity);
        }
        assert!(spit_or_bullet_from_nbt(&summoned("minecraft:arrow", &[]), None).is_none());
        // a spit and a target-less bullet are saved and selectable (`/data`, `@e`).
        let spit = spit_or_bullet_from_nbt(&summoned("minecraft:llama_spit", &[]), None).unwrap();
        assert!(saved_with_chunk(&spit));
        let saved = spit_or_bullet_to_nbt(&spit, [1; 4], None, None).unwrap();
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
