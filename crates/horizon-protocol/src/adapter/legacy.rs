//! Connection-local registry names used by the pre-component NBT item format.
use super::{ServerVersion, ids, v1_20_3_ids as old};
use crate::{Error, Result, packets::configuration, registry};
use horizon_world::nbt::Tag;
use std::sync::Arc;

pub(super) fn registries(version: ServerVersion) -> ids::LegacyRegistries {
    let Some(ids) = version.ids() else {
        return Default::default();
    };
    if !version.at_most(ServerVersion::V1_20_3) {
        return Default::default();
    }
    let mut out = ids::LegacyRegistries::new();
    for (registry, names) in [
        ("minecraft:item", old::OLD_ITEM_NAMES),
        ("minecraft:block", old::BLOCK_NAMES),
        ("minecraft:entity_type", old::ENTITY_TYPE_NAMES),
        ("minecraft:block_entity_type", old::BLOCK_ENTITY_TYPE_NAMES),
        ("minecraft:attribute", old::ATTRIBUTE_NAMES),
        ("minecraft:sound_event", old::SOUND_EVENT_NAMES),
        ("minecraft:mob_effect", old::MOB_EFFECT_NAMES),
        ("minecraft:potion", old::POTION_NAMES),
        ("minecraft:enchantment", old::ENCHANTMENT_NAMES),
        ("minecraft:painting_variant", old::PAINTING_VARIANT_NAMES),
        ("minecraft:banner_pattern", old::BANNER_PATTERN_NAMES),
        ("minecraft:instrument", old::INSTRUMENT_NAMES),
    ] {
        let remap = ids.by_registry(registry);
        let mut table = registry::Ids::default();
        for (raw, name) in names.iter().enumerate() {
            let pinned = if registry == "minecraft:potion" {
                // 1.20.5 removed EMPTY: all 42 surviving potions moved down one slot.
                (raw > 0).then_some(raw as i32 - 1)
            } else {
                remap.up(raw as i32)
            };
            let Some(pinned) = pinned.and_then(|id| usize::try_from(id).ok()) else {
                continue;
            };
            table
                .names
                .resize(table.names.len().max(pinned + 1), String::new());
            table.names[pinned] = (*name).into();
            table.ids.insert((*name).into(), pinned as i32);
        }
        for &(kind, before, current) in old::NAME_ALIASES {
            if kind == registry
                && let Some(id) = table.id(before)
            {
                table.ids.insert(current.into(), id);
            }
        }
        out.insert(registry.into(), table);
    }
    out
}

/// A 765 RegistryAccess codec writes all synchronized registries in one compound, with explicit ids.
/// Split that compound into current per-registry packets while retaining each registry's order.
pub(super) fn split_registry_data(tag: &Tag) -> Result<Vec<configuration::RegistryData>> {
    let Tag::Compound(registries) = tag else {
        return Err(Error::new("invalid legacy registry compound"));
    };
    let mut out = Vec::new();
    for (key, value) in registries {
        let registry = key.to_string_lossy();
        let Some(Tag::List(entries)) = value.get("value") else {
            return Err(Error::new("missing legacy registry entries"));
        };
        if entries.len() > 65536 {
            return Err(Error::new("legacy registry is too large"));
        }
        let mut ordered = vec![None; entries.len()];
        for entry in entries {
            let Some(Tag::String(name)) = entry.get("name") else {
                return Err(Error::new("missing legacy registry name"));
            };
            let Some(Tag::Int(id)) = entry.get("id") else {
                return Err(Error::new("missing legacy registry id"));
            };
            let id = usize::try_from(*id).map_err(|_| Error::new("negative legacy registry id"))?;
            let slot = ordered
                .get_mut(id)
                .ok_or_else(|| Error::new("legacy registry id out of range"))?;
            if slot.is_some() {
                return Err(Error::new("duplicate legacy registry id"));
            }
            *slot = Some(configuration::RegistryEntry {
                id: name.to_string_lossy(),
                data: entry.get("element").cloned(),
            });
        }
        let entries = ordered
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error::new("non-contiguous legacy registry ids"))?;
        out.push(configuration::RegistryData { registry, entries });
    }
    Ok(out)
}

pub(super) fn remember(session: &super::Session, packet: &configuration::RegistryData) {
    let mut tables = session.legacy_registries.lock().unwrap();
    Arc::make_mut(&mut tables).insert(
        packet.registry.clone(),
        registry::Ids::from_names(packet.entries.iter().map(|e| e.id.clone()).collect()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, id: i32) -> Tag {
        Tag::compound([
            ("name", Tag::String(name.into())),
            ("id", Tag::Int(id)),
            (
                "element",
                Tag::compound([("synthetic", Tag::Int(id.saturating_add(10)))]),
            ),
        ])
    }

    #[test]
    fn whole_registry_sync_uses_explicit_ids_and_connection_local_names() {
        let tag = Tag::compound([(
            "minecraft:trim_material",
            Tag::compound([
                ("type", Tag::String("minecraft:trim_material".into())),
                (
                    "value",
                    Tag::List(vec![entry("minecraft:gold", 1), entry("minecraft:iron", 0)]),
                ),
            ]),
        )]);
        let packets = split_registry_data(&tag).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].entries[0].id, "minecraft:iron");
        assert_eq!(
            packets[0].entries[1]
                .data
                .as_ref()
                .unwrap()
                .get("synthetic"),
            Some(&Tag::Int(11))
        );
        let session = super::super::Session::new(ServerVersion::V1_20_3);
        remember(&session, &packets[0]);
        let _names = ids::legacy_registries(session.legacy_registries.lock().unwrap().clone());
        assert_eq!(
            ids::legacy_registry_id("minecraft:trim_material", "minecraft:gold"),
            Some(1)
        );
        assert_eq!(
            ids::legacy_registry_name("minecraft:trim_material", 0).as_deref(),
            Some("minecraft:iron")
        );
        assert_eq!(
            ids::legacy_registry_id("minecraft:item", "minecraft:turtle_scute"),
            ids::legacy_registry_id("minecraft:item", "minecraft:scute")
        );
        assert_eq!(
            ids::legacy_registry_id("minecraft:potion", "minecraft:water"),
            Some(0)
        );
    }

    #[test]
    fn malformed_registry_ids_fail_instead_of_reordering_or_allocating_by_id() {
        for entries in [
            vec![entry("a", -1)],
            vec![entry("a", i32::MAX)],
            vec![entry("a", 0), entry("b", 0)],
        ] {
            let tag = Tag::compound([(
                "minecraft:trim_material",
                Tag::compound([("value", Tag::List(entries))]),
            )]);
            assert!(split_registry_data(&tag).is_err());
        }
    }
}
