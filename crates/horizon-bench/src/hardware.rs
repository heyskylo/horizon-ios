//! The machine a report was measured on. CPU, RAM and OS are detected here; the GPU and the display
//! come from the program that rendered (the game knows its adapter, the launcher its window).

#[cfg(any(target_os = "macos", target_os = "linux"))]
use horizon_system::command_output as output;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    /// `macos`, `linux`, `windows`.
    pub os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    pub arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_threads: Option<usize>,
    /// Physical cores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u16>,
    /// Base and top boost clock, where the OS gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_base_mhz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_boost_mhz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ram_mb: Option<u64>,
    /// The drive under the game folder: `nvme`, `ssd` or `hdd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
    /// `laptop` or `desktop`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Gpu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    pub vendor: String,
    pub driver: String,
    /// `metal`, `vulkan`, `dx12`, `gl`.
    pub backend: String,
    /// `integrated`, `discrete`, `virtual`, `cpu`, `other`.
    pub kind: String,
    /// Its own memory in MiB (rounded to 256), for a discrete GPU whose driver says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_mb: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Display {
    /// Physical pixels.
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_hz: Option<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Hardware {
    pub fn detect() -> Hardware {
        let probes = probes();
        Hardware {
            os: std::env::consts::OS.to_owned(),
            os_version: os_version(),
            arch: std::env::consts::ARCH.to_owned(),
            cpu: horizon_system::cpu(),
            cpu_threads: std::thread::available_parallelism().ok().map(|n| n.get()),
            cpu_cores: probes.cores,
            cpu_base_mhz: probes.base,
            cpu_boost_mhz: probes.boost,
            ram_mb: horizon_system::ram_bytes().map(|b| b >> 20),
            storage: None,
            form: probes.form,
            gpu: None,
            display: None,
        }
    }

    /// [`Hardware::detect`] with the kind of drive `game_dir` is on.
    pub fn detect_in(game_dir: &std::path::Path) -> Hardware {
        Hardware {
            storage: horizon_system::storage(game_dir).map(str::to_owned),
            ..Hardware::detect()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// What `horizon_system::Machine` reads.
struct Probes {
    cores: Option<u16>,
    base: Option<u32>,
    boost: Option<u32>,
    form: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
fn probes() -> Probes {
    let s = horizon_system::Machine::detect();
    let some = |n: u32| (n > 0).then_some(n);
    Probes {
        cores: (s.cores > 0).then_some(s.cores),
        base: some(s.cpu_base_mhz),
        boost: some(s.cpu_boost_mhz),
        form: (!s.form.is_empty()).then(|| s.form.to_owned()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// `ac` or `battery`; `None` where it can't be read (and on machines without a battery on Linux).
pub fn power_source() -> Option<&'static str> {
    horizon_system::power()
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(target_os = "macos")]
fn os_version() -> Option<String> {
    output("sw_vers", &["-productVersion"])
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(target_os = "linux")]
fn os_version() -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    let name = key_value(&release, "PRETTY_NAME", '=')?
        .trim_matches('"')
        .to_owned();
    Some(match output("uname", &["-r"]) {
        Some(kernel) => format!("{name} (kernel {kernel})"),
        None => name,
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(target_os = "linux")]
fn key_value<'a>(text: &'a str, key: &str, sep: char) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once(sep)?;
        (k.trim() == key).then(|| v.trim())
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(target_os = "windows")]
fn os_version() -> Option<String> {
    horizon_system::windows_version()
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn os_version() -> Option<String> {
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn detects_this_machine() {
        let hw = Hardware::detect();
        assert_eq!(hw.os, std::env::consts::OS);
        assert!(hw.cpu_threads.unwrap() >= 1);
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(hw.cpu.is_some(), "{hw:?}");
            assert!(hw.ram_mb.unwrap() > 256, "{hw:?}");
            assert!(hw.os_version.is_some(), "{hw:?}");
        }
    }

    /// `hardware.cpu` is the brand name, not the family and model `PROCESSOR_IDENTIFIER` holds.
    #[cfg(windows)]
    #[test]
    fn windows_reports_the_cpu_brand() {
        let cpu = Hardware::detect().cpu.expect("a CPU name");
        assert!(
            !cpu.is_empty() && !cpu.contains(" Family ") && !cpu.contains("Stepping"),
            "family and model, not a name: {cpu:?}"
        );
        assert_eq!(cpu, cpu.trim());
    }
}
