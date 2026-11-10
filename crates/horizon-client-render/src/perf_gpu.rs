//! The GPU pass timers' readings as the capture's `gpu` messages (docs/perf-platform.md §3.5); apart from `perf_capture`, which only Linux has.
use horizon_perf_format::Interner;
use horizon_perf_format::v1::{
    GpuBackend, GpuCounter, GpuCounts, GpuDevice, GpuFragmentUnit, GpuFrame, GpuFrames, GpuPart,
    GpuPartCounts, GpuPartsMode, GpuPass, GpuPassSplit, GpuPasses, GpuSource, Side,
};
use std::time::Instant;

use crate::gfx::pack_timing::{
    Counters, FragmentUnit, GpuReadout, PartsMode, PassCounters, Readings, TimedFrame, TimedFrames,
};

/// Every time one timer's frames gave a pass or a part during a capture.
#[derive(Default)]
struct Times {
    passes: Vec<(String, Vec<f32>)>,
    parts: Vec<(String, Vec<(String, Vec<f32>)>)>,
}

fn times_of<'a>(list: &'a mut Vec<(String, Vec<f32>)>, name: &str) -> &'a mut Vec<f32> {
    let i = match list.iter().position(|e| e.0 == name) {
        Some(i) => i,
        None => {
            list.push((name.to_owned(), Vec::new()));
            list.len() - 1
        }
    };
    &mut list[i].1
}

