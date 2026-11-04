//! DamageType resource-codec probe; Java's actual DIRECT_CODEC is the oracle.
use horizon_world::{damage_source::Definition, nbt::Tag};
use serde_json::json;
use std::io::{self, BufRead};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        let result = match Definition::from_json("test:damage", line.as_bytes()) {
            Ok(d) => {
                let nbt = d.to_nbt();
                let bits = match nbt.get("exhaustion").unwrap() {
                    Tag::Float(bits) => *bits,
                    _ => panic!("codec must write a float"),
                };
                json!([
                    d.message_id,
                    d.scaling.name(),
                    format!("{:08x}", d.exhaustion.to_bits()),
                    d.effects,
                    d.death_message_type,
                    format!("{:08x}", bits),
                    nbt.get("effects").is_some(),
                    nbt.get("death_message_type").is_some()
                ])
            }
            Err(_) => json!(false),
        };
        println!("{result}");
    }
    Ok(())
}
