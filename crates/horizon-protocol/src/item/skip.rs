//! Reading past the data components Horizon doesn't interpret: each
//! function consumes exactly what its vanilla StreamCodec writes
//! (DataComponents' `networkSynchronized` codecs; a component without one
//! goes as its persistent Codec's NBT, `ByteBufCodecs.fromCodecWithRegistries`).
//! Item data has no length prefix, so a component read short or long makes
//! everything after it unreadable.
use super::{Component, Nested, Template};
use crate::{
    Error, Result,
    buf::{Codec, Reader},
};

type Read = fn(&mut Reader) -> Result<()>;

/// DataComponents' registry order (26.3) for the ids `Component::read`
/// doesn't decode itself.
pub(super) fn component(kind: i32, r: &mut Reader) -> Result<()> {
    match kind {
        // Unit.STREAM_CODEC: glider, waxed.
        34 | 120 => Ok(()),
        // Components without a network codec go as their NBT: intangible_projectile,
        // map_decorations, debug_stick_state, recipes, lock, container_loot.
        22 | 49 | 59 | 68 | 81 | 82 => nbt(r),
        // A VarInt (VAR_INT, an idMapper, a registry or holderRegistry id): damage_type,
        // enchantable, additional_trade_cost, block_transformer, villager_food, dye,
        // ominous_bottle_amplifier, base_color, the mob variants and colours, the pottery
        // pattern and cushion_color.
        8 | 31 | 42 | 43 | 44 | 46 | 65 | 75 | 88..=108 | 110..=117 | 121 => varint(r),
        // FLOAT: minimum_attack_charge, potion_duration_scale.
        7 | 54 => f32(r),
        // Identifier.STREAM_CODEC: tooltip_style, note_block_sound.
        35 | 73 => ident(r),
        // UseEffects: canSprint, interactVibrations, speedMultiplier.
        5 => {
            r.bool()?;
            r.bool()?;
            f32(r)
        }
        // AdventureModePredicate: can_place_on, can_break.
        14 | 15 => list(r, block_predicate),
        16 => list(r, attribute_modifier),
        // FoodProperties.DIRECT_STREAM_CODEC: nutrition, saturation, canAlwaysEat.
        23 => {
            varint(r)?;
            f32(r)?;
            boolean(r)
        }
        // Consumable: consumeSeconds, animation, sound, hasConsumeParticles, onConsumeEffects.
        24 => {
            f32(r)?;
            varint(r)?;
            sound(r)?;
            boolean(r)?;
            list(r, consume_effect)
        }
        // UseRemainder, SulfurCubeContent: one template.
        25 | 80 => template(r),
        // UseCooldown: seconds, cooldownGroup.
        26 => {
            f32(r)?;
            optional(r, ident)
        }
        // DamageResistant, Repairable, provides_banner_patterns: a holder set.
        27 | 33 | 67 => holder_set(r),
        28 => tool(r),
        // Weapon: itemDamagePerAttack, disableBlockingForSeconds.
        29 => {
            varint(r)?;
            f32(r)
        }
        32 => equippable(r),
        // DeathProtection: deathEffects.
        36 => list(r, consume_effect),
        37 => blocks_attacks(r),
        // PiercingWeapon: dealsKnockback, dismounts, sound, hitSound.
        38 => {
            boolean(r)?;
            boolean(r)?;
            optional(r, sound)?;
            optional(r, sound)
        }
        39 => kinetic_weapon(r),
        // SwingAnimation: type, duration.
        40 | 41 => {
            varint(r)?;
            varint(r)
        }
        // BundleContents: templates.
        52 => list(r, template),
        // SuspiciousStewEffects: effect, duration.
        55 => list(r, |r| {
            varint(r)?;
            varint(r)
        }),
        // ArmorTrim: material, then Holder<TrimPattern> (assetId, description, decal).
        58 => {
            trim_material(r)?;
            holder(r, |r| {
                ident(r)?;
                nbt(r)?;
                boolean(r)
            })
        }
        // TypedEntityData (entity_data, block_entity_data): the type id, then a compound.
        60 | 62 => {
            varint(r)?;
            nbt(r)
        }
        // CustomData: bucket_entity_data.
        61 => nbt(r),
        // InstrumentComponent: Holder<Instrument> (soundEvent, useDuration, range,
        // durabilityDamage, description).
        63 => holder(r, |r| {
            sound(r)?;
            f32(r)?;
            f32(r)?;
            varint(r)?;
            nbt(r)
        }),
        64 => trim_material(r),
        // JukeboxPlayable: Holder<JukeboxSong> (soundEvent, description, lengthInSeconds,
        // comparatorOutput).
        66 => holder(r, |r| {
            sound(r)?;
            nbt(r)?;
            f32(r)?;
            varint(r)
        }),
        // LodestoneTracker: target (GlobalPos: dimension, BlockPos), tracked.
        69 => {
            optional(r, |r| {
                ident(r)?;
                r.i64().map(drop)
            })?;
            boolean(r)
        }
        70 => firework_explosion(r),
        // Fireworks: flightDuration, explosions.
        71 => {
            varint(r)?;
            list(r, firework_explosion)
        }
        // BannerPatternLayers: Holder<BannerPattern> (assetId, translationKey), colour.
        74 => list(r, |r| {
            holder(r, |r| {
                ident(r)?;
                string(r)
            })?;
            varint(r)
        }),
        // PotDecorations: back, left, right, front.
        76 => (0..4).try_for_each(|_| optional(r, template)),
        // ItemContainerContents: optional templates.
        77 => list(r, |r| optional(r, template)),
        // BlockItemStateProperties: a string to string map.
        78 => list(r, |r| {
            string(r)?;
            string(r)
        }),
        // Bees: Occupant (entity type and data, ticksInHive, minTicksInHive).
        79 => list(r, |r| {
            varint(r)?;
            nbt(r)?;
            varint(r)?;
            varint(r)
        }),
        83 => sound(r),
        // Compostable: layers.
        84 => resolvable(r, i32),
        // CookingFuel (burnTime, speedMultiplier), BrewingFuel (uses, speedMultiplier).
        85 | 86 => {
            resolvable(r, i32)?;
            resolvable(r, f32)
        }
        // MobVisibility: targetingEntityTypes, visibility.
        87 => {
            holder_set(r)?;
            f32(r)
        }
        // Holder<PaintingVariant>: width, height, assetId, title, author.
        109 => holder(r, |r| {
            varint(r)?;
            varint(r)?;
            ident(r)?;
            optional(r, nbt)?;
            optional(r, nbt)
        }),
        // SignText: four messages, four optional filtered ones, colour, hasGlowingText.
        118 | 119 => {
            (0..4).try_for_each(|_| nbt(r))?;
            optional(r, |r| (0..4).try_for_each(|_| nbt(r)))?;
            varint(r)?;
            boolean(r)
        }
        other => Err(Error::new(format!("unsupported item component {other}"))),
    }
}

