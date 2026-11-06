//! Command-tag data carried to a converted mob's new entity ID.
use horizon_client::entity_nbt;
use horizon_world::nbt::Tag;
use std::io::Write;

pub fn main() {
    let args: Vec<_> = super::args().collect();
    let input = std::fs::read_to_string(&args[1]).unwrap();
    let mut out = std::fs::File::create(&args[2]).unwrap();
    for line in input.lines() {
        let values: Vec<String> = serde_json::from_str(line).unwrap();
        let raw = Tag::compound([
            (
                "Tags",
                Tag::List(
                    values
                        .iter()
                        .map(|s| Tag::String(s.as_str().into()))
                        .collect(),
                ),
            ),
            ("UUID", Tag::IntArray(vec![1, 2, 3, 4])),
            ("id", Tag::String("minecraft:husk".into())),
        ]);
        let converted = entity_nbt::conversion_tags(Some(&raw))
            .unwrap_or_else(|| Tag::compound([("Tags", Tag::List(Vec::new()))]));
        assert!(converted.get("UUID").is_none() && converted.get("id").is_none());
        let Tag::List(tags) = converted.get("Tags").unwrap() else {
            panic!("missing tags")
        };
        let mut encoded: Vec<String> = tags
            .iter()
            .map(|tag| {
                let Tag::String(tag) = tag else {
                    panic!("not a string")
                };
                tag.0.iter().map(|unit| format!("{unit:04x}")).collect()
            })
            .collect();
        encoded.sort();
        writeln!(out, "{}", serde_json::to_string(&encoded).unwrap()).unwrap();
    }
}
