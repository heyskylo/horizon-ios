use horizon_client::{
    Result, item_data,
    items::{ItemStack, Items},
};
use horizon_items::item_nbt;
use horizon_world::loot::PackOverlay;
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

fn query(items: &Items, stack: &ItemStack, block: &str) -> String {
    format!(
        "{} {} {} {}",
        items.mining_speed(Some(stack), block).to_bits(),
        items.correct_for_drops(Some(stack), block),
        items
            .mining_damage(stack)
            .map_or_else(|| "none".into(), |n| n.to_string()),
        items.mining_damage_capacity(stack)
    )
}

pub fn main() -> Result<()> {
    let args: Vec<_> = crate::args().collect();
    let root = Path::new(&args[3]);
    let items = Items::catalog(root, &PackOverlay::NONE)?;
    let mut out = BufWriter::new(File::create(&args[2])?);
    for line in BufReader::new(File::open(&args[1])?).lines() {
        let row: serde_json::Value = serde_json::from_str(&line?)?;
        let input = row["item"].as_str().unwrap();
        let (id, components) = input
            .find('[')
            .map_or((input, None), |i| (&input[..i], Some(&input[i..])));
        let mut stack = ItemStack::new(id, 1);
        stack.count = row["count"].as_u64().unwrap_or(1) as u32;
        if let Some(components) = components {
            let tag = item_data::command_components(components)?;
            (stack.damage, stack.extra) = item_data::decode_components(Some(&tag));
        }
        let block = row["block"].as_str().unwrap();
        let restored = if stack.count == 0 || id == "minecraft:air" {
            stack.clone()
        } else {
            item_nbt::item_from_nbt(&item_nbt::item_to_nbt(&stack)).unwrap()
        };
        writeln!(
            out,
            "{} | {}",
            query(&items, &stack, block),
            query(&items, &restored, block)
        )?;
    }
    Ok(())
}
