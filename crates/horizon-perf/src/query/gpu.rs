use horizon_perf_format::v1::{GpuCounts, GpuFragmentUnit, GpuFrame, GpuPasses, GpuSource, Side};

use super::rows::{
    GpuCountersRow, GpuCountsRow, GpuDeviceRow, GpuFramePartRow, GpuFramePassRow, GpuFrameRow,
    GpuPartCountsRow, GpuPartRow, GpuPassRow, GpuSummaryRow, MsStats, Row,
};
use super::{Ctx, QueryError, Reply, num};
use crate::ids::{Id, Kind};
use crate::view::View;
use crate::wire::{BoundName, GpuSourceName, Unit, fine};

/// How much longer one side's frame p50 must be than the other's for it to be the bound.
const BOUND_MARGIN: f64 = 1.2;
/// `bound`'s rule, as the summary row states it.
pub const BOUND_RULE: &str = "gpu when the GPU frame p50 is at least 1.2 x the CPU frame p50, cpu when the CPU frame p50 is at least 1.2 x the GPU frame p50, else mixed";

/// Which side bounds the frames, by `BOUND_RULE`; none without both medians.
pub(crate) fn bound(gpu_p50: Option<f64>, cpu_p50: Option<f64>) -> Option<BoundName> {
    let (gpu, cpu) = (gpu_p50?, cpu_p50?);
    Some(if gpu >= BOUND_MARGIN * cpu {
        BoundName::Gpu
    } else if cpu >= BOUND_MARGIN * gpu {
        BoundName::Cpu
    } else {
        BoundName::Mixed
    })
}

/// The nearest-rank `pct` percentile of sorted, non-empty values.
fn rank(sorted: &[f64], pct: usize) -> f64 {
    let at = (pct * sorted.len()).div_ceil(100).clamp(1, sorted.len());
    sorted[at - 1]
}

pub(crate) fn ms_stats(values: &[f64]) -> MsStats {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let Some(&max) = v.last() else {
        return MsStats::default();
    };
    MsStats {
        samples: v.len() as u64,
        p50: Some(num(rank(&v, 50))),
        p95: Some(num(rank(&v, 95))),
        p99: Some(num(rank(&v, 99))),
        max: Some(num(max)),
    }
}

/// Every time one timer's frames gave each pass and part, and the frames' totals.
#[derive(Default)]
struct Series {
    totals: Vec<f64>,
    passes: Vec<(String, Vec<f64>)>,
    parts: Vec<(String, String, Vec<f64>)>,
}

impl Series {
    fn of(view: &View, sides: &[Side], source: GpuSource) -> Self {
        let mut series = Self::default();
        for (side, frame) in frames(view, sides).filter(|(_, f)| f.source() == source) {
            series.totals.extend(frame.total_ms.map(f64::from));
            for pass in &frame.passes {
                let name = view.text(side, pass.name).into_owned();
                if let Some(ms) = pass.ms {
                    series.pass(&name).push(f64::from(ms));
                }
                for part in &pass.parts {
                    let part_name = view.text(side, part.name).into_owned();
                    series.part(&name, &part_name).push(f64::from(part.ms));
                }
            }
        }
        series
    }

    fn pass(&mut self, name: &str) -> &mut Vec<f64> {
        let i = match self.passes.iter().position(|p| p.0 == name) {
            Some(i) => i,
            None => {
                self.passes.push((name.to_owned(), Vec::new()));
                self.passes.len() - 1
            }
        };
        &mut self.passes[i].1
    }

    fn part(&mut self, pass: &str, part: &str) -> &mut Vec<f64> {
        let i = match self.parts.iter().position(|p| p.0 == pass && p.1 == part) {
            Some(i) => i,
            None => {
                self.parts
                    .push((pass.to_owned(), part.to_owned(), Vec::new()));
                self.parts.len() - 1
            }
        };
        &mut self.parts[i].2
    }

    fn pass_times(&self, name: &str) -> &[f64] {
        self.passes
            .iter()
            .find(|p| p.0 == name)
            .map_or(&[], |p| p.1.as_slice())
    }

    fn part_times(&self, pass: &str, part: &str) -> &[f64] {
        self.parts
            .iter()
            .find(|p| p.0 == pass && p.1 == part)
            .map_or(&[], |p| p.2.as_slice())
    }
}

