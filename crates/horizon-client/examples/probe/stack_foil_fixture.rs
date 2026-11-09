//! Actual stack decoding -> render view -> item-model foil selection, without a GPU.
use horizon_assets::item_model::{
    ClientItem, Context, DisplayContext, FoilType, ItemModels, LayerContent, NoEnv,
};
use horizon_client::{item_data::decode_components, item_render::stack_view, items::ItemStack};
use horizon_world::nbt::Tag;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};

fn model_flags(input: &Value) -> horizon_client::Result<Value> {
    let text = input["nbt"].as_str().ok_or("missing NBT text")?;
    let (tag, used) = horizon_world::nbt::parse_snbt(text)?;
    if used != text.len() {
        return Err("trailing NBT".into());
    }
    let mut stack = ItemStack::new("minecraft:diamond", 1);
    (stack.damage, stack.extra) =
        decode_components(Some(&Tag::compound([("minecraft:custom_model_data", tag)])));
    let view = stack_view(&stack);
    let mut models = ItemModels::default();
    models.prototypes.insert(
        "minecraft:diamond".into(),
        BTreeMap::from([("minecraft:item_model".into(), json!("minecraft:diamond"))]),
    );
    models.definitions.insert("minecraft:diamond".into(), ClientItem::decode(&json!({
        "model": {"type": "minecraft:condition", "property": "minecraft:custom_model_data", "index": input["index"],
            "on_true": {"type": "minecraft:model", "model": "horizon:item/yes"},
            "on_false": {"type": "minecraft:model", "model": "horizon:item/no"}}
    }))?);
    let resolved = models.resolve(
        &view,
        &Context {
            display: DisplayContext::Gui,
            owner: None,
            level: None,
            seed: 0,
        },
        &NoEnv,
    );
    if resolved.layers.len() != 1 {
        return Err("expected one conditional model layer".into());
    }
    let LayerContent::Model { model } = &resolved.layers[0].content else {
        return Err("expected model content".into());
    };
    Ok(
        json!({"id": input["id"], "component": view.patch["minecraft:custom_model_data"], "selected": model}),
    )
}

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<String> = crate::args().collect();
    let mut models = ItemModels::default();
    models.definitions.insert(
        "horizon:fixture".into(),
        ClientItem::from_json(
            r#"{"model":{"type":"minecraft:model","model":"horizon:item/fixture"}}"#,
        )?,
    );
    let mut output = String::new();
    for line in fs::read_to_string(args.get(1).ok_or("missing INPUT")?)?.lines() {
        let input: Value = serde_json::from_str(line)?;
        if input["kind"] == "flags" {
            output.push_str(&serde_json::to_string(&model_flags(&input)?)?);
            output.push('\n');
            continue;
        }
        let item = input["item"].as_str().ok_or("missing item")?;
        models.prototypes.insert(
            item.into(),
            BTreeMap::from([
                ("minecraft:item_model".into(), json!("horizon:fixture")),
                (
                    "minecraft:enchantment_glint_override".into(),
                    input["default_glint"].clone(),
                ),
            ]),
        );
        let mut stack = ItemStack::new(item, 1);
        if let Some(nbt) = input.get("nbt") {
            let text = nbt.as_str().ok_or("missing NBT text")?;
            let (tag, used) = horizon_world::nbt::parse_snbt(text)?;
            if used != text.len() {
                return Err("trailing NBT".into());
            }
            (stack.damage, stack.extra) = decode_components(Some(&Tag::compound([(
                "minecraft:enchantment_glint_override",
                tag,
            )])));
        }
        let view = stack_view(&stack);
        let resolved = models.resolve(
            &view,
            &Context {
                display: DisplayContext::Gui,
                owner: None,
                level: None,
                seed: 0,
            },
            &NoEnv,
        );
        if resolved.layers.len() != 1 {
            return Err("expected one fixture model layer".into());
        }
        output.push_str(&serde_json::to_string(&json!({
            "id": input["id"],
            "component": view.patch.get("minecraft:enchantment_glint_override").and_then(Option::as_ref),
            "foil": resolved.layers[0].foil != FoilType::None,
        }))?);
        output.push('\n');
    }
    fs::write(args.get(2).ok_or("missing OUTPUT")?, output)?;
    Ok(())
}
