//! The two component layouts changed in 1.21.6, from ItemAttributeModifiers.Entry.STREAM_CODEC
//! and Equippable.STREAM_CODEC. Raw values stored in a Stack always use the pinned layout.
use super::*;
use crate::Error;

pub(super) fn read(kind: i32, r: &mut Reader) -> Result<Option<Vec<u8>>> {
    if ids::quirks().components_before_1_21_6 {
        rewrite(kind, r, true)
    } else {
        Ok(None)
    }
}

pub(super) fn write(kind: i32, bytes: &[u8]) -> Option<Vec<u8>> {
    if matches!(ids::active(), Some((ids, ids::Way::Down)) if ids.quirks.components_before_1_21_6) {
        rewrite(kind, &mut Reader::new(bytes), false).ok().flatten()
    } else {
        None
    }
}

fn rewrite(kind: i32, r: &mut Reader, up: bool) -> Result<Option<Vec<u8>>> {
    let mut w = Writer::new();
    match kind {
        16 => {
            let n = r.count(65536)?;
            w.varint(n as i32);
            for _ in 0..n {
                let attribute = r.varint()?;
                let mapped = match ids::active() {
                    Some((ids, ids::Way::Up)) => ids.attribute.up(attribute),
                    Some((ids, ids::Way::Down)) => ids.attribute.down(attribute),
                    None => Some(attribute),
                }
                .ok_or_else(|| Error::new("attribute absent from target registry"))?;
                w.varint(mapped);
                w.string(&r.string(32767)?);
                w.f64(r.f64()?);
                w.varint(r.varint()?);
                w.varint(r.varint()?);
                if up {
                    w.varint(0); // Display.Default, added in 1.21.6.
                } else {
                    match r.varint()? {
                        0 | 1 => {}
                        2 => {
                            r.nbt()?;
                        }
                        _ => return Err(Error::new("unknown attribute modifier display")),
                    }
                }
            }
        }
        component::EQUIPPABLE => {
            w.varint(r.varint()?);
            sound(r, &mut w)?;
            for _ in 0..2 {
                let present = r.bool()?;
                w.bool(present);
                if present {
                    w.string(&r.string(32767)?);
                }
            }
            let allowed = r.bool()?;
            w.bool(allowed);
            if allowed {
                let n = r.varint()?;
                if n == 0 {
                    w.varint(0);
                    w.string(&r.string(32767)?);
                } else {
                    let count = usize::try_from(n - 1)
                        .map_err(|_| Error::new("negative holder set size"))?;
                    if count > r.remaining() {
                        return Err(Error::new("holder set past the end"));
                    }
                    let entities = (0..count)
                        .map(|_| r.varint().map(ids::entity_type))
                        .collect::<Result<Vec<_>>>()?;
                    let entities: Vec<_> = entities.into_iter().filter(|id| *id >= 0).collect();
                    w.varint(entities.len() as i32 + 1);
                    for entity in entities {
                        w.varint(entity);
                    }
                }
            }
            for _ in 0..4 {
                w.bool(r.bool()?);
            }
            if up {
                w.bool(false); // Equippable's default canBeSheared.
                w.varint(1448); // Pinned item.shears.snip (1447) as a sound holder.
            } else {
                r.bool()?;
                sound(r, &mut Writer::new())?;
            }
        }
        _ => return Ok(None),
    }
    Ok(Some(w.data))
}

fn sound(r: &mut Reader, w: &mut Writer) -> Result<()> {
    let holder = r.varint()?;
    if holder == 0 {
        w.varint(0);
        w.string(&r.string(32767)?);
        let fixed = r.bool()?;
        w.bool(fixed);
        if fixed {
            w.f32(r.f32()?);
        }
    } else {
        w.varint(ids::sound_event(holder - 1) + 1);
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

    #[test]
    fn vanilla_1_21_5_component_stacks_match_the_pinned_oracle_and_go_back() {
        // Independent ItemStack.OPTIONAL_STREAM_CODEC output from vanilla 1.21.5 and 26.3.
        // Covers multiple modifiers, bare/list/direct-sound equippables and a following damage value.
        for (label, old, pinned) in [
            (
                "attributes",
                "01f80601000d02020b6d696e6563726166743a783ff80000000000000001150b6d696e6563726166743a793fd00000000000000200",
                "01a40801001002030b6d696e6563726166743a783ff80000000000000001001a0b6d696e6563726166743a793fd0000000000000020000",
            ),
            (
                "equip_bare",
                "01f80601001c034700000001010100",
                "01a40801002003470000000101010000a80b",
            ),
            (
                "equip_list",
                "01f80601001c0448010e6d696e6563726166743a676f6c64011a6d696e6563726166743a6d6973632f70756d706b696e626c757201035e900100000001",
                "01a4080100200448010e6d696e6563726166743a676f6c64011a6d696e6563726166743a6d6973632f70756d706b696e626c75720103659a010000000100a80b",
            ),
            (
                "equip_direct",
                "01f80601001c04000e6d696e6563726166743a74657374014080000000000001010100",
                "01a40801002004000e6d696e6563726166743a7465737401408000000000000101010000a80b",
            ),
            (
                "combined",
                "01f80603000d01000b6d696e6563726166743a7a4008000000000000000003111c034700000001010100",
                "01a40803001001010b6d696e6563726166743a7a400800000000000000000003112003470000000101010000a80b",
            ),
        ] {
            let old = bytes(old);
            let pinned = bytes(pinned);
            let mut r = Reader::new(&old);
            let stack = {
                let _up = ids::renumbering(ServerVersion::V1_21_5.ids().unwrap(), Way::Up);
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
                let _down = ids::renumbering(ServerVersion::V1_21_5.ids().unwrap(), Way::Down);
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
}
