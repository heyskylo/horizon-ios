use horizon_perf_format::v1::{Capture, HealthSample, Kind, Side, Start, Stop};

use crate::diff::{self, Diff, DiffOptions, signed};
use crate::error::{Error, side_name};
use crate::health::{live, max, mean, trailing_mean};
use crate::view::View;

mod bench;
mod hot;
mod sections;

/// Default size of a summary in tokens, measured as bytes / 4.
pub const DEFAULT_BUDGET: usize = 1500;

const TARGET_TPS: f64 = 20.0;
const TRAILING_WINDOWS: [(&str, f64); 3] = [("1m", 60.0), ("5m", 300.0), ("15m", 900.0)];

/// An earlier capture the summary is compared against.
#[derive(Clone, Debug)]
pub struct Baseline {
    pub name: String,
    pub capture: Capture,
}

#[derive(Clone, Debug)]
pub struct SummaryOptions {
    pub budget_tokens: usize,
    pub side: Option<Side>,
    /// The span thread of the phase table; the side's main thread when unset.
    pub thread: Option<String>,
    /// Phases of the slow ticks or frames only.
    pub slow: bool,
    /// Numeric rows gain a delta and a verdict against this capture.
    pub baseline: Option<Baseline>,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self {
            budget_tokens: DEFAULT_BUDGET,
            side: None,
            thread: None,
            slow: false,
            baseline: None,
        }
    }
}

/// The agent report: a header, the health verdict and the span-based sections per side, server first.
pub fn summarize(capture: &Capture, name: &str, options: &SummaryOptions) -> Result<String, Error> {
    let view = View::new(capture);
    let present = |side: Side| {
        capture.start.iter().any(|s| s.side() == side)
            || capture.health.iter().any(|h| h.side() == side)
            || capture.stop.iter().any(|s| s.side() == side)
            || view.sides().contains(&side)
    };
    if let Some(side) = options.side
        && !present(side)
    {
        return Err(Error::NoSuchSide { side });
    }
    let sides: Vec<Side> = [Side::Server, Side::Client]
        .into_iter()
        .filter(|&side| options.side.is_none_or(|only| only == side) && present(side))
        .collect();
    if sides.is_empty() {
        return Err(Error::Empty);
    }
    let mut parts: Vec<Section> = Vec::new();
    let mut detail = false;
    for &side in &sides {
        detail |= side_sections(&view, name, side, options, &mut parts)?;
    }
    if detail {
        parts.push(vec![Line::body(sections::NOTE.to_string())]);
        parts.push(vec![Line::fixed(sections::next(&view, &sides, options))]);
    }
    Ok(fit(parts, options.budget_tokens))
}

type Section = Vec<Line>;

#[derive(Clone, Copy, PartialEq)]
enum LineKind {
    /// Never cut: headers and the last line.
    Fixed,
    Body,
    /// A table's column heading; it goes with its last row.
    Heading,
}

struct Line {
    text: String,
    kind: LineKind,
    /// Rank of a row in a list that is not in rank order itself; the lowest goes first when the list is cut.
    weight: Option<f64>,
}

impl Line {
    fn fixed(text: String) -> Self {
        Self {
            text,
            kind: LineKind::Fixed,
            weight: None,
        }
    }

    fn body(text: String) -> Self {
        Self {
            text,
            kind: LineKind::Body,
            weight: None,
        }
    }

    fn ranked(text: String, weight: f64) -> Self {
        Self {
            text,
            kind: LineKind::Body,
            weight: Some(weight),
        }
    }

    fn heading(text: String) -> Self {
        Self {
            text,
            kind: LineKind::Heading,
            weight: None,
        }
    }
}

/// Removes the row a cut gives up first: the lowest-ranked one when rows are ranked, else the last.
fn drop_row(part: &mut Section) {
    let lowest = part
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.weight.map(|w| (i, w)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i);
    match lowest {
        Some(i) => {
            part.remove(i);
        }
        None => {
            part.pop();
        }
    }
}