fn boolean(r: &mut Reader) -> Result<()> {
    r.bool().map(drop)
}
fn varint(r: &mut Reader) -> Result<()> {
    r.varint().map(drop)
}
fn f32(r: &mut Reader) -> Result<()> {
    r.f32().map(drop)
}
fn i32(r: &mut Reader) -> Result<()> {
    r.i32().map(drop)
}
/// ByteBufCodecs.STRING_UTF8.
fn string(r: &mut Reader) -> Result<()> {
    r.string(32767).map(drop)
}
/// Identifier.STREAM_CODEC, ResourceKey.streamCodec.
fn ident(r: &mut Reader) -> Result<()> {
    string(r)
}
/// A text component or compound (fromCodecWithRegistries, COMPOUND_TAG).
fn nbt(r: &mut Reader) -> Result<()> {
    r.nbt().map(drop)
}
fn template(r: &mut Reader) -> Result<()> {
    Template::read(r).map(drop)
}
/// ByteBufCodecs.optional: a flag, then the value.
fn optional(r: &mut Reader, value: Read) -> Result<()> {
    if r.bool()? { value(r) } else { Ok(()) }
}
/// ByteBufCodecs.list and map: a VarInt count, then the elements (a map's
/// key and value each). Every element is at least a byte, so the count
/// can't pass what is left.
fn list(r: &mut Reader, element: Read) -> Result<()> {
    let n = r.count(r.remaining())?;
    (0..n).try_for_each(|_| element(r))
}
/// ByteBufCodecs.holder: 0 and the value itself, or the registry id plus one.
/// Vanilla looks the id up with `byIdOrThrow(id - 1)`, which refuses a negative one.
fn holder(r: &mut Reader, direct: Read) -> Result<()> {
    match r.varint()? {
        0 => direct(r),
        ..0 => Err(Error::new("negative holder id")),
        _ => Ok(()),
    }
}
/// ByteBufCodecs.holderSet: 0 and a tag id, or the size plus one and the ids.
/// Vanilla's `new ArrayList(size)` refuses a negative size.
fn holder_set(r: &mut Reader) -> Result<()> {
    match r.varint()? {
        0 => ident(r),
        n => {
            let n = usize::try_from(n).map_err(|_| Error::new("negative holder set size"))? - 1;
            if n > r.remaining() {
                return Err(Error::new("holder set past the end"));
            }
            (0..n).try_for_each(|_| varint(r))
        }
    }
}
/// SoundEvent.STREAM_CODEC: a holder of a location and an optional fixed range.
fn sound(r: &mut Reader) -> Result<()> {
    holder(r, |r| {
        ident(r)?;
        optional(r, f32)
    })
}
/// ResolvableInt, ResolvableFloat: Either a constant (true) or a context provider's key.
fn resolvable(r: &mut Reader, constant: Read) -> Result<()> {
    if r.bool()? { constant(r) } else { ident(r) }
}
/// TrimMaterial.STREAM_CODEC: a holder of a palette id and a description.
fn trim_material(r: &mut Reader) -> Result<()> {
    holder(r, |r| {
        ident(r)?;
        nbt(r)
    })
}
/// MobEffectInstance: the effect id, then its Details.
fn mob_effect(r: &mut Reader) -> Result<()> {
    varint(r)?;
    effect_details(r)
}
/// MobEffectInstance.Details: amplifier, duration, ambient, showParticles, showIcon, then the
/// hidden effect's Details, recursively.
fn effect_details(r: &mut Reader) -> Result<()> {
    let _nested = Nested::enter()?;
    varint(r)?;
    varint(r)?;
    boolean(r)?;
    boolean(r)?;
    boolean(r)?;
    optional(r, effect_details)
}
/// ConsumeEffect.STREAM_CODEC: the consume_effect_type id, then that type's fields.
fn consume_effect(r: &mut Reader) -> Result<()> {
    match r.varint()? {
        // apply_effects: effects, probability.
        0 => {
            list(r, mob_effect)?;
            f32(r)
        }
        // remove_effects: a mob effect holder set.
        1 => holder_set(r),
        // clear_all_effects.
        2 => Ok(()),
        // teleport_randomly: diameter, directionalParticles.
        3 => {
            f32(r)?;
            boolean(r)
        }
        // play_sound.
        4 => sound(r),
        other => Err(Error::new(format!("unknown consume effect type {other}"))),
    }
}
/// BlockPredicate: blocks, properties, nbt, then DataComponentMatchers (exact, partial).
fn block_predicate(r: &mut Reader) -> Result<()> {
    optional(r, holder_set)?;
    block_properties_and_nbt(r)?;
    component_matchers(r)
}
pub(super) fn block_properties_and_nbt(r: &mut Reader) -> Result<()> {
    // StatePropertiesPredicate: PropertyMatchers, each a name and an
    // Either<ExactMatcher (value), RangedMatcher (min, max)>.
    optional(r, |r| {
        list(r, |r| {
            string(r)?;
            if r.bool()? {
                string(r)
            } else {
                optional(r, string)?;
                optional(r, string)
            }
        })
    })?;
    optional(r, nbt)
}
pub(super) fn component_matchers(r: &mut Reader) -> Result<()> {
    // DataComponentExactPredicate: TypedDataComponents, a component type id and its value.
    list(r, |r| {
        let _nested = Nested::enter()?;
        let kind = r.varint()?;
        Component::read(kind, r).map(drop)
    })?;
    // DataComponentPredicate.STREAM_CODEC: an Either of a predicate type id or a component
    // type id, then the predicate as NBT (fromCodecWithRegistries).
    list(r, |r| {
        boolean(r)?;
        varint(r)?;
        nbt(r)
    })
}
/// ItemAttributeModifiers.Entry: attribute, modifier (id, amount, operation), slot, display.
fn attribute_modifier(r: &mut Reader) -> Result<()> {
    varint(r)?;
    ident(r)?;
    r.f64()?;
    varint(r)?;
    varint(r)?;
    // Display.Type: default, hidden, or override and its text.
    match r.varint()? {
        0 | 1 => Ok(()),
        2 => nbt(r),
        other => Err(Error::new(format!(
            "unknown attribute modifier display {other}"
        ))),
    }
}
/// Tool: rules (blocks, speed, correctForDrops), defaultMiningSpeed, damagePerBlock,
/// canDestroyBlocksInCreative.
fn tool(r: &mut Reader) -> Result<()> {
    list(r, |r| {
        holder_set(r)?;
        optional(r, f32)?;
        optional(r, boolean)
    })?;
    f32(r)?;
    varint(r)?;
    boolean(r)
}
/// Equippable: slot, equipSound, assetId, cameraOverlay, allowedEntities, dispensable,
/// swappable, damageOnHurt, equipOnInteract, canBeSheared, shearingSound.
fn equippable(r: &mut Reader) -> Result<()> {
    varint(r)?;
    sound(r)?;
    optional(r, ident)?;
    optional(r, ident)?;
    optional(r, holder_set)?;
    (0..5).try_for_each(|_| boolean(r))?;
    sound(r)
}
/// BlocksAttacks: blockDelaySeconds, disableCooldownScale, damageReductions (angle, type,
/// base, factor), itemDamage (threshold, base, factor), bypassedBy, blockSound, disableSound.
fn blocks_attacks(r: &mut Reader) -> Result<()> {
    f32(r)?;
    f32(r)?;
    list(r, |r| {
        f32(r)?;
        optional(r, holder_set)?;
        f32(r)?;
        f32(r)
    })?;
    (0..3).try_for_each(|_| f32(r))?;
    optional(r, holder_set)?;
    optional(r, sound)?;
    optional(r, sound)
}
/// KineticWeapon: contactCooldownTicks, delayTicks, the dismount, knockback and damage
/// conditions (maxDurationTicks, minSpeed, minRelativeSpeed), forwardMovement,
/// damageMultiplier, sound, hitSound.
fn kinetic_weapon(r: &mut Reader) -> Result<()> {
    varint(r)?;
    varint(r)?;
    (0..3).try_for_each(|_| {
        optional(r, |r| {
            varint(r)?;
            f32(r)?;
            f32(r)
        })
    })?;
    f32(r)?;
    f32(r)?;
    optional(r, sound)?;
    optional(r, sound)
}
/// PotionContents' customEffects and customName (`Component::read` reads
/// the potion and colour before them).
pub(super) fn potion_effects_and_name(r: &mut Reader) -> Result<()> {
    list(r, mob_effect)?;
    optional(r, string)
}
/// FireworkExplosion: shape, colors, fadeColors, hasTrail, hasTwinkle.
fn firework_explosion(r: &mut Reader) -> Result<()> {
    varint(r)?;
    list(r, i32)?;
    list(r, i32)?;
    boolean(r)?;
    boolean(r)
}