/// The timed frames of `sides`, in the order written.
fn frames<'a>(view: &View<'a>, sides: &'a [Side]) -> impl Iterator<Item = (Side, &'a GpuFrame)> {
    view.capture
        .gpu_frames
        .iter()
        .filter(|b| sides.contains(&b.side()))
        .flat_map(|b| b.frames.iter().map(move |f| (b.side(), f)))
}

/// `part` of `whole`, when both are known and `whole` is not zero.
fn share_of(part: Option<f64>, whole: Option<f64>) -> Option<f64> {
    let (part, whole) = (part?, whole.filter(|w| *w > 0.0)?);
    Some(fine(part / whole))
}

fn counts_row(c: &GpuCounts) -> GpuCountsRow {
    GpuCountsRow {
        vertex_shader_invocations: c.vertex_shader_invocations,
        clipper_invocations: c.clipper_invocations,
        clipper_primitives_out: c.clipper_primitives_out,
        fragment_shader_invocations: c.fragment_shader_invocations,
    }
}

/// Shaded pixels per target pixel to two decimals; a quad counter stands for four pixels each.
fn fragments_per_pixel(unit: GpuFragmentUnit, fragments: u64, pixels: u64) -> Option<f64> {
    let per = match unit {
        GpuFragmentUnit::Unspecified => return None,
        GpuFragmentUnit::Pixel => 1,
        GpuFragmentUnit::Quad => 4,
    };
    (pixels > 0).then(|| ((fragments * per) as f64 / pixels as f64 * 100.0).round() / 100.0)
}

/// The pass rows of one timer's section with its frames' statistics, biggest p50 (else median) first; passes seen only through their counters last.
fn pass_rows(view: &View, gpu: &GpuPasses, series: &Series) -> Vec<GpuPassRow> {
    let side = gpu.side();
    let text = |id: u32| view.text(side, id).into_owned();
    let unit = gpu
        .device
        .as_ref()
        .map_or(GpuFragmentUnit::Unspecified, |d| d.fragment_unit());
    let frame_p50 = ms_stats(&series.totals).p50;
    let total: f64 = gpu.passes.iter().filter_map(|p| p.ms).map(f64::from).sum();
    let source: GpuSourceName = gpu.source().into();
    let row = |name: String, ms: Option<f64>, parts: Vec<(String, Option<f64>)>| {
        let stats = ms_stats(series.pass_times(&name));
        let share = match frame_p50 {
            Some(_) => share_of(stats.p50, frame_p50),
            None => ms.filter(|_| total > 0.0).map(|ms| fine(ms / total)),
        };
        let parts = parts
            .into_iter()
            .map(|(part, ms)| {
                let stats = ms_stats(series.part_times(&name, &part));
                GpuPartRow {
                    ms: ms.or(stats.p50).unwrap_or(0.0),
                    share: share_of(stats.p50, frame_p50),
                    name: part,
                    samples: stats.samples,
                    p50: stats.p50,
                    p95: stats.p95,
                    p99: stats.p99,
                    max: stats.max,
                }
            })
            .collect();
        GpuPassRow {
            id: Id::new(Kind::Gpu, name.as_str()).to_string(),
            name,
            source,
            ms,
            share,
            samples: stats.samples,
            p50: stats.p50,
            p95: stats.p95,
            p99: stats.p99,
            max: stats.max,
            parts,
            counters: None,
        }
    };
    let mut listed: Vec<(String, Option<f64>, Vec<(String, Option<f64>)>)> = gpu
        .passes
        .iter()
        .map(|p| {
            let parts = p
                .parts
                .iter()
                .map(|part| (text(part.name), Some(num(f64::from(part.ms)))))
                .collect();
            (text(p.name), p.ms.map(|ms| num(f64::from(ms))), parts)
        })
        .collect();
    for (pass, _) in &series.passes {
        if !listed.iter().any(|l| l.0 == *pass) {
            listed.push((pass.clone(), None, Vec::new()));
        }
    }
    for (pass, part, _) in &series.parts {
        let at = match listed.iter().position(|l| l.0 == *pass) {
            Some(at) => at,
            None => {
                listed.push((pass.clone(), None, Vec::new()));
                listed.len() - 1
            }
        };
        if !listed[at].2.iter().any(|p| p.0 == *part) {
            listed[at].2.push((part.clone(), None));
        }
    }
    let mut rows: Vec<GpuPassRow> = listed
        .into_iter()
        .map(|(name, ms, parts)| row(name, ms, parts))
        .collect();
    for c in &gpu.counters {
        let pass = text(c.pass);
        let at = rows.iter().position(|r| r.name == pass).unwrap_or_else(|| {
            rows.push(row(pass, None, Vec::new()));
            rows.len() - 1
        });
        let total = c.total.unwrap_or_default();
        rows[at].counters = Some(GpuCountersRow {
            total: counts_row(&total),
            pixels: c.pixels,
            fragments_per_pixel: fragments_per_pixel(
                unit,
                total.fragment_shader_invocations,
                c.pixels,
            ),
            parts: c
                .parts
                .iter()
                .map(|part| GpuPartCountsRow {
                    name: text(part.name),
                    counts: counts_row(&part.counts.unwrap_or_default()),
                })
                .collect(),
        });
    }
    rows.sort_by(|a, b| {
        let ms = |r: &GpuPassRow| r.p50.or(r.ms).unwrap_or(f64::NEG_INFINITY);
        ms(b).total_cmp(&ms(a)).then_with(|| a.name.cmp(&b.name))
    });
    rows
}

