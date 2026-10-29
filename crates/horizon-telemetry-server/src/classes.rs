//! The hardware classes the public output breaks results down by. Coarse on purpose: a class
//! holds many machines, and a published cell still needs `MIN_INSTALLS` different installs.
//! The website's `scripts/benchmarks.mjs` (horizon-rust/horizon-site) has the same functions, tested on the same fixtures.
//!
//! The raw values (the exact memory, the driver, the screen) stay in the database for the
//! retention period and are never published.
use horizon_telemetry::payload::System;
use serde_json::Value;

/// The dimensions, in the order [`of_report`] and [`of_system`] return them.
pub const DIMENSIONS: [&str; 5] = ["ram", "vram", "storage", "os", "gpu_kind"];

pub type Classes = [Option<&'static str>; 5];

/// Installed memory in GB (a little under the size on the box: some is reserved).
pub fn ram(gb: f64) -> Option<&'static str> {
    Some(match gb {
        g if g.is_nan() || g <= 0.0 => return None,
        g if g <= 5.0 => "4 GB or less",
        g if g <= 10.0 => "8 GB",
        g if g <= 20.0 => "16 GB",
        g if g <= 40.0 => "32 GB",
        _ => "64 GB or more",
    })
}

/// A GPU's own memory in MiB; an integrated GPU shares the system's.
pub fn vram(mb: f64, kind: &str) -> Option<&'static str> {
    if gpu_kind(kind) == Some("integrated") {
        return Some("shared");
    }
    Some(match mb {
        m if m.is_nan() || m <= 0.0 => return None,
        m if m <= 2560.0 => "2 GB or less",
        m if m <= 4608.0 => "4 GB",
        m if m <= 6656.0 => "6 GB",
        m if m <= 8704.0 => "8 GB",
        m if m <= 12800.0 => "12 GB",
        m if m <= 16896.0 => "16 GB",
        _ => "more than 16 GB",
    })
}

pub fn storage(s: &str) -> Option<&'static str> {
    ["nvme", "ssd", "hdd"].into_iter().find(|k| *k == s)
}

pub fn os(s: &str) -> Option<&'static str> {
    ["linux", "macos", "windows"].into_iter().find(|k| *k == s)
}

/// `integrated` or `discrete`, from either wording (`IntegratedGpu`, as wgpu says it, too).
pub fn gpu_kind(s: &str) -> Option<&'static str> {
    match s {
        "integrated" | "IntegratedGpu" => Some("integrated"),
        "discrete" | "DiscreteGpu" => Some("discrete"),
        _ => None,
    }
}

/// A benchmark report's classes (`docs/benchmark-report.md`'s `hardware`).
pub fn of_report(report: &Value) -> Classes {
    let hw = &report["hardware"];
    let kind = hw["gpu"]["kind"].as_str().unwrap_or("");
    [
        hw["ram_mb"].as_f64().and_then(|mb| ram(mb / 1024.0)),
        vram(hw["gpu"]["vram_mb"].as_f64().unwrap_or(0.0), kind),
        storage(hw["storage"].as_str().unwrap_or("")),
        os(hw["os"].as_str().unwrap_or("")),
        gpu_kind(kind),
    ]
}

/// A telemetry item's machine's classes.
pub fn of_system(s: &System) -> Classes {
    [
        ram(f64::from(s.ram_gb)),
        vram(f64::from(s.vram_mb), &s.gpu_kind),
        storage(&s.storage),
        os(&s.os),
        gpu_kind(&s.gpu_kind),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_are_coarse() {
        assert_eq!(ram(15.6), Some("16 GB"));
        assert_eq!(ram(31.0), Some("32 GB"));
        assert_eq!(ram(64.0), Some("64 GB or more"));
        assert_eq!(ram(0.0), None);
        assert_eq!(vram(16376.0, "discrete"), Some("16 GB"));
        assert_eq!(vram(8192.0, "DiscreteGpu"), Some("8 GB"));
        assert_eq!(vram(0.0, "integrated"), Some("shared"));
        assert_eq!(vram(0.0, ""), None);
        assert_eq!(storage("nvme"), Some("nvme"));
        assert_eq!(storage("tape"), None);
        assert_eq!(gpu_kind("virtual"), None);
    }
}
