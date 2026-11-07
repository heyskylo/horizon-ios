use horizon_perf_format::v1::{Machine, Options, Overhead, Settings, Side, Start, Stop};
use serde_json::{Value, json};

use super::{Ctx, QueryError, Reply, fine, kind_name, num, opt_num};
use crate::error::side_name;

/// `KIND_DEDICATED` -> `dedicated`.
fn short(name: &str, prefix: &str) -> String {
    name.strip_prefix(prefix).unwrap_or(name).to_lowercase()
}

/// What each side recorded and on what: its start, its stop and how many of each section it wrote.
pub(super) fn meta(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let rows = ctx
        .sides(0)?
        .into_iter()
        .map(|side| {
            let c = view.capture;
            let start = c.start.iter().find(|s| s.side() == side);
            let stop = c.stop.iter().find(|s| s.side() == side);
            let mut row = json!({
                "side": side_name(side),
                "seconds": num(view.end(side)),
                "sections": sections(c, side),
            });
            if let Some(start) = start {
                merge(&mut row, start_json(start));
            }
            if let Some(stop) = stop {
                row["stop"] = stop_json(stop);
            }
            row
        })
        .collect();
    Ok(Reply::new("meta", rows))
}

fn merge(row: &mut Value, extra: Value) {
    if let (Some(row), Value::Object(extra)) = (row.as_object_mut(), extra) {
        row.extend(extra);
    }
}

fn sections(c: &horizon_perf_format::v1::Capture, side: Side) -> Value {
    json!({
        "history": c.history.iter().filter(|m| m.side() == side).count(),
        "slow_history": c.slow_history.iter().filter(|m| m.side() == side).count(),
        "world": c.world.iter().filter(|m| m.side() == side).count(),
        "marks": c.marks.iter().filter(|m| m.side() == side).count(),
        "warnings": c.warnings.iter().filter(|m| m.side() == side).count(),
        "health": c.health.iter().filter(|m| m.side() == side).count(),
        "counters": c.counter_defs.iter().filter(|m| m.side() == side).count(),
        "slow": c.slow.iter().filter(|m| m.side() == side).count(),
        "span_blocks": c.span_blocks.iter().filter(|m| m.side() == side).count(),
        "memory": c.memory.iter().filter(|m| m.side() == side).count(),
        "spans": c.spans.iter().filter(|m| m.side() == side).count(),
        "workers": c.workers.iter().filter(|m| m.side() == side).count(),
        "net": c.net.iter().filter(|m| m.side() == side).count(),
    })
}

fn start_json(s: &Start) -> Value {
    json!({
        "kind": kind_name(s.kind()),
        "trigger": short(s.trigger().as_str_name(), "TRIGGER_"),
        "started_unix_ms": s.started_unix_ms,
        "uptime_s": num(s.uptime_s),
        "same_process": s.same_process,
        "players": s.players,
        "world_fs_used_mb": s.world_fs_used_mb,
        "world_fs_total_mb": s.world_fs_total_mb,
        "build": s.build.as_ref().map(|b| json!({
            "version": b.version,
            "commit": b.commit,
            "profile": b.profile,
            "target": b.target,
            "features": b.features,
            "symbols": short(b.symbols().as_str_name(), "SYMBOLS_"),
        })),
        "machine": s.machine.as_ref().map(machine_json),
        "options": s.options.as_ref().map(options_json),
        "settings": s.settings.as_ref().map(settings_json),
        "mods": s.mods.iter().map(|m| json!({
            "id": m.id,
            "version": m.version,
            "kind": short(m.kind().as_str_name(), "MOD_KIND_"),
            "crate": m.crate_name,
        })).collect::<Vec<_>>(),
    })
}

fn machine_json(m: &Machine) -> Value {
    json!({
        "os": m.os,
        "os_version": m.os_version,
        "arch": m.arch,
        "cpu": m.cpu,
        "cpu_threads": m.cpu_threads,
        "cpu_cores": m.cpu_cores,
        "ram_mb": m.ram_mb,
        "cgroup_cpu_cores": opt_num(m.cgroup_cpu_cores.map(f64::from)),
        "cgroup_memory_mb": m.cgroup_memory_mb,
        "gpu": m.gpu,
    })
}

fn options_json(o: &Options) -> Value {
    json!({
        "hz": o.hz,
        "threads": short(o.threads().as_str_name(), "THREAD_SELECTION_"),
        "ticks_over_ms": opt_num(o.ticks_over_ms.map(f64::from)),
        "wall": o.wall,
        "timeline": o.timeline,
        "gpu": short(o.gpu().as_str_name(), "GPU_MODE_"),
        "samples": o.samples,
        "window_s": opt_num(o.window_s),
    })
}

fn settings_json(s: &Settings) -> Value {
    json!({
        "view_distance": s.view_distance,
        "simulation_distance": s.simulation_distance,
        "max_players": s.max_players,
        "network_compression_threshold": s.network_compression_threshold,
    })
}

fn stop_json(s: &Stop) -> Value {
    json!({
        "t": num(s.t),
        "reason": short(s.reason().as_str_name(), "STOP_REASON_"),
        "overhead": s.overhead.as_ref().map(overhead_json),
    })
}

fn overhead_json(o: &Overhead) -> Value {
    json!({
        "spans_ms_per_tick": num(f64::from(o.spans_ms_per_tick)),
        "spans_share": fine(f64::from(o.spans_share)),
        "sampler_share_of_thread": fine(f64::from(o.sampler_share_of_thread)),
        "writer_core_share": fine(f64::from(o.writer_core_share)),
        "dropped_samples": o.dropped_samples,
        "dropped_messages": o.dropped_messages,
        "timeline_stopped_t": opt_num(o.timeline_stopped_t),
    })
}
