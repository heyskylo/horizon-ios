use horizon_perf_format::v1::{GpuCounts, GpuFragmentUnit, GpuPasses, GpuSource};

use super::rows::{
    GpuCountersRow, GpuCountsRow, GpuDeviceRow, GpuPartCountsRow, GpuPartRow, GpuPassRow, Row,
};
use super::{Ctx, QueryError, Reply, num};
use crate::ids::{Id, Kind};
use crate::view::View;
use crate::wire::{Unit, fine};

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

/// The pass rows of one timer's section, biggest time first; passes seen only through their counters last.
pub(crate) fn pass_rows(view: &View, gpu: &GpuPasses) -> Vec<GpuPassRow> {
    let side = gpu.side();
    let text = |id: u32| view.text(side, id).into_owned();
    let unit = gpu
        .device
        .as_ref()
        .map_or(GpuFragmentUnit::Unspecified, |d| d.fragment_unit());
    let total: f64 = gpu.passes.iter().filter_map(|p| p.ms).map(f64::from).sum();
    let mut rows: Vec<GpuPassRow> = gpu
        .passes
        .iter()
        .map(|p| {
            let name = text(p.name);
            GpuPassRow {
                id: Id::new(Kind::Gpu, name.as_str()).to_string(),
                name,
                source: gpu.source().into(),
                ms: p.ms.map(|ms| num(f64::from(ms))),
                share: p
                    .ms
                    .filter(|_| total > 0.0)
                    .map(|ms| fine(f64::from(ms) / total)),
                parts: p
                    .parts
                    .iter()
                    .map(|part| GpuPartRow {
                        name: text(part.name),
                        ms: num(f64::from(part.ms)),
                    })
                    .collect(),
                counters: None,
            }
        })
        .collect();
    for c in &gpu.counters {
        let pass = text(c.pass);
        let at = rows.iter().position(|r| r.name == pass).unwrap_or_else(|| {
            rows.push(GpuPassRow {
                id: Id::new(Kind::Gpu, pass.as_str()).to_string(),
                name: pass,
                source: gpu.source().into(),
                ms: None,
                share: None,
                parts: Vec::new(),
                counters: None,
            });
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
        let ms = |r: &GpuPassRow| r.ms.unwrap_or(f64::NEG_INFINITY);
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
        .flat_map(|g| pass_rows(view, g))
        .collect()
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
    })];
    rows.extend(passes.into_iter().map(Row::GpuPass));
    Ok(Reply::new(Unit::MsPerFrame, rows))
}
