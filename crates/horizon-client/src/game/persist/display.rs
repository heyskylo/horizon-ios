use super::super::{Game, commands::OtherEntity, native_entities::DisplayTransformation};
use horizon_client::{
    entity_nbt,
    nbt_path::int_value,
    save::{float, number, text},
    scoreboard::{component_tag, tag_to_json},
};
use horizon_items::item_nbt::{item_from_nbt, item_to_nbt_with_items};
use horizon_world::nbt::Tag;

impl Game<'_> {
    /// `Display.addAdditionalSaveData`: save the settings as the codecs read
    /// them, including defaults. The raw start delay is only used by tracking.
    pub(super) fn write_display_nbt(&self, entity: &OtherEntity, tag: &mut Tag) {
        let transformation = entity
            .item_transformation
            .unwrap_or_else(|| DisplayTransformation::from_tag(tag.get("transformation")));
        let int = |key, default| tag.get(key).and_then(int_value).unwrap_or(default);
        let real = |key, default| {
            let value = float(tag.get(key)).map_or(default, |v| v as f32);
            Tag::Float(if value == 0.0 { 0 } else { value.to_bits() })
        };
        let brightness = tag.get("brightness").and_then(|value| {
            let light = |key| {
                // Codec.INT uses Number.intValue, unlike ValueInput.getIntOr,
                // which floors FloatTag and DoubleTag before narrowing.
                let value = value.get(key)?;
                let light = match value {
                    Tag::Float(_) | Tag::Double(_) => float(Some(value))? as i32,
                    _ => number(Some(value))? as i32,
                };
                (0..=15).contains(&light).then_some(Tag::Int(light))
            };
            Some(Tag::compound([
                ("block", light("block")?),
                ("sky", light("sky")?),
            ]))
        });
        let values = [
            ("transformation", transformation.to_tag()),
            (
                "billboard",
                saved_enum(
                    tag,
                    "billboard",
                    "fixed",
                    &["vertical", "horizontal", "center"],
                ),
            ),
            (
                "interpolation_duration",
                Tag::Int(int("interpolation_duration", 0)),
            ),
            (
                "teleport_duration",
                Tag::Int(int("teleport_duration", 0).clamp(0, 59)),
            ),
            ("view_range", real("view_range", 1.0)),
            ("shadow_radius", real("shadow_radius", 0.0)),
            ("shadow_strength", real("shadow_strength", 1.0)),
            ("width", real("width", 0.0)),
            ("height", real("height", 0.0)),
            (
                "glow_color_override",
                Tag::Int(int("glow_color_override", -1)),
            ),
        ];
        let Tag::Compound(fields) = tag else {
            return;
        };
        fields.remove(&"start_interpolation".into());
        fields.remove(&"brightness".into());
        if let Some(brightness) = brightness {
            fields.insert("brightness".into(), brightness);
        }
        for (key, value) in values {
            fields.insert(key.into(), value);
        }

        match entity.kind.as_str() {
            "minecraft:text_display" => write_text_display(tag),
            "minecraft:block_display" => {
                let state = tag
                    .get("block_state")
                    .and_then(|value| self.state_from_nbt(value));
                let saved = state.map_or_else(
                    || Tag::String("minecraft:air".into()),
                    |s| self.state_to_nbt(s),
                );
                if let Tag::Compound(fields) = tag {
                    fields.insert("block_state".into(), saved);
                }
            }
            "minecraft:item_display" => {
                let context = saved_enum(
                    tag,
                    "item_display",
                    "none",
                    &[
                        "thirdperson_lefthand",
                        "thirdperson_righthand",
                        "firstperson_lefthand",
                        "firstperson_righthand",
                        "head",
                        "gui",
                        "ground",
                        "fixed",
                        "on_shelf",
                    ],
                );
                let item = tag
                    .get("item")
                    .and_then(item_from_nbt)
                    .filter(|stack| self.items().get(&stack.item).is_some());
                let Tag::Compound(fields) = tag else {
                    return;
                };
                fields.insert("item_display".into(), context);
                if let Some(stack) = item {
                    fields.insert("item".into(), item_to_nbt_with_items(&stack, self.items()));
                } else {
                    fields.remove(&"item".into());
                }
            }
            _ => {}
        }
    }
}

fn saved_enum(tag: &Tag, key: &str, default: &str, names: &[&str]) -> Tag {
    let value = text(tag.get(key)).filter(|value| names.contains(&value.as_str()));
    Tag::String(value.as_deref().unwrap_or(default).into())
}

/// `Display.TextDisplay.addAdditionalSaveData`: the saved text and its layout and style.
fn write_text_display(tag: &mut Tag) {
    let int = |key, default| tag.get(key).and_then(int_value).unwrap_or(default);
    let text = match tag.get("text") {
        Some(value @ Tag::String(_)) => value.clone(),
        Some(value @ Tag::Compound(fields)) if !fields.is_empty() => {
            component_tag(&tag_to_json(value))
        }
        Some(value @ Tag::List(parts)) if !parts.is_empty() => component_tag(&tag_to_json(value)),
        _ => Tag::String("".into()),
    };
    let values = [
        ("text", text),
        ("line_width", Tag::Int(int("line_width", 200))),
        ("text_opacity", Tag::Byte(int("text_opacity", -1) as i8)),
        ("background", Tag::Int(int("background", 0x4000_0000))),
        (
            "alignment",
            saved_enum(tag, "alignment", "center", &["left", "right"]),
        ),
        (
            "shadow",
            Tag::Byte(i8::from(entity_nbt::flag(tag, "shadow"))),
        ),
        (
            "see_through",
            Tag::Byte(i8::from(entity_nbt::flag(tag, "see_through"))),
        ),
        (
            "default_background",
            Tag::Byte(i8::from(entity_nbt::flag(tag, "default_background"))),
        ),
    ];
    if let Tag::Compound(fields) = tag {
        for (key, value) in values {
            fields.insert(key.into(), value);
        }
    }
}
