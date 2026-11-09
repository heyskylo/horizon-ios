use horizon_world::status_effect::{BlendState, Details, EffectInstance};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
type Effect = EffectInstance<u8>;
fn n(v: &Value, k: &str) -> i32 {
    v[k].as_i64().unwrap() as i32
}
fn fl(v: &Value, k: &str) -> f32 {
    f32::from_bits(v[k].as_u64().unwrap() as u32)
}
fn bits(v: f32) -> u32 {
    if v.is_nan() { 0x7fc00000 } else { v.to_bits() }
}
fn details(v: &Value) -> Details {
    Details {
        duration: n(v, "duration"),
        amplifier: n(v, "amplifier"),
        ambient: v["ambient"].as_bool().unwrap(),
        visible: v["visible"].as_bool().unwrap(),
        show_icon: v["icon"].as_bool().unwrap(),
    }
}
fn id(ids: &mut Vec<Effect>, e: &Effect) -> usize {
    if let Some(i) = ids.iter().position(|v| v.same_reference(e)) {
        i
    } else {
        ids.push(e.clone());
        ids.len() - 1
    }
}
fn dump(out: &mut dyn Write, slots: &[Effect], ids: &mut Vec<Effect>) {
    let mut visit = Vec::<Effect>::new();
    for (i, e) in slots.iter().enumerate() {
        writeln!(out, "slot {i} {}", id(ids, e)).unwrap();
        if !visit.iter().any(|v| v.same_reference(e)) {
            visit.push(e.clone());
        }
    }
    let mut i = 0;
    while i < visit.len() {
        let e = visit[i].clone();
        let d = e.read();
        let h = d
            .hidden
            .as_ref()
            .map(|v| id(ids, v).to_string())
            .unwrap_or("none".into());
        if let Some(v) = &d.hidden
            && !visit.iter().any(|e| e.same_reference(v))
        {
            visit.push(v.clone());
        }
        writeln!(
            out,
            "node {} {} {} {} {} {} {} {h} {} {}",
            id(ids, &e),
            d.effect,
            d.details.duration,
            d.details.amplifier,
            d.details.ambient,
            d.details.visible,
            d.details.show_icon,
            bits(d.blend.factor),
            bits(d.blend.previous)
        )
        .unwrap();
        i += 1;
    }
}
fn run(f: Value, out: &mut dyn Write) {
    let mut slots: Vec<Effect> = Vec::new();
    for v in f["instances"].as_array().unwrap() {
        let h = n(v, "hidden");
        let e = Effect::new(
            n(v, "effect") as u8,
            details(v),
            (h >= 0).then(|| slots[h as usize].clone()),
        );
        slots.push(e);
    }
    let mut ids = Vec::new();
    dump(out, &slots, &mut ids);
    for op in f["ops"].as_array().unwrap() {
        let a = n(op, "a") as usize;
        let b = n(op, "b") as usize;
        let result = match op["op"].as_str().unwrap() {
            "alias" => {
                slots[b] = slots[a].clone();
                "ok".into()
            }
            "copy" => {
                slots[b] = slots[a].copy_instance();
                "ok".into()
            }
            "scale" => {
                slots[b] = slots[a].scaled(fl(op, "factor"));
                "ok".into()
            }
            "update" => slots[a].update(&slots[b]).to_string(),
            "eq" => (slots[a] == slots[b]).to_string(),
            "same" => slots[a].same_reference(&slots[b]).to_string(),
            "ends" => slots[a].ends_within(n(op, "value")).to_string(),
            "infinite" => slots[a].infinite().to_string(),
            "copy-blend" => {
                slots[a].copy_blend_from(&slots[b]);
                "ok".into()
            }
            "set" => {
                let mut e = slots[a].write();
                e.details = details(op);
                e.blend = BlendState {
                    factor: fl(op, "factor"),
                    previous: fl(op, "previous"),
                };
                "ok".into()
            }
            "map" => slots[a]
                .map_duration(|value| {
                    writeln!(out, "map {value}").unwrap();
                    if op["mutate"].as_bool().unwrap() {
                        slots[b].write().details.duration = n(op, "value");
                    }
                    if op["fail"].as_bool().unwrap() {
                        Err("callback")
                    } else {
                        Ok(value.wrapping_mul(n(op, "value")))
                    }
                })
                .map(|v| v.to_string())
                .unwrap_or_else(str::to_string),
            _ => panic!("unknown operation"),
        };
        writeln!(out, "result {result}").unwrap();
        dump(out, &slots, &mut ids);
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).unwrap();
    let dir = Path::new(&arg);
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    for line in fs::read_to_string(dir.join("inputs.txt"))?.lines() {
        run(serde_json::from_str(line)?, &mut out);
    }
    Ok(())
}
