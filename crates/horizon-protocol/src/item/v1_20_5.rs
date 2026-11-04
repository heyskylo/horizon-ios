//! UUID/name attribute modifiers before 1.21. Food's missing remainder uses the shared old reader.
use super::*;
use crate::Error;

pub(super) fn active() -> bool {
    ids::active().is_some_and(|(ids, _)| ids.quirks.components_before_1_21)
}

// The UUID identities renamed by vanilla's AttributeModifierIdFix. These are game constants,
// including aliases that the fix collapses to one modern identity.
const MODIFIER_IDS: &[(u128, &str)] = &[
    (
        0x736565d2e1a7403da3f81aeb3e302542,
        "minecraft:creative_mode_block_range",
    ),
    (
        0x98491ef697b14584ae8271a8cc85cf73,
        "minecraft:creative_mode_entity_range",
    ),
    (0x91aeaa56376b4498935b2f7f68070635, "minecraft:effect.speed"),
    (
        0x7107de5e7ce84030940e514c1f160890,
        "minecraft:effect.slowness",
    ),
    (0xaf8b6e3f33284c0aaa365ba2bb9dbef3, "minecraft:effect.haste"),
    (
        0x55fced67e92a486e9800b47f202c4386,
        "minecraft:effect.mining_fatigue",
    ),
    (
        0x648d70646a604f598abec2c23a6dd7a9,
        "minecraft:effect.strength",
    ),
    (
        0xc0105bf3aef846b09ebc92943757ccbe,
        "minecraft:effect.jump_boost",
    ),
    (
        0x22653b89116e49dc9b6b9971489b5be5,
        "minecraft:effect.weakness",
    ),
    (
        0x5d6f0ba2118646acb896c61c5cee99cc,
        "minecraft:effect.health_boost",
    ),
    (
        0xeae29cf0701e4ed6883a96f798f3dab5,
        "minecraft:effect.absorption",
    ),
    (0x03c3c89d70374b42869fb146bcb64d2e, "minecraft:effect.luck"),
    (
        0xcc5af1422bd24215b6362605aed11727,
        "minecraft:effect.unluck",
    ),
    (0x6555be7463b341f1a24577833b3c2562, "minecraft:evil"),
    (0x1eaf83ff72074596b37ad7a07b3ec4ce, "minecraft:powder_snow"),
    (0x662a6b8dda3e4c1c881396ea6097278d, "minecraft:sprinting"),
    (0x020e0dfb87ae46539556831010e291a0, "minecraft:attacking"),
    (0x766bfa6411f311ea8d71362b9e155667, "minecraft:baby"),
    (0x7e0292f2943448d5a29f9583af7df27f, "minecraft:covered"),
    (0x9e36292401de4ddda2b2d0f7a405a174, "minecraft:suffocating"),
    (0x5cd17e52a79a43d3a52990fde04b181e, "minecraft:drinking"),
    (0xb9766b5995664402bc1f2ee2a276d836, "minecraft:baby"),
    (0x49455a497ec545bab8863b90b23a1718, "minecraft:attacking"),
    (0x845db27cc624495f8c9f6020a9a58b6b, "minecraft:armor.boots"),
    (
        0xd8499b040e664726ab2964469d734e0d,
        "minecraft:armor.leggings",
    ),
    (
        0x9f3d476dc1184544836564846904b48e,
        "minecraft:armor.chestplate",
    ),
    (0x2ad3f246fee14e67b88669fd380bb150, "minecraft:armor.helmet"),
    (0xc1c727718b8eba4aace081a93c8928b2, "minecraft:armor.body"),
    (
        0xb572ecd2ac0c4071abde9594af072a37,
        "minecraft:enchantment.fire_protection",
    ),
    (
        0x40a9968f5c664e2fb7f42ec2f4b3e450,
        "minecraft:enchantment.blast_protection",
    ),
    (
        0x07a65791f64d4e7986c7f83932f007ec,
        "minecraft:enchantment.respiration",
    ),
    (
        0x60b1b7dbfffd4ad0817cd6c6a93d8a45,
        "minecraft:enchantment.aqua_affinity",
    ),
    (
        0x11dc269a447646c0aff39e17d7eb6801,
        "minecraft:enchantment.depth_strider",
    ),
    (
        0x87f46a96686f4796b03522e16ee9e038,
        "minecraft:enchantment.soul_speed",
    ),
    (
        0xb9716dbd50df4080850e70347d24e687,
        "minecraft:enchantment.soul_speed",
    ),
    (
        0x92437d00c3a74f2e8f6c1f21585d5dd0,
        "minecraft:enchantment.swift_sneak",
    ),
    (
        0x5d3d087bdebe4037b53ed84f3ff51f17,
        "minecraft:enchantment.sweeping_edge",
    ),
    (
        0x3ceb37c0db6246b5bd02785457b01d96,
        "minecraft:enchantment.efficiency",
    ),
    (
        0xcb3f55d3645c4f38a4979c13a33db5cf,
        "minecraft:base_attack_damage",
    ),
    (
        0xfa233e1c41804865b01bbcce9785aca3,
        "minecraft:base_attack_speed",
    ),
];
const MODIFIER_NAMES: &[(&str, &str)] = &[
    ("Random spawn bonus", "minecraft:random_spawn_bonus"),
    (
        "Random zombie-spawn bonus",
        "minecraft:zombie_random_spawn_bonus",
    ),
    ("Leader zombie bonus", "minecraft:leader_zombie_bonus"),
    (
        "Zombie reinforcement callee charge",
        "minecraft:reinforcement_callee_charge",
    ),
    (
        "Zombie reinforcement caller charge",
        "minecraft:reinforcement_caller_charge",
    ),
];

