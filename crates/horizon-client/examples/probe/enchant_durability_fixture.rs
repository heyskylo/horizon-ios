//! Rust side of tools/test_enchant_durability_parity.py: `Enchantments` value
//! effects for the `minecraft:item_damage` (Unbreaking) and
//! `minecraft:repair_with_xp` (Mending) components on one stack per case.
use horizon_client::{
    enchant::{EffectContext, Enchantments},
    item_data::StackData,
    items::{ItemStack, Items},
};
use horizon_core::random::LegacyRandom;
use horizon_world::loot::PackOverlay;
use serde_json::Value;
use std::{fs, io::Write, path::Path};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = crate::args().collect();
    let input = fs::read_to_string(args.get(1).ok_or("missing INPUT")?)?;
    let root = input
        .lines()
        .find_map(|line| line.strip_prefix("#resources "))
        .map(Path::new)
        .ok_or("no #resources line")?;
    let enchantments = Enchantments::load(root);
    let items = Items::catalog(root, &PackOverlay::NONE)?;
    let out = fs::File::create(args.get(2).ok_or("missing OUTPUT")?)?;
    let mut out = std::io::BufWriter::new(out);
    for line in input.lines() {
        if line.starts_with("#resources ") || line.trim().is_empty() {
            continue;
        }
        let case: Value = serde_json::from_str(line)?;
        let item = case["item"].as_str().unwrap();
        let level = case["level"].as_u64().unwrap() as u32;
        let component = if case["mode"] == "unbreaking" {
            "minecraft:item_damage"
        } else {
            "minecraft:repair_with_xp"
        };
        let stack = ItemStack {
            item: item.into(),
            count: 1,
            damage: case["damage"].as_u64().unwrap() as u32,
            extra: Some(Box::new(StackData {
                enchantments: vec![(
                    component_enchantment(case["mode"].as_str().unwrap()).into(),
                    level,
                )],
                ..Default::default()
            })),
        };
        let ctx = EffectContext {
            tool: Some(&stack.item),
            items: Some(&items),
            ..Default::default()
        };
        let mut random = LegacyRandom::new(case["seed"].as_i64().unwrap());
        let value = enchantments.modify_value(
            Some(&stack),
            component,
            &ctx,
            &mut random,
            case["amount"].as_i64().unwrap() as f32,
        );
        let value = if case["mode"] == "mending" {
            value.max(0.0)
        } else {
            value
        };
        writeln!(out, "{}", value as i64)?;
    }
    Ok(())
}

fn component_enchantment(mode: &str) -> &'static str {
    if mode == "unbreaking" {
        "minecraft:unbreaking"
    } else {
        "minecraft:mending"
    }
}
