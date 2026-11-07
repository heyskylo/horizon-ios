use std::collections::HashMap;

use horizon_perf_format::Field;
use horizon_perf_format::v1::{
    HealthSample, Kind, Side, SlowEvent, Stop, StopReason, Unit, Warning,
};
use serde_json::{Value, json};

use crate::error::side_name;
use crate::follow::Entry;

/// Spans listed on a slow-event line.
const SLOW_SPANS_SHOWN: usize = 3;

/// A line as text and as the JSON object `--json` prints instead.
type Line = (String, Value);

/// Turns the fields a follow reader yields into the lines `horizon-perf follow` prints.
pub struct Watcher {
    every_s: f64,
    json: bool,
    strings: HashMap<(i32, u32), String>,
    last_health: HashMap<i32, f64>,
}

impl Watcher {
    /// `every_s` is the least capture time between two health lines of one side.
    pub fn new(every_s: f64, json: bool) -> Self {
        Self {
            every_s,
            json,
            strings: HashMap::new(),
            last_health: HashMap::new(),
        }
    }

    pub fn feed(&mut self, entry: &Entry) -> Vec<String> {
        let Entry::Field(field) = entry else {
            return Vec::new();
        };
        let line = match field {
            Field::String(s) => {
                self.strings.insert((s.side, s.id), s.text.clone());
                None
            }
            Field::Start(s) => Some((
                format!(
                    "{:>6.1}s  {}  capture started ({})",
                    0.0,
                    side_name(s.side()),
                    kind_name(s.kind())
                ),
                json!({"event": "start", "side": side_name(s.side()), "kind": kind_name(s.kind())}),
            )),
            Field::Health(h) => self.health(h),
            Field::Slow(e) => Some(self.slow(e)),
            Field::Mark(m) => Some((
                format!("{:>6.1}s  {}  mark {}", m.t, side_name(m.side()), m.name),
                json!({"event": "mark", "side": side_name(m.side()), "t": m.t, "name": m.name}),
            )),
            Field::Warning(w) => Some(warning(w)),
            Field::Stop(s) => Some(stop(s)),
            _ => None,
        };
        line.map(|(text, value)| vec![if self.json { value.to_string() } else { text }])
            .unwrap_or_default()
    }

    fn health(&mut self, h: &HealthSample) -> Option<Line> {
        let side = h.side;
        if let Some(&last) = self.last_health.get(&side)
            && h.t < last + self.every_s - 1e-9
        {
            return None;
        }
        self.last_health.insert(side, h.t);
        let name = side_name(h.side());
        let mut parts = Vec::new();
        let mut value = json!({"event": "health", "side": name, "t": h.t, "rss_mb": h.rss_mb});
        for (key, label, v) in [
            ("tps", "TPS", h.tps),
            ("mspt_avg", "MSPT avg", h.mspt_avg),
            ("mspt_p95", "p95", h.mspt_p95),
            ("mspt_max", "max", h.mspt_max),
            ("fps", "FPS", h.fps),
            ("frame_ms_p95", "frame p95", h.frame_ms_p95),
        ] {
            if let Some(v) = v {
                value[key] = json!(v);
                parts.push(format!("{label} {v:.1}"));
            }
        }
        parts.push(format!("RSS {:.0} MB", h.rss_mb));
        if h.side() == Side::Server {
            value["players"] = json!(h.players);
            parts.push(format!("{} players", h.players));
        }
        Some((
            format!("{:>6.1}s  {name}  {}", h.t, parts.join(" · ")),
            value,
        ))
    }

    fn slow(&self, e: &SlowEvent) -> Line {
        let name = side_name(e.side());
        let unit = match e.unit() {
            Unit::Frame => "frame",
            _ => "tick",
        };
        let spans: Vec<(String, f32)> = e
            .top
            .iter()
            .take(SLOW_SPANS_SHOWN)
            .map(|s| {
                let label = self
                    .strings
                    .get(&(e.side, s.name))
                    .cloned()
                    .unwrap_or_else(|| format!("#{}", s.name));
                (label, s.wall_ms)
            })
            .collect();
        let listed = spans
            .iter()
            .map(|(label, ms)| format!("{label} {ms:.1}"))
            .collect::<Vec<_>>()
            .join(", ");
        let json = json!({
            "event": "slow", "side": name, "t": e.t, "unit": unit, "counter": e.counter,
            "wall_ms": e.wall_ms,
            "top": spans.iter().map(|(label, ms)| json!({"name": label, "wall_ms": ms})).collect::<Vec<_>>(),
        });
        (
            format!(
                "{:>6.1}s  {name}  slow {unit} {}: {:.1} ms ({listed})",
                e.t, e.counter, e.wall_ms
            ),
            json,
        )
    }
}

fn warning(w: &Warning) -> Line {
    let name = side_name(w.side());
    let code = w.code().as_str_name();
    (
        format!("{:>6.1}s  {name}  warning {code} {:?}", w.t, w.args),
        json!({"event": "warning", "side": name, "t": w.t, "code": code, "args": w.args}),
    )
}

fn stop(s: &Stop) -> Line {
    let name = side_name(s.side());
    let reason = match s.reason() {
        StopReason::Normal => "normal",
        StopReason::WindowCap => "window cap",
        StopReason::Disconnected => "disconnected",
        StopReason::Shutdown => "shutdown",
        StopReason::Unspecified => "unknown",
    };
    (
        format!("{:>6.1}s  {name}  stopped ({reason})", s.t),
        json!({"event": "stop", "side": name, "t": s.t, "reason": reason}),
    )
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Dedicated => "dedicated",
        Kind::Integrated => "integrated",
        Kind::Client => "client",
        Kind::Unspecified => "unknown",
    }
}
