use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use flate2::read::GzDecoder;
use prost::Message;

use crate::v1::{Capture, HealthSample, Kind, Side, Unit};

const TARGET_TPS: f64 = 20.0;
const PHASES: usize = 5;

/// A few lines on a capture's server side, for a host with only the shipped binaries: what ran, its health and its heaviest tick phases.
pub fn server_brief(path: &Path) -> io::Result<Vec<String>> {
    let mut inflated = Vec::new();
    GzDecoder::new(File::open(path)?).read_to_end(&mut inflated)?;
    let capture = Capture::decode(inflated.as_slice()).map_err(io::Error::other)?;
    Ok(brief_of(&capture))
}

fn brief_of(capture: &Capture) -> Vec<String> {
    let server = |side: i32| side == Side::Server as i32;
    let kind = match capture
        .start
        .iter()
        .find(|s| server(s.side))
        .map(|s| s.kind())
    {
        Some(Kind::Integrated) => "integrated server",
        _ => "dedicated server",
    };
    let seconds = capture.stop.iter().find(|s| server(s.side)).map(|s| s.t);
    let ticks = capture
        .spans
        .iter()
        .find(|s| server(s.side) && s.unit() == Unit::Tick);
    let mut head = kind.to_owned();
    if let Some(seconds) = seconds {
        head.push_str(&format!(", {seconds:.1} s"));
    }
    if let Some(ticks) = ticks {
        head.push_str(&format!(", {} ticks", ticks.count));
    }
    let mut lines = vec![head];
    let health: Vec<&HealthSample> = capture.health.iter().filter(|h| server(h.side)).collect();
    let field = |pick: fn(&HealthSample) -> Option<f32>| {
        health.iter().filter_map(move |h| pick(h)).map(f64::from)
    };
    let mut parts = Vec::new();
    if let Some(tps) = mean(field(|h| h.tps)) {
        parts.push(format!("TPS {tps:.1}/{TARGET_TPS:.0}"));
    }
    if let Some(avg) = mean(field(|h| h.mspt_avg)) {
        let worst = field(|h| h.mspt_max).fold(avg, f64::max);
        parts.push(format!("MSPT mean {avg:.1} max {worst:.1} ms"));
    }
    if !parts.is_empty() {
        lines.push(format!("health  {}", parts.join(" · ")));
    }
    if let Some(last) = health.last() {
        let cores = mean(health.iter().map(|h| f64::from(h.cpu_process_cores))).unwrap_or(0.0);
        lines.push(format!(
            "        CPU {cores:.1} cores · RSS {:.0} MB · {} players · {} chunks · {} entities",
            last.rss_mb, last.players, last.chunks, last.entities
        ));
    }
    if let Some(ticks) = ticks.filter(|t| t.count > 0) {
        let names: HashMap<u32, &str> = capture
            .strings
            .iter()
            .filter(|s| server(s.side))
            .map(|s| (s.id, s.text.as_str()))
            .collect();
        let mut roots: Vec<_> = ticks.spans.iter().filter(|s| s.parent == 0).collect();
        roots.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
        let phases: Vec<String> = roots
            .iter()
            .take(PHASES)
            .map(|s| {
                let name = names.get(&s.name).copied().unwrap_or("?");
                format!("{name} {:.2}", s.wall_ms / ticks.count as f64)
            })
            .collect();
        if !phases.is_empty() {
            lines.push(format!("phases  ms/tick: {}", phases.join(" · ")));
        }
    }
    lines
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0u32), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / f64::from(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1::{SpanSummary, SpanTotal, Start, Stop, StringEntry};

    fn sample(t: f64, tps: f32, avg: f32, max: f32) -> HealthSample {
        HealthSample {
            side: Side::Server as i32,
            t,
            tps: Some(tps),
            mspt_avg: Some(avg),
            mspt_max: Some(max),
            cpu_process_cores: 1.5,
            rss_mb: 2048.0,
            players: 3,
            chunks: 900,
            entities: 120,
            ..HealthSample::default()
        }
    }

    fn total(name: u32, parent: u32, wall_ms: f64) -> SpanTotal {
        SpanTotal {
            name,
            parent,
            wall_ms,
            ..SpanTotal::default()
        }
    }

    #[test]
    fn a_dedicated_capture_reads_as_its_health_and_heaviest_phases() {
        let server = Side::Server as i32;
        let names = ["tick_lan", "level_overworld", "tick_blocks", "streaming"];
        let capture = Capture {
            start: vec![Start {
                side: server,
                kind: Kind::Dedicated as i32,
                ..Start::default()
            }],
            strings: (1..)
                .zip(names)
                .map(|(id, text)| StringEntry {
                    side: server,
                    id,
                    text: text.into(),
                })
                .collect(),
            health: vec![sample(1.0, 20.0, 4.0, 9.0), sample(2.0, 19.0, 6.0, 31.5)],
            spans: vec![SpanSummary {
                side: server,
                unit: Unit::Tick as i32,
                count: 40,
                spans: vec![
                    total(1, 0, 80.0),
                    total(2, 0, 120.0),
                    total(3, 2, 100.0),
                    total(4, 0, 10.0),
                ],
                ..SpanSummary::default()
            }],
            stop: vec![Stop {
                side: server,
                t: 2.0,
                ..Stop::default()
            }],
            ..Capture::default()
        };
        assert_eq!(
            brief_of(&capture),
            [
                "dedicated server, 2.0 s, 40 ticks",
                "health  TPS 19.5/20 · MSPT mean 5.0 max 31.5 ms",
                "        CPU 1.5 cores · RSS 2048 MB · 3 players · 900 chunks · 120 entities",
                "phases  ms/tick: level_overworld 3.00 · tick_lan 2.00 · streaming 0.25",
            ]
        );
    }
}
