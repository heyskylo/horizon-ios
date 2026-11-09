//! Easings and timeline tracks for tools/test_environment_timeline_parity.py.
use horizon_world::environment_attributes::{Entry, Value, attribute};
use horizon_world::environment_timeline::{Easing, Track};
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

fn ease(json: &str, x: f32) -> Result<String, Box<dyn Error>> {
    Ok(match Easing::parse(&serde_json::from_str(json)?) {
        Ok(easing) => bits(easing.apply(x)),
        Err(_) => "error".into(),
    })
}

fn track(
    id: &str,
    period: &str,
    json: &str,
    base: &str,
    ticks: &str,
) -> Result<String, Box<dyn Error>> {
    let attribute = attribute(id).ok_or("unknown attribute")?;
    let base = match Entry::parse(attribute, &serde_json::from_str(base)?)? {
        Entry::Override(value) => value,
        _ => return Err("the base is not a plain value".into()),
    };
    let period = if period == "-" {
        None
    } else {
        Some(period.parse()?)
    };
    let Ok(track) = Track::parse(attribute, &serde_json::from_str(json)?) else {
        return Ok("error".into());
    };
    if let Some(p) = period
        && track.validate_period(p).is_err()
    {
        return Ok("error".into());
    }
    let sampler = track.sampler(period);
    let values: Vec<String> = ticks
        .split(',')
        .map(|t| t.parse::<i64>().map(|t| format(&sampler.apply(&base, t))))
        .collect::<Result<_, _>>()?;
    Ok(values.join(";"))
}

pub fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = crate::args().collect();
    let mut output = String::new();
    for line in std::fs::read_to_string(&args[1])?.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let result = match f[0] {
            "ease" => ease(f[1], f[2].parse()?)?,
            "track" => track(f[1], f[2], f[3], f[4], f[5])?,
            _ => return Err("unknown case".into()),
        };
        writeln!(output, "{result}")?;
    }
    std::fs::write(&args[2], output)?;
    Ok(())
}
