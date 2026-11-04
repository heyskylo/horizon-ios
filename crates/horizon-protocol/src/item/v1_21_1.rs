//! Item patches before 1.21.2 split FoodProperties into food, consumable and use_remainder.
use super::*;
use crate::Error;

const FOOD: i32 = 23;
const CONSUMABLE: i32 = 24;
const FIRE_RESISTANT: i32 = 27;
const OLD_FOOD: i32 = 20;
const OLD_FIRE_RESISTANT: i32 = 21;

pub(super) fn reading() -> bool {
    matches!(ids::active(),Some((ids,ids::Way::Up)) if ids.quirks.components_before_1_21_2)
}
pub(super) fn writing() -> bool {
    matches!(ids::active(),Some((ids,ids::Way::Down)) if ids.quirks.components_before_1_21_2)
}

pub(super) fn read_stack(r: &mut Reader) -> Result<Option<Stack>> {
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
    let (additional_id, hide_id) = v1_21_4::tooltip_ids();
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
        if old == OLD_FOOD {
            read_food(r, &mut stack)?;
            continue;
        }
        if old == OLD_FIRE_RESISTANT {
            stack
                .added
                .push((FIRE_RESISTANT, Component::Raw(fire_types())));
            continue;
        }
        let kind = ids::component(old);
        let value = match kind {
            component::POTION_CONTENTS => read_potion(r)?,
            component::INSTRUMENT => read_instrument(r)?,
            _ => v1_21_4::read_component(kind, r)?,
        };
        if v1_21_4::TOOLTIP_FLAGS.contains(&kind) && !r.bool()? {
            hidden.push(kind);
        }
        stack.added.push((kind, value));
    }
    if additional {
        hidden.extend(
            stack
                .added
                .iter()
                .map(|(kind, _)| *kind)
                .filter(|kind| v1_21_4::ADDITIONAL.contains(kind)),
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
        if old == OLD_FOOD {
            for kind in [FOOD, CONSUMABLE, component::USE_REMAINDER] {
                remove(&mut stack, kind);
            }
        } else {
            let kind = if old == additional_id || old == hide_id {
                component::TOOLTIP_DISPLAY
            } else if old == OLD_FIRE_RESISTANT {
                FIRE_RESISTANT
            } else {
                ids::component(old)
            };
            remove(&mut stack, kind);
        }
    }
    Ok(Some(stack))
}

fn remove(stack: &mut Stack, kind: i32) {
    if kind >= 0 && !stack.removed.contains(&kind) {
        stack.removed.push(kind);
    }
}
fn fire_types() -> Vec<u8> {
    let mut w = Writer::new();
    w.varint(0);
    w.string("minecraft:is_fire");
    w.data
}

fn read_potion(r: &mut Reader) -> Result<Component> {
    let potion = if r.bool()? { Some(r.varint()?) } else { None };
    let color = if r.bool()? { Some(r.i32()?) } else { None };
    let mut tail = Writer::new();
    let n = r.count(65536)?;
    tail.varint(n as i32);
    for _ in 0..n {
        effect(r, &mut tail, true)?;
    }
    tail.bool(false); // The optional custom name was added in 1.21.2.
    Ok(Component::Potion {
        potion,
        color,
        rest: if n == 0 { Vec::new() } else { tail.data },
    })
}
fn write_potion(value: &Component, w: &mut Writer) -> Result<()> {
    let Component::Potion {
        potion,
        color,
        rest,
    } = value
    else {
        return Err(Error::new("potion has no decoded value"));
    };
    w.bool(potion.is_some());
    if let Some(id) = potion {
        w.varint(*id);
    }
    w.bool(color.is_some());
    if let Some(color) = color {
        w.i32(*color);
    }
    if rest.is_empty() {
        w.varint(0);
    } else {
        let mut r = Reader::new(rest);
        let n = r.count(65536)?;
        w.varint(n as i32);
        for _ in 0..n {
            effect(&mut r, w, false)?;
        }
        if r.bool()? {
            r.string(32767)?;
        }
    }
    Ok(())
}
fn read_instrument(r: &mut Reader) -> Result<Component> {
    let holder = r.varint()?;
    if holder != 0 {
        return Ok(Component::Instrument(holder - 1));
    }
    let mut w = Writer::new();
    w.varint(0);
    sound(r, &mut w, true)?;
    w.f32(r.varint()? as f32 / 20.0);
    w.f32(r.f32()?);
    w.varint(0); // Pinned durability damage defaults to zero.
    w.nbt(&Tag::String("".into()));
    Ok(Component::Raw(w.data))
}
fn write_instrument(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(bytes);
    let mut w = Writer::new();
    let holder = r.varint()?;
    w.varint(holder);
    if holder == 0 {
        sound(&mut r, &mut w, false)?;
        w.varint((r.f32()? * 20.0) as i32);
        w.f32(r.f32()?);
        r.varint()?;
        r.nbt()?;
    }
    Ok(w.data)
}
fn sound(r: &mut Reader, w: &mut Writer, up: bool) -> Result<()> {
    let holder = r.varint()?;
    if holder == 0 {
        w.varint(0);
        w.string(&r.string(32767)?);
        let range = r.bool()?;
        w.bool(range);
        if range {
            w.f32(r.f32()?);
        }
    } else {
        let mapped = match ids::active() {
            Some((ids, _)) if up => ids.sound_event.up(holder - 1),
            Some((ids, _)) => ids.sound_event.down(holder - 1),
            None => Some(holder - 1),
        }
        .ok_or_else(|| Error::new("instrument sound absent from target registry"))?;
        w.varint(mapped + 1);
    }
    Ok(())
}

