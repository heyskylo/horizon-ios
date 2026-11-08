//! Environment attribute entries for tools/test_environment_attribute_modifier_parity.py: each
//! case parses an entry for an attribute and applies it to a subject value.
use horizon_world::environment_attributes::{ATTRIBUTES, Attribute, Entry, Kind, Value, attribute};
use std::{error::Error, fmt::Write};

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

fn run(attribute: &Attribute, subject: &str, entry: &str) -> Result<String, Box<dyn Error>> {
    let subject = match Entry::parse(attribute, &serde_json::from_str(subject)?)? {
        Entry::Override(value) => value,
        _ => return Err("the subject is not a plain value".into()),
    };
    let Ok(entry) = Entry::parse(attribute, &serde_json::from_str(entry)?) else {
        return Ok("error".into());
    };
    let value = entry.apply(&subject);
    Ok(format!(
        "{}\t{}",
        format(&value),
        format(&attribute.sanitize(value.clone()))
    ))
}

/// Every attribute in registration order: id, type, default, and how a float range clamps.
fn registry(output: &mut String) -> std::fmt::Result {
    for a in &ATTRIBUTES {
        let kind = match a.kind {
            Kind::Boolean => "minecraft:boolean",
            Kind::Float => "minecraft:float",
            Kind::AngleDegrees => "minecraft:angle_degrees",
            Kind::RgbColor => "minecraft:rgb_color",
            Kind::ArgbColor => "minecraft:argb_color",
            Kind::Other => "other",
        };
        let default = a.default_value();
        let range = match default {
            Some(Value::Float(_)) => format!(
                "{} {}",
                format(&a.sanitize(Value::Float(f32::NEG_INFINITY))),
                format(&a.sanitize(Value::Float(f32::INFINITY)))
            ),
            _ => "-".into(),
        };
        let default = default.map_or("unported".into(), |v| format(&v));
        writeln!(output, "minecraft:{}\t{kind}\t{default}\t{range}", a.id)?;
    }
    Ok(())
}

pub fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = crate::args().collect();
    let mut output = String::new();
    for line in std::fs::read_to_string(&args[1])?.lines() {
        if line == "registry" {
            registry(&mut output)?;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let attribute = attribute(fields[0]).ok_or("unknown attribute")?;
        writeln!(output, "{}", run(attribute, fields[1], fields[2])?)?;
    }
    std::fs::write(&args[2], output)?;
    Ok(())
}
