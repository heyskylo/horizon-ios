//! The GPU pass timers' readings as the capture's `gpu` messages (docs/perf-platform.md §3.5); apart from `perf_capture`, which only Linux has.
use horizon_perf_format::Interner;
use horizon_perf_format::v1::{
    GpuBackend, GpuCounter, GpuCounts, GpuDevice, GpuFragmentUnit, GpuPart, GpuPartCounts,
    GpuPartsMode, GpuPass, GpuPasses, GpuSource, Side,
};

use crate::gfx::pack_timing::{
    Counters, FragmentUnit, GpuReadout, PartsMode, PassCounters, Readings,
};

fn backend(backend: wgpu::Backend) -> GpuBackend {
    match backend {
        wgpu::Backend::Vulkan => GpuBackend::Vulkan,
        wgpu::Backend::Metal => GpuBackend::Metal,
        wgpu::Backend::Dx12 => GpuBackend::Dx12,
        wgpu::Backend::Gl => GpuBackend::Gl,
        _ => GpuBackend::Other,
    }
}

fn counts(c: Counters) -> GpuCounts {
    GpuCounts {
        vertex_shader_invocations: c.vertex_shader_invocations,
        clipper_invocations: c.clipper_invocations,
        clipper_primitives_out: c.clipper_primitives_out,
        fragment_shader_invocations: c.fragment_shader_invocations,
    }
}

fn counter(strings: &mut Interner, pass: &str, c: &PassCounters) -> GpuCounter {
    GpuCounter {
        pass: strings.intern(pass),
        total: Some(counts(c.total)),
        pixels: c.pixels,
        parts: c
            .parts
            .iter()
            .map(|(name, part)| GpuPartCounts {
                name: strings.intern(name),
                counts: Some(counts(*part)),
            })
            .collect(),
    }
}

fn device(adapter: &wgpu::AdapterInfo, readings: &Readings) -> GpuDevice {
    let driver = format!("{} {}", adapter.driver, adapter.driver_info);
    GpuDevice {
        adapter: adapter.name.clone(),
        backend: backend(adapter.backend) as i32,
        driver: driver.trim().to_owned(),
        parts_mode: match readings.support.parts {
            PartsMode::InsidePasses => GpuPartsMode::InsidePasses,
            PartsMode::SeparatePasses => GpuPartsMode::SeparatePasses,
        } as i32,
        counters: readings.support.counters,
        fragment_unit: match readings.support.fragment_unit {
            None => GpuFragmentUnit::Unspecified,
            Some(FragmentUnit::Pixel) => GpuFragmentUnit::Pixel,
            Some(FragmentUnit::Quad) => GpuFragmentUnit::Quad,
        } as i32,
    }
}

fn section(
    adapter: &wgpu::AdapterInfo,
    side: Side,
    source: GpuSource,
    readings: &Readings,
    strings: &mut Interner,
) -> GpuPasses {
    let mut passes: Vec<GpuPass> = readings
        .passes
        .iter()
        .map(|(name, ms)| GpuPass {
            name: strings.intern(name),
            ms: Some(*ms),
            parts: Vec::new(),
        })
        .collect();
    for (pass, parts) in &readings.parts {
        let name = strings.intern(pass);
        let at = passes
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| {
                passes.push(GpuPass {
                    name,
                    ms: None,
                    parts: Vec::new(),
                });
                passes.len() - 1
            });
        passes[at].parts = parts
            .iter()
            .map(|(part, ms)| GpuPart {
                name: strings.intern(part),
                ms: *ms,
            })
            .collect();
    }
    GpuPasses {
        side: side as i32,
        device: Some(device(adapter, readings)),
        passes,
        counters: readings
            .counters
            .iter()
            .map(|(pass, c)| counter(strings, pass, c))
            .collect(),
        source: source as i32,
    }
}