fn read_food(r: &mut Reader, stack: &mut Stack) -> Result<()> {
    let mut food = Writer::new();
    food.varint(r.varint()?);
    food.f32(r.f32()?);
    food.bool(r.bool()?);
    let seconds = r.f32()?;
    let remainder = if r.bool()? {
        Some(Template::read(r)?)
    } else {
        None
    };
    let n = r.count(65536)?;
    let mut consumable = Writer::new();
    consumable.f32(seconds);
    consumable.varint(1); // ItemUseAnimation.EAT
    consumable.varint(703); // Pinned entity.generic.eat (702), as a sound holder.
    consumable.bool(true);
    consumable.varint(n as i32);
    for _ in 0..n {
        consumable.varint(0); // ApplyStatusEffectsConsumeEffect
        consumable.varint(1);
        effect(r, &mut consumable, true)?;
        consumable.f32(r.f32()?);
    }
    stack.added.push((FOOD, Component::Raw(food.data)));
    stack
        .added
        .push((CONSUMABLE, Component::Raw(consumable.data)));
    if let Some(remainder) = remainder {
        let mut w = Writer::new();
        let _pinned = ids::unrenumbered();
        Template::write(&remainder, &mut w);
        stack
            .added
            .push((component::USE_REMAINDER, Component::Raw(w.data)));
    } else {
        remove(stack, component::USE_REMAINDER);
    }
    Ok(())
}

