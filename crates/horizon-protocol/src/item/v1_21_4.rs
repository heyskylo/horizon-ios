//! ItemStack/DataComponentPatch before 1.21.5: tooltip booleans, count-first nested stacks
//! and the older Tool, BlockPredicate and ResolvableProfile stream layouts.
use super::*;
use crate::Error;

pub(super) fn reading() -> bool {
    matches!(ids::active(),Some((ids,ids::Way::Up)) if ids.quirks.components_before_1_21_5)
}
pub(super) fn writing() -> bool {
    matches!(ids::active(),Some((ids,ids::Way::Down)) if ids.quirks.components_before_1_21_5)
}
pub(super) const TOOLTIP_FLAGS: &[i32] = &[4, 13, 14, 15, 16, 45, 47, 58, 64];
// The old hide_additional_tooltip hid item-specific tooltip providers.
pub(super) const ADDITIONAL: &[i32] = &[
    74, 79, 62, 78, 52, 51, 77, 82, 70, 71, 63, 48, 109, 76, 53, 95, 57,
];

pub(super) fn tooltip_ids() -> (i32, i32) {
    if ids::active().is_some_and(|(ids, _)| ids.quirks.components_before_1_21_2) {
        (14, 15)
    } else {
        (15, 16)
    }
}

pub(super) fn read_stack(r: &mut Reader) -> Result<Option<Stack>> {
    let _nested = Nested::enter()?;
    let count = r.varint()?;
    if count <= 0 {
        return Ok(None);
    }
    let mut stack = Stack::new(ids::item(r.varint()?), count);
    let positive = r.count(65536)?;
    let negative = r.count(65536)?;
    let mut hide = false;
    let mut additional = false;
    let mut hidden = Vec::new();
    let (additional_id, hide_id) = tooltip_ids();
    for _ in 0..positive {
        let old = r.varint()?;
        if old == additional_id {
            additional = true;
            continue;
        }
        if old == hide_id {
            hide = true;
            continue;
        }
        let Some((kind, component)) = read_added_legacy(old, r)? else {
            continue;
        };
        if TOOLTIP_FLAGS.contains(&kind) && !r.bool()? {
            hidden.push(kind);
        }
        stack.added.push((kind, component));
    }
    if additional {
        hidden.extend(
            stack
                .added
                .iter()
                .map(|(kind, _)| *kind)
                .filter(|kind| ADDITIONAL.contains(kind)),
        );
    }
    if hide || !hidden.is_empty() {
        hidden.sort_unstable();
        hidden.dedup();
        stack.added.push((
            component::TOOLTIP_DISPLAY,
            Component::TooltipDisplay { hide, hidden },
        ));
    }
    for _ in 0..negative {
        let old = r.varint()?;
        let kind = if old == additional_id || old == hide_id {
            component::TOOLTIP_DISPLAY
        } else {
            ids::component(old)
        };
        if kind >= 0 && !stack.removed.contains(&kind) {
            stack.removed.push(kind);
        }
    }
    Ok(Some(stack))
}

/// Apply shared older-layout changes before this era's tooltip and component codecs.
/// This path also serves pre-1.21.2 stacks after their food and fire-resistance cases.
pub(super) fn read_added_legacy(raw: i32, r: &mut Reader) -> Result<Option<(i32, Component)>> {
    let _nested = Nested::enter()?;
    if let Some(name) = ids::dropped_component(raw) {
        skip_dropped(name, r)?;
        return Ok(None);
    }
    let kind = ids::component(raw);
    // InstrumentComponent's EitherHolder wrapper starts in 1.21.5; this era sends
    // Instrument.STREAM_CODEC directly. Earlier duration layouts are handled by v1_21_1.
    let layout = if kind == component::INSTRUMENT {
        Older::Pinned
    } else {
        older_layout(kind, r)?
    };
    let component = match layout {
        Older::Dropped => return Ok(None),
        Older::Rewritten(bytes) => {
            let _pinned = ids::pinned();
            Component::read(kind, &mut Reader::new(&bytes))?
        }
        Older::Pinned => read_component(kind, r)?,
    };
    Ok(Some((kind, component)))
}