/// One `GpuPasses` per timer that has timed something; the caller appends `strings`' new entries first.
pub fn gpu_passes(readout: &GpuReadout, side: Side, strings: &mut Interner) -> Vec<GpuPasses> {
    [
        (GpuSource::Horizon, &readout.own),
        (GpuSource::ShaderPack, &readout.pack),
    ]
    .into_iter()
    .filter_map(|(source, readings)| {
        let readings = readings.as_ref().filter(|r| !r.is_empty())?;
        Some(section(&readout.adapter, side, source, readings, strings))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::pack_timing::DetailSupport;

    fn adapter() -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "Test GPU".into(),
            vendor: 0x10de,
            device: 1,
            device_type: wgpu::DeviceType::DiscreteGpu,
            device_pci_bus_id: String::new(),
            driver: "NVIDIA".into(),
            driver_info: "570.1".into(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        }
    }

    fn support(unit: Option<FragmentUnit>) -> DetailSupport {
        DetailSupport {
            parts: PartsMode::InsidePasses,
            encoder_stamps: true,
            counters: true,
            fragment_unit: unit,
        }
    }

    #[test]
    fn known_medians_parts_and_counters_come_through_with_their_names() {
        let c = |v, f| Counters {
            vertex_shader_invocations: v,
            clipper_invocations: 2_050_000,
            clipper_primitives_out: 1_200_000,
            fragment_shader_invocations: f,
        };
        let own = Readings {
            passes: vec![("World".into(), 1.5), ("HUD".into(), 0.25)],
            parts: vec![("GPU culling".into(), vec![("cull".into(), 0.125)])],
            counters: vec![(
                "World".into(),
                PassCounters {
                    total: c(4_100_000, 6_300_000),
                    pixels: 2_073_600,
                    parts: vec![
                        ("before translucent".into(), c(3_000_000, 5_100_000)),
                        ("translucent terrain".into(), c(1_100_000, 1_200_000)),
                    ],
                },
            )],
            support: support(Some(FragmentUnit::Quad)),
        };
        let readout = GpuReadout {
            adapter: adapter(),
            own: Some(own),
            pack: None,
        };
        let mut strings = Interner::default();
        let got = gpu_passes(&readout, Side::Client, &mut strings);
        assert_eq!(got.len(), 1);
        let g = &got[0];
        assert_eq!((g.side(), g.source()), (Side::Client, GpuSource::Horizon));
        let d = g.device.as_ref().unwrap();
        assert_eq!(d.adapter, "Test GPU");
        assert_eq!(d.backend(), GpuBackend::Vulkan);
        assert_eq!(d.driver, "NVIDIA 570.1");
        assert_eq!(d.parts_mode(), GpuPartsMode::InsidePasses);
        assert!(d.counters);
        assert_eq!(d.fragment_unit(), GpuFragmentUnit::Quad);

        // World, HUD, then the pass known only through its parts.
        let entries = strings.take_new(Side::Client);
        let name = |id: u32| entries.iter().find(|e| e.id == id).unwrap().text.as_str();
        let named: Vec<_> = g.passes.iter().map(|p| name(p.name)).collect();
        assert_eq!(named, ["World", "HUD", "GPU culling"]);
        let shown: Vec<_> = g.passes.iter().map(|p| p.ms).collect();
        assert_eq!(shown, [Some(1.5), Some(0.25), None]);
        assert_eq!(name(g.passes[2].parts[0].name), "cull");
        assert_eq!(g.passes[2].parts[0].ms, 0.125);
        let counted = &g.counters[0];
        assert_eq!(counted.pixels, 2_073_600);
        assert_eq!(
            counted.total.as_ref().unwrap().fragment_shader_invocations,
            6_300_000
        );
        assert_eq!(counted.parts.len(), 2);
        assert_eq!(
            counted.parts[1]
                .counts
                .as_ref()
                .unwrap()
                .vertex_shader_invocations,
            1_100_000
        );
        // The pass's counters and its time name the same string.
        assert_eq!(counted.pass, g.passes[0].name);
    }

    #[test]
    fn a_timer_that_timed_nothing_adds_no_section() {
        let idle = Readings {
            passes: Vec::new(),
            parts: Vec::new(),
            counters: Vec::new(),
            support: support(None),
        };
        let pack = Readings {
            passes: vec![("composite".into(), 0.75)],
            ..idle.clone()
        };
        let readout = GpuReadout {
            adapter: adapter(),
            own: Some(idle),
            pack: Some(pack),
        };
        let got = gpu_passes(&readout, Side::Client, &mut Interner::default());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].source(), GpuSource::ShaderPack);
        assert_eq!(
            got[0].device.as_ref().unwrap().fragment_unit(),
            GpuFragmentUnit::Unspecified
        );
    }
}
