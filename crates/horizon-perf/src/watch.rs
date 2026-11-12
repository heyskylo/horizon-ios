use std::collections::HashMap;

use horizon_perf_format::Field;
use horizon_perf_format::v1::{HealthSample, Side, SlowEvent, Stop, StopReason, Warning};
use serde::{Deserialize, Serialize};

use crate::error::side_name;
use crate::follow::Entry;
use crate::wire::{KindName, SideName, StopReasonName, UnitKind, WarningCodeName};

/// Spans listed on a slow-event line.
const SLOW_SPANS_SHOWN: usize = 3;

/// One line of `follow --json`: what happened, tagged by `event`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Start {
        side: SideName,
        kind: KindName,
    },
    Health(HealthNote),
    Slow(SlowNote),
    Mark {
        side: SideName,
        t: f64,
        name: String,
    },
    Warning {
        side: SideName,
        t: f64,
        code: WarningCodeName,
        args: Vec<f64>,
    },
    Stop {
        side: SideName,
        t: f64,
        reason: StopReasonName,
    },
}

/// A health sample; only the fields it holds are present, and `players` only for a server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthNote {
    pub side: SideName,
    pub t: f64,
    pub rss_mb: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mspt_avg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mspt_p95: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mspt_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_ms_p95: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlowNote {
    pub side: SideName,
    pub t: f64,
    pub unit: UnitKind,
    pub counter: u64,
    pub wall_ms: f64,
    pub top: Vec<SlowSpan>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlowSpan {
    pub name: String,
    pub wall_ms: f64,
}

/// A line as text and as the event `--json` prints instead.
type Line = (String, Event);

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
        let json = self.json;
        self.line(entry)
            .map(|(text, event)| {
                if json {
                    serde_json::to_string(&event).expect("an event always serializes")
                } else {
                    text
                }
            })
            .into_iter()
            .collect()
    }

    /// The event this entry reports, if it reports one.
    pub fn event(&mut self, entry: &Entry) -> Option<Event> {
        self.line(entry).map(|(_, event)| event)
    }

    fn line(&mut self, entry: &Entry) -> Option<Line> {
        let Entry::Field(field) = entry else {
            return None;
        };
        match field {
            Field::String(s) => {
                self.strings.insert((s.side, s.id), s.text.clone());
                None
            }
            Field::Start(s) => Some((
                format!(
                    "{:>6.1}s  {}  capture started ({})",
                    0.0,
                    side_name(s.side()),
                    KindName::from(s.kind())
                ),
                Event::Start {
                    side: s.side().into(),
                    kind: s.kind().into(),
                },
            )),
            Field::Health(h) => self.health(h),
            Field::Slow(e) => Some(self.slow(e)),
            Field::Mark(m) => Some((
                format!("{:>6.1}s  {}  mark {}", m.t, side_name(m.side()), m.name),
                Event::Mark {
                    side: m.side().into(),
                    t: m.t,
                    name: m.name.clone(),
                },
            )),
            Field::Warning(w) => Some(warning(w)),
            Field::Stop(s) => Some(stop(s)),
            _ => None,
        }
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
        let mut note = HealthNote {
            side: h.side().into(),
            t: h.t,
            rss_mb: f64::from(h.rss_mb),
            tps: None,
            mspt_avg: None,
            mspt_p95: None,
            mspt_max: None,
            fps: None,
            frame_ms_p95: None,
            players: None,
        };
        for (slot, label, v) in [
            (&mut note.tps, "TPS", h.tps),
            (&mut note.mspt_avg, "MSPT avg", h.mspt_avg),
            (&mut note.mspt_p95, "p95", h.mspt_p95),
            (&mut note.mspt_max, "max", h.mspt_max),
            (&mut note.fps, "FPS", h.fps),
            (&mut note.frame_ms_p95, "frame p95", h.frame_ms_p95),
        ] {
            if let Some(v) = v {
                *slot = Some(f64::from(v));
                parts.push(format!("{label} {v:.1}"));
            }
        }
        parts.push(format!("RSS {:.0} MB", h.rss_mb));
        if h.side() == Side::Server {
            note.players = Some(h.players);
            parts.push(format!("{} players", h.players));
        }
        Some((
            format!("{:>6.1}s  {name}  {}", h.t, parts.join(" · ")),
            Event::Health(note),
        ))
    }

    fn slow(&self, e: &SlowEvent) -> Line {
        let name = side_name(e.side());
        let unit = UnitKind::from(e.unit());
        let spans: Vec<SlowSpan> = e
            .top
            .iter()
            .take(SLOW_SPANS_SHOWN)
            .map(|s| SlowSpan {
                name: self
                    .strings
                    .get(&(e.side, s.name))
                    .cloned()
                    .unwrap_or_else(|| format!("#{}", s.name)),
                wall_ms: f64::from(s.wall_ms),
            })
            .collect();
        let listed = e
            .top
            .iter()
            .take(SLOW_SPANS_SHOWN)
            .zip(&spans)
            .map(|(s, span)| format!("{} {:.1}", span.name, s.wall_ms))
            .collect::<Vec<_>>()
            .join(", ");
        let note = SlowNote {
            side: e.side().into(),
            t: e.t,
            unit,
            counter: e.counter,
            wall_ms: f64::from(e.wall_ms),
            top: spans,
        };
        (
            format!(
                "{:>6.1}s  {name}  slow {unit} {}: {:.1} ms ({listed})",
                e.t, e.counter, e.wall_ms
            ),
            Event::Slow(note),
        )
    }
}

fn warning(w: &Warning) -> Line {
    let name = side_name(w.side());
    let code = WarningCodeName::from(w.code());
    (
        format!("{:>6.1}s  {name}  warning {code} {:?}", w.t, w.args),
        Event::Warning {
            side: w.side().into(),
            t: w.t,
            code,
            args: w.args.clone(),
        },
    )
}

fn stop(s: &Stop) -> Line {
    let name = side_name(s.side());
    let words = match s.reason() {
        StopReason::Normal => "normal",
        StopReason::WindowCap => "window cap",
        StopReason::Disconnected => "disconnected",
        StopReason::Shutdown => "shutdown",
        StopReason::Unspecified => "unknown",
    };
    (
        format!("{:>6.1}s  {name}  stopped ({words})", s.t),
        Event::Stop {
            side: s.side().into(),
            t: s.t,
            reason: s.reason().into(),
        },
    )
}
