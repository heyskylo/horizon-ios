#![forbid(unsafe_code)]
//! Runs tools/test_entity_components_parity.py's entities and advancement
//! entity predicates through `horizon_client::advancements::entity`, with the
//! variant component `mobs::finalize::species_variant_component` reports.
use horizon_client::{
    Result,
    advancements::{EntityView, World, entity},
    mobs::{
        Species,
        finalize::{species_variant_component, variant_index},
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

/// Nothing but the entity: these predicates read only its type and components.
struct Empty;
impl World for Empty {
    fn dimension(&self) -> &str {
        "minecraft:overworld"
    }
    fn weather(&self) -> (bool, bool) {
        (false, false)
    }
    fn biome(&self, _: [i32; 3]) -> String {
        "minecraft:plains".into()
    }
    fn structures(&self, _: [i32; 3]) -> Vec<String> {
        Vec::new()
    }
    fn block(&self, _: [i32; 3]) -> (String, BTreeMap<String, String>) {
        ("minecraft:air".into(), BTreeMap::new())
    }
    fn fluid(&self, _: [i32; 3]) -> Option<String> {
        None
    }
    fn can_see_sky(&self, _: [i32; 3]) -> bool {
        true
    }
    fn light(&self, _: [i32; 3]) -> i32 {
        15
    }
    fn smokey(&self, _: [i32; 3]) -> bool {
        false
    }
    fn in_tag(&self, _: &str, _: &str, _: &str) -> bool {
        false
    }
}

fn species(id: &str) -> Species {
    match id {
        "minecraft:wolf" => Species::Wolf,
        "minecraft:cat" => Species::Cat,
        "minecraft:frog" => Species::Frog,
        "minecraft:pig" => Species::Pig,
        "minecraft:cow" => Species::Cow,
        "minecraft:chicken" => Species::Chicken,
        other => panic!("no species for {other}"),
    }
}

pub fn main() -> Result<()> {
    let args: Vec<String> = crate::args().collect();
    let input = BufReader::new(File::open(&args[1])?);
    let mut out = BufWriter::new(File::create(&args[2])?);
    for (n, line) in input.lines().enumerate() {
        let c: Value = serde_json::from_str(&line?)?;
        writeln!(out, "== {n}")?;
        let kind = c["type"].as_str().unwrap();
        let s = species(kind);
        let variant = variant_index(s, c["variant"].as_str().unwrap());
        let view = EntityView {
            kind: kind.to_owned(),
            position: [0.5, 64.0, 0.5],
            components: species_variant_component(s, variant)
                .map(|(id, v)| (id.to_owned(), v))
                .into_iter()
                .collect(),
            ..Default::default()
        };
        for p in c["predicates"].as_array().unwrap() {
            writeln!(out, "{}", entity(p, &view, &Empty, [2.5, 64.0, 0.5]))?;
        }
    }
    out.flush()?;
    Ok(())
}
