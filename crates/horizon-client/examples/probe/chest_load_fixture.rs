//! Chest inventory loading without unpacking loot, for the vanilla differential harness.
use horizon_client::save;
use std::{fs, io::Write};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = super::args().collect();
    let input = fs::read_to_string(&args[1])?;
    let mut out = fs::File::create(&args[2])?;
    for line in input.lines() {
        let tag = horizon_world::nbt::parse_snbt(line)?.0;
        let (_, chest) = save::decode_block_entity(&tag).ok_or("not a chest")?;
        let mut result = String::new();
        for (i, stack) in chest.slots().iter().enumerate() {
            if let Some(stack) = stack {
                use std::fmt::Write;
                write!(result, "{i}:{}:{};", stack.item, stack.count)?;
            }
        }
        writeln!(out, "{}", if result.is_empty() { "empty" } else { &result })?;
    }
    Ok(())
}