pub(super) fn read_component(kind: i32, r: &mut Reader) -> Result<Component> {
    match kind {
        14 | 15 | 28 => Ok(Component::Raw(rewrite(kind, r, true)?)),
        component::TRIM => {
            let material = r.varint()?;
            let pattern = r.varint()?;
            if material == 0 || pattern == 0 {
                return Err(Error::new(
                    "inline pre-1.21.5 trim holders are not supported",
                ));
            }
            Ok(Component::Trim {
                material: material - 1,
                pattern: pattern - 1,
            })
        }
        _ => Component::read(kind, r),
    }
}

pub(super) fn write_stack(stack: Option<&Stack>, w: &mut Writer) {
    let Some(stack) = stack else {
        w.varint(0);
        return;
    };
    let item = ids::item(stack.item);
    if stack.count <= 0 || item < 0 {
        w.varint(0);
        return;
    }
    let tooltip = stack.component(component::TOOLTIP_DISPLAY);
    let (hide, hidden) = match tooltip {
        Some(Component::TooltipDisplay { hide, hidden }) => (*hide, hidden.as_slice()),
        _ => (false, &[][..]),
    };
    let added: Vec<_> = stack
        .added
        .iter()
        .filter(|(kind, _)| ids::component(*kind) >= 0)
        .collect();
    let additional = hidden.iter().any(|kind| ADDITIONAL.contains(kind));
    let (additional_id, hide_id) = tooltip_ids();
    let mut removed: Vec<_> = stack
        .removed
        .iter()
        .map(|kind| ids::component(*kind))
        .filter(|id| *id >= 0)
        .collect();
    if stack.removed.contains(&component::TOOLTIP_DISPLAY) {
        removed.extend([additional_id, hide_id]);
    }
    w.varint(stack.count);
    w.varint(item);
    w.varint(added.len() as i32 + i32::from(hide) + i32::from(additional));
    w.varint(removed.len() as i32);
    for (kind, value) in added {
        w.varint(ids::component(*kind));
        write_component(*kind, value, w);
        if TOOLTIP_FLAGS.contains(kind) {
            w.bool(!hidden.contains(kind));
        }
    }
    if hide {
        w.varint(hide_id);
    }
    if additional {
        w.varint(additional_id);
    }
    for kind in removed {
        w.varint(kind);
    }
}

pub(super) fn write_component(kind: i32, value: &Component, w: &mut Writer) {
    match (kind, value) {
        (14 | 15 | 28, Component::Raw(bytes)) => {
            let rewritten =
                rewrite(kind, &mut Reader::new(bytes), false).unwrap_or_else(|_| bytes.clone());
            w.bytes(&rewritten);
        }
        _ => write_value(kind, value, w),
    }
}