fn source_order(source: GpuSource) -> u8 {
    match source {
        GpuSource::Horizon => 0,
        GpuSource::ShaderPack => 1,
        GpuSource::Unspecified => 2,
    }
}

/// The pass rows of every GPU section of `sides`, Horizon's own passes first.
pub(crate) fn rows_of(view: &View, sides: &[horizon_perf_format::v1::Side]) -> Vec<GpuPassRow> {
    let mut sections: Vec<&GpuPasses> = view
        .capture
        .gpu
        .iter()
        .filter(|g| sides.contains(&g.side()))
        .collect();
    sections.sort_by_key(|g| source_order(g.source()));
    sections
        .into_iter()
        .flat_map(|g| pass_rows(view, g, &Series::of(view, sides, g.source())))
        .collect()
}

/// The Horizon timer's frame totals, the CPU frame times beside them, and which bounds the frames.
fn summary_row(view: &View, sides: &[Side]) -> GpuSummaryRow {
    let gpu_frame = ms_stats(&Series::of(view, sides, GpuSource::Horizon).totals);
    let cpu: Vec<f64> = view
        .capture
        .gpu_frames
        .iter()
        .filter(|b| sides.contains(&b.side()))
        .flat_map(|b| b.cpu_frame_ms.iter().map(|&ms| f64::from(ms)))
        .collect();
    let cpu_frame = ms_stats(&cpu);
    GpuSummaryRow {
        id: Id::new(Kind::Gpu, "frame").to_string(),
        bound: bound(gpu_frame.p50, cpu_frame.p50),
        bound_rule: BOUND_RULE.to_owned(),
        gpu_frame,
        cpu_frame,
    }
}

/// Each timed frame of the window (`--from`/`--to` narrow them): its total and its passes' times, and whether it was a slow frame.
pub(super) fn gpu_frames(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let side = ctx.side(0)?;
    let window = ctx.window(0, side, f64::NEG_INFINITY)?;
    let mut slow: Vec<(f64, f64)> = view
        .capture
        .slow
        .iter()
        .filter(|s| s.side() == side)
        .map(|s| (s.t, f64::from(s.wall_ms)))
        .collect();
    slow.sort_by(|a, b| a.0.total_cmp(&b.0));
    let sides = [side];
    let mut rows: Vec<GpuFrameRow> = frames(view, &sides)
        .filter(|(_, f)| window.contains(f.t))
        .map(|(side, f)| {
            let text = |id: u32| view.text(side, id).into_owned();
            let slow_wall_ms = slow_frame(&slow, f.t);
            GpuFrameRow {
                id: String::new(),
                t: num(f.t),
                source: f.source().into(),
                total_ms: f.total_ms.map(|ms| num(f64::from(ms))),
                slow: slow_wall_ms.is_some(),
                slow_wall_ms: slow_wall_ms.map(num),
                passes: f
                    .passes
                    .iter()
                    .map(|p| GpuFramePassRow {
                        name: text(p.name),
                        ms: p.ms.map(|ms| num(f64::from(ms))),
                        parts: p
                            .parts
                            .iter()
                            .map(|part| GpuFramePartRow {
                                name: text(part.name),
                                ms: num(f64::from(part.ms)),
                            })
                            .collect(),
                    })
                    .collect(),
            }
        })
        .collect();
    rows.sort_by(|a, b| a.t.total_cmp(&b.t));
    for (i, row) in rows.iter_mut().enumerate() {
        row.id = Id::new(Kind::Gpu, format!("frame {i}")).to_string();
    }
    let mut reply = Reply::new(
        Unit::MsPerFrame,
        rows.into_iter().map(Row::GpuFrame).collect(),
    );
    reply.window = Some(window);
    Ok(reply)
}

