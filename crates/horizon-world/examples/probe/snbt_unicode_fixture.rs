//! Compact SNBT over valid Unicode keys and recursive integer compounds.
use horizon_world::{nbt::Tag, snbt_text::to_snbt};
use std::io::Write;

fn tag(value: &serde_json::Value) -> Tag {
    if let Some(number) = value.as_i64() {
        Tag::Int(number.try_into().unwrap())
    } else {
        Tag::compound(
            value["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| (entry[0].as_str().unwrap(), tag(&entry[1]))),
        )
    }
}

pub fn main() {
    let args: Vec<_> = super::args().collect();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    for line in std::fs::read_to_string(&args[1]).unwrap().lines() {
        let input: serde_json::Value = serde_json::from_str(line).unwrap();
        for unit in to_snbt(&tag(&input)).encode_utf16() {
            write!(out, "{unit:04x}").unwrap();
        }
        writeln!(out).unwrap();
    }
}
