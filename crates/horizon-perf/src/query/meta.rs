use horizon_perf_format::v1::{
    Capture, GameRules, HorizonOptions, Machine, Options, Overhead, Settings, Side, Start, Stop,
};

use super::rows::{
    BuildInfo, GameRulesInfo, HorizonOptionsInfo, MachineInfo, MetaRow, ModInfo, OptionsInfo,
    OverheadInfo, Row, Sections, SettingsInfo, StartInfo, StopInfo,
};
use super::{Ctx, QueryError, Reply, fine, num, opt_num};
use crate::wire::{KindName, SideName, Unit};

/// What each side recorded and on what: its start, its stop and how many of each section it wrote.
pub(super) fn meta(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let rows = ctx
        .sides(0)?
        .into_iter()
        .map(|side| {
            let c = view.capture;
            Row::Meta(MetaRow {
                side: SideName::from(side),
                seconds: num(view.end(side)),
                sections: sections(c, side),
                start: c.start.iter().find(|s| s.side() == side).map(start_info),
                stop: c.stop.iter().find(|s| s.side() == side).map(stop_info),
            })
        })
        .collect();
    Ok(Reply::new(Unit::Meta, rows))
}

fn sections(c: &Capture, side: Side) -> Sections {
    Sections {
        history: c.history.iter().filter(|m| m.side() == side).count(),
        slow_history: c.slow_history.iter().filter(|m| m.side() == side).count(),
        world: c.world.iter().filter(|m| m.side() == side).count(),
        marks: c.marks.iter().filter(|m| m.side() == side).count(),
        warnings: c.warnings.iter().filter(|m| m.side() == side).count(),
        health: c.health.iter().filter(|m| m.side() == side).count(),
        counters: c.counter_defs.iter().filter(|m| m.side() == side).count(),
        slow: c.slow.iter().filter(|m| m.side() == side).count(),
        span_blocks: c.span_blocks.iter().filter(|m| m.side() == side).count(),
        memory: c.memory.iter().filter(|m| m.side() == side).count(),
        spans: c.spans.iter().filter(|m| m.side() == side).count(),
        workers: c.workers.iter().filter(|m| m.side() == side).count(),
        gpu: c.gpu.iter().filter(|m| m.side() == side).count(),
        net: c.net.iter().filter(|m| m.side() == side).count(),
        samples: c.samples.iter().filter(|m| m.side() == side).count(),
    }
}

fn start_info(s: &Start) -> StartInfo {
    StartInfo {
        kind: KindName::from(s.kind()),
        trigger: s.trigger().into(),
        started_unix_ms: s.started_unix_ms,
        uptime_s: num(s.uptime_s),
        same_process: s.same_process,
        players: s.players,
        world_fs_used_mb: s.world_fs_used_mb,
        world_fs_total_mb: s.world_fs_total_mb,
        build: s.build.as_ref().map(|b| BuildInfo {
            version: b.version.clone(),
            commit: b.commit.clone(),
            profile: b.profile.clone(),
            target: b.target.clone(),
            features: b.features.clone(),
            symbols: b.symbols().into(),
        }),
        machine: s.machine.as_ref().map(machine_info),
        options: s.options.as_ref().map(options_info),
        settings: s.settings.as_ref().map(settings_info),
        game_rules: s.game_rules.as_ref().map(game_rules_info),
        horizon_options: s.horizon_options.as_ref().map(horizon_options_info),
        mods: s
            .mods
            .iter()
            .map(|m| ModInfo {
                id: m.id.clone(),
                version: m.version.clone(),
                kind: m.kind().into(),
                crate_name: m.crate_name.clone(),
            })
            .collect(),
    }
}

fn machine_info(m: &Machine) -> MachineInfo {
    MachineInfo {
        os: m.os.clone(),
        os_version: m.os_version.clone(),
        arch: m.arch.clone(),
        cpu: m.cpu.clone(),
        cpu_threads: m.cpu_threads,
        cpu_cores: m.cpu_cores,
        ram_mb: m.ram_mb,
        cgroup_cpu_cores: opt_num(m.cgroup_cpu_cores.map(f64::from)),
        cgroup_memory_mb: m.cgroup_memory_mb,
        gpu: m.gpu.clone(),
    }
}

fn options_info(o: &Options) -> OptionsInfo {
    OptionsInfo {
        hz: o.hz,
        threads: o.threads().into(),
        ticks_over_ms: opt_num(o.ticks_over_ms.map(f64::from)),
        wall: o.wall,
        timeline: o.timeline,
        gpu: o.gpu().into(),
        samples: o.samples,
        window_s: opt_num(o.window_s),
    }
}

fn game_rules_info(g: &GameRules) -> GameRulesInfo {
    GameRulesInfo {
        random_tick_speed: g.random_tick_speed,
        max_entity_cramming: g.max_entity_cramming,
        fire_spread_radius_around_player: g.fire_spread_radius_around_player,
        advance_time: g.advance_time,
        mob_griefing: g.mob_griefing,
        spawn_mobs: g.spawn_mobs,
        spawn_monsters: g.spawn_monsters,
        spawner_blocks_work: g.spawner_blocks_work,
    }
}

fn horizon_options_info(h: &HorizonOptions) -> HorizonOptionsInfo {
    HorizonOptionsInfo {
        max_view_distance: h.max_view_distance,
        lod_distance: h.lod_distance,
        anticheat: h.anticheat,
    }
}

fn settings_info(s: &Settings) -> SettingsInfo {
    SettingsInfo {
        view_distance: s.view_distance,
        simulation_distance: s.simulation_distance,
        max_players: s.max_players,
        network_compression_threshold: s.network_compression_threshold,
    }
}

fn stop_info(s: &Stop) -> StopInfo {
    StopInfo {
        t: num(s.t),
        reason: s.reason().into(),
        overhead: s.overhead.as_ref().map(overhead_info),
    }
}

fn overhead_info(o: &Overhead) -> OverheadInfo {
    OverheadInfo {
        spans_ms_per_tick: num(f64::from(o.spans_ms_per_tick)),
        spans_share: fine(f64::from(o.spans_share)),
        sampler_share_of_thread: fine(f64::from(o.sampler_share_of_thread)),
        writer_core_share: fine(f64::from(o.writer_core_share)),
        dropped_samples: o.dropped_samples,
        dropped_messages: o.dropped_messages,
        timeline_stopped_t: opt_num(o.timeline_stopped_t),
    }
}