/// Vanilla's known UUID/name identities, then its lower-case UUID fallback. Synced entity
/// attributes have no name field and pass an empty name.
pub(crate) fn modifier_id(uuid: u128, name: &str) -> String {
    if let Some((_, id)) = MODIFIER_IDS.iter().find(|(old, _)| *old == uuid) {
        (*id).into()
    } else if let Some((_, id)) = MODIFIER_NAMES.iter().find(|(old, _)| *old == name) {
        (*id).into()
    } else {
        format!("minecraft:{}", crate::crypto::uuid_string(uuid))
    }
}

pub(super) fn modifier_uuid(id: &str) -> u128 {
    if let Some((uuid, _)) = MODIFIER_IDS.iter().find(|(_, name)| *name == id) {
        return *uuid;
    }
    if let Some(uuid) = id.strip_prefix("minecraft:") {
        let hex = uuid.replace('-', "");
        if hex.len() == 32
            && let Ok(uuid) = u128::from_str_radix(&hex, 16)
        {
            return uuid;
        }
    }
    // A new arbitrary identifier has no old UUID. Assign a stable UUID using Java's
    // UUID.nameUUIDFromBytes convention, rather than introducing a random identity per write.
    let mut bytes = crate::crypto::md5(id.as_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    u128::from_be_bytes(bytes)
}
pub(super) fn modifier_name(id: &str) -> &str {
    MODIFIER_NAMES
        .iter()
        .find(|(_, name)| *name == id)
        .map_or(id, |(old, _)| *old)
}

pub(super) fn read_attributes(r: &mut Reader) -> Result<Vec<u8>> {
    let n = r.count(65536)?;
    let mut w = Writer::new();
    w.varint(n as i32);
    for _ in 0..n {
        let attribute = r.varint()?;
        let mapped = ids::active().and_then(|(ids, _)| ids.attribute.up(attribute));
        w.varint(mapped.ok_or_else(|| Error::new("attribute absent from target registry"))?);
        let uuid = r.uuid()?;
        w.string(&modifier_id(uuid, &r.string(32767)?));
        w.f64(r.f64()?);
        w.varint(r.varint()?);
        w.varint(r.varint()?); // EquipmentSlotGroup ids 0..9 are unchanged.
        w.varint(0); // Display.Default, added in 1.21.6.
    }
    Ok(w.data)
}

pub(super) fn write_attributes(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(bytes);
    let mut entries = Vec::new();
    for _ in 0..r.count(65536)? {
        let attribute = r.varint()?;
        let mapped = ids::active().and_then(|(ids, _)| ids.attribute.down(attribute));
        let id = r.string(32767)?;
        let amount = r.f64()?;
        let operation = r.varint()?;
        let slot = r.varint()?;
        match r.varint()? {
            0 | 1 => {}
            2 => {
                r.nbt()?;
            }
            _ => return Err(Error::new("unknown attribute modifier display")),
        }
        let Some(attribute) = mapped.filter(|_| (0..=9).contains(&slot)) else {
            continue;
        };
        let mut entry = Writer::new();
        entry.varint(attribute);
        entry.uuid(modifier_uuid(&id));
        // Vanilla's upgrade removes arbitrary legacy names; a new identifier cannot recover
        // them. Known name aliases use their old spelling, other names use the identifier.
        entry.string(modifier_name(&id));
        entry.f64(amount);
        entry.varint(operation);
        entry.varint(slot);
        entries.push(entry.data);
    }
    if r.remaining() != 0 {
        return Err(Error::new("trailing attribute modifier data"));
    }
    let mut w = Writer::new();
    w.varint(entries.len() as i32);
    for entry in entries {
        w.bytes(&entry);
    }
    Ok(w.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ServerVersion, ids::Way};

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
    fn normalize(stack: &mut Stack) {
        for (kind, value) in &mut stack.added {
            match value {
                Component::Stacks(stacks) => stacks.iter_mut().for_each(normalize),
                Component::Enchantments(levels) => levels.sort_unstable(),
                Component::TooltipDisplay { hidden, .. } => hidden.sort_unstable(),
                Component::Raw(data) if *kind == component::CONTAINER => {
                    let mut r = Reader::new(data);
                    let mut w = Writer::new();
                    let n = r.varint().unwrap();
                    w.varint(n);
                    for _ in 0..n {
                        let present = r.bool().unwrap();
                        w.bool(present);
                        if present {
                            let mut nested = Template::read(&mut r).unwrap();
                            normalize(&mut nested);
                            Template::write(&nested, &mut w);
                        }
                    }
                    assert_eq!(r.remaining(), 0);
                    *data = w.data;
                }
                _ => {}
            }
        }
        stack.added.sort_by_key(|(kind, _)| *kind);
        stack.removed.sort_unstable();
    }
    fn canonical(mut stack: Option<Stack>) -> Option<Stack> {
        if let Some(stack) = &mut stack {
            normalize(stack);
        }
        stack
    }

    #[test]
    fn vanilla_component_fixtures_normalize_and_downgrade_before_1_21() {
        // Independent vanilla DataComponentPatch.STREAM_CODEC output, with the real count/item
        // fields. These 21 inputs are identical in 1.20.5 and 1.20.6. Pinned fixtures use 26.3.
        for (label, old, pinned) in [
            (
                "damage_model",
                "01cf0602000db9600311",
                "01a408020011014640e4000000000311",
            ),
            (
                "potion",
                "01cf06020003111f0119010001e2400112022801000000",
                "01a40802000311350119010001e240011202280100000000",
            ),
            ("potion_empty", "01cf0601001f000000", "01a40801003500000000"),
            (
                "instrument",
                "01cf06020003112800c3055042800000",
                "01a408020003113f00ce06408000004280000000080000",
            ),
            (
                "instrument_direct_sound",
                "01cf0601002800000e6d696e6563726166743a7465737401408000005a42000000",
                "01a40801003f00000e6d696e6563726166743a746573740140800000409000004200000000080000",
            ),
            (
                "attributes",
                "01cf0602000c0102cb3f55d3645c4f38a4979c13a33db5cf1c6d696e6563726166743a626173655f61747461636b5f64616d6167653ff80000000000000001000311",
                "01a40803001001031c6d696e6563726166743a626173655f61747461636b5f64616d6167653ff8000000000000000100031112000110",
            ),
            (
                "food_simple",
                "01cf060200031114033fa00000003fcccccd00",
                "01a40803000311183fcccccd01bf05010017033fa0000000",
            ),
            (
                "food_full",
                "01cf060200031114033fa00000013f4ccccd02120228010000003e80000009000a000101003f400000",
                "01a40803000311183f4ccccd01bf0501020001120228010000003e800000000109000a000101003f40000017033fa0000001",
            ),
            (
                "fire",
                "01cf060200031115",
                "01a408020003111b00116d696e6563726166743a69735f66697265",
            ),
            ("removed_food", "01cf06000114", "01a4080003181719"),
            ("removed_fire", "01cf06000115", "01a40800011b"),
            (
                "hidden",
                "01cf0604000f04001800ff0000000e",
                "01a4080300042f00ff0000120102042f",
            ),
            (
                "tool",
                "01cf06010016020301b50201408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b61786500004000000002",
                "01a40801001c020301900301408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b6178650000400000000201",
            ),
            (
                "profile",
                "01cf0601002e010b486f72697a6f6e576972650000",
                "01a40801004800010b486f72697a6f6e57697265000000000000",
            ),
            (
                "container",
                "01cf0601003306030100000000000002e4020000",
                "01a40801004d0601010300000000000001d903020000",
            ),
            (
                "bundle",
                "01cf0601001e0202e402000003cf0601000d09",
                "01a40801003402d903020000a408030100110141100000000000",
            ),
            (
                "can_break",
                "01cf0601000b01010201000000",
                "01a40802001200010f0f0101020100000000",
            ),
            (
                "attribute_custom",
                "01cf0602000c01110000000000000000000000000000000117486f72697a6f6e20637573746f6d206d6f6469666965723fd00000000000000200010311",
                "01a408020010011a2e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303030313fd00000000000000200000311",
            ),
            (
                "attribute_named",
                "01cf0602000c010a000000000000000000000000000000021252616e646f6d20737061776e20626f6e75733fe00000000000000100010311",
                "01a40802001001101c6d696e6563726166743a72616e646f6d5f737061776e5f626f6e75733fe00000000000000100000311",
            ),
            (
                "attribute_slots",
                "01cf0601000c0a000000000000000000000000000000006404736c6f743ff00000000000000000000000000000000000000000000000006504736c6f743ff00000000000000001000000000000000000000000000000006604736c6f743ff00000000000000002000000000000000000000000000000006704736c6f743ff00000000000000003000000000000000000000000000000006804736c6f743ff00000000000000004000000000000000000000000000000006904736c6f743ff00000000000000005000000000000000000000000000000006a04736c6f743ff00000000000000006000000000000000000000000000000006b04736c6f743ff00000000000000007000000000000000000000000000000006c04736c6f743ff00000000000000008000000000000000000000000000000006d04736c6f743ff0000000000000000901",
                "01a4080100100a012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036343ff0000000000000000000012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036353ff0000000000000000100012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036363ff0000000000000000200012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036373ff0000000000000000300012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036383ff0000000000000000400012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036393ff0000000000000000500012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036613ff0000000000000000600012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036623ff0000000000000000700012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036633ff0000000000000000800012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303036643ff0000000000000000900",
            ),
            (
                "container_food",
                "01cf06010033020001cf06010014023e4ccccd003fcccccd00",
                "01a40801004d020001a40801020017023e4ccccd00183fcccccd01bf050100",
            ),
        ] {
            let old = bytes(old);
            let pinned = bytes(pinned);
            let mut r = Reader::new(&old);
            let stack = {
                let _up = ids::renumbering(ServerVersion::V1_20_5.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap_or_else(|e| panic!("{label}: {e}"))
            };
            assert_eq!(r.remaining(), 0, "{label}");
            let expected = Slot::read(&mut Reader::new(&pinned))
                .unwrap_or_else(|e| panic!("{label} pinned: {e}"));
            assert_eq!(canonical(stack.clone()), canonical(expected), "{label}");
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ServerVersion::V1_20_5.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut w);
            }
            // Arbitrary old names and name-based UUIDs are discarded by vanilla's identity
            // upgrade. Downgrade assigns a deterministic name/UUID; tooltip units may reorder.
            if !matches!(
                label,
                "attribute_custom" | "attribute_named" | "attribute_slots" | "hidden"
            ) {
                assert_eq!(w.data, old, "{label}");
            }
            let mut r = Reader::new(&w.data);
            let returned = {
                let _up = ids::renumbering(ServerVersion::V1_20_5.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap_or_else(|e| panic!("{label} returned: {e}"))
            };
            assert_eq!(r.remaining(), 0, "{label}");
            assert_eq!(canonical(returned), canonical(stack.clone()), "{label}");
            let mut w = Writer::new();
            DelimitedSlot::write(&stack, &mut w);
            assert_eq!(
                DelimitedSlot::read(&mut Reader::new(&w.data)).unwrap(),
                stack,
                "{label}"
            );
        }
    }

    #[test]
    fn static_enchantment_ids_survive_both_vanilla_map_orders() {
        // 1.20.5 and 1.20.6 encode the same static registry map in different iteration orders.
        // The adapter supplies that registry's names separately; component ids remain unchanged.
        for fixture in [
            "01cf0603000311170128010109020d03000400",
            "01cf06030003111701280101090200040d0300",
        ] {
            let old = bytes(fixture);
            let stack = {
                let _up = ids::renumbering(ServerVersion::V1_20_5.ids().unwrap(), Way::Up);
                Slot::read(&mut Reader::new(&old)).unwrap().unwrap()
            };
            let mut levels = match stack.component(component::ENCHANTMENTS).unwrap() {
                Component::Enchantments(levels) => levels.clone(),
                _ => panic!("enchantment component not decoded"),
            };
            levels.sort_unstable();
            assert_eq!(levels, [(0, 4), (13, 3)]);
            assert_eq!(
                stack.component(component::STORED_ENCHANTMENTS),
                Some(&Component::Enchantments(vec![(40, 1)]))
            );
            assert_eq!(stack.damage(), 17);
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ServerVersion::V1_20_5.ids().unwrap(), Way::Down);
                Slot::write(&Some(stack), &mut w);
            }
            assert_eq!(w.data, old);
        }
    }

    #[test]
    fn modifier_identity_follows_vanilla_uuid_and_name_precedence() {
        let attack = 0xcb3f55d3645c4f38a4979c13a33db5cf;
        assert_eq!(
            modifier_id(attack, "Random spawn bonus"),
            "minecraft:base_attack_damage"
        );
        assert_eq!(modifier_uuid("minecraft:base_attack_damage"), attack);
        assert_eq!(
            modifier_id(1, "Random spawn bonus"),
            "minecraft:random_spawn_bonus"
        );
        assert_eq!(
            modifier_id(1, "other"),
            "minecraft:00000000-0000-0000-0000-000000000001"
        );
        assert_eq!(
            modifier_uuid("minecraft:00000000-0000-0000-0000-000000000001"),
            1
        );
        let id = "minecraft:random_spawn_bonus";
        assert_eq!(modifier_id(modifier_uuid(id), modifier_name(id)), id);
        // Independently encoded with Java UUID.nameUUIDFromBytes using UTF-8.
        assert_eq!(
            modifier_uuid("horizon:custom"),
            0x3e28e6c2e6603577b8e69e191403de58
        );
    }
}