/// Cuts the longest list's last or lowest-ranked row until it fits, then whole sections from the bottom; fixed lines stay.
fn fit(mut parts: Vec<Section>, budget_tokens: usize) -> String {
    let size = |parts: &[Section]| -> usize {
        parts
            .iter()
            .flatten()
            .map(|l| l.text.len() + 1)
            .sum::<usize>()
            / 4
    };
    let cuttable =
        |part: &Section| !part.is_empty() && part.iter().all(|l| l.kind != LineKind::Fixed);
    let shortest = |part: &Section| part.iter().filter(|l| l.kind == LineKind::Heading).count() + 1;
    while size(&parts) > budget_tokens {
        let longest = parts
            .iter()
            .enumerate()
            .filter(|(_, p)| cuttable(p) && p.len() > shortest(p))
            .max_by_key(|(i, p)| (p.len(), *i))
            .map(|(i, _)| i);
        if let Some(i) = longest {
            drop_row(&mut parts[i]);
            continue;
        }
        match parts.iter().rposition(cuttable) {
            Some(i) => {
                parts.remove(i);
            }
            None => break,
        }
    }
    let mut text = parts
        .into_iter()
        .flatten()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

fn side_sections(
    view: &View,
    name: &str,
    side: Side,
    options: &SummaryOptions,
    parts: &mut Vec<Section>,
) -> Result<bool, Error> {
    let capture = view.capture;
    let start = capture.start.iter().find(|s| s.side() == side);
    let stop = capture.stop.iter().find(|s| s.side() == side);
    let samples = live(capture, side);
    parts.push(vec![Line::fixed(header(name, side, start, stop, &samples))]);
    let compared = options.baseline.as_ref().map(|base| {
        let wanted = DiffOptions {
            side: Some(side),
            thread: options.thread.clone(),
            limit: usize::MAX,
            ..Default::default()
        };
        (base, diff::diff(&base.capture, capture, &wanted))
    });
    if let Some((base, result)) = &compared {
        parts.push(vec![Line::fixed(baseline_line(
            base,
            capture,
            side,
            options.slow,
            result,
        ))]);
    }
    let compared = compared.and_then(|(_, result)| result.ok());
    let mut health: Vec<Line> = health_rows(view, side, start, stop, &samples)
        .into_iter()
        .map(Line::body)
        .collect();
    if let Some(text) = compared.as_ref().and_then(health_delta) {
        health.push(Line::body(text));
    }
    parts.push(health);
    let more = sections::all(view, side, options, compared.as_ref())?;
    let detail = !more.is_empty();
    parts.extend(more);
    Ok(detail)
}

fn baseline_line(
    base: &Baseline,
    capture: &Capture,
    side: Side,
    slow: bool,
    result: &Result<Diff, Error>,
) -> String {
    let d = match result {
        Ok(d) => d,
        Err(e) => return format!("baseline: {}, cannot compare: {e}", base.name),
    };
    let kind = match diff::build_mismatch(&base.capture, capture, side) {
        Some((was, now)) => format!("different build kind ({was} vs {now})"),
        None => "same build kind".to_string(),
    };
    let mut text = format!("baseline: {}, {kind}, {}", base.name, d.blocks_text());
    if slow {
        text.push_str(" (slow phases are not compared)");
    }
    text
}

/// "vs base h:mspt_avg -25.3 faster · …": the health fields' shifts against the baseline.
fn health_delta(d: &Diff) -> Option<String> {
    let parts: Vec<String> = d
        .rows
        .iter()
        .filter(|r| r.id.starts_with("h:"))
        .filter_map(|r| {
            Some(format!(
                "{} {} {}",
                r.id,
                signed(r.effect?),
                r.verdict.mark()
            ))
        })
        .collect();
    (!parts.is_empty()).then(|| format!("vs base {}", parts.join(" · ")))
}

fn header(
    name: &str,
    side: Side,
    start: Option<&Start>,
    stop: Option<&Stop>,
    samples: &[&HealthSample],
) -> String {
    let what = match start.map(|s| s.kind()) {
        Some(Kind::Dedicated) => "dedicated server",
        Some(Kind::Integrated) => "integrated server",
        Some(Kind::Client) => "client",
        _ => side_name(side),
    };
    let seconds = stop
        .map(|s| s.t)
        .or_else(|| max(samples.iter().map(|h| h.t)))
        .unwrap_or(0.0);
    let mut parts = vec![what.to_string(), format!("{seconds:.1} s")];
    if let Some(build) = start.and_then(|s| s.build.as_ref()) {
        let mut text = String::from("Horizon");
        for piece in [
            non_empty(&build.version).map(str::to_string),
            non_empty(&build.commit).map(|c| format!("({c})")),
            non_empty(&build.profile).map(str::to_string),
        ]
        .into_iter()
        .flatten()
        {
            text.push(' ');
            text.push_str(&piece);
        }
        parts.push(text);
    }
    if let Some(machine) = start.and_then(|s| s.machine.as_ref()) {
        let platform = [
            non_empty(&machine.os).map(capitalize),
            non_empty(&machine.arch).map(str::to_string),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        if !platform.is_empty() {
            parts.push(platform);
        }
        if machine.cpu_threads > 0 {
            parts.push(format!("{} threads", machine.cpu_threads));
        }
    }
    format!("{name}: {}", parts.join(", "))
}

fn health_rows(
    view: &View,
    side: Side,
    start: Option<&Start>,
    stop: Option<&Stop>,
    samples: &[&HealthSample],
) -> Vec<String> {
    let capture = view.capture;
    if samples.is_empty() {
        return vec!["health  no health samples in this capture".into()];
    }
    let field = |pick: fn(&HealthSample) -> Option<f32>| {
        samples.iter().filter_map(move |h| pick(h)).map(f64::from)
    };
    let mut first = Vec::new();
    if side == Side::Server {
        if let Some(tps) = mean(field(|h| h.tps)) {
            let mut text = format!("TPS {tps:.1}/{TARGET_TPS:.0}");
            let end = stop.map_or_else(|| max(samples.iter().map(|h| h.t)).unwrap_or(0.0), |s| s.t);
            let trailing: Vec<String> = TRAILING_WINDOWS
                .iter()
                .filter_map(|(label, window)| {
                    trailing_mean(capture, side, end, *window, |h| h.tps)
                        .map(|v| format!("{label} {v:.1}"))
                })
                .collect();
            if !trailing.is_empty() {
                text.push_str(&format!(" ({})", trailing.join(", ")));
            }
            first.push(text);
        }
        let mut mspt = Vec::new();
        if let Some(v) = mean(field(|h| h.mspt_avg)) {
            mspt.push(format!("avg {v:.1}"));
        }
        if let Some(v) = max(field(|h| h.mspt_p95)) {
            mspt.push(format!("worst-second p95 {v:.1}"));
        }
        if let Some(v) = max(field(|h| h.mspt_max)) {
            mspt.push(format!("max {v:.1}"));
        }
        if !mspt.is_empty() {
            first.push(format!("MSPT {} ms", mspt.join(" · ")));
        }
        if let Some(text) = sections::slow_share(view, side) {
            first.push(text);
        }
    } else {
        let mut fps = Vec::new();
        if let Some(v) = mean(field(|h| h.fps)) {
            fps.push(format!("FPS avg {v:.1}"));
        }
        let mut frame = Vec::new();
        if let Some(v) = mean(field(|h| h.frame_ms_p50)) {
            frame.push(format!("p50 {v:.1}"));
        }
        for (label, v) in [
            ("worst-second p95", max(field(|h| h.frame_ms_p95))),
            ("worst-second p99", max(field(|h| h.frame_ms_p99))),
            ("max", max(field(|h| h.frame_ms_max))),
        ] {
            if let Some(v) = v {
                frame.push(format!("{label} {v:.1}"));
            }
        }
        first.extend(fps);
        if !frame.is_empty() {
            first.push(format!("frame {} ms", frame.join(" · ")));
        }
    }

    let mut second = Vec::new();
    let cpu = mean(samples.iter().map(|h| f64::from(h.cpu_process_cores)));
    let system = mean(samples.iter().map(|h| f64::from(h.cpu_system_cores)));
    if let Some(cpu) = cpu {
        let mut text = format!("CPU {cpu:.1} cores");
        if let Some(system) = system {
            let threads = start
                .and_then(|s| s.machine.as_ref())
                .map_or(0, |m| m.cpu_threads);
            if threads > 0 {
                text.push_str(&format!(" (system {system:.1} of {threads})"));
            } else {
                text.push_str(&format!(" (system {system:.1})"));
            }
        }
        second.push(text);
    }
    let (first_sample, last) = (samples[0], samples[samples.len() - 1]);
    second.push(format!(
        "RSS {} ({:+.0} MB over the window)",
        megabytes(f64::from(last.rss_mb)),
        f64::from(last.rss_mb) - f64::from(first_sample.rss_mb)
    ));
    if side == Side::Server {
        second.push(format!("{} players", last.players));
    }
    if last.chunks > 0 {
        second.push(format!("{} chunks", thousands(last.chunks)));
    }
    if last.entities > 0 {
        second.push(format!("{} entities", thousands(last.entities)));
    }

    let mut rows = Vec::new();
    if !first.is_empty() {
        rows.push(format!("health  {}", first.join(" · ")));
    }
    let lead = if rows.is_empty() {
        "health  "
    } else {
        "        "
    };
    rows.push(format!("{lead}{}", second.join(" · ")));
    rows
}

fn non_empty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn megabytes(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

fn thousands(n: impl std::fmt::Display) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
