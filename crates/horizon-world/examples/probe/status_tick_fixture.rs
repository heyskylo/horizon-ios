use horizon_world::status_effect::{BlendState, Details, EffectInstance};
use horizon_world::status_effect::{BlendWorld, TickWorld};
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
struct World<'a> {
    f: Value,
    out: &'a mut dyn Write,
    slots: Vec<Effect>,
    ids: Vec<Effect>,
    calls: i32,
    changed: bool,
}
impl World<'_> {
    fn config(&self) -> &Value {
        &self.f[if self.changed { "after" } else { "initial" }]
    }
    fn reset(&mut self) {
        let c = self.config().clone();
        for (i, v) in c["instances"].as_array().unwrap().iter().enumerate() {
            let mut e = self.slots[i].write();
            e.details = details(v);
            e.blend = BlendState {
                factor: fl(v, "factor"),
                previous: fl(v, "previous"),
            };
        }
    }
    fn all(&mut self) {
        writeln!(
            self.out,
            "state {} {} {}",
            self.calls,
            self.changed,
            n(self.config(), "tick")
        )
        .unwrap();
        dump(self.out, &self.slots, &mut self.ids);
    }
    fn event(&mut self, name: String) -> Result<(), &'static str> {
        writeln!(self.out, "event {name}").unwrap();
        let i = self.calls;
        self.calls += 1;
        self.all();
        if i == n(&self.f, "fail") {
            return Err("callback");
        }
        if i == n(&self.f, "mutate") {
            self.changed = true;
            self.reset();
        }
        Ok(())
    }
}
impl TickWorld<u8> for World<'_> {
    type Error = &'static str;
    type Level = i32;
    fn actor_tick_count(&self) -> i32 {
        n(self.config(), "tick")
    }
    fn should_apply(
        &mut self,
        e: u8,
        time: i32,
        amp: i32,
        _: &Effect,
    ) -> Result<bool, Self::Error> {
        self.event(format!("should {e} {time} {amp}"))?;
        Ok(self.config()["should"].as_bool().unwrap())
    }
    fn apply_tick(&mut self, e: u8, l: i32, amp: i32, _: &Effect) -> Result<bool, Self::Error> {
        self.event(format!("apply {e} {l} {amp}"))?;
        Ok(self.config()["apply"].as_bool().unwrap())
    }
    fn downgraded(&mut self, _: &Effect) -> Result<(), Self::Error> {
        self.event("downgrade".into())
    }
}
impl BlendWorld<u8> for World<'_> {
    type Error = &'static str;
    fn advance_ticks(&mut self, e: u8, _: &Effect) -> Result<i32, Self::Error> {
        self.event(format!("advance {e}"))?;
        Ok(n(self.config(), "advance"))
    }
    fn duration_ticks(&mut self, e: u8, blending_in: bool, _: &Effect) -> Result<i32, Self::Error> {
        self.event(format!("blend-duration {e} {blending_in}"))?;
        Ok(n(
            self.config(),
            if blending_in { "blend_in" } else { "blend_out" },
        ))
    }
}
fn run(f: Value, out: &mut dyn Write) {
    let mut slots: Vec<Effect> = Vec::new();
    for v in f["initial"]["instances"].as_array().unwrap() {
        let h = n(v, "hidden");
        let e = Effect::new(
            n(v, "effect") as u8,
            details(v),
            (h >= 0).then(|| slots[h as usize].clone()),
        );
        slots.push(e);
    }
    let mut w = World {
        f,
        out,
        slots,
        ids: Vec::new(),
        calls: 0,
        changed: false,
    };
    w.reset();
    w.all();
    let ops = w.f["ops"].as_array().unwrap().clone();
    for op in ops {
        let a = n(&op, "a") as usize;
        let b = n(&op, "b") as usize;
        let e = w.slots[a].clone();
        let result: Result<String, &'static str> = match op["op"].as_str().unwrap() {
            "server" => e
                .tick_server(&mut w, n(&op, "level"))
                .map(|v| v.to_string()),
            "client" => e.tick_client(&mut w).map(|_| "ok".into()),
            "skip" => e.skip_blending(&mut w).map(|_| "ok".into()),
            "factor" => Ok(bits(
                e.blend_factor(fl(&op, "partial"), op["removed"].as_bool().unwrap()),
            )
            .to_string()),
            "update" => Ok(e.update(&w.slots[b]).to_string()),
            "alias" => {
                w.slots[b] = e;
                Ok("ok".into())
            }
            "copy" => {
                w.slots[b] = e.copy_instance();
                Ok("ok".into())
            }
            _ => panic!("unknown operation"),
        };
        writeln!(w.out, "result {}", result.unwrap_or_else(str::to_string)).unwrap();
        w.all();
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
