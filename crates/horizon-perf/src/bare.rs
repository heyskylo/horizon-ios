//! A bare `benchmarks/*.json` read as a one-sided client capture, so every command that takes a capture takes a report.

use horizon_bench::Report;
use horizon_perf_format::FORMAT as CAPTURE_FORMAT;
use horizon_perf_format::v1::{
    BenchResult, Build, Capture, Kind, Machine, Side, Start, Stop, StopReason, Trigger,
};

/// The capture a report stands for: its start (build and machine), the report itself in `bench`, and a stop at the seconds it measured.
pub fn capture_of(bytes: &[u8]) -> Result<Capture, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let report = Report::parse(text)?;
    let side = Side::Client as i32;
    let hardware = &report.hardware;
    let start = Start {
        side,
        kind: Kind::Client as i32,
        trigger: Trigger::Benchmark as i32,
        build: Some(Build {
            version: report.app.version.clone(),
            commit: report.app.commit.clone().unwrap_or_default(),
            ..Build::default()
        }),
        machine: Some(Machine {
            os: hardware.os.clone(),
            os_version: hardware.os_version.clone().unwrap_or_default(),
            arch: hardware.arch.clone(),
            cpu: hardware.cpu.clone().unwrap_or_default(),
            cpu_threads: hardware.cpu_threads.unwrap_or(0) as u32,
            cpu_cores: u32::from(hardware.cpu_cores.unwrap_or(0)),
            ram_mb: hardware.ram_mb.unwrap_or(0),
            gpu: hardware
                .gpu
                .as_ref()
                .map(|g| g.name.clone())
                .unwrap_or_default(),
            ..Machine::default()
        }),
        ..Start::default()
    };
    let seconds: f64 = report.frames.iter().map(|f| f.seconds).sum();
    Ok(Capture {
        format: CAPTURE_FORMAT,
        start: vec![start],
        bench: vec![BenchResult {
            side,
            format: report.format.clone(),
            report_json: text.to_string(),
            corpus_id: String::new(),
        }],
        stop: vec![Stop {
            side,
            t: seconds,
            reason: StopReason::Normal as i32,
            overhead: None,
        }],
        ..Capture::default()
    })
}