fn effect(r: &mut Reader, w: &mut Writer, up: bool) -> Result<()> {
    let old = r.varint()?;
    let mapped = match ids::active() {
        Some((ids, _)) if up => ids.mob_effect.up(old),
        Some((ids, _)) => ids.mob_effect.down(old),
        None => Some(old),
    }
    .ok_or_else(|| Error::new("effect absent from target registry"))?;
    w.varint(mapped);
    // Hidden effects form a linked list, not a tree; iterate without an unbounded call stack.
    loop {
        w.varint(r.varint()?);
        w.varint(r.varint()?);
        for _ in 0..3 {
            w.bool(r.bool()?);
        }
        let hidden = r.bool()?;
        w.bool(hidden);
        if !hidden {
            return Ok(());
        }
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
    let (hide, hidden) = match stack.component(component::TOOLTIP_DISPLAY) {
        Some(Component::TooltipDisplay { hide, hidden }) => (*hide, hidden.as_slice()),
        _ => (false, &[][..]),
    };
    let (additional_id, hide_id) = v1_21_4::tooltip_ids();
    let mut added = Vec::new();
    for (kind, value) in &stack.added {
        let old = ids::component(*kind);
        if old < 0 {
            continue;
        }
        let mut body = Writer::new();
        if *kind == FOOD {
            if stack.removed.contains(&CONSUMABLE) || stack.removed.contains(&FOOD) {
                continue;
            }
            let Ok(bytes) = write_food(stack, value) else {
                continue;
            };
            body.bytes(&bytes);
        } else if *kind == FIRE_RESISTANT {
            if !matches!(value,Component::Raw(bytes) if *bytes==fire_types()) {
                continue;
            }
            // The old component is Unit.
        } else if *kind == component::POTION_CONTENTS {
            if write_potion(value, &mut body).is_err() {
                continue;
            }
        } else if let (component::INSTRUMENT, Component::Raw(bytes)) = (*kind, value) {
            let Ok(bytes) = write_instrument(bytes) else {
                continue;
            };
            body.bytes(&bytes);
        } else {
            v1_21_4::write_component(*kind, value, &mut body);
            if v1_21_4::TOOLTIP_FLAGS.contains(kind) {
                body.bool(!hidden.contains(kind));
            }
        }
        added.push((old, body.data));
    }
    if hide {
        added.push((hide_id, Vec::new()));
    }
    if hidden.iter().any(|kind| v1_21_4::ADDITIONAL.contains(kind)) {
        added.push((additional_id, Vec::new()));
    }
    let mut removed = Vec::new();
    for kind in &stack.removed {
        let old = if *kind == FOOD || *kind == CONSUMABLE {
            OLD_FOOD
        } else {
            ids::component(*kind)
        };
        if old >= 0 && !removed.contains(&old) {
            removed.push(old);
        }
    }
    if stack.removed.contains(&component::TOOLTIP_DISPLAY) {
        removed.extend([additional_id, hide_id]);
    }
    removed.sort_unstable();
    removed.dedup();
    w.varint(stack.count);
    w.varint(item);
    w.varint(added.len() as i32);
    w.varint(removed.len() as i32);
    for (kind, body) in added {
        w.varint(kind);
        w.bytes(&body);
    }
    for kind in removed {
        w.varint(kind);
    }
}

fn raw(value: &Component) -> Result<&[u8]> {
    if let Component::Raw(bytes) = value {
        Ok(bytes)
    } else {
        Err(Error::new("food component is not encoded"))
    }
}
fn write_food(stack: &Stack, value: &Component) -> Result<Vec<u8>> {
    let mut food = Reader::new(raw(value)?);
    let mut w = Writer::new();
    w.varint(food.varint()?);
    w.f32(food.f32()?);
    w.bool(food.bool()?);
    let mut effects = Vec::new();
    let seconds = if let Some(value) = stack.component(CONSUMABLE) {
        let mut r = Reader::new(raw(value)?);
        let seconds = r.f32()?;
        r.varint()?; // animation has no field in old FoodProperties
        skip_sound(&mut r)?;
        r.bool()?;
        for _ in 0..r.count(65536)? {
            match r.varint()? {
                0 => {
                    let n = r.count(65536)?;
                    let mut group = Vec::with_capacity(n);
                    for _ in 0..n {
                        let mut value = Writer::new();
                        effect(&mut r, &mut value, false)?;
                        group.push(value.data);
                    }
                    let probability = r.f32()?;
                    effects.extend(group.into_iter().map(|effect| (effect, probability)));
                }
                1 => skip_holder_set(&mut r)?,
                2 => {}
                3 => {
                    r.f32()?;
                    r.bool()?;
                }
                4 => skip_sound(&mut r)?,
                _ => return Err(Error::new("unknown consume effect")),
            }
        }
        seconds
    } else {
        1.6
    };
    w.f32(seconds);
    let remainder = if let Some(value) = stack.component(component::USE_REMAINDER) {
        let _pinned = ids::unrenumbered();
        Some(Template::read(&mut Reader::new(raw(value)?))?)
    } else {
        None
    };
    w.bool(remainder.is_some());
    if let Some(remainder) = remainder {
        Template::write(&remainder, &mut w);
    }
    w.varint(effects.len() as i32);
    for (effect, probability) in effects {
        w.bytes(&effect);
        w.f32(probability);
    }
    Ok(w.data)
}
fn skip_sound(r: &mut Reader) -> Result<()> {
    if r.varint()? == 0 {
        r.string(32767)?;
        if r.bool()? {
            r.f32()?;
        }
    }
    Ok(())
}
fn skip_holder_set(r: &mut Reader) -> Result<()> {
    let count = r.varint()?;
    if count == 0 {
        r.string(32767)?;
    } else {
        let n = usize::try_from(count - 1).map_err(|_| Error::new("negative holder set"))?;
        if n > r.remaining() {
            return Err(Error::new("holder set past the end"));
        }
        for _ in 0..n {
            r.varint()?;
        }
    }
    Ok(())
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
    fn canonical(mut stack: Option<Stack>) -> Option<Stack> {
        if let Some(stack) = &mut stack {
            stack.added.sort_by_key(|(kind, _)| *kind);
            stack.removed.sort_unstable();
        }
        stack
    }
    #[test]
    fn only_the_fire_tag_downgrades_to_fire_resistance() {
        for (label, pinned, old) in [
            (
                "fire tag",
                "01a408020003111b00116d696e6563726166743a69735f66697265",
                "01d0060200031115",
            ),
            (
                "projectile tag",
                "01a408020003111b00176d696e6563726166743a69735f70726f6a656374696c65",
                "01d00601000311",
            ),
            (
                "explicit damage type",
                "01a408020003111b0200",
                "01d00601000311",
            ),
            ("empty damage set", "01a408020003111b01", "01d00601000311"),
            ("removed resistance", "01a408010103111b", "01d0060101031115"),
        ] {
            let pinned = bytes(pinned);
            let mut r = Reader::new(&pinned);
            let stack = Slot::read(&mut r).unwrap_or_else(|e| panic!("{label}: {e}"));
            assert_eq!(r.remaining(), 0, "{label}");
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ServerVersion::V1_21_1.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut w);
            }
            // The unit component cannot express another tag or an explicit set. Damage stays
            // present, and removing resistance still removes the old fire-resistance unit.
            assert_eq!(w.data, bytes(old), "{label}");
        }
    }
    #[test]
    fn vanilla_component_fixtures_preserve_food_effects_remainders_and_following_fields() {
        // The actual vanilla DataComponentPatch.STREAM_CODEC plus count/item fields from each
        // version's registry. Encoding the parsed patch directly retains explicit removals.
        for (label, old, pinned) in [
            (
                "damage_model",
                "01d00602000db9600311",
                "01a408020011014640e4000000000311",
            ),
            (
                "potion",
                "01d006020003111f0119010001e2400112022801000000",
                "01a40802000311350119010001e240011202280100000000",
            ),
            ("potion_empty", "01d00601001f000000", "01a40801003500000000"),
            (
                "instrument",
                "01d006020003112800c3055042800000",
                "01a408020003113f00ce06408000004280000000080000",
            ),
            (
                "instrument_direct_sound",
                "01d00601002800000e6d696e6563726166743a7465737401408000005a42000000",
                "01a40801003f00000e6d696e6563726166743a746573740140800000409000004200000000080000",
            ),
            (
                "attributes",
                "01d00601000c02020b6d696e6563726166743a783ff80000000000000001150b6d696e6563726166743a793fd0000000000000020000",
                "01a40802001002030b6d696e6563726166743a783ff80000000000000001001a0b6d696e6563726166743a793fd000000000000002000012000110",
            ),
            (
                "food_simple",
                "01d0060200031114033fa00000003fcccccd0000",
                "01a40803010311183fcccccd01bf05010017033fa000000019",
            ),
            (
                "food_full",
                "01d0060200031114033fa00000013f4ccccd01019f06000002120228010000003e80000009000a000101003f400000",
                "01a40804000311183f4ccccd01bf0501020001120228010000003e800000000109000a000101003f40000017033fa000000119ee07010000",
            ),
            (
                "food_nested",
                "01d006010014013f000000003fcccccd0101d00602000d071500",
                "01a4080300183fcccccd01bf05010017013f0000000019a408010200110140e000000000001b00116d696e6563726166743a69735f66697265",
            ),
            (
                "fire",
                "01d0060200031115",
                "01a408020003111b00116d696e6563726166743a69735f66697265",
            ),
            ("removed_food", "01d006000114", "01a4080003181719"),
            ("removed_fire", "01d006000115", "01a40800011b"),
            (
                "hidden",
                "01d00604000f04001800ff0000000e",
                "01a4080300042f00ff0000120102042f",
            ),
            (
                "tool",
                "01d006010016020301b50201408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b61786500004000000002",
                "01a40801001c020301900301408000000101001a6d696e6563726166743a6d696e6561626c652f7069636b6178650000400000000201",
            ),
            (
                "profile",
                "01d00601002f010b486f72697a6f6e576972650000",
                "01a40801004800010b486f72697a6f6e57697265000000000000",
            ),
            (
                "container",
                "01d00601003406030100000000000002e4020000",
                "01a40801004d0601010300000000000001d903020000",
            ),
            (
                "bundle",
                "01d00601001e0202e402000003d00601000d09",
                "01a40801003402d903020000a408030100110141100000000000",
            ),
            (
                "can_break",
                "01d00601000b01010201000000",
                "01a40802001200010f0f0101020100000000",
            ),
        ] {
            let old = bytes(old);
            let pinned = bytes(pinned);
            let mut r = Reader::new(&old);
            let stack = {
                let _up = ids::renumbering(ServerVersion::V1_21_1.ids().unwrap(), Way::Up);
                Slot::read(&mut r).unwrap_or_else(|e| panic!("{label}: {e}"))
            };
            assert_eq!(r.remaining(), 0, "{label}");
            let expected = Slot::read(&mut Reader::new(&pinned))
                .unwrap_or_else(|e| panic!("{label} pinned: {e}"));
            assert_eq!(canonical(stack.clone()), canonical(expected), "{label}");
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ServerVersion::V1_21_1.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut w);
            }
            // Tooltip unit components may move within the unordered patch when coalesced.
            if label != "hidden" {
                assert_eq!(w.data, old, "{label}");
            }
            let mut r = Reader::new(&w.data);
            let back = {
                let _up = ids::renumbering(ServerVersion::V1_21_1.ids().unwrap(), Way::Up);
                Slot::read(&mut r)
                    .unwrap_or_else(|e| panic!("{label} returned: {e}; bytes={:02x?}", w.data))
            };
            assert_eq!(r.remaining(), 0, "{label}");
            assert_eq!(canonical(back), canonical(stack.clone()), "{label}");
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
