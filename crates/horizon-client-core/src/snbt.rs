//! Tag text output: `StringTagVisitor` (compact SNBT) and
//! `TextComponentTagVisitor` (the coloured form commands print), plus Java's
//! number formatting.
use crate::text::{self, Style, Text};
use horizon_world::nbt::Tag;
use horizon_world::snbt_text::string_of;
pub use horizon_world::snbt_text::{java_double, java_float, quote_and_escape, to_snbt};

// ---- the coloured form -----------------------------------------------------

/// Writer provenance for a live moving-piston command read, or its selected subtree.
#[derive(Clone, Copy)]
pub enum PistonPart {
    Entity,
    State,
    Properties,
}

struct Pretty {
    indentation: String,
    result: Text,
    indent_depth: usize,
    depth: usize,
    piston: Option<PistonPart>,
}
fn is_numeric(t: &Tag) -> bool {
    matches!(
        t,
        Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_)
    )
}
impl Pretty {
    fn lit(&mut self, s: &str, color: Option<u32>) {
        let t = match color {
            Some(c) => Text::styled(s, Style::color(c)),
            None => Text::literal(s),
        };
        self.result.push(t);
    }
    fn number(&mut self, s: &str, suffix: &str) {
        self.lit(s, Some(text::GOLD));
        if !suffix.is_empty() {
            self.lit(suffix, Some(text::RED));
        }
    }
    fn quoted(&mut self, input: &str, color: u32) {
        let q = quote_and_escape(input);
        let (open, inner) = (&q[..1], &q[1..q.len() - 1]);
        self.lit(open, None);
        self.lit(inner, Some(color));
        self.lit(open, None);
    }
    fn array<T: std::fmt::Display>(&mut self, prefix: &str, items: &[T], suffix: &str) {
        self.lit("[", None);
        self.lit(prefix, Some(text::RED));
        self.lit(";", None);
        for (i, v) in items.iter().enumerate().take(128) {
            self.lit(" ", None);
            self.number(&v.to_string(), suffix);
            if i != items.len() - 1 {
                self.lit(",", None);
            }
        }
        if items.len() > 128 {
            self.lit("<...>", Some(text::GRAY));
        }
        self.lit("]", None);
    }
    fn sub(&mut self, tag: &Tag, indent: bool) {
        self.sub_with(tag, indent, None);
    }
    fn sub_with(&mut self, tag: &Tag, indent: bool, piston: Option<PistonPart>) {
        let previous = std::mem::replace(&mut self.piston, piston);
        if indent {
            self.indent_depth += 1;
        }
        self.depth += 1;
        self.visit(tag);
        if indent {
            self.indent_depth -= 1;
        }
        self.depth -= 1;
        self.piston = previous;
    }
    fn visit(&mut self, tag: &Tag) {
        match tag {
            Tag::End => {}
            Tag::Byte(v) => self.number(&v.to_string(), "b"),
            Tag::Short(v) => self.number(&v.to_string(), "s"),
            Tag::Int(v) => self.number(&v.to_string(), ""),
            Tag::Long(v) => self.number(&v.to_string(), "L"),
            Tag::Float(v) => self.number(&java_float(f32::from_bits(*v)), "f"),
            Tag::Double(v) => self.number(&java_double(f64::from_bits(*v)), "d"),
            Tag::String(s) => self.quoted(&string_of(s), text::GREEN),
            Tag::ByteArray(a) => {
                let v: Vec<i8> = a.iter().map(|b| *b as i8).collect();
                self.array("B", &v, "b");
            }
            Tag::IntArray(a) => self.array("I", a, ""),
            Tag::LongArray(a) => self.array("L", a, "L"),
            Tag::List(l) => self.list(l),
            Tag::Compound(c) => self.compound(c),
        }
    }
    fn list(&mut self, l: &[Tag]) {
        if l.is_empty() {
            self.lit("[", None);
            self.lit("]", None);
        } else if self.depth >= 64 {
            self.lit("[", None);
            self.lit("<...>", Some(text::GRAY));
            self.lit("]", None);
        } else if l.len() >= 8 || l.iter().all(is_numeric) {
            self.lit("[", None);
            for (i, t) in l.iter().enumerate() {
                if i != 0 {
                    self.lit(",", None);
                    self.lit(" ", None);
                }
                self.sub(t, false);
            }
            self.lit("]", None);
        } else {
            self.lit("[", None);
            if !self.indentation.is_empty() {
                self.lit("\n", None);
            }
            let entry = self.indentation.repeat(self.indent_depth + 1);
            let spacing = if self.indentation.is_empty() {
                " "
            } else {
                "\n"
            };
            for (i, t) in l.iter().enumerate().take(128) {
                self.lit(&entry, None);
                self.sub(t, true);
                if i != l.len() - 1 {
                    self.lit(",", None);
                    self.lit(spacing, None);
                }
            }
            if l.len() > 128 {
                self.lit(&entry, None);
                self.lit("<...>", Some(text::GRAY));
            }
            if !self.indentation.is_empty() {
                self.lit("\n", None);
                let close = self.indentation.repeat(self.indent_depth);
                self.lit(&close, None);
            }
            self.lit("]", None);
        }
    }
    fn compound(&mut self, c: &horizon_world::nbt::Compound) {
        if c.is_empty() {
            self.lit("{", None);
            self.lit("}", None);
        } else if self.depth >= 64 {
            self.lit("{", None);
            self.lit("<...>", Some(text::GRAY));
            self.lit("}", None);
        } else {
            self.lit("{", None);
            if !self.indentation.is_empty() {
                self.lit("\n", None);
            }
            let entry = self.indentation.repeat(self.indent_depth + 1);
            let spacing = if self.indentation.is_empty() {
                " "
            } else {
                "\n"
            };
            let n = c.len();
            let entries = match self.piston {
                Some(part) => piston_hash_order(c, part),
                None => java_hash_order(c),
            };
            for (i, (k, v)) in entries.into_iter().enumerate() {
                self.lit(&entry, None);
                let key = string_of(k);
                if !key.is_empty()
                    && key
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-'))
                {
                    self.lit(&key, Some(text::AQUA));
                } else {
                    self.quoted(&key, text::AQUA);
                }
                self.lit(":", None);
                self.lit(" ", None);
                let child = match (self.piston, key.as_str()) {
                    (Some(PistonPart::Entity), "blockState") => Some(PistonPart::State),
                    (Some(PistonPart::State), "properties") => Some(PistonPart::Properties),
                    _ => None,
                };
                self.sub_with(v, true, child);
                if i + 1 < n {
                    self.lit(",", None);
                    self.lit(spacing, None);
                }
            }
            if !self.indentation.is_empty() {
                self.lit("\n", None);
                let close = self.indentation.repeat(self.indent_depth);
                self.lit(&close, None);
            }
            self.lit("}", None);
        }
    }
}
fn hash_bucket(len: usize, key: &horizon_world::nbt::JavaString) -> usize {
    let mut capacity = 16usize;
    while len > capacity * 3 / 4 {
        capacity *= 2;
    }
    let h = string_of(key)
        .encode_utf16()
        .fold(0u32, |h, ch| h.wrapping_mul(31).wrapping_add(u32::from(ch)));
    ((h ^ (h >> 16)) as usize) & (capacity - 1)
}
fn piston_hash_order(
    c: &horizon_world::nbt::Compound,
    part: PistonPart,
) -> Vec<(&horizon_world::nbt::JavaString, &Tag)> {
    let mut entries: Vec<_> = c.iter().collect();
    match part {
        PistonPart::Entity => {
            // saveAdditional, then the base's components and saveMetadata.
            let fields = [
                "blockState",
                "facing",
                "progress",
                "extending",
                "source",
                "components",
                "id",
                "x",
                "y",
                "z",
            ];
            entries.sort_by_key(|(k, _)| {
                (
                    hash_bucket(c.len(), k),
                    fields
                        .iter()
                        .position(|s| *s == string_of(k))
                        .unwrap_or(fields.len()),
                )
            });
        }
        PistonPart::Properties => {
            // StateDefinition builds property codecs in sorted-name order.
            // DFU's nested map-pair encoder writes the second codec first.
            entries.sort_by_key(|(k, _)| (hash_bucket(c.len(), k), std::cmp::Reverse(*k)));
        }
        PistonPart::State => return java_hash_order(c),
    }
    entries
}
/// The iteration order of the `HashMap<String, Tag>` a `CompoundTag` holds,
/// for a map that grew to its size by insertion: buckets by
/// `HashMap.hash(String.hashCode())`, keys sharing a bucket in key order.
pub fn java_hash_order(
    c: &horizon_world::nbt::Compound,
) -> Vec<(&horizon_world::nbt::JavaString, &Tag)> {
    // Keys sharing a bucket iterate in insertion order: the order vanilla's
    // savers write them, as far as it is known here.
    const WRITE_ORDER: &[&str] = &[
        // ItemStack.CODEC.
        "id",
        "count",
        "components",
        // Entity.saveWithoutId.
        "Pos",
        "Motion",
        "Rotation",
        "fall_distance",
        "Fire",
        "Air",
        "OnGround",
        "Invulnerable",
        "PortalCooldown",
        "UUID",
        "CustomName",
        "CustomNameVisible",
        "Silent",
        "NoGravity",
        "Glowing",
        "TicksFrozen",
        "HasVisualFire",
        "Tags",
        "data",
        "Passengers",
        // LivingEntity.addAdditionalSaveData.
        "Health",
        "HurtTime",
        "DeathTime",
        "AbsorptionAmount",
        "current_impulse_context_reset_grace_time",
        "current_explosion_impact_pos",
        "attributes",
        "active_effects",
        "FallFlying",
        "sleeping_pos",
        "Brain",
        "equipment",
        // ArmorStand.
        "Invisible",
        "Small",
        "ShowArms",
        "DisabledSlots",
        "NoBasePlate",
        "Marker",
        "Pose",
        // Mannequin.addAdditionalSaveData.
        "profile",
        "hidden_layers",
        "main_hand",
        "pose",
        "immovable",
        "description",
        "hide_description",
    ];
    // BlockEntity.saveMetadata writes the position after the entity's data.
    const AFTER: &[&str] = &["x", "y", "z"];
    let rank = |k: &horizon_world::nbt::JavaString| {
        let k = string_of(k);
        WRITE_ORDER
            .iter()
            .position(|w| *w == k)
            .or_else(|| {
                AFTER
                    .iter()
                    .position(|w| *w == k)
                    .map(|i| WRITE_ORDER.len() + 1 + i)
            })
            .unwrap_or(WRITE_ORDER.len())
    };
    let mut entries: Vec<_> = c.iter().collect();
    entries.sort_by_key(|(k, _)| (hash_bucket(c.len(), k), rank(k)));
    entries
}
/// `NbtUtils.toPrettyComponent`: the coloured, unindented form.
pub fn pretty(tag: &Tag) -> Text {
    pretty_indented(tag, "")
}
pub fn pretty_indented(tag: &Tag, indentation: &str) -> Text {
    let mut p = Pretty {
        indentation: indentation.to_owned(),
        result: Text::empty(),
        indent_depth: 0,
        depth: 0,
        piston: None,
    };
    p.visit(tag);
    p.result
}
/// Command-only writer order; ordinary pretty printing keeps its existing policy.
pub fn pretty_piston(tag: &Tag, part: PistonPart) -> Text {
    let mut p = Pretty {
        indentation: String::new(),
        result: Text::empty(),
        indent_depth: 0,
        depth: 0,
        piston: Some(part),
    };
    p.visit(tag);
    p.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_world::nbt::parse_snbt;

    fn rt(s: &str) -> String {
        to_snbt(&parse_snbt(s).unwrap().0)
    }
    #[test]
    fn live_piston_writer_order_does_not_change_default_pretty() {
        let tag = parse_snbt(
            r#"{id:"minecraft:piston",blockState:{id:"minecraft:piston_head",properties:{facing:"east",short:"false",type:"normal"}},components:{},x:1,y:100,z:0,facing:5b,progress:0.0f,source:1b,extending:1b}"#,
        ).unwrap().0;
        let ordinary = r#"{blockState: {id: "minecraft:piston_head", properties: {facing: "east", short: "false", type: "normal"}}, components: {}, x: 1, facing: 5b, progress: 0.0f, y: 100, z: 0, id: "minecraft:piston", source: 1b, extending: 1b}"#;
        assert_eq!(pretty(&tag).plain(), ordinary);
        assert_eq!(
            pretty_piston(&tag, PistonPart::Entity).plain(),
            r#"{blockState: {id: "minecraft:piston_head", properties: {short: "false", facing: "east", type: "normal"}}, components: {}, x: 1, facing: 5b, progress: 0.0f, y: 100, z: 0, source: 1b, id: "minecraft:piston", extending: 1b}"#,
        );
        let state = tag.get("blockState").unwrap();
        assert_eq!(
            pretty_piston(state, PistonPart::State).plain(),
            r#"{id: "minecraft:piston_head", properties: {short: "false", facing: "east", type: "normal"}}"#,
        );
        let props = state.get("properties").unwrap();
        assert_eq!(
            pretty_piston(props, PistonPart::Properties).plain(),
            r#"{short: "false", facing: "east", type: "normal"}"#,
        );
        assert_eq!(
            pretty(props).plain(),
            r#"{facing: "east", short: "false", type: "normal"}"#
        );
        assert_eq!(pretty(&tag).plain(), ordinary);
    }
    #[test]
    fn mannequin_compound_orders_pose_before_immovable_in_shared_bucket() {
        let tag = parse_snbt(
            r#"{AbsorptionAmount:0.0f,Air:300s,Brain:{memories:{}},DeathTime:0s,FallFlying:0b,Fire:0s,Health:20.0f,HurtTime:0s,Invulnerable:0b,Motion:[0.0d,0.0d,0.0d],NoGravity:1b,OnGround:0b,PortalCooldown:0,Pos:[2.5d,100.0d,2.5d],Rotation:[0.0f,0.0f],Tags:["m1"],UUID:[I;369,0,0,1],attributes:[],current_impulse_context_reset_grace_time:0,description:{text:"Guard"},hide_description:1b,immovable:1b,main_hand:"left",pose:"crouching",profile:{name:"alex"}}"#,
        )
        .unwrap()
        .0;
        let rendered = pretty(&tag).plain();
        let pose_idx = rendered.find("pose: \"crouching\"").unwrap();
        let immovable_idx = rendered.find("immovable: 1b").unwrap();
        assert!(
            pose_idx < immovable_idx,
            "expected `pose` before `immovable` in bucket 5, got: {rendered}"
        );
    }
    #[test]
    fn compact_form_matches_vanilla() {
        assert_eq!(
            rt("{b:1b,a:\"x\",c:[1,2],d:1.5f,e:2d,f:3L,g:4s}"),
            "{a:\"x\",b:1b,c:[1,2],d:1.5f,e:2.0d,f:3L,g:4s}"
        );
        assert_eq!(rt("{\"a b\":1}"), "{\"a b\":1}");
        assert_eq!(rt("{true:1}"), "{\"true\":1}");
        assert_eq!(rt("[B;1b,2b]"), "[B;1B,2B]");
        assert_eq!(rt("[I;1,2]"), "[I;1,2]");
        assert_eq!(rt("[L;1L]"), "[L;1L]");
        assert_eq!(quote_and_escape("say \"hi\""), "'say \"hi\"'");
        assert_eq!(quote_and_escape("it's \"x\""), "\"it's \\\"x\\\"\"");
        assert_eq!(quote_and_escape("a\nb\\"), "\"a\\nb\\\\\"");
    }
    #[test]
    fn java_number_text() {
        assert_eq!(java_double(1.0), "1.0");
        assert_eq!(java_double(0.1), "0.1");
        assert_eq!(java_double(1.0e7), "1.0E7");
        assert_eq!(java_double(12345678.9), "1.23456789E7");
        assert_eq!(java_double(0.0001), "1.0E-4");
        assert_eq!(java_double(-3.5), "-3.5");
        assert_eq!(java_float(0.3), "0.3");
        assert_eq!(java_float(100.0), "100.0");
        assert_eq!(java_double(f64::NAN), "NaN");
    }
    #[test]
    fn pretty_form_is_coloured_without_changing_text() {
        let tag = parse_snbt("{Pos:[1.5d,2.0d],id:\"minecraft:pig\",n:3b}")
            .unwrap()
            .0;
        let t = pretty(&tag);
        assert_eq!(
            t.plain(),
            "{Pos: [1.5d, 2.0d], id: \"minecraft:pig\", n: 3b}"
        );
        // keys aqua, strings green, numbers gold.
        let colors: Vec<_> = t.spans.iter().filter_map(|s| s.style.color).collect();
        assert!(colors.contains(&text::AQUA));
        assert!(colors.contains(&text::GREEN));
        assert!(colors.contains(&text::GOLD));
        let long = parse_snbt("{l:[{a:1},{b:2}]}").unwrap().0;
        assert_eq!(pretty(&long).plain(), "{l: [{a: 1}, {b: 2}]}");
    }
}