impl Times {
    fn add(&mut self, frame: &TimedFrame) {
        for (pass, ms) in &frame.passes {
            times_of(&mut self.passes, pass).push(*ms);
        }
        for (pass, parts) in &frame.parts {
            let i = match self.parts.iter().position(|e| e.0 == *pass) {
                Some(i) => i,
                None => {
                    self.parts.push((pass.clone(), Vec::new()));
                    self.parts.len() - 1
                }
            };
            for (part, ms) in parts {
                times_of(&mut self.parts[i].1, part).push(*ms);
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.passes.is_empty() && self.parts.is_empty()
    }

    /// `readings` with its times the medians over the whole capture instead of its last frames.
    fn medians_into(&self, readings: &Readings) -> Readings {
        let median = |v: &[f32]| {
            let mut v = v.to_vec();
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        Readings {
            passes: self
                .passes
                .iter()
                .map(|(name, v)| (name.clone(), median(v)))
                .collect(),
            parts: self
                .parts
                .iter()
                .map(|(pass, parts)| {
                    let parts = parts.iter().map(|(n, v)| (n.clone(), median(v))).collect();
                    (pass.clone(), parts)
                })
                .collect(),
            ..readings.clone()
        }
    }
}

/// A capture's timed frames on their way into `GpuFrames` blocks, every time kept for the whole capture's medians.
#[derive(Default)]
pub struct FrameLog {
    pending: Vec<GpuFrame>,
    cpu_frame_ms: Vec<f32>,
    own: Times,
    pack: Times,
}

impl FrameLog {
    /// The CPU's own ms of one frame the window drew.
    pub fn cpu_frame(&mut self, ms: f32) {
        self.cpu_frame_ms.push(ms);
    }

    /// The frames timed since the last call; `t_of` puts a frame's start on the capture clock.
    pub fn add(
        &mut self,
        frames: &TimedFrames,
        t_of: impl Fn(Instant) -> f64,
        strings: &mut Interner,
    ) {
        for (source, list, times) in [
            (GpuSource::Horizon, &frames.own, &mut self.own),
            (GpuSource::ShaderPack, &frames.pack, &mut self.pack),
        ] {
            for frame in list {
                times.add(frame);
                self.pending
                    .push(frame_message(frame, source, t_of(frame.at), strings));
            }
        }
    }

    /// The frames and CPU frame times since the last block; none when there are neither.
    pub fn take_block(&mut self, side: Side) -> Option<GpuFrames> {
        if self.pending.is_empty() && self.cpu_frame_ms.is_empty() {
            return None;
        }
        Some(GpuFrames {
            side: side as i32,
            frames: std::mem::take(&mut self.pending),
            cpu_frame_ms: std::mem::take(&mut self.cpu_frame_ms),
        })
    }
}

fn frame_message(
    frame: &TimedFrame,
    source: GpuSource,
    t: f64,
    strings: &mut Interner,
) -> GpuFrame {
    let mut passes: Vec<GpuPass> = frame
        .passes
        .iter()
        .map(|(name, ms)| GpuPass {
            name: strings.intern(name),
            ms: Some(*ms),
            parts: Vec::new(),
        })
        .collect();
    for (pass, parts) in &frame.parts {
        let parts = parts
            .iter()
            .map(|(part, ms)| GpuPart {
                name: strings.intern(part),
                ms: *ms,
            })
            .collect();
        let name = strings.intern(pass);
        match passes.iter_mut().find(|p| p.name == name) {
            Some(p) => p.parts = parts,
            None => passes.push(GpuPass {
                name,
                ms: None,
                parts,
            }),
        }
    }
    GpuFrame {
        source: source as i32,
        t,
        total_ms: frame.total_ms,
        passes,
    }
}

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
        pass_split: match readings.support.parts {
            PartsMode::InsidePasses => GpuPassSplit::DrawGroups,
            PartsMode::SeparatePasses => GpuPassSplit::PassesOnly,
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

/// One `GpuPasses` per timer that has timed something, its times the medians of `log` where it has them; the caller appends `strings`' new entries first.
pub fn gpu_passes(
    readout: &GpuReadout,
    log: &FrameLog,
    side: Side,
    strings: &mut Interner,
) -> Vec<GpuPasses> {
    [
        (GpuSource::Horizon, &readout.own, &log.own),
        (GpuSource::ShaderPack, &readout.pack, &log.pack),
    ]
    .into_iter()
    .filter_map(|(source, readings, times)| {
        let readings = readings.as_ref().filter(|r| !r.is_empty())?;
        let whole = (!times.is_empty()).then(|| times.medians_into(readings));
        let readings = whole.as_ref().unwrap_or(readings);
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
        let got = gpu_passes(&readout, &FrameLog::default(), Side::Client, &mut strings);
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
        let got = gpu_passes(
            &readout,
            &FrameLog::default(),
            Side::Client,
            &mut Interner::default(),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].source(), GpuSource::ShaderPack);
        assert_eq!(
            got[0].device.as_ref().unwrap().fragment_unit(),
            GpuFragmentUnit::Unspecified
        );
    }

    #[test]
    fn the_end_section_takes_the_whole_capture_median_and_each_block_its_frames() {
        let start = Instant::now();
        let frame = |i: u32, world: f32| TimedFrame {
            at: start + std::time::Duration::from_millis(u64::from(i) * 10),
            total_ms: Some(world + 1.0),
            passes: vec![("World".into(), world), ("HUD".into(), 0.5)],
            parts: vec![("World".into(), vec![("sky".into(), world / 4.0)])],
        };
        let mut log = FrameLog::default();
        let mut strings = Interner::default();
        let worlds = [1.0, 2.0, 3.0, 4.0, 100.0];
        let frames = TimedFrames {
            own: worlds
                .iter()
                .enumerate()
                .map(|(i, &w)| frame(i as u32, w))
                .collect(),
            pack: Vec::new(),
        };
        log.add(
            &frames,
            |at| at.duration_since(start).as_secs_f64(),
            &mut strings,
        );
        log.cpu_frame(6.0);
        let block = log.take_block(Side::Client).expect("a block");
        assert_eq!(block.frames.len(), 5);
        assert_eq!(block.cpu_frame_ms, [6.0]);
        assert!((block.frames[4].t - 0.04).abs() < 1e-9);
        assert_eq!(block.frames[4].total_ms, Some(101.0));
        assert_eq!(block.frames[4].passes[0].parts[0].ms, 25.0);
        assert!(log.take_block(Side::Client).is_none());

        // The timer's own median (9.0) is of its last frames; the capture's is the third of five sorted.
        let own = Readings {
            passes: vec![("World".into(), 9.0)],
            parts: Vec::new(),
            counters: Vec::new(),
            support: support(None),
        };
        let readout = GpuReadout {
            adapter: adapter(),
            own: Some(own),
            pack: None,
        };
        let got = gpu_passes(&readout, &log, Side::Client, &mut strings);
        let world = &got[0].passes[0];
        assert_eq!(world.ms, Some(3.0));
        assert_eq!(world.parts[0].ms, 0.75);
        assert_eq!(
            got[0].device.as_ref().unwrap().pass_split(),
            GpuPassSplit::DrawGroups
        );
    }
}