fn block_set(r: &mut Reader, w: &mut Writer) -> Result<()> {
    let n = r.varint()?;
    w.varint(n);
    if n == 0 {
        w.string(&r.string(32767)?);
    } else {
        let n = usize::try_from(n - 1).map_err(|_| Error::new("negative holder set"))?;
        if n > r.remaining() {
            return Err(Error::new("holder set past the end"));
        }
        for _ in 0..n {
            let old = r.varint()?;
            let mapped = match ids::active() {
                Some((ids, ids::Way::Up)) => ids.block.up(old),
                Some((ids, ids::Way::Down)) => ids.block.down(old),
                None => Some(old),
            };
            w.varint(mapped.unwrap_or(0));
        }
    }
    Ok(())
}
pub(super) fn rewrite(kind: i32, r: &mut Reader, up: bool) -> Result<Vec<u8>> {
    let mut w = Writer::new();
    let n = r.count(65536)?;
    w.varint(n as i32);
    for _ in 0..n {
        if kind == 28 {
            block_set(r, &mut w)?;
            let speed = r.bool()?;
            w.bool(speed);
            if speed {
                w.f32(r.f32()?);
            }
            let drops = r.bool()?;
            w.bool(drops);
            if drops {
                w.bool(r.bool()?);
            }
        } else {
            let blocks = r.bool()?;
            w.bool(blocks);
            if blocks {
                block_set(r, &mut w)?;
            }
            let start = r.position();
            skip::block_properties_and_nbt(r)?;
            w.bytes(r.since(start));
            if up {
                w.varint(0);
                w.varint(0);
            } else {
                skip::component_matchers(r)?;
            }
        }
    }
    if kind == 28 {
        w.f32(r.f32()?);
        w.varint(r.varint()?);
        if up {
            w.bool(true);
        } else {
            r.bool()?;
        }
    }
    Ok(w.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ServerVersion, ids::Way};

    #[test]
    fn pre_1_21_5_instruments_have_no_either_holder_flag() {
        // 1.21.3/1.21.4 DataComponents uses Instrument.STREAM_CODEC (a holder),
        // rather than 1.21.5's InstrumentComponent wrapping EitherHolder.
        let old = [1, 1, 2, 0, 50, 3, 3, 17];
        for version in [ServerVersion::V1_21_3, ServerVersion::V1_21_4] {
            let mut r = Reader::new(&old);
            let stack = {
                let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap().unwrap()
            };
            assert_eq!(r.remaining(), 0, "{version:?}");
            assert_eq!(
                stack.component(component::INSTRUMENT),
                Some(&Component::Instrument(2)),
                "{version:?}"
            );
            assert_eq!(stack.damage(), 17, "{version:?}");
            let mut down = Writer::new();
            {
                let _down = ids::renumbering(version.ids().unwrap(), Way::Down);
                Slot::write(&Some(stack), &mut down);
            }
            assert_eq!(down.data, old, "{version:?}");
        }
    }

    #[test]
    fn legacy_pots_and_removed_map_colors_keep_the_following_component_aligned() {
        // Raw ids from each official registry report. PotDecorations writes four item ids;
        // MapItemColor writes an INT and has no pinned counterpart. Stone is item 1 throughout.
        for (version, pot, map_color, sherd, brick) in [
            (ServerVersion::V1_20_6, 50, 25, 1285, 921),
            (ServerVersion::V1_21_1, 51, 25, 1288, 921),
            (ServerVersion::V1_21_4, 61, 35, 1341, 954),
        ] {
            let mut old = Writer::new();
            old.varint(1);
            old.varint(1);
            old.varint(3);
            old.varint(1);
            old.varint(map_color);
            old.i32(0x12345678);
            old.varint(pot);
            old.varint(4);
            for item in [sherd, brick, brick, brick] {
                old.varint(item);
            }
            old.varint(3); // Damage follows the rewritten and discarded values.
            old.varint(17);
            old.varint(map_color);

            let mut r = Reader::new(&old.data);
            let stack = {
                let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap()
            };
            assert_eq!(r.remaining(), 0, "{version:?}");
            let mut expected = Writer::new();
            expected.bytes(&[1, 1, 2, 0, 76, 1]);
            expected.varint(1598); // Pinned angler_pottery_sherd.
            expected.bytes(&[1, 0, 0, 0, 0, 0, 3, 17]);
            let expected = Slot::read(&mut Reader::new(&expected.data)).unwrap();
            assert_eq!(stack, expected, "{version:?}");

            // No old ids may remain in a cached patch or be renumbered a second time.
            let mut wire = Writer::new();
            DelimitedSlot::write(&stack, &mut wire);
            assert_eq!(
                DelimitedSlot::read(&mut Reader::new(&wire.data)).unwrap(),
                expected,
                "{version:?}"
            );

            let mut down = Writer::new();
            {
                let _down = ids::renumbering(version.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut down);
            }
            // Only the removed map color is lost; the old pot's item ids and damage survive.
            let mut expected_old = Writer::new();
            expected_old.bytes(&[1, 1, 2, 0]);
            expected_old.varint(pot);
            expected_old.varint(4);
            for item in [sherd, brick, brick, brick] {
                expected_old.varint(item);
            }
            expected_old.bytes(&[3, 17]);
            assert_eq!(down.data, expected_old.data, "{version:?}");
        }
    }

    #[test]
    fn deeply_nested_legacy_container_stacks_are_rejected() {
        for (version, container) in [
            (ServerVersion::V1_20_6, 51),
            (ServerVersion::V1_21_1, 52),
            (ServerVersion::V1_21_4, 62),
        ] {
            let mut old = Writer::new();
            for _ in 0..100 {
                old.bytes(&[1, 1, 1, 0]);
                old.varint(container);
                old.varint(1);
            }
            old.bytes(&[1, 1, 0, 0]);
            let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
            assert!(
                Slot::read(&mut Reader::new(&old.data)).is_err(),
                "{version:?}"
            );
        }
    }

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn tooltip_booleans_become_one_display_component_and_go_back() {
        let old = [1, 1, 4, 0, 4, 0, 10, 1, 0, 3, 0, 34, 0, 255, 0, 0, 0, 16];
        let stack = {
            let _up = ids::renumbering(ServerVersion::V1_21_4.ids().unwrap(), Way::Up);
            Slot::read(&mut Reader::new(&old)).unwrap()
        };
        assert_eq!(
            stack
                .as_ref()
                .unwrap()
                .component(component::TOOLTIP_DISPLAY),
            Some(&Component::TooltipDisplay {
                hide: true,
                hidden: vec![4, 13, 47]
            })
        );
        let mut w = Writer::new();
        {
            let _down = ids::renumbering(ServerVersion::V1_21_4.ids().unwrap(), Way::Down);
            Slot::write(&stack, &mut w);
        }
        assert_eq!(w.data, old);
    }

    #[test]
    fn old_recipe_stacks_keep_their_item_and_count_in_both_directions() {
        for (version, item) in [
            (ServerVersion::V1_21_4, 371),
            (ServerVersion::V1_21_5, 378),
            (ServerVersion::V1_21_6, 378),
            (ServerVersion::V1_21_8, 378),
        ] {
            let mut old = Writer::new();
            old.varint(3);
            old.varint(item);
            old.bytes(&[0, 0]);
            let stack = {
                let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
                Template::read(&mut Reader::new(&old.data)).unwrap()
            };
            assert_eq!((stack.item, stack.count), (473, 3));
            let mut pinned = Writer::new();
            Template::write(&stack, &mut pinned);
            assert_eq!(pinned.data, [0xd9, 3, 3, 0, 0]);
            let mut down = Writer::new();
            {
                let _down = ids::renumbering(version.ids().unwrap(), Way::Down);
                Template::write(&stack, &mut down);
            }
            assert_eq!(down.data, old.data);
        }
    }

    #[test]
    fn vanilla_1_21_4_component_stacks_match_the_pinned_oracle_and_go_back() {
        // Independent ItemStack.OPTIONAL_STREAM_CODEC output from vanilla 1.21.4 and 26.3.
        // Covers multiple modifiers, bare/list/direct-sound equippables and a following damage value.
        for (label, old, pinned) in [
            (
                "attributes",
                "01ef0601000d02020b6d696e6563726166743a783ff80000000000000001150b6d696e6563726166743a793fd0000000000000020001",
                "01a40801001002030b6d696e6563726166743a783ff80000000000000001001a0b6d696e6563726166743a793fd0000000000000020000",
            ),
            (
                "equip_bare",
                "01ef0601001c0347000000010101",
                "01a40801002003470000000101010000a80b",
            ),
            (
                "equip_list",
                "01ef0601001c0448010e6d696e6563726166743a676f6c64011a6d696e6563726166743a6d6973632f70756d706b696e626c757201035e8f01000000",
                "01a4080100200448010e6d696e6563726166743a676f6c64011a6d696e6563726166743a6d6973632f70756d706b696e626c75720103659a010000000000a80b",
            ),
            (
                "equip_direct",
                "01ef0601001c04000e6d696e6563726166743a746573740140800000000000010101",
                "01a40801002004000e6d696e6563726166743a7465737401408000000000000101010000a80b",
            ),
            (
                "combined",
                "01ef0603000d01000b6d696e6563726166743a7a400800000000000000000103111c0347000000010101",
                "01a40803001001010b6d696e6563726166743a7a400800000000000000000003112003470000000101010000a80b",
            ),
            (
                "tool",
                "01ef0601001a020301c30201408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b61786500004000000002",
                "01a40801001c020301900301408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b6178650000400000000201",
            ),
            (
                "profile",
                "01ef06010039010b486f72697a6f6e576972650000",
                "01a40801004800010b486f72697a6f6e57697265000000000000",
            ),
            (
                "color_hidden",
                "01ef0601002200ff000000",
                "01a40802002f00ff00001200012f",
            ),
            (
                "unbreakable_hidden",
                "01ef0601000400",
                "01a40802000412000104",
            ),
            (
                "container",
                "01ef0601003e06030100000000000002f3020000",
                "01a40801004d0601010300000000000001d903020000",
            ),
            (
                "can_break_hidden",
                "01ef0601000c01010201000000",
                "01a40802001200010f0f0101020100000000",
            ),
        ] {
            let old = bytes(old);
            let pinned = bytes(pinned);
            let mut r = Reader::new(&old);
            let stack = {
                let _up = ids::renumbering(ServerVersion::V1_21_4.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap()
            };
            assert_eq!(r.remaining(), 0, "{label}");
            let mut expected = Slot::read(&mut Reader::new(&pinned)).unwrap();
            let mut actual = stack.clone();
            actual
                .as_mut()
                .unwrap()
                .added
                .sort_by_key(|(kind, _)| *kind);
            expected
                .as_mut()
                .unwrap()
                .added
                .sort_by_key(|(kind, _)| *kind);
            assert_eq!(actual, expected, "{label}");
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ServerVersion::V1_21_4.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut w);
            }
            assert_eq!(w.data, old, "{label}");
            // The cached delimited patch must also contain normalized values.
            let mut w = Writer::new();
            DelimitedSlot::write(&stack, &mut w);
            let read_back = DelimitedSlot::read(&mut Reader::new(&w.data)).unwrap();
            assert_eq!(read_back, stack, "{label}");
        }
    }
    #[test]
    fn vanilla_1_21_3_component_stacks_match_the_pinned_oracle_and_go_back() {
        // Independent vanilla ItemStack.OPTIONAL_STREAM_CODEC outputs, including integer model data.
        for (label, old, pinned) in [
            (
                "attributes",
                "01e60601000d02020b6d696e6563726166743a783ff80000000000000001150b6d696e6563726166743a793fd0000000000000020001",
                "01a40801001002030b6d696e6563726166743a783ff80000000000000001001a0b6d696e6563726166743a793fd0000000000000020000",
            ),
            (
                "equip_bare",
                "01e60601001c0347000000010101",
                "01a40801002003470000000101010000a80b",
            ),
            (
                "equip_list",
                "01e60601001c0448010e6d696e6563726166743a676f6c64011a6d696e6563726166743a6d6973632f70756d706b696e626c757201035f9001000000",
                "01a4080100200448010e6d696e6563726166743a676f6c64011a6d696e6563726166743a6d6973632f70756d706b696e626c75720103659a010000000000a80b",
            ),
            (
                "equip_direct",
                "01e60601001c04000e6d696e6563726166743a746573740140800000000000010101",
                "01a40801002004000e6d696e6563726166743a7465737401408000000000000101010000a80b",
            ),
            (
                "combined",
                "01e60603000d01000b6d696e6563726166743a7a400800000000000000000103111c0347000000010101",
                "01a40803001001010b6d696e6563726166743a7a400800000000000000000003112003470000000101010000a80b",
            ),
            (
                "model",
                "01e60602000eb9600311",
                "01a408020011014640e4000000000311",
            ),
            (
                "model_negative",
                "01e60601000eefffffff0f",
                "01a40801001101c1880000000000",
            ),
            (
                "model_large",
                "01e60601000e80808008",
                "01a408010011014b800000000000",
            ),
            (
                "tool",
                "01e60601001a020301c30201408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b61786500004000000002",
                "01a40801001c020301900301408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b6178650000400000000201",
            ),
            (
                "profile",
                "01e606010039010b486f72697a6f6e576972650000",
                "01a40801004800010b486f72697a6f6e57697265000000000000",
            ),
            (
                "color_hidden",
                "01e60601002200ff000000",
                "01a40802002f00ff00001200012f",
            ),
            (
                "unbreakable_hidden",
                "01e60601000400",
                "01a40802000412000104",
            ),
            (
                "container",
                "01e60601003e06030100000000000002f1020000",
                "01a40801004d0601010300000000000001d903020000",
            ),
            (
                "can_break_hidden",
                "01e60601000c01010201000000",
                "01a40802001200010f0f0101020100000000",
            ),
        ] {
            let old = bytes(old);
            let pinned = bytes(pinned);
            let mut r = Reader::new(&old);
            let stack = {
                let _up = ids::renumbering(ServerVersion::V1_21_3.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap()
            };
            assert_eq!(r.remaining(), 0, "{label}");
            let mut expected = Slot::read(&mut Reader::new(&pinned)).unwrap();
            let mut actual = stack.clone();
            actual
                .as_mut()
                .unwrap()
                .added
                .sort_by_key(|(kind, _)| *kind);
            expected
                .as_mut()
                .unwrap()
                .added
                .sort_by_key(|(kind, _)| *kind);
            assert_eq!(actual, expected, "{label}");
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ServerVersion::V1_21_3.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut w);
            }
            assert_eq!(w.data, old, "{label}");
            let mut w = Writer::new();
            DelimitedSlot::write(&stack, &mut w);
            assert_eq!(
                DelimitedSlot::read(&mut Reader::new(&w.data)).unwrap(),
                stack,
                "{label}"
            );
        }
    }
}
