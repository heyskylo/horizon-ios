//! `horizon-client --benchmark SCENARIO`: the run `tools/perf_bench.py` drives from outside, driven
//! from inside, so anyone can measure their machine with nothing but the game. A thread sends the
//! same control commands `horizon-ctl` would, through the game's own command queue, and writes a
//! report (docs/benchmark-report.md) to `benchmarks/` in the game folder.
//!
//! The scenes, the world (seed 1234, creative) and every step and figure match `tools/perf_bench.py`,
//! so reports from the two can be compared.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use horizon_bench::compare::Ab;
use horizon_bench::{App, Frames, Kind, Report};
use serde_json::Value;

use crate::control::Request;

pub const SEED: i64 = 1234;
/// The world the benchmark creates under `saves/`, replaced on every run.
pub const WORLD: &str = "horizon-benchmark";
/// Written into the world so a later run only ever replaces its own world.
const MARKER: &str = "horizon-benchmark.txt";

/// A straight flight over terrain: name, start, end, pitch (`tools/graphics_bench.py`'s paths).
type Scene = (&'static str, [f64; 3], [f64; 3], f64);
pub const SCENES: [Scene; 4] = [
    (
        "village",
        [-960.0, 125.0, 80.0],
        [-960.0, 125.0, 272.0],
        35.0,
    ),
    (
        "forest",
        [-1100.0, 120.0, -560.0],
        [-1100.0, 120.0, -368.0],
        25.0,
    ),
    (
        "ocean",
        [-300.0, 85.0, -420.0],
        [-300.0, 85.0, -228.0],
        10.0,
    ),
    (
        "mountain",
        [560.0, 260.0, 860.0],
        [752.0, 260.0, 860.0],
        30.0,
    ),
];
const FLIGHT_SECONDS: f64 = 12.0;
const SETTLE: Duration = Duration::from_secs(2);
const LOAD_LIMIT: Duration = Duration::from_secs(600);

/// Where the trip starts and ends: 512 blocks at 16 blocks/s, the pace that scrambles section ids.
const TRAVEL_FROM: [f64; 3] = [-960.0, 150.0, -176.0];
const TRAVEL_TO: [f64; 3] = [-960.0, 150.0, 336.0];
const TRAVEL_PITCH: f64 = 30.0;
const TRAVEL_SECONDS: f64 = 32.0;
const HOLD_BLOCKS: u32 = 4;
const BLOCK_SECONDS: f64 = 4.0;
const WARMUP_SECONDS: f64 = 2.0;
const PAIRS: u32 = 8;
/// Frame-rate settings the benchmark can set for a run; `a` is the default.
pub struct Switch {
    pub name: &'static str,
    pub word: &'static str,
    pub key: &'static str,
    pub a: &'static str,
    pub b: &'static str,
}
pub const SWITCHES: [Switch; 4] = [
    Switch {
        name: "gpu-culling",
        word: "gpu-culling",
        key: "gpu_culling",
        a: "off",
        b: "on",
    },
    Switch {
        name: "section-cull",
        word: "section-cull",
        key: "cull",
        a: "on",
        b: "off",
    },
    Switch {
        name: "section-sides",
        word: "section-sides",
        key: "sides",
        a: "on",
        b: "off",
    },
    Switch {
        name: "section-draws",
        word: "section-draws",
        key: "flat",
        a: "flat",
        b: "indirect",
    },
];

/// What `--benchmark` runs.
pub enum Scenario {
    /// `standard` flies every scene; a scene's name flies only that one.
    Scenes(Vec<Scene>),
    /// A 512-block flight, then held at its end.
    Travel,
    /// The flight with the switch's B applied, then its two values interleaved B A A B.
    Ab(&'static Switch),
}

impl Scenario {
    pub fn parse(name: &str) -> Result<Scenario, String> {
        match name {
            "standard" => Ok(Scenario::Scenes(SCENES.to_vec())),
            "travel" => Ok(Scenario::Travel),
            name if name.starts_with("ab:") => SWITCHES
                .iter()
                .find(|s| s.name == &name[3..])
                .map(Scenario::Ab)
                .ok_or_else(|| {
                    format!(
                        "unknown switch {}: {}",
                        &name[3..],
                        SWITCHES.map(|s| s.name).join(", ")
                    )
                }),
            name => SCENES
                .iter()
                .find(|s| s.0 == name)
                .map(|s| Scenario::Scenes(vec![*s]))
                .ok_or_else(|| {
                    format!("unknown benchmark {name}: standard, village, forest, ocean, mountain, travel or ab:SWITCH ({})", SWITCHES.map(|s| s.name).join(", "))
                }),
        }
    }
}

/// `--benchmark-set SWITCH=VALUE`: the value must be one of the switch's two.
pub fn parse_set(value: &str) -> Result<(String, String), String> {
    let (name, setting) = value.split_once('=').ok_or("expected SWITCH=VALUE")?;
    let names = || SWITCHES.map(|s| s.name).join(", ");
    let switch = SWITCHES
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown switch {name}: {}", names()))?;
    if setting != switch.a && setting != switch.b {
        return Err(format!("{name} is {} or {}", switch.a, switch.b));
    }
    Ok((name.to_owned(), setting.to_owned()))
}

/// What the game knows about the run before it starts.
pub struct Setup {
    pub scenario: String,
    pub started: Instant,
    pub app: App,
    pub simulation_distance: u32,
    pub vsync: bool,
    /// `None`: unlimited.
    pub fps_cap: Option<u32>,
    pub offscreen: bool,
    pub game_dir: PathBuf,
    /// `--benchmark-set`: a switch and its value, applied before the world opens.
    pub set: Option<(String, String)>,
    /// `--profile`: record a .hzperf capture of the run and embed the report in it.
    pub profile: bool,
}

/// Starts the driver; the game quits when it is done, and the report's path is printed.
pub fn start(requests: Sender<Request>, setup: Setup) {
    thread::Builder::new()
        .name("horizon-benchmark".into())
        .spawn(move || {
            let control = Control(requests);
            match run(&control, setup) {
                Ok(path) => println!("Horizon: benchmark report {}", path.display()),
                Err(e) => eprintln!("Horizon: benchmark failed: {e}"),
            }
            let _ = control.call("quit");
        })
        .expect("spawn the benchmark thread");
}

struct Control(Sender<Request>);

impl Control {
    fn call(&self, line: &str) -> Result<Value, String> {
        let (reply, response) = mpsc::channel();
        self.0
            .send(Request {
                line: line.to_owned(),
                reply,
            })
            .map_err(|_| "the game has exited".to_owned())?;
        // Flights and loads reply when they finish.
        let text = response
            .recv_timeout(LOAD_LIMIT)
            .map_err(|_| format!("{line}: no reply"))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        match value.get("error").and_then(Value::as_str) {
            Some(e) => Err(format!("{line}: {e}")),
            None => Ok(value),
        }
    }
    fn status(&self) -> Value {
        self.call("status").unwrap_or(Value::Null)
    }
}

/// The wait before a scene's flight, spent timing Horizon's own render passes on the GPU:
/// `gpu_pass.<scene>.<pass>_ms`, where a slow GPU spends its frame. The timing is off again for
/// the flight, whose frames it would slow. False, having waited for nothing, on Metal: its passes
/// overlap on the GPU, so their times add up to more than the frame.
fn time_passes(control: &Control, scene: &str, report: &mut Report) -> Result<bool, String> {
    if report
        .hardware
        .gpu
        .as_ref()
        .is_some_and(|g| g.backend == "metal")
    {
        return Ok(false);
    }
    control.call("gpu-passes on")?;
    let held = control.call(&format!("bench {}", SETTLE.as_secs_f64()));
    control.call("gpu-passes off")?;
    for (pass, ms) in held?["gpu_passes"].as_object().into_iter().flatten() {
        let pass: String = pass
            .trim()
            .chars()
            .map(|c| match c {
                'a'..='z' | '0'..='9' => c,
                'A'..='Z' => c.to_ascii_lowercase(),
                _ => '_',
            })
            .collect();
        if let Some(ms) = ms.as_f64() {
            report.metric(&format!("gpu_pass.{scene}.{pass}_ms"), &[ms]);
        }
    }
    Ok(true)
}

fn run(control: &Control, setup: Setup) -> Result<PathBuf, String> {
    let scenario = Scenario::parse(&setup.scenario)?;
    let mut report = Report::new(Kind::Game, setup.app.clone());
    let pid = std::process::id();
    report.conditions.load_avg_1m = horizon_bench::load_average();
    report.conditions.power = horizon_bench::power_source().map(str::to_owned);
    let rss = RssSampler::start(pid);

    let title = poll(control, LOAD_LIMIT, |s| {
        s["screen"] == "title" && !s["renderer"].is_null()
    })?;
    report.metric("startup.title_ms", &[ms(setup.started.elapsed())]);
    let renderer = &title["renderer"];
    report.hardware.gpu = Some(horizon_bench::Gpu {
        name: text(&renderer["gpu"]),
        vendor: match text(&renderer["vendor"]) {
            // Metal reports no vendor ID.
            v if v.is_empty() && text(&renderer["gpu"]).starts_with("Apple") => "apple".into(),
            v => v,
        },
        driver: text(&renderer["driver"]),
        backend: text(&renderer["backend"]).to_lowercase(),
        kind: text(&renderer["device_type"]),
        vram_mb: None,
    });
    if let Some(gpu) = report.hardware.gpu.as_mut() {
        let vendor = horizon_telemetry::system::gpu_vendor(&gpu.name, 0);
        let vendor = if gpu.vendor.is_empty() {
            vendor
        } else {
            gpu.vendor.as_str()
        }
        .to_owned();
        if gpu.kind.contains("iscrete") {
            gpu.vram_mb = horizon_telemetry::system::vram_mb(&vendor);
        }
    }
    report.hardware.storage =
        horizon_telemetry::system::storage(&setup.game_dir).map(str::to_owned);
    report.hardware.display = serde_json::from_value(title["display"].clone()).ok();

    let mut steady = Vec::new();
    match &scenario {
        Scenario::Ab(switch) => keeping_switch(control, switch, || {
            measure(control, &setup, &scenario, &mut report, &mut steady)
        })?,
        _ => measure(control, &setup, &scenario, &mut report, &mut steady)?,
    }

    let status = control.status();
    let renderer = &status["renderer"];
    // On Wayland the window is on a screen only once it has been shown: ask again.
    if report.hardware.display.is_none() {
        report.hardware.display = serde_json::from_value(status["display"].clone()).ok();
    }
    let c = &mut report.conditions;
    c.scenario = Some(setup.scenario.clone());
    c.seed = Some(SEED.to_string());
    c.render_distance = status["render_distance"].as_u64().map(|d| d as u32);
    c.simulation_distance = Some(setup.simulation_distance);
    c.window = serde_json::from_value(renderer["size"].clone()).ok();
    c.vsync = Some(setup.vsync);
    c.fps_cap = setup.fps_cap;
    c.offscreen = Some(setup.offscreen);
    let (repeats, warmup) = match scenario {
        Scenario::Travel => (HOLD_BLOCKS, None),
        Scenario::Ab(_) => (2 * PAIRS, Some(WARMUP_SECONDS)),
        Scenario::Scenes(_) => (1, Some(SETTLE.as_secs_f64())),
    };
    (c.repeats, c.warmup_s) = (Some(repeats), warmup);
    if let Some(bytes) = renderer["gpu_allocated_bytes"].as_f64() {
        report.metric("memory.gpu_mb", &[bytes / f64::from(1 << 20)]);
    }
    let samples = rss.stop();
    if let Some(peak) = samples.iter().map(|s| s.1).max() {
        report.metric("memory.rss_peak_mb", &[peak as f64 / f64::from(1 << 20)]);
    }
    let mut during: Vec<u64> = samples
        .iter()
        .filter(|(t, _)| steady.iter().any(|(a, b)| a <= t && t <= b))
        .map(|s| s.1)
        .collect();
    during.sort_unstable();
    if let Some(&median) = during.get(during.len() / 2) {
        report.metric(
            "memory.rss_steady_mb",
            &[median as f64 / f64::from(1 << 20)],
        );
    }
    if let Ok(meta) = std::env::current_exe().and_then(std::fs::metadata) {
        report.metric("game.binary_bytes", &[meta.len() as f64]);
    }
    crate::telemetry::keep_benchmark(&crate::logging::game_home(), &report);
    let written = report
        .write_to(&setup.game_dir.join("benchmarks"))
        .map_err(|e| e.to_string())?;
    if setup.profile {
        match control.call(&stop_capture(&written)) {
            Ok(done) => println!("Horizon: benchmark capture {}", text(&done["path"])),
            Err(e) => eprintln!("Horizon: the benchmark capture failed: {e}"),
        }
    }
    Ok(written)
}

/// Starts the capture the run's frames feed; its trigger is the benchmark's.
const START_CAPTURE: &str = "profile client start";

/// Ends the capture with the written report embedded in it.
fn stop_capture(report: &Path) -> String {
    format!("profile client stop --bench {}", report.display())
}

/// Opens the benchmark world and flies the scenario in it.
fn measure(
    control: &Control,
    setup: &Setup,
    scenario: &Scenario,
    report: &mut Report,
    steady: &mut Vec<(Instant, Instant)>,
) -> Result<(), String> {
    match (scenario, &setup.set) {
        (Scenario::Ab(switch), _) => apply_set(control, switch.name, switch.b)?,
        (_, Some((name, value))) => apply_set(control, name, value)?,
        _ => {}
    }
    replace_world(&setup.game_dir)?;
    control.call(&format!("open {WORLD} {SEED} creative"))?;
    // The world opens on a worker: its folder may not exist yet when `open` replies.
    std::fs::create_dir_all(setup.game_dir.join("saves").join(WORLD)).map_err(|e| e.to_string())?;
    std::fs::write(
        setup.game_dir.join("saves").join(WORLD).join(MARKER),
        "Created by horizon-client --benchmark; replaced by the next run.\n",
    )
    .map_err(|e| e.to_string())?;
    let loaded = wait_loaded(control, |_| true)?;
    report.metric("startup.world_ms", &[ms(loaded - setup.started)]);

    if setup.profile {
        control.call(START_CAPTURE)?;
    }

    match scenario {
        Scenario::Scenes(scenes) => {
            for &(name, start, end, pitch) in scenes {
                control.call("time 6000")?;
                load_scene(control, name, start, end, pitch, report)?;
                if !time_passes(control, name, report)? {
                    thread::sleep(SETTLE);
                }
                let frames = fly(control, name, start, end, FLIGHT_SECONDS, pitch, steady)?;
                fps_metrics(report, name, std::slice::from_ref(&frames));
                report.frames.push(frames);
            }
        }
        Scenario::Travel => travel(control, report, steady)?,
        Scenario::Ab(switch) => {
            trip(control, report, steady)?;
            let plan = Plan {
                pairs: PAIRS,
                warmup: Duration::from_secs_f64(WARMUP_SECONDS),
            };
            ab_blocks(control, switch, &plan, report, steady)?;
        }
    }
    Ok(())
}

/// Reads a switch before the run and puts it back after, whatever the run did and however it ended.
fn keeping_switch<T>(
    control: &Control,
    switch: &Switch,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let initial = control.call(switch.word)?[switch.key].clone();
    let result = run();
    let restored = restore(control, switch, &initial);
    match result {
        Ok(value) => restored.map(|()| value),
        Err(e) => Err(e),
    }
}

/// Sends the arm whose reply has the value the switch had.
fn restore(control: &Control, switch: &Switch, initial: &Value) -> Result<(), String> {
    if control.call(switch.word)?[switch.key] == *initial {
        return Ok(());
    }
    for value in [switch.a, switch.b] {
        if control.call(&format!("{} {value}", switch.word))?[switch.key] == *initial {
            return Ok(());
        }
    }
    Err(format!("{} could not be put back", switch.name))
}

/// Sets a switch with its control word; a refusal (GPU culling not offered) is the game's reason.
fn apply_set(control: &Control, name: &str, value: &str) -> Result<(), String> {
    let switch = SWITCHES
        .iter()
        .find(|s| s.name == name)
        .ok_or("unknown switch")?;
    control.call(&format!("{} {value}", switch.word)).map(drop)
}

/// Teleports to a scene's start, holds there and waits for the view to load, reporting the load.
fn load_scene(
    control: &Control,
    name: &str,
    start: [f64; 3],
    end: [f64; 3],
    pitch: f64,
    report: &mut Report,
) -> Result<(), String> {
    let meshed = control.status()["meshed_sections"].as_f64().unwrap_or(0.0);
    let generated = control.status()["generated_chunks"].as_f64().unwrap_or(0.0);
    let [x, y, z] = start;
    control.call(&format!("tp {x} {y} {z}"))?;
    let teleported = Instant::now();
    // A zero-length flight holds the player at the start, flying.
    control.call(&format!("fly {x} {y} {z} {x} {y} {z} 0.1 {pitch}"))?;
    control.call(&format!("look {:.3} {pitch}", yaw(start, end)))?;
    let loaded = wait_loaded(control, near(start))?;
    let load = loaded - teleported;
    report.metric(&format!("chunks.{name}.load_ms"), &[ms(load)]);
    let status = control.status();
    let meshes = status["meshed_sections"].as_f64().unwrap_or(0.0) - meshed;
    let chunks = status["generated_chunks"].as_f64().unwrap_or(0.0) - generated;
    report.metric(&format!("chunks.{name}.meshed_count"), &[meshes]);
    report.metric(&format!("chunks.{name}.generated_count"), &[chunks]);
    if load.as_secs_f64() > 0.0 {
        report.metric(
            &format!("chunks.{name}.generated_per_s"),
            &[chunks / load.as_secs_f64()],
        );
    }
    Ok(())
}

/// Whether a status position is within 2 blocks of `target`.
fn near(target: [f64; 3]) -> impl Fn(&Value) -> bool {
    move |p| {
        let p: Vec<f64> = p
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        p.len() == 3 && (0..3).map(|i| (p[i] - target[i]).powi(2)).sum::<f64>() < 4.0
    }
}

/// Flies the camera from `start` to `end` and returns the flight's frames.
fn fly(
    control: &Control,
    phase: &str,
    start: [f64; 3],
    end: [f64; 3],
    seconds: f64,
    pitch: f64,
    steady: &mut Vec<(Instant, Instant)>,
) -> Result<Frames, String> {
    let ([x, y, z], [ex, ey, ez]) = (start, end);
    let begin = Instant::now();
    let flight = control.call(&format!(
        "fly {x} {y} {z} {ex} {ey} {ez} {seconds} {pitch} raw"
    ))?;
    steady.push((begin, Instant::now()));
    frames_of(&flight, phase, 0)
}

/// The frames of a `fly ... raw` or `bench ... raw` reply.
fn frames_of(reply: &Value, phase: &str, repeat: u32) -> Result<Frames, String> {
    let series = |key: &str| -> Vec<f32> {
        reply[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .map(|v| v as f32)
            .collect()
    };
    Frames::of(
        phase,
        repeat,
        &series("frame_ms"),
        &series("cpu_ms"),
        &series("gpu_ms"),
    )
    .ok_or_else(|| format!("{phase} drew no frames"))
}

/// The phase's frame figures over its blocks (one for a flight).
fn fps_metrics(report: &mut Report, phase: &str, blocks: &[Frames]) {
    let each = |f: fn(&Frames) -> f64| blocks.iter().map(f).collect::<Vec<_>>();
    report.metric(&format!("fps.{phase}.avg_fps"), &each(|b| b.avg_fps));
    report.metric(
        &format!("fps.{phase}.low_1pct_fps"),
        &each(|b| b.low_1pct_fps),
    );
    report.metric(
        &format!("fps.{phase}.low_01pct_fps"),
        &each(|b| b.low_01pct_fps),
    );
}

/// The median CPU and GPU time of a frame over the phase's blocks, where measured.
fn timing_metrics(report: &mut Report, phase: &str, blocks: &[Frames]) {
    let cpu: Vec<f64> = blocks.iter().filter_map(|b| b.cpu_p50_ms).collect();
    let gpu: Vec<f64> = blocks.iter().filter_map(|b| b.gpu_p50_ms).collect();
    report.metric(&format!("cpu.{phase}.p50_ms"), &cpu);
    report.metric(&format!("gpu.{phase}.p50_ms"), &gpu);
}

/// The flight, then the view at its end loaded.
fn trip(
    control: &Control,
    report: &mut Report,
    steady: &mut Vec<(Instant, Instant)>,
) -> Result<(), String> {
    for line in [
        "command gamerule advance_time false",
        "command gamerule advance_weather false",
        "command weather clear",
        "time 6000",
    ] {
        control.call(line)?;
    }
    let (from, to) = (TRAVEL_FROM, TRAVEL_TO);
    load_scene(control, "travel", from, to, TRAVEL_PITCH, report)?;
    let flight = fly(
        control,
        "travel",
        from,
        to,
        TRAVEL_SECONDS,
        TRAVEL_PITCH,
        steady,
    )?;
    let blocks = std::slice::from_ref(&flight);
    fps_metrics(report, "travel", blocks);
    timing_metrics(report, "travel", blocks);
    report.frames.push(flight);

    let [x, y, z] = to;
    control.call(&format!("fly {x} {y} {z} {x} {y} {z} 0.1 {TRAVEL_PITCH}"))?;
    control.call(&format!("look {:.3} {TRAVEL_PITCH}", yaw(from, to)))?;
    wait_loaded(control, near(to))?;
    Ok(())
}

/// The state that matters: fly 512 blocks over terrain that loads and unloads, then hold at the end.
fn travel(
    control: &Control,
    report: &mut Report,
    steady: &mut Vec<(Instant, Instant)>,
) -> Result<(), String> {
    trip(control, report, steady)?;
    let mut blocks = Vec::new();
    for repeat in 0..HOLD_BLOCKS {
        let held = control.call(&format!("bench {BLOCK_SECONDS} raw"))?;
        blocks.push(frames_of(&held, "hold", repeat)?);
    }
    fps_metrics(report, "hold", &blocks);
    timing_metrics(report, "hold", &blocks);
    report.frames.extend(blocks);
    Ok(())
}

struct Plan {
    pairs: u32,
    warmup: Duration,
}

/// The two values of a switch in B A A B order, a block each, then each arm's passes timed once.
fn ab_blocks(
    control: &Control,
    switch: &Switch,
    plan: &Plan,
    report: &mut Report,
    steady: &mut Vec<(Instant, Instant)>,
) -> Result<(), String> {
    let began = Instant::now();
    let mut blocks = Vec::new();
    for pair in 0..plan.pairs {
        control.call("time 6000")?;
        for (n, b) in [(0, pair % 2 == 0), (1, pair % 2 == 1)] {
            let (phase, value) = if b {
                ("ab.b", switch.b)
            } else {
                ("ab.a", switch.a)
            };
            control.call(&format!("{} {value}", switch.word))?;
            thread::sleep(plan.warmup);
            let held = control.call(&format!("bench {BLOCK_SECONDS} raw"))?;
            blocks.push(frames_of(&held, phase, 2 * pair + n)?);
        }
    }
    steady.push((began, Instant::now()));
    for phase in ["ab.a", "ab.b"] {
        let arm: Vec<Frames> = blocks
            .iter()
            .filter(|f| f.phase == phase)
            .cloned()
            .collect();
        fps_metrics(report, phase, &arm);
        timing_metrics(report, phase, &arm);
    }
    let ab = Ab::from_frames(
        switch.name,
        switch.a,
        switch.b,
        BLOCK_SECONDS,
        plan.warmup.as_secs_f64(),
        &blocks,
    )?;
    if ab.first_block_outside_range {
        eprintln!(
            "Horizon: benchmark warning: the first B block, measured before any switch, is outside the later B blocks: switching {} may reset what is measured",
            switch.name
        );
    }
    report.frames.extend(blocks);
    report.ab = Some(ab);
    for (scene, value) in [("ab_a", switch.a), ("ab_b", switch.b)] {
        control.call(&format!("{} {value}", switch.word))?;
        time_passes(control, scene, report)?;
    }
    Ok(())
}

/// Removes the previous run's world, never one the benchmark didn't create.
fn replace_world(game_dir: &Path) -> Result<(), String> {
    let world = game_dir.join("saves").join(WORLD);
    if !world.exists() {
        return Ok(());
    }
    if !world.join(MARKER).exists() {
        return Err(format!(
            "{} exists and was not created by the benchmark: rename it",
            world.display()
        ));
    }
    std::fs::remove_dir_all(&world).map_err(|e| e.to_string())
}

fn poll(
    control: &Control,
    limit: Duration,
    ready: impl Fn(&Value) -> bool,
) -> Result<Value, String> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        let status = control.status();
        if ready(&status) {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("not ready after {} s", limit.as_secs()))
}

/// Fewer section meshes than this in a second is the game remeshing blocks that change (random
/// ticks), not loading.
const QUIET_MESHES_PER_S: f64 = 100.0;

/// Waits until the player is spawned where `near` says, nothing is queued, the resident and
/// generated chunk counts have not changed for a second and the last sections are drawn (they wait
/// for their neighbours' light, so after the counts settle); returns when the last of these
/// happened (`tools/perf_bench.py`'s `wait_loaded`). With `status.streaming`, the sections are
/// drawn once no stage has held work for half a second: blocks changing after the load (fluids
/// settling, random ticks) only remesh in bursts. Without it, meshing has to go quiet.
///
/// Right after a teleport (and at spawn) the game reports nothing queued and steady counts for over
/// a second before it starts loading: the counts must change once, or stay full for 5 s, and the
/// resident chunks reach [`view_chunks`], before "steady" means loaded.
fn wait_loaded(control: &Control, near: impl Fn(&Value) -> bool) -> Result<Instant, String> {
    let deadline = Instant::now() + LOAD_LIMIT;
    let (mut last, mut changed, mut meshing) = (None, Instant::now(), Instant::now());
    let mut previous: Option<(Instant, f64)> = None;
    let (mut first, mut moved, began) = (None, false, Instant::now());
    while Instant::now() < deadline {
        let s = control.status();
        let now = Instant::now();
        let position = &s["player"]["position"];
        if position.is_null()
            || !near(position)
            || s["pending_chunks"] != 0
            || s["screen"] == "title"
        {
            (last, changed, previous, meshing) = (None, now, None, now);
        } else {
            let counts = (s["chunks"].as_u64(), s["generated_chunks"].as_u64());
            let resident = counts.0.unwrap_or(0);
            let min_chunks = view_chunks(s["render_distance"].as_u64().unwrap_or(0));
            let first = *first.get_or_insert(counts);
            moved = moved
                || counts != first
                || (min_chunks > 0
                    && resident >= min_chunks
                    && now - began > Duration::from_secs(5));
            if min_chunks > 0 && (!moved || resident < min_chunks) {
                (last, changed, previous, meshing) = (None, now, None, now);
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            if last != Some(counts) {
                (last, changed) = (Some(counts), now);
            }
            let meshed = s["meshed_sections"].as_f64().unwrap_or(0.0);
            let quiet = if let Some(stages) = s["streaming"].as_object() {
                // Protos outside the view distance wait for neighbours that are never generated.
                if stages
                    .iter()
                    .any(|(k, v)| k != "protos" && v.as_u64().unwrap_or(0) > 0)
                {
                    meshing = now;
                }
                Duration::from_millis(500)
            } else {
                if let Some((at, before)) = previous
                    && meshed - before > QUIET_MESHES_PER_S * (now - at).as_secs_f64()
                {
                    meshing = now;
                }
                Duration::from_secs(1)
            };
            previous = Some((now, meshed));
            if now - changed >= Duration::from_secs(1) && now - meshing >= quiet {
                return Ok(changed.max(meshing));
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("not loaded after {} s", LOAD_LIMIT.as_secs()))
}

/// 80% of the view circle (pi x RD^2): the game keeps 637 columns at RD 12 and 3725 at RD 32
/// (`tools/perf_bench.py`'s `view_chunks`).
fn view_chunks(distance: u64) -> u64 {
    (0.8 * std::f64::consts::PI * (distance * distance) as f64) as u64
}

/// Vanilla's yaw facing from `start` to `end`.
fn yaw(start: [f64; 3], end: [f64; 3]) -> f64 {
    (-(end[0] - start[0])).atan2(end[2] - start[2]).to_degrees()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// Samples the process's resident memory four times a second, as `tools/perf_bench.py` does.
struct RssSampler {
    stop: Sender<()>,
    thread: thread::JoinHandle<Vec<(Instant, u64)>>,
}

impl RssSampler {
    fn start(pid: u32) -> Self {
        let (stop, stopped) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut samples = Vec::new();
            while let Err(mpsc::RecvTimeoutError::Timeout) =
                stopped.recv_timeout(Duration::from_millis(250))
            {
                samples.extend(horizon_bench::rss_bytes(pid).map(|b| (Instant::now(), b)));
            }
            samples
        });
        Self { stop, thread }
    }
    fn stop(self) -> Vec<(Instant, u64)> {
        let _ = self.stop.send(());
        self.thread.join().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capture_commands_are_the_ones_the_game_parses() {
        use crate::client_perf::request::{self, ProfileSide, Request, StartOptions};
        let words = |line: &str| request::parse(&line.split(' ').skip(1).collect::<Vec<_>>());
        let start = words(START_CAPTURE).unwrap();
        assert_eq!(start.side, Some(ProfileSide::Client));
        assert_eq!(start.request, Request::Start(StartOptions::default()));
        let path = Path::new("/home/a b/game/benchmarks/game-20261108-travel.json");
        let stop = words(&stop_capture(path)).unwrap();
        assert_eq!(stop.side, Some(ProfileSide::Client));
        assert_eq!(
            stop.request,
            Request::Stop {
                bench: Some(path.display().to_string())
            }
        );
    }

    #[test]
    fn scenarios() {
        let scenes = |name: &str| match Scenario::parse(name) {
            Ok(Scenario::Scenes(scenes)) => scenes,
            _ => panic!("{name} is not scenes"),
        };
        assert_eq!(scenes("standard").len(), 4);
        assert_eq!(scenes("ocean")[0].0, "ocean");
        assert!(matches!(Scenario::parse("travel"), Ok(Scenario::Travel)));
        assert!(matches!(
            Scenario::parse("ab:gpu-culling"),
            Ok(Scenario::Ab(s)) if s.name == "gpu-culling"
        ));
        let Err(e) = Scenario::parse("ab:nope") else {
            panic!("nope is no switch")
        };
        assert!(
            e.contains("gpu-culling") && e.contains("section-draws"),
            "{e}"
        );
        let Err(e) = Scenario::parse("nether") else {
            panic!("nether is no scenario")
        };
        assert!(e.contains("travel") && e.contains("mountain"), "{e}");
    }

    #[test]
    fn a_set_names_a_switch_and_one_of_its_two_values() {
        assert_eq!(
            parse_set("gpu-culling=on"),
            Ok(("gpu-culling".into(), "on".into()))
        );
        assert_eq!(
            parse_set("section-draws=indirect"),
            Ok(("section-draws".into(), "indirect".into()))
        );
        let nope = parse_set("gpu-culling=maybe").unwrap_err();
        assert!(nope.contains("off or on"), "{nope}");
        let unknown = parse_set("fog=on").unwrap_err();
        assert!(unknown.contains("gpu-culling") && unknown.contains("section-draws"));
        assert!(parse_set("gpu-culling").is_err());
    }

    #[test]
    fn view_chunks_matches_perf_bench() {
        assert_eq!(view_chunks(12), 361);
        assert_eq!(view_chunks(32), 2573);
        assert_eq!(view_chunks(0), 0);
    }
    /// The status a fake control answers with: the player is where it should
    /// be, nothing is queued, and the world holds `chunks` resident chunks.
    fn status(chunks: u64) -> String {
        serde_json::json!({
            "player": {"position": [0.0, 64.0, 0.0]},
            "pending_chunks": 0,
            "screen": "playing",
            "render_distance": 12,
            "chunks": chunks,
            "generated_chunks": chunks,
            "meshed_sections": 0,
            "streaming": {},
        })
        .to_string()
    }

    #[test]
    fn the_load_waits_for_the_chunks_a_teleport_starts() {
        // Right after a teleport the game reports steady counts and nothing
        // queued for over a second, before the integrated server has sent the
        // first chunk: the quiet rule alone would call that loaded, and the
        // scene would be measured over an empty view (#1749).
        const LATE: usize = 30;
        let (tx, rx) = mpsc::channel::<Request>();
        let polls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&polls);
        let server = thread::spawn(move || {
            let mut seen = 0;
            while let Ok(request) = rx.recv() {
                if request.line == "status" {
                    let chunks = if seen < LATE { 0 } else { 400 };
                    seen += 1;
                    counted.store(seen, std::sync::atomic::Ordering::SeqCst);
                    let _ = request.reply.send(status(chunks));
                } else {
                    let _ = request.reply.send("{}".to_owned());
                }
            }
        });
        let control = Control(tx);
        wait_loaded(&control, |_| true).expect("the scene loads");
        assert!(
            polls.load(std::sync::atomic::Ordering::SeqCst) > LATE,
            "the load finished before the chunks arrived"
        );
        drop(control);
        let _ = server.join();
    }

    /// The wait before a flight times the passes and turns the timing off again before the
    /// flight; the report gets one metric per pass. Not on Metal.
    #[test]
    fn the_wait_times_the_passes_but_not_the_flight() {
        let time_on = |backend: &str| {
            let (tx, rx) = mpsc::channel::<Request>();
            let server = thread::spawn(move || {
                let mut lines = Vec::new();
                while let Ok(request) = rx.recv() {
                    let reply = if request.line.starts_with("bench ") {
                        serde_json::json!({
                            "frames": 120,
                            "gpu_passes": {"World": 1.5, "World after culling": 0.25, "HUD": 0.01},
                        })
                    } else {
                        serde_json::json!({ "ok": true })
                    };
                    let _ = request.reply.send(reply.to_string());
                    lines.push(request.line);
                }
                lines
            });
            let hardware = horizon_bench::Hardware {
                gpu: Some(horizon_bench::Gpu {
                    backend: backend.into(),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let mut report = Report::with_hardware(Kind::Game, App::default(), hardware);
            let control = Control(tx);
            let timed = time_passes(&control, "forest", &mut report).expect("the wait runs");
            drop(control);
            (timed, server.join().unwrap(), report.metrics)
        };

        let (timed, lines, metrics) = time_on("vulkan");
        assert!(timed);
        assert_eq!(lines, ["gpu-passes on", "bench 2", "gpu-passes off"]);
        let mean = |name: &str| metrics.get(name).map(|s| s.mean);
        assert_eq!(mean("gpu_pass.forest.world_ms"), Some(1.5));
        assert_eq!(mean("gpu_pass.forest.world_after_culling_ms"), Some(0.25));
        assert_eq!(mean("gpu_pass.forest.hud_ms"), Some(0.01));
        assert_eq!(metrics.len(), 3);

        let (timed, lines, metrics) = time_on("metal");
        assert!(!timed);
        assert!(
            lines.is_empty(),
            "Metal's overlapping passes are not timed: {lines:?}"
        );
        assert!(metrics.is_empty());
    }

    /// A fake game for the trip: the player stands where `tp` or `fly` put it and counts settle.
    fn trip_server(rx: mpsc::Receiver<Request>) -> thread::JoinHandle<Vec<String>> {
        thread::spawn(move || {
            let (mut lines, mut at, mut polls) = (Vec::new(), [0.0_f64; 3], 0_u64);
            while let Ok(request) = rx.recv() {
                let words: Vec<&str> = request.line.split(' ').collect();
                let reply = match words[0] {
                    "status" => {
                        polls += 1;
                        let mut s: Value =
                            serde_json::from_str(&status(400 + polls.min(3))).unwrap();
                        s["player"]["position"] = serde_json::json!(at);
                        s
                    }
                    "bench" | "fly" if words.contains(&"raw") => {
                        serde_json::json!({
                            "frames": 200,
                            "frame_ms": vec![5.0; 200],
                            "cpu_ms": vec![2.0; 200],
                            "gpu_ms": vec![4.0; 200],
                        })
                    }
                    _ => serde_json::json!({ "ok": true }),
                };
                if matches!(words[0], "tp" | "fly") {
                    let n: Vec<f64> = words[1..4].iter().filter_map(|w| w.parse().ok()).collect();
                    at = [n[0], n[1], n[2]];
                    polls = 0;
                }
                let _ = request.reply.send(reply.to_string());
                lines.push(request.line);
            }
            lines
        })
    }

    #[test]
    fn the_trip_flies_512_blocks_then_holds_four_blocks_at_its_end() {
        let (tx, rx) = mpsc::channel::<Request>();
        let server = trip_server(rx);
        let mut report = Report::new(Kind::Game, App::default());
        let control = Control(tx);
        let mut steady = Vec::new();
        travel(&control, &mut report, &mut steady).expect("the trip runs");
        drop(control);
        let lines: Vec<String> = server
            .join()
            .unwrap()
            .into_iter()
            .filter(|l| l != "status")
            .map(|l| l.replace("-0.000", "0.000"))
            .collect();
        // Facing +Z is yaw 0, which the formatter may print as -0.
        let mut expected = vec![
            "command gamerule advance_time false",
            "command gamerule advance_weather false",
            "command weather clear",
            "time 6000",
            "tp -960 150 -176",
            "fly -960 150 -176 -960 150 -176 0.1 30",
            "look 0.000 30",
            "fly -960 150 -176 -960 150 336 32 30 raw",
            "fly -960 150 336 -960 150 336 0.1 30",
            "look 0.000 30",
        ];
        expected.extend(["bench 4 raw"; 4]);
        assert_eq!(lines, expected);

        let phases: Vec<_> = report
            .frames
            .iter()
            .map(|f| (f.phase.as_str(), f.repeat))
            .collect();
        assert_eq!(
            phases,
            [
                ("travel", 0),
                ("hold", 0),
                ("hold", 1),
                ("hold", 2),
                ("hold", 3)
            ]
        );
        assert_eq!(steady.len(), 1);
        let metric = |name: &str| report.metrics.get(name).map(|s| (s.n, s.mean));
        assert_eq!(metric("fps.travel.avg_fps"), Some((1, 200.0)));
        assert_eq!(metric("fps.hold.avg_fps"), Some((4, 200.0)));
        assert_eq!(metric("cpu.hold.p50_ms"), Some((4, 2.0)));
        assert_eq!(metric("gpu.hold.p50_ms"), Some((4, 4.0)));
        assert_eq!(metric("gpu.travel.p50_ms"), Some((1, 4.0)));
        assert!(metric("chunks.travel.load_ms").is_some());
    }

    #[test]
    fn a_refused_command_ends_the_trip_with_the_games_reason() {
        let (tx, rx) = mpsc::channel::<Request>();
        let server = thread::spawn(move || {
            while let Ok(request) = rx.recv() {
                let reply = if request.line.starts_with("command weather") {
                    serde_json::json!({ "error": "no such weather" })
                } else {
                    serde_json::json!({ "ok": true })
                };
                let _ = request.reply.send(reply.to_string());
            }
        });
        let mut report = Report::new(Kind::Game, App::default());
        let control = Control(tx);
        let error = travel(&control, &mut report, &mut Vec::new()).unwrap_err();
        assert!(error.contains("no such weather"), "{error}");
        assert!(report.frames.is_empty());
        drop(control);
        server.join().unwrap();
    }

    #[test]
    fn a_refused_switch_is_the_games_reason() {
        let (tx, rx) = mpsc::channel::<Request>();
        let server = thread::spawn(move || {
            let mut lines = Vec::new();
            while let Ok(request) = rx.recv() {
                let reply = match request.line.as_str() {
                    "gpu-culling on" => serde_json::json!({ "error": "not offered here" }),
                    _ => serde_json::json!({ "ok": true }),
                };
                let _ = request.reply.send(reply.to_string());
                lines.push(request.line);
            }
            lines
        });
        let control = Control(tx);
        apply_set(&control, "section-draws", "indirect").unwrap();
        let error = apply_set(&control, "gpu-culling", "on").unwrap_err();
        assert!(error.contains("not offered here"), "{error}");
        drop(control);
        assert_eq!(
            server.join().unwrap(),
            ["section-draws indirect", "gpu-culling on"]
        );
    }

    /// A fake game with GPU culling: `on` is its state; `fail_at` makes that `bench` (counted from 1) fail.
    fn culling_server(
        rx: mpsc::Receiver<Request>,
        mut on: bool,
        offered: bool,
        fail_at: Option<usize>,
    ) -> thread::JoinHandle<(Vec<String>, bool)> {
        thread::spawn(move || {
            let (mut lines, mut benches) = (Vec::new(), 0);
            while let Ok(request) = rx.recv() {
                let reply = match request.line.as_str() {
                    "gpu-culling on" if !offered => {
                        serde_json::json!({ "error": "not offered here" })
                    }
                    "gpu-culling on" | "gpu-culling off" => {
                        on = request.line.ends_with("on");
                        serde_json::json!({ "gpu_culling": on })
                    }
                    "gpu-culling" => serde_json::json!({ "gpu_culling": on }),
                    "bench 4 raw" => {
                        benches += 1;
                        let gpu = if on { 5.0 } else { 3.0 };
                        if Some(benches) == fail_at {
                            serde_json::json!({ "error": "the window closed" })
                        } else {
                            serde_json::json!({
                                "frame_ms": vec![gpu + 1.0; 200],
                                "cpu_ms": vec![1.0; 200],
                                "gpu_ms": vec![gpu; 200],
                            })
                        }
                    }
                    "bench 2" => serde_json::json!({
                        "gpu_passes": {"World": if on { 1.5 } else { 0.5 }},
                    }),
                    _ => serde_json::json!({ "ok": true }),
                };
                let _ = request.reply.send(reply.to_string());
                lines.push(request.line);
            }
            (lines, on)
        })
    }

    const GPU_CULLING: &Switch = &SWITCHES[0];

    fn quick(pairs: u32) -> Plan {
        Plan {
            pairs,
            warmup: Duration::ZERO,
        }
    }

    fn vulkan_report() -> Report {
        let gpu = horizon_bench::Gpu {
            backend: "vulkan".into(),
            ..Default::default()
        };
        let hardware = horizon_bench::Hardware {
            gpu: Some(gpu),
            ..Default::default()
        };
        Report::with_hardware(Kind::Game, App::default(), hardware)
    }

    #[test]
    fn the_blocks_run_b_a_a_b_and_each_arm_is_timed_by_pass() {
        let (tx, rx) = mpsc::channel::<Request>();
        let server = culling_server(rx, false, true, None);
        let mut report = vulkan_report();
        let control = Control(tx);
        keeping_switch(&control, GPU_CULLING, || {
            ab_blocks(
                &control,
                GPU_CULLING,
                &quick(6),
                &mut report,
                &mut Vec::new(),
            )
        })
        .expect("the blocks run");
        drop(control);
        let (lines, on) = server.join().unwrap();
        let arms: String = lines
            .iter()
            .take_while(|l| *l != "gpu-passes on")
            .filter_map(|l| match l.as_str() {
                "gpu-culling on" => Some('B'),
                "gpu-culling off" => Some('A'),
                _ => None,
            })
            .collect();
        assert_eq!(arms, "BAABBAABBAABA");
        assert_eq!(lines.iter().filter(|l| *l == "time 6000").count(), 6);
        assert!(!on, "the initial value is back");
        assert_eq!(lines.last().unwrap(), "gpu-culling off");

        let positions: Vec<_> = report
            .frames
            .iter()
            .map(|f| (f.phase.as_str(), f.repeat))
            .collect();
        assert_eq!(
            positions[..4],
            [("ab.b", 0), ("ab.a", 1), ("ab.a", 2), ("ab.b", 3)]
        );
        let ab = report.ab.as_ref().expect("an A/B run has its result");
        let gpu = &ab.result["gpu_p50_ms"];
        assert_eq!((gpu.pairs, gpu.a, gpu.b, gpu.diff), (6, 3.0, 5.0, 2.0));
        assert_eq!(gpu.verdict, horizon_bench::compare::Verdict::Slower);
        assert!(!ab.first_block_outside_range);
        let metric = |name: &str| report.metrics.get(name).map(|s| (s.n, s.mean));
        assert_eq!(metric("gpu.ab.a.p50_ms"), Some((6, 3.0)));
        assert_eq!(metric("gpu.ab.b.p50_ms"), Some((6, 5.0)));
        assert_eq!(metric("gpu_pass.ab_a.world_ms"), Some((1, 0.5)));
        assert_eq!(metric("gpu_pass.ab_b.world_ms"), Some((1, 1.5)));
    }

    #[test]
    fn a_setting_that_was_on_is_on_again_after_the_run() {
        let (tx, rx) = mpsc::channel::<Request>();
        let server = culling_server(rx, true, true, None);
        let control = Control(tx);
        keeping_switch(&control, GPU_CULLING, || {
            ab_blocks(
                &control,
                GPU_CULLING,
                &quick(2),
                &mut vulkan_report(),
                &mut Vec::new(),
            )
        })
        .expect("the blocks run");
        drop(control);
        assert!(server.join().unwrap().1);
    }

    #[test]
    fn the_setting_is_put_back_when_the_run_fails_midway() {
        let (tx, rx) = mpsc::channel::<Request>();
        // The 3rd block is A: the setting is off when it fails.
        let server = culling_server(rx, true, true, Some(3));
        let control = Control(tx);
        let mut report = vulkan_report();
        let error = keeping_switch(&control, GPU_CULLING, || {
            ab_blocks(
                &control,
                GPU_CULLING,
                &quick(6),
                &mut report,
                &mut Vec::new(),
            )
        })
        .unwrap_err();
        assert!(error.contains("the window closed"), "{error}");
        assert!(report.frames.is_empty() && report.ab.is_none());
        drop(control);
        let (lines, on) = server.join().unwrap();
        assert!(on, "on was the player's setting");
        assert_eq!(lines.last().unwrap(), "gpu-culling on");
    }

    #[test]
    fn a_refused_switch_ends_the_run_with_the_games_reason_and_changes_nothing() {
        let (tx, rx) = mpsc::channel::<Request>();
        let server = culling_server(rx, false, false, None);
        let control = Control(tx);
        let error = keeping_switch(&control, GPU_CULLING, || {
            apply_set(&control, "gpu-culling", "on")
        })
        .unwrap_err();
        assert!(error.contains("not offered here"), "{error}");
        drop(control);
        let (lines, on) = server.join().unwrap();
        assert_eq!(lines, ["gpu-culling", "gpu-culling on", "gpu-culling"]);
        assert!(!on);
    }

    #[test]
    fn yaw_matches_vanilla() {
        // South (+Z) is 0, west (-X) 90, east (+X) -90.
        assert!((yaw([0.0; 3], [0.0, 0.0, 10.0])).abs() < 1e-9);
        assert!((yaw([0.0; 3], [-10.0, 0.0, 0.0]) - 90.0).abs() < 1e-9);
        assert!((yaw([0.0; 3], [10.0, 0.0, 0.0]) + 90.0).abs() < 1e-9);
    }

    #[test]
    fn only_replaces_its_own_world() {
        let dir =
            std::env::temp_dir().join(format!("horizon-benchmark-test-{}", std::process::id()));
        let world = dir.join("saves").join(WORLD);
        std::fs::create_dir_all(&world).unwrap();
        assert!(
            replace_world(&dir).is_err(),
            "a player's world of that name stays"
        );
        assert!(world.exists());
        std::fs::write(world.join(MARKER), "").unwrap();
        replace_world(&dir).unwrap();
        assert!(!world.exists());
        replace_world(&dir).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
