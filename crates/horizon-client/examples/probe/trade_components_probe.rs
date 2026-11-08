//! Headless trade costs/modifiers: no game, window or server is started.
use horizon_client::{
    enchant::Enchantments,
    items::{ItemStack, Items},
    merchant::{TradeContext, Trades},
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::loot::PackOverlay;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<_> = crate::args().collect();
    let root = Path::new(&args[1]);
    let items = Items::catalog(root, &PackOverlay::NONE)?;
    let enchantments = Enchantments::load(root);
    let trades = Trades {
        trades: HashMap::new(),
        tags: HashMap::new(),
        sets: HashMap::new(),
        potion_tags: HashMap::new(),
    };
    let ctx = TradeContext {
        items: &items,
        enchantments: &enchantments,
        variant: "minecraft:plains",
        origin: [0.0; 3],
        maps: None,
    };
    let mut output = String::new();
    for line in fs::read_to_string(&args[2])?.lines() {
        let v: Value = serde_json::from_str(line)?;
        let mut random = LegacyRandom::new(v["seed"].as_i64().unwrap());
        let o = trades.offer_from(&ctx, &v["trade"], &mut random);
        let result = if let Some(o) = o {
            let mut levels = o.result.crafting_enchantments().to_vec();
            levels.sort_by(|a, b| a.0.cmp(&b.0));
            let payments: Vec<_> = ["plain", "renamed", "wrong", "extra_potion"]
                .iter()
                .map(|kind| {
                    let mut p =
                        ItemStack::new("minecraft:potion", 2).with_potion(if *kind == "wrong" {
                            "minecraft:awkward"
                        } else {
                            "minecraft:water"
                        });
                    if *kind == "renamed" {
                        p.data_mut()
                            .set_loot_component("minecraft:custom_name", &json!("Bottle"));
                    }
                    if *kind == "extra_potion" {
                        p.data_mut()
                            .potion_extra
                            .push(("custom_color".into(), horizon_world::nbt::Tag::Int(123)));
                    }
                    let mut a = Some(p);
                    let accepted = o.take(&items, &mut a, &mut None);
                    json!([accepted, a.map_or(0, |s| s.count)])
                })
                .collect();
            json!({"item": &*o.result.item, "count": o.result.count, "levels": levels.iter().map(|(id,l)| json!([&**id,l])).collect::<Vec<_>>(), "a": o.base_cost_a.stack.count, "b": o.cost_b.as_ref().map(|b| b.stack.count), "payments": payments})
        } else {
            Value::Null
        };
        output.push_str(&json!({"result": result, "next":random.next_long()}).to_string());
        output.push('\n');
    }
    fs::write(&args[3], output)?;
    Ok(())
}
