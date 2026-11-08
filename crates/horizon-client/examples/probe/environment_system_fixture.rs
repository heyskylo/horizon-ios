//! Environment attribute layer stacks for tools/test_environment_system_parity.py: per case, a
//! dimension's attributes, the biome layer and the weather layers, sampled at a position whose
//! biome mix comes from the Gaussian sampler over a test grid of biomes.
use horizon_world::environment_attributes::{ATTRIBUTES, AttributeMap, Kind, Value};
use horizon_world::environment_system::{BiomeBlend, Builder, Conditions, gaussian_sample};
use serde_json::Value as Json;
use std::{collections::HashMap, error::Error, fmt::Write};

fn bits(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn format(value: &Value) -> String {
    match value {
        Value::Bool(b) => b.to_string(),
        Value::Float(f) => bits(*f),
        Value::Rgb(v) => v.map(bits).join(" "),
        Value::Argb(v) => v.map(bits).join(" "),
        Value::Other(json) => json.to_string(),
    }
}

/// The oracle's test grid: a cell's biome is picked by a hash of its coordinates.
fn cell(grid: &[String], x: i32, y: i32, z: i32) -> &str {
    let hash = (x
        .wrapping_mul(31)
        .wrapping_add(y)
        .wrapping_mul(31)
        .wrapping_add(z))
        & i32::MAX;
    &grid[hash as usize % grid.len()]
}

fn run(
    maps: &HashMap<String, AttributeMap>,
    case: &Json,
    output: &mut String,
) -> Result<(), Box<dyn Error>> {
    let strings = |key: &str| -> Vec<String> {
        case[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let grid = strings("grid");
    let position: Vec<f64> = case["position"]
        .as_array()
        .ok_or("position")?
        .iter()
        .filter_map(Json::as_f64)
        .collect();
    let map = |id: &str| maps.get(id).ok_or_else(|| format!("no map {id}"));

    let mut builder = Builder::default();
    builder.constant_layer(map(case["dimension"].as_str().ok_or("dimension")?)?);
    let biomes = strings("biomes")
        .iter()
        .map(|id| map(id))
        .collect::<Result<Vec<_>, _>>()?;
    builder.biome_layer(biomes);
    if case["weather"].as_bool() == Some(true) {
        builder.weather_layers();
    }
    let system = builder.build();

    let quart = |c: f64| (c / 4.0).floor() as i32;
    let here = map(cell(
        &grid,
        quart(position[0]),
        quart(position[1]),
        quart(position[2]),
    ))?;
    let mut blend = BiomeBlend::default();
    let blended = case["blend"].as_bool() == Some(true);
    if blended {
        let scaled = [position[0] * 0.25, position[1] * 0.25, position[2] * 0.25];
        gaussian_sample(
            scaled,
            |x, y, z| &maps[cell(&grid, x, y, z)],
            |w, m| blend.accumulate(w, m),
        );
    }
    let biome = |_full: bool| Some(here);
    let conditions = Conditions {
        rain: case["rain"].as_f64().ok_or("rain")? as f32,
        thunder: case["thunder"].as_f64().ok_or("thunder")? as f32,
        blend: blended.then_some(&blend),
        biome: &biome,
    };
    for attribute in ATTRIBUTES.iter().filter(|a| a.kind != Kind::Other) {
        let value = system.value(attribute, &conditions).ok_or("no value")?;
        let dimension = system
            .dimension_value(attribute, &conditions)
            .ok_or("no value")?;
        writeln!(
            output,
            "minecraft:{}\t{}\t{}",
            attribute.id,
            format(&value),
            format(&dimension)
        )?;
    }
    Ok(())
}

pub fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = crate::args().collect();
    let input = std::fs::read_to_string(&args[1])?;
    let mut lines = input.lines();
    let data: Json = serde_json::from_str(lines.next().ok_or("no data")?)?;
    let mut maps = HashMap::new();
    for (id, json) in data.as_object().ok_or("data")? {
        maps.insert(
            id.clone(),
            AttributeMap::parse(json).map_err(|e| format!("{id}: {e}"))?,
        );
    }
    let mut output = String::new();
    for line in lines {
        run(&maps, &serde_json::from_str(line)?, &mut output)?;
    }
    std::fs::write(&args[2], output)?;
    Ok(())
}