/// The wall ms of the slow frame (end `t`, sorted) whose span holds `t`, give or take a millisecond.
fn slow_frame(slow: &[(f64, f64)], t: f64) -> Option<f64> {
    const SLACK_S: f64 = 0.001;
    let from = slow.partition_point(|s| s.0 < t - SLACK_S);
    slow[from..]
        .iter()
        .find(|(end, wall_ms)| end - wall_ms / 1000.0 - SLACK_S <= t)
        .map(|s| s.1)
}

/// Median GPU time per pass, the parts of a pass and its pipeline counters, from a capture made with `--gpu`.
pub(super) fn gpu(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let sides = ctx.sides(0)?;
    let sections: Vec<&GpuPasses> = view
        .capture
        .gpu
        .iter()
        .filter(|g| sides.contains(&g.side()))
        .collect();
    let Some(device) = sections.iter().find_map(|g| g.device.as_ref()) else {
        return Err(QueryError::new(
            "this capture has no gpu section for the selected side; capture with profile client start --gpu",
        ));
    };
    let mut passes = rows_of(view, &sides);
    if !ctx.q.args.is_empty() {
        for arg in &ctx.q.args {
            if !passes.iter().any(|p| p.name == *arg || p.id == *arg) {
                let names: Vec<&str> = passes.iter().map(|p| p.name.as_str()).collect();
                return Err(QueryError::new(format!(
                    "no gpu pass named {arg}; passes: {}",
                    names.join(", ")
                )));
            }
        }
        passes.retain(|p| ctx.q.args.iter().any(|a| p.name == *a || p.id == *a));
    }
    passes.truncate(ctx.q.filters.limit);
    let mut rows = vec![Row::GpuDevice(GpuDeviceRow {
        id: Id::new(Kind::Gpu, "device").to_string(),
        adapter: device.adapter.clone(),
        backend: device.backend().into(),
        driver: device.driver.clone(),
        parts_mode: device.parts_mode().into(),
        counters: device.counters,
        fragment_unit: device.fragment_unit().into(),
        pass_split: device.pass_split().into(),
    })];
    rows.push(Row::GpuSummary(summary_row(view, &sides)));
    rows.extend(passes.into_iter().map(Row::GpuPass));
    Ok(Reply::new(Unit::MsPerFrame, rows))
}

#[cfg(test)]
mod tests {
    use super::{BoundName, bound, ms_stats};

    #[test]
    fn the_bound_needs_one_median_a_fifth_above_the_other() {
        assert_eq!(bound(Some(12.0), Some(10.0)), Some(BoundName::Gpu));
        assert_eq!(bound(Some(11.9), Some(10.0)), Some(BoundName::Mixed));
        assert_eq!(bound(Some(10.0), Some(12.0)), Some(BoundName::Cpu));
        assert_eq!(bound(None, Some(12.0)), None);
        assert_eq!(bound(Some(1.0), None), None);
    }

    #[test]
    fn one_sample_is_every_percentile() {
        let one = ms_stats(&[2.5]);
        assert_eq!(
            (one.samples, one.p50, one.p99, one.max),
            (1, Some(2.5), Some(2.5), Some(2.5))
        );
        assert_eq!(ms_stats(&[]).p50, None);
    }
}
