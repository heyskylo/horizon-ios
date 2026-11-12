//! GPU time of each pass of a shader pack, of either kind, and of Horizon's own passes: shown in
//! the F3 overlay while it is open, and recorded in a capture made with `--gpu`. One frame
//! at a time is timed, and the CPU never waits for it: its timestamps are resolved in a later
//! frame, once the GPU has finished it (Metal resolves them without waiting for the passes that
//! write them), then read back when mapped. The next frame after that is timed again, once `gap`
//! has passed since the last.

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// Timestamp pairs per frame at most: a pass, a part or an encoder span takes one.
const CAPACITY: u32 = 128;
const TIMESTAMP_BYTES: u64 = CAPACITY as u64 * 16;
const _: () = assert!(TIMESTAMP_BYTES.is_multiple_of(wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT));
/// Pipeline statistics queries per frame at most: a counted pass or a segment of one takes one.
const STATS_CAPACITY: u32 = 32;
const STAT_FLAGS: wgpu::PipelineStatisticsTypes =
    wgpu::PipelineStatisticsTypes::VERTEX_SHADER_INVOCATIONS
        .union(wgpu::PipelineStatisticsTypes::CLIPPER_INVOCATIONS)
        .union(wgpu::PipelineStatisticsTypes::CLIPPER_PRIMITIVES_OUT)
        .union(wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS);
const STAT_BYTES: u64 = 32;
const BUFFER_BYTES: u64 = TIMESTAMP_BYTES + STATS_CAPACITY as u64 * STAT_BYTES;
/// Timed frames each pass's median is taken over.
const WINDOW: usize = 15;
/// Timed frames kept for `take_frames` while nobody takes them.
const FRAME_LOG: usize = 4096;

/// How a pass's parts are timed (`compute_parts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartsMode {
    /// One compute pass, a timestamp pair around each part.
    InsidePasses,
    /// A compute pass per part with pass-boundary stamps (adds boundaries).
    SeparatePasses,
}

/// What `fragment_shader_invocations` counts on a device: pixels (NVIDIA) or shaded 2x2 quads (RADV).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentUnit {
    Pixel,
    Quad,
}

impl FragmentUnit {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pixel => "pixel",
            Self::Quad => "quad",
        }
    }

    /// The unit a calibration draw of `pixels` fragments shows, or none when `count` fits neither (within 12.5%).
    fn of_calibration(count: u64, pixels: u64) -> Option<Self> {
        let near = |scaled: u64| (7 * pixels..=9 * pixels).contains(&scaled);
        if near(8 * count) {
            Some(Self::Pixel)
        } else if near(32 * count) {
            Some(Self::Quad)
        } else {
            None
        }
    }

    /// The pixels `count` fragment invocations stand for.
    pub fn pixels_of(self, count: u64) -> u64 {
        match self {
            Self::Pixel => count,
            Self::Quad => count * 4,
        }
    }
}

/// What detail timing can show on this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetailSupport {
    pub parts: PartsMode,
    pub encoder_stamps: bool,
    pub counters: bool,
    /// What the fragment counter counts, from a calibration draw; none until it ran or when it fit neither unit.
    pub fragment_unit: Option<FragmentUnit>,
}

/// What a timer shows at one moment, as plain data: the medians of its last timed frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Readings {
    pub passes: Vec<(String, f32)>,
    pub parts: Vec<(String, Vec<(String, f32)>)>,
    pub counters: Vec<(String, PassCounters)>,
    pub support: DetailSupport,
}

impl Readings {
    /// Whether the timer has shown nothing yet.
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty() && self.parts.is_empty() && self.counters.is_empty()
    }
}

/// One timed frame: its passes' and parts' ms, a name's occurrences in the frame summed.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedFrame {
    /// When its commands began recording.
    pub at: Instant,
    /// From its first timestamp to its last.
    pub total_ms: Option<f32>,
    pub passes: Vec<(String, f32)>,
    pub parts: Vec<(String, Vec<(String, f32)>)>,
}

/// Both timers' frames timed since they were last taken.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimedFrames {
    pub own: Vec<TimedFrame>,
    pub pack: Vec<TimedFrame>,
}

/// The GPU's pass timings for a capture, with the adapter they were taken on.
#[derive(Clone, Debug)]
pub struct GpuReadout {
    pub adapter: wgpu::AdapterInfo,
    /// Horizon's own passes.
    pub own: Option<Readings>,
    /// The running shader pack's passes.
    pub pack: Option<Readings>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Plain,
    Parts,
    /// Counts instead of timing: its times are thrown away.
    Counters,
}

/// What a counted pass or a segment of one shaded and sent through the pipeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub vertex_shader_invocations: u64,
    pub clipper_invocations: u64,
    pub clipper_primitives_out: u64,
    /// Pixels on NVIDIA, 2x2 quads on RADV.
    pub fragment_shader_invocations: u64,
}

/// A counted pass: its totals, the pixels of its depth attachment, and its segments when it has two or more.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PassCounters {
    pub total: Counters,
    pub pixels: u64,
    pub parts: Vec<(String, Counters)>,
}

/// One statistics query: a stretch of the pass `owner`.
struct Segment {
    owner: usize,
    part: String,
    pixels: u64,
}

struct CounterSamples {
    pass: String,
    pixels: u64,
    total: VecDeque<Counters>,
    parts: Vec<(String, VecDeque<Counters>)>,
}

/// A timed span: a pass, or a part of one.
struct Entry {
    name: String,
    /// The entry this one is a part of.
    parent: Option<usize>,
    /// Timestamp pair (queries `2s` and `2s + 1`); none for a parent made of separate passes.
    stamp: Option<u32>,
}

/// A closure that records one part of a compute pass.
pub type Part<'f> = (&'static str, &'f mut dyn FnMut(&mut wgpu::ComputePass<'_>));

enum Stage {
    /// The next frame is timed.
    Idle,
    /// The frame being recorded is timed.
    Recording,
    /// Submitted; the flag is set once the GPU has finished it.
    Running(Arc<AtomicBool>),
    /// Resolved and copied; the flag is set once the copy is mapped.
    Reading(Arc<AtomicBool>, u64),
}

pub struct PassTiming {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f32,
    stage: Stage,
    /// The spans of the timed frame, in order.
    entries: Vec<Entry>,
    /// Timestamp pairs the timed frame has used.
    stamps_used: u32,
    /// The entry the last `writes` or `compute_writes` made.
    last_pass: Option<usize>,
    parts_mode: PartsMode,
    encoder_stamps: bool,
    counters: bool,
    fragment_unit: Option<FragmentUnit>,
    want_detail: bool,
    kind: FrameKind,
    /// Created by the first `set_detail(true)` on a device with pipeline statistics.
    stats: Option<wgpu::QuerySet>,
    /// The statistics queries of the timed frame, in query order.
    segments: Vec<Segment>,
    /// Each counted pass's last `WINDOW` counters frames.
    counter_samples: Vec<CounterSamples>,
    /// Each pass's last `WINDOW` times, in milliseconds.
    samples: Vec<(String, VecDeque<f32>)>,
    /// Each part's last `WINDOW` times, by (pass, part).
    part_samples: Vec<(String, String, VecDeque<f32>)>,
    /// `mark`'s pipeline, for the target format it was made for.
    marker: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    /// Least time between timed frames.
    pub gap: Duration,
    /// When the last timed frame began.
    timed_at: Option<Instant>,
    /// Whether timed frames are kept for `take_frames`.
    keep_frames: bool,
    frames: VecDeque<TimedFrame>,
}

impl PassTiming {
    /// `None` when the device can't time passes.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let has = device.features();
        if !has.contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let size = BUFFER_BYTES;
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("Shader pack pass timing"),
                ty: wgpu::QueryType::Timestamp,
                count: CAPACITY * 2,
            }),
            resolve: buffer(
                "Shader pack timing resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            read: buffer(
                "Shader pack timing read",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            period_ns: queue.get_timestamp_period(),
            stage: Stage::Idle,
            entries: Vec::new(),
            stamps_used: 0,
            last_pass: None,
            parts_mode: if has.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES) {
                PartsMode::InsidePasses
            } else {
                PartsMode::SeparatePasses
            },
            encoder_stamps: has.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            counters: has.contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
            fragment_unit: None,
            want_detail: false,
            kind: FrameKind::Plain,
            stats: None,
            segments: Vec::new(),
            counter_samples: Vec::new(),
            samples: Vec::new(),
            part_samples: Vec::new(),
            marker: None,
            gap: Duration::ZERO,
            timed_at: None,
            keep_frames: false,
            frames: VecDeque::new(),
        })
    }

    /// The features detail timing wants that the adapter has.
    pub fn features(adapter: &wgpu::Adapter) -> wgpu::Features {
        adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                | wgpu::Features::PIPELINE_STATISTICS_QUERY)
    }

    /// A timer that records parts the given way.
    #[cfg(test)]
    pub fn with_parts_mode(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mode: PartsMode,
    ) -> Option<Self> {
        let mut timing = Self::new(device, queue)?;
        timing.parts_mode = mode;
        Some(timing)
    }

    pub fn detail_support(&self) -> DetailSupport {
        DetailSupport {
            parts: self.parts_mode,
            encoder_stamps: self.encoder_stamps,
            counters: self.counters,
            fragment_unit: self.fragment_unit,
        }
    }

    /// Asks for dispatch parts; a frame already recording keeps its layout.
    pub fn set_detail(&mut self, on: bool) {
        if !on {
            self.part_samples.clear();
            self.counter_samples.clear();
        }
        self.want_detail = on;
    }

    /// Whether the frame being recorded records parts.
    pub fn parts_frame(&self) -> bool {
        self.recording() && self.kind == FrameKind::Parts
    }

    /// Whether the frame being recorded counts.
    pub fn counters_frame(&self) -> bool {
        self.recording() && self.kind == FrameKind::Counters
    }

    /// Whether the frame being recorded stamps parts inside its passes (any timed frame, where the device can).
    fn in_pass_parts(&self) -> bool {
        self.recording()
            && self.kind != FrameKind::Counters
            && self.parts_mode == PartsMode::InsidePasses
    }

    /// Keeps every timed frame for `take_frames` while `on`; off drops those kept.
    pub fn keep_frames(&mut self, on: bool) {
        if !on {
            self.frames.clear();
        }
        self.keep_frames = on;
    }

    /// The frames timed since the last call, oldest first.
    pub fn take_frames(&mut self) -> Vec<TimedFrame> {
        self.frames.drain(..).collect()
    }

    /// At the start of a frame's passes: moves the last timed frame along, and decides whether
    /// this one is timed.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        match &self.stage {
            // A timed frame that was never submitted: time this one instead.
            Stage::Recording => self.stage = Stage::Idle,
            Stage::Running(done) if done.load(Ordering::Acquire) => {
                let mut bytes = u64::from(self.stamps_used) * 16;
                let mut encoder = device.create_command_encoder(&Default::default());
                encoder.resolve_query_set(&self.queries, 0..self.stamps_used * 2, &self.resolve, 0);
                let stats_used = self.segments.len() as u32;
                if let Some(stats) = self.stats.as_ref().filter(|_| stats_used > 0) {
                    encoder.resolve_query_set(stats, 0..stats_used, &self.resolve, TIMESTAMP_BYTES);
                    bytes = TIMESTAMP_BYTES + u64::from(stats_used) * STAT_BYTES;
                }
                encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, bytes);
                queue.submit([encoder.finish()]);
                let mapped = Arc::new(AtomicBool::new(false));
                let flag = mapped.clone();
                self.read
                    .slice(..bytes)
                    .map_async(wgpu::MapMode::Read, move |r| {
                        flag.store(r.is_ok(), Ordering::Release)
                    });
                self.stage = Stage::Reading(mapped, bytes);
            }
            Stage::Reading(mapped, bytes) if mapped.load(Ordering::Acquire) => {
                let bytes = *bytes;
                self.collect(bytes);
                self.stage = Stage::Idle;
            }
            _ => {}
        }
        if self.want_detail && self.counters && self.stats.is_none() {
            self.stats = Some(device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("Pass pipeline statistics"),
                ty: wgpu::QueryType::PipelineStatistics(STAT_FLAGS),
                count: STATS_CAPACITY,
            }));
            self.fragment_unit = calibrate_fragment_unit(device, queue);
        }
        if matches!(self.stage, Stage::Idle)
            && self.timed_at.is_none_or(|at| at.elapsed() >= self.gap)
        {
            self.entries.clear();
            self.segments.clear();
            self.stamps_used = 0;
            self.last_pass = None;
            self.kind = match (self.want_detail, self.kind, self.stats.is_some()) {
                (false, ..) => FrameKind::Plain,
                (true, FrameKind::Parts, true) => FrameKind::Counters,
                (true, ..) => FrameKind::Parts,
            };
            self.stage = Stage::Recording;
            self.timed_at = Some(Instant::now());
        }
    }

    /// Whether the frame being recorded is timed.
    pub fn recording(&self) -> bool {
        matches!(self.stage, Stage::Recording)
    }

    /// Timestamp writes for the pass `name`, in a timed frame.
    pub fn writes(&mut self, name: &str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let i = self.push_pass(name)?;
        Some(stamps(&self.queries, i))
    }

    /// Timestamp writes for the compute pass `name`, in a timed frame. Shader packs' compute
    /// passes come as `<program> (compute)`, apart from the fragment program of the same name
    /// (`deferred2.csh` beside `deferred2.fsh`), whose time theirs would otherwise add to.
    pub fn compute_writes(&mut self, name: &str) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        let i = self.push_pass(name)?;
        Some(compute_stamps(&self.queries, i))
    }

    /// The timestamp pair of the timed frame's next top-level span, `name`.
    fn push(&mut self, name: &str) -> Option<u32> {
        let i = self.push_entry(name, None, true)?;
        self.entries[i].stamp
    }

    /// `push` for a pass, remembered as the last one.
    fn push_pass(&mut self, name: &str) -> Option<u32> {
        let i = self.push_entry(name, None, true);
        self.last_pass = i;
        self.entries[i?].stamp
    }

    /// The index of a new entry in the timed frame; `None` when not recording or out of timestamps.
    fn push_entry(&mut self, name: &str, parent: Option<usize>, stamped: bool) -> Option<usize> {
        if !matches!(self.stage, Stage::Recording) || (stamped && self.stamps_used >= CAPACITY) {
            return None;
        }
        let stamp = stamped.then(|| {
            self.stamps_used += 1;
            self.stamps_used - 1
        });
        self.entries.push(Entry {
            name: name.to_owned(),
            parent,
            stamp,
        });
        Some(self.entries.len() - 1)
    }

    /// The statistics query index of a new segment of the pass `owner`; `None` when out of queries.
    fn open_segment(&mut self, owner: usize, part: &str, pixels: u64) -> Option<u32> {
        if self.segments.len() as u32 >= STATS_CAPACITY {
            return None;
        }
        self.segments.push(Segment {
            owner,
            part: part.to_owned(),
            pixels,
        });
        Some(self.segments.len() as u32 - 1)
    }

    /// Starts an encoder-level span `name` (a timed frame, on a device that can); give the result to `encoder_end`.
    pub fn encoder_begin(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        name: &str,
    ) -> Option<usize> {
        if !self.recording() || self.kind == FrameKind::Counters || !self.encoder_stamps {
            return None;
        }
        let i = self.push_entry(name, None, true)?;
        encoder.write_timestamp(&self.queries, 2 * self.entries[i].stamp?);
        Some(i)
    }

    /// Ends the span `encoder_begin` started.
    pub fn encoder_end(&mut self, encoder: &mut wgpu::CommandEncoder, entry: Option<usize>) {
        if let Some(s) = entry.and_then(|i| self.entries[i].stamp) {
            encoder.write_timestamp(&self.queries, 2 * s + 1);
        }
    }

    /// A pass that ends once everything drawn into `target` before it has, in a timed frame:
    /// the next pass counts from there, not from when the GPU queued it behind them.
    pub fn mark(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (target, format): super::timing::Target,
    ) {
        if self.marker.as_ref().is_none_or(|(f, _)| *f != format) {
            let module = crate::gfx::shader_module(
                device,
                wgpu::ShaderModuleDescriptor {
                    label: Some("Shader pack timing mark"),
                    source: wgpu::ShaderSource::Wgsl(super::timing::MARKER.into()),
                },
            );
            let pipeline = super::timing::end_pipeline(device, &module, format);
            self.marker = Some((format, pipeline));
        }
        // An unnamed pass: its end counts, it is never listed.
        let Some(i) = self.push("") else {
            return;
        };
        let (_, pipeline) = self.marker.as_ref().expect("made above");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shader pack timing mark"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: Some(stamps(&self.queries, i)),
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_scissor_rect(0, 0, 1, 1);
        pass.draw(0..3, 0..1);
    }

    /// After the frame is submitted.
    pub fn submitted(&mut self, queue: &wgpu::Queue) {
        if !matches!(self.stage, Stage::Recording) {
            return;
        }
        if self.stamps_used == 0 {
            self.stage = Stage::Idle;
            return;
        }
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        queue.on_submitted_work_done(move || flag.store(true, Ordering::Release));
        self.stage = Stage::Running(done);
    }

    fn collect(&mut self, bytes: u64) {
        let slice = self.read.slice(..bytes);
        let Ok(data) = slice.get_mapped_range() else {
            return;
        };
        let stamps: Vec<u64> = data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        drop(data);
        self.read.unmap();
        if self.kind == FrameKind::Counters {
            if self.want_detail {
                self.collect_counters(&stamps);
            }
            return;
        }
        let layout: Vec<_> = self.entries.iter().map(|e| (e.stamp, e.parent)).collect();
        let times = entry_times(&stamps, &layout, self.period_ns);
        let mut passes: Vec<(String, f32)> = Vec::new();
        let mut parts: Vec<(String, Vec<(String, f32)>)> = Vec::new();
        for (entry, ms) in self.entries.iter().zip(times) {
            let Some(ms) = ms.filter(|_| !entry.name.is_empty()) else {
                continue;
            };
            match entry.parent {
                None => add_ms(&mut passes, &entry.name, ms),
                Some(p) => {
                    let pass = &self.entries[p].name;
                    let i = match parts.iter().position(|o| o.0 == *pass) {
                        Some(i) => i,
                        None => {
                            parts.push((pass.clone(), Vec::new()));
                            parts.len() - 1
                        }
                    };
                    add_ms(&mut parts[i].1, &entry.name, ms);
                }
            }
        }
        record(
            &mut self.samples,
            passes.iter().map(|(name, ms)| (name, Some(*ms))),
        );
        for (pass, list) in &parts {
            for (part, ms) in list {
                let i = match self
                    .part_samples
                    .iter()
                    .position(|s| s.0 == *pass && s.1 == *part)
                {
                    Some(i) => i,
                    None => {
                        self.part_samples
                            .push((pass.clone(), part.clone(), VecDeque::new()));
                        self.part_samples.len() - 1
                    }
                };
                window_push(&mut self.part_samples[i].2, *ms);
            }
        }
        if self.keep_frames
            && let Some(at) = self.timed_at
        {
            if self.frames.len() == FRAME_LOG {
                self.frames.pop_front();
            }
            self.frames.push_back(TimedFrame {
                at,
                total_ms: frame_span(&stamps, &layout, self.period_ns),
                passes,
                parts,
            });
        }
    }

    /// Files the counters of the counters frame just read (`words` are the whole readback).
    fn collect_counters(&mut self, words: &[u64]) {
        let first = (TIMESTAMP_BYTES / 8) as usize;
        let mut seen: Vec<(usize, Counters, Vec<(String, Counters)>, u64)> = Vec::new();
        for (i, seg) in self.segments.iter().enumerate() {
            let Some(w) = words.get(first + 4 * i..first + 4 * i + 4) else {
                continue;
            };
            let c = Counters {
                vertex_shader_invocations: w[0],
                clipper_invocations: w[1],
                clipper_primitives_out: w[2],
                fragment_shader_invocations: w[3],
            };
            match seen.iter_mut().find(|s| s.0 == seg.owner) {
                Some(s) => {
                    s.1 = s.1.plus(c);
                    s.2.push((seg.part.clone(), c));
                }
                None => seen.push((seg.owner, c, vec![(seg.part.clone(), c)], seg.pixels)),
            }
        }
        for (owner, total, parts, pixels) in seen {
            let name = &self.entries[owner].name;
            let i = match self.counter_samples.iter().position(|s| s.pass == *name) {
                Some(i) => i,
                None => {
                    self.counter_samples.push(CounterSamples {
                        pass: name.clone(),
                        pixels,
                        total: VecDeque::new(),
                        parts: Vec::new(),
                    });
                    self.counter_samples.len() - 1
                }
            };
            let samples = &mut self.counter_samples[i];
            samples.pixels = pixels;
            window_push(&mut samples.total, total);
            for (part, c) in parts {
                let j = match samples.parts.iter().position(|p| p.0 == part) {
                    Some(j) => j,
                    None => {
                        samples.parts.push((part, VecDeque::new()));
                        samples.parts.len() - 1
                    }
                };
                window_push(&mut samples.parts[j].1, c);
            }
        }
    }

    /// Each counted pass's counters (medians over its last counters frames), in first-run order.
    pub fn counter_medians(&self) -> Vec<(String, PassCounters)> {
        self.counter_samples
            .iter()
            .filter(|s| !s.total.is_empty())
            .map(|s| {
                let parts = if s.parts.len() >= 2 {
                    s.parts
                        .iter()
                        .map(|(n, v)| (n.clone(), median_counters(v)))
                        .collect()
                } else {
                    Vec::new()
                };
                let counted = PassCounters {
                    total: median_counters(&s.total),
                    pixels: s.pixels,
                    parts,
                };
                (s.pass.clone(), counted)
            })
            .collect()
    }

    /// Everything the timer shows now.
    pub fn readings(&self) -> Readings {
        Readings {
            passes: self.medians(),
            parts: self.part_medians(),
            counters: self.counter_medians(),
            support: self.detail_support(),
        }
    }

    /// Each top-level pass's median ms over its last timed frames, in first-run order.
    pub fn medians(&self) -> Vec<(String, f32)> {
        self.samples
            .iter()
            .filter(|s| !s.1.is_empty())
            .map(|(name, v)| (name.clone(), median(v)))
            .collect()
    }

    /// Each pass's parts with their median times in milliseconds, in the order they first ran.
    pub fn part_medians(&self) -> Vec<(String, Vec<(String, f32)>)> {
        let mut out: Vec<(String, Vec<(String, f32)>)> = Vec::new();
        for (pass, part, v) in self.part_samples.iter().filter(|s| !s.2.is_empty()) {
            let entry = (part.clone(), median(v));
            match out.iter_mut().find(|o| o.0 == *pass) {
                Some(o) => o.1.push(entry),
                None => out.push((pass.clone(), vec![entry])),
            }
        }
        out
    }
}

/// Draws a full-screen triangle on a small target with a statistics query and reads the count, once at setup.
fn calibrate_fragment_unit(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<FragmentUnit> {
    const SIDE: u32 = 64;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fragment counter calibration"),
        source: wgpu::ShaderSource::Wgsl(
            "@vertex fn vs(@builtin(vertex_index) v: u32) -> @builtin(position) vec4<f32> {
                let p = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u));
                return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
            }
            @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }"
                .into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
        }),
        multiview_mask: None,
        cache: None,
    });
    let target = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::PipelineStatistics(
            wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS,
        ),
        count: 1,
    });
    let buffer = |usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 8,
            usage,
            mapped_at_creation: false,
        })
    };
    let resolve = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
    let read = buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.begin_pipeline_statistics_query(&queries, 0);
        pass.draw(0..3, 0..1);
        pass.end_pipeline_statistics_query();
    }
    encoder.resolve_query_set(&queries, 0..1, &resolve, 0);
    encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 8);
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    let count = u64::from_le_bytes(
        read.slice(..).get_mapped_range().ok()?[..8]
            .try_into()
            .ok()?,
    );
    let pixels = u64::from(SIDE * SIDE);
    let unit = FragmentUnit::of_calibration(count, pixels);
    log::info!("fragment counter calibration: {count} for {pixels} pixels, unit {unit:?}");
    unit
}

fn round_to(value: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (value * scale).round() / scale
}

fn counters_json(c: Counters) -> serde_json::Map<String, serde_json::Value> {
    let Value::Object(fields) = json!({
        "vertex_shader_invocations": c.vertex_shader_invocations,
        "clipper_invocations": c.clipper_invocations,
        "clipper_primitives_out": c.clipper_primitives_out,
        "fragment_shader_invocations": c.fragment_shader_invocations,
    }) else {
        unreachable!()
    };
    fields
}

/// Dispatch times by pass, as the `gpu_pass_parts` object.
pub fn parts_json(parts: &[(String, Vec<(String, f32)>)]) -> Value {
    parts
        .iter()
        .map(|(pass, parts)| {
            let times = parts
                .iter()
                .map(|(part, ms)| (part.clone(), json!(round_to(f64::from(*ms), 3))))
                .collect::<serde_json::Map<_, _>>();
            (pass.clone(), Value::Object(times))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Counters by pass, as the `gpu_counters` object; `fragments_per_pixel` only when the unit is known.
pub fn counters_by_pass_json(
    counters: &[(String, PassCounters)],
    unit: Option<FragmentUnit>,
) -> Value {
    counters
        .iter()
        .map(|(pass, c)| {
            let mut fields = counters_json(c.total);
            fields.insert("pixels".into(), json!(c.pixels));
            if let Some(unit) = unit.filter(|_| c.pixels > 0) {
                let shaded = unit.pixels_of(c.total.fragment_shader_invocations) as f64;
                let per_pixel = round_to(shaded / c.pixels as f64, 2);
                fields.insert("fragments_per_pixel".into(), json!(per_pixel));
            }
            if !c.parts.is_empty() {
                let parts = c
                    .parts
                    .iter()
                    .map(|(name, part)| (name.clone(), Value::Object(counters_json(*part))))
                    .collect::<serde_json::Map<_, _>>();
                fields.insert("parts".into(), Value::Object(parts));
            }
            (pass.clone(), Value::Object(fields))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// How detail was measured: the parts mode, whether counters exist and what they count.
pub fn support_json(support: DetailSupport) -> Value {
    json!({
        "parts_mode": match support.parts {
            PartsMode::InsidePasses => "inside_passes",
            PartsMode::SeparatePasses => "separate_passes",
        },
        "counters": support.counters,
        "fragment_unit": support.fragment_unit.map(FragmentUnit::name),
    })
}

/// The keys `stats` and `bench` add while detail is on.
pub fn detail_json(
    parts: &[(String, Vec<(String, f32)>)],
    counters: &[(String, PassCounters)],
    support: DetailSupport,
) -> Value {
    json!({
        "gpu_pass_detail": support_json(support),
        "gpu_pass_parts": parts_json(parts),
        "gpu_counters": counters_by_pass_json(counters, support.fragment_unit),
    })
}

/// Adds `ms` to `name`'s time in `list`, or lists it.
fn add_ms(list: &mut Vec<(String, f32)>, name: &str, ms: f32) {
    match list.iter_mut().find(|e| e.0 == name) {
        Some(e) => e.1 += ms,
        None => list.push((name.to_owned(), ms)),
    }
}

fn window_push<T>(window: &mut VecDeque<T>, value: T) {
    if window.len() == WINDOW {
        window.pop_front();
    }
    window.push_back(value);
}

impl Counters {
    fn plus(self, o: Self) -> Self {
        Self {
            vertex_shader_invocations: self.vertex_shader_invocations + o.vertex_shader_invocations,
            clipper_invocations: self.clipper_invocations + o.clipper_invocations,
            clipper_primitives_out: self.clipper_primitives_out + o.clipper_primitives_out,
            fragment_shader_invocations: self.fragment_shader_invocations
                + o.fragment_shader_invocations,
        }
    }
}

fn median_counters(window: &VecDeque<Counters>) -> Counters {
    let field = |f: fn(&Counters) -> u64| {
        let mut v: Vec<u64> = window.iter().map(f).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    Counters {
        vertex_shader_invocations: field(|c| c.vertex_shader_invocations),
        clipper_invocations: field(|c| c.clipper_invocations),
        clipper_primitives_out: field(|c| c.clipper_primitives_out),
        fragment_shader_invocations: field(|c| c.fragment_shader_invocations),
    }
}

fn median(window: &VecDeque<f32>) -> f32 {
    let mut v: Vec<f32> = window.iter().copied().collect();
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
) -> wgpu::ComputePass<'e> {
    encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes,
    })
}

/// A compute pass `name` made of `parts`: one pass, or in a parts frame each part timed.
pub fn compute_parts(
    encoder: &mut wgpu::CommandEncoder,
    timing: Option<&mut PassTiming>,
    name: &str,
    parts: &mut [Part<'_>],
) {
    let mode = timing.as_deref().and_then(|t| {
        if t.parts_frame() {
            Some(t.parts_mode)
        } else {
            t.in_pass_parts().then_some(PartsMode::InsidePasses)
        }
    });
    match (timing, mode) {
        (Some(t), Some(PartsMode::InsidePasses)) => {
            let mut pass = begin(encoder, name, t.compute_writes(name));
            let parent = t.last_pass;
            for (part, run) in parts.iter_mut() {
                let stamp = parent
                    .and_then(|p| t.push_entry(part, Some(p), true))
                    .and_then(|i| t.entries[i].stamp);
                if let Some(s) = stamp {
                    pass.write_timestamp(&t.queries, 2 * s);
                }
                run(&mut pass);
                if let Some(s) = stamp {
                    pass.write_timestamp(&t.queries, 2 * s + 1);
                }
            }
        }
        (Some(t), Some(PartsMode::SeparatePasses)) => {
            let parent = t.push_entry(name, None, false);
            for (part, run) in parts.iter_mut() {
                let stamp = parent
                    .and_then(|p| t.push_entry(part, Some(p), true))
                    .and_then(|i| t.entries[i].stamp);
                let mut pass = begin(encoder, name, stamp.map(|s| compute_stamps(&t.queries, s)));
                run(&mut pass);
            }
        }
        (timing, _) => {
            let mut pass = begin(encoder, name, timing.and_then(|t| t.compute_writes(name)));
            for (_, run) in parts.iter_mut() {
                run(&mut pass);
            }
        }
    }
}

/// A render pass whose open statistics query ends when it drops, so none is left open at the pass's end.
pub struct CountedPass<'e> {
    pass: wgpu::RenderPass<'e>,
    counting: bool,
    owner: Option<usize>,
    pixels: u64,
    /// The draw group being timed (`stage`): its timestamp pair and the timer's queries.
    stage: Option<(u32, wgpu::QuerySet)>,
}

impl<'e> std::ops::Deref for CountedPass<'e> {
    type Target = wgpu::RenderPass<'e>;
    fn deref(&self) -> &Self::Target {
        &self.pass
    }
}

impl std::ops::DerefMut for CountedPass<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.pass
    }
}

impl Drop for CountedPass<'_> {
    fn drop(&mut self) {
        if self.counting {
            self.pass.end_pipeline_statistics_query();
        }
        if let Some((s, queries)) = self.stage.take() {
            self.pass.write_timestamp(&queries, 2 * s + 1);
        }
    }
}

impl<'e> CountedPass<'e> {
    /// A pass that never counts.
    pub fn plain(pass: wgpu::RenderPass<'e>) -> Self {
        Self {
            pass,
            counting: false,
            owner: None,
            pixels: 0,
            stage: None,
        }
    }

    /// Ends the timed draw group, if any, and times the rest of the pass as `name`, where the device stamps inside passes.
    pub fn stage(&mut self, timing: Option<&mut PassTiming>, name: &str) {
        let Some(t) = timing.filter(|t| t.in_pass_parts()) else {
            return;
        };
        let Some(owner) = self.owner.filter(|&o| t.entries[o].stamp.is_some()) else {
            return;
        };
        if let Some((s, queries)) = self.stage.take() {
            self.pass.write_timestamp(&queries, 2 * s + 1);
        }
        if let Some(s) = t
            .push_entry(name, Some(owner), true)
            .and_then(|i| t.entries[i].stamp)
        {
            self.pass.write_timestamp(&t.queries, 2 * s);
            self.stage = Some((s, t.queries.clone()));
        }
    }

    /// Ends the open query, if any, and counts the rest of the pass as the segment `part`.
    pub fn split(&mut self, timing: Option<&mut PassTiming>, part: &str) {
        if self.counting {
            self.pass.end_pipeline_statistics_query();
            self.counting = false;
        }
        if let (Some(t), Some(owner)) = (timing, self.owner)
            && t.counters_frame()
            && let Some(i) = t.open_segment(owner, part, self.pixels)
            && let Some(stats) = &t.stats
        {
            self.pass.begin_pipeline_statistics_query(stats, i);
            self.counting = true;
        }
    }
}

/// Counts `pass` as the segment `first` of the pass the timer's last `writes` made, in a counters frame.
pub fn counted<'e>(
    timing: Option<&mut PassTiming>,
    pass: wgpu::RenderPass<'e>,
    pixels: u64,
    first: &str,
) -> CountedPass<'e> {
    let mut counted = CountedPass::plain(pass);
    counted.pixels = pixels;
    if let Some(t) = timing {
        counted.owner = t.last_pass.take();
        counted.split(Some(t), first);
    }
    counted
}

/// Adds one timed frame's pass times to `samples`, each pass's window of its last `WINDOW`
/// frames. Passes of one name in a frame (a program drawn over several render passes) count as
/// one sample, their sum: as separate samples they shared one window, and its median was the
/// time of neither. Unnamed and unstamped passes are left out.
fn record<'a>(
    samples: &mut Vec<(String, VecDeque<f32>)>,
    frame: impl Iterator<Item = (&'a String, Option<f32>)>,
) {
    let mut sums: Vec<(&str, f32)> = Vec::new();
    for (name, ms) in frame {
        let Some(ms) = ms.filter(|_| !name.is_empty()) else {
            continue;
        };
        match sums.iter_mut().find(|s| s.0 == name) {
            Some(s) => s.1 += ms,
            None => sums.push((name, ms)),
        }
    }
    for (name, ms) in sums {
        let i = match samples.iter().position(|s| s.0 == name) {
            Some(i) => i,
            None => {
                samples.push((name.to_owned(), VecDeque::new()));
                samples.len() - 1
            }
        };
        let window = &mut samples[i].1;
        if window.len() == WINDOW {
            window.pop_front();
        }
        window.push_back(ms);
    }
}

/// Pass `i`'s beginning and end timestamps.
fn stamps(queries: &wgpu::QuerySet, i: u32) -> wgpu::RenderPassTimestampWrites<'_> {
    wgpu::RenderPassTimestampWrites {
        query_set: queries,
        beginning_of_pass_write_index: Some(2 * i),
        end_of_pass_write_index: Some(2 * i + 1),
    }
}

/// The compute pass's twin of `stamps`.
fn compute_stamps(queries: &wgpu::QuerySet, i: u32) -> wgpu::ComputePassTimestampWrites<'_> {
    wgpu::ComputePassTimestampWrites {
        query_set: queries,
        beginning_of_pass_write_index: Some(2 * i),
        end_of_pass_write_index: Some(2 * i + 1),
    }
}

/// Each entry's ms from its stamps (tiled GPUs overlap: count from the later of start and previous end; no pair sums its parts).
fn entry_times(
    stamps: &[u64],
    entries: &[(Option<u32>, Option<usize>)],
    period_ns: f32,
) -> Vec<Option<f32>> {
    let pair = |s: u32| {
        let at = 2 * s as usize;
        stamps.get(at..at + 2).map(|p| (p[0], p[1]))
    };
    let link = |previous_end: &mut u64, s: u32| {
        let (begin, end) = pair(s)?;
        if begin == 0 || end < begin {
            return None;
        }
        let from = begin.max(*previous_end).min(end);
        *previous_end = end;
        Some((end - from) as f32 * period_ns / 1e6)
    };
    let mut times = vec![None; entries.len()];
    let mut end = 0;
    for (i, &(stamp, parent)) in entries.iter().enumerate() {
        if let Some(s) = stamp
            && parent.is_none_or(|p| entries[p].0.is_none())
        {
            times[i] = link(&mut end, s);
        }
    }
    for (p, &(stamp, _)) in entries.iter().enumerate() {
        match stamp {
            Some(ps) => {
                let mut end = pair(ps).map_or(0, |b| b.0);
                for (i, &(stamp, parent)) in entries.iter().enumerate() {
                    if let (Some(s), true) = (stamp, parent == Some(p)) {
                        times[i] = link(&mut end, s);
                    }
                }
            }
            None => {
                let parts = (0..entries.len()).filter(|&i| entries[i].1 == Some(p));
                let known: Vec<f32> = parts.filter_map(|i| times[i]).collect();
                times[p] = (!known.is_empty()).then(|| known.iter().sum());
            }
        }
    }
    times
}

/// A frame's ms from its first stamp to its last, over the pairs that are whole.
fn frame_span(
    stamps: &[u64],
    entries: &[(Option<u32>, Option<usize>)],
    period_ns: f32,
) -> Option<f32> {
    let (first, last) = entries
        .iter()
        .filter_map(|e| {
            let at = 2 * e.0? as usize;
            let p = stamps.get(at..at + 2)?;
            (p[0] != 0 && p[1] >= p[0]).then_some((p[0], p[1]))
        })
        .fold((u64::MAX, 0), |(f, l), (b, e)| (f.min(b), l.max(e)));
    (last >= first).then(|| (last - first) as f32 * period_ns / 1e6)
}

#[cfg(test)]
mod tests {
    use super::{
        Counters, DetailSupport, FragmentUnit, PartsMode, PassCounters, PassTiming, compute_parts,
        counted, counters_by_pass_json, detail_json, entry_times, frame_span, record,
    };
    use serde_json::json;
    use std::collections::VecDeque;

    fn interleave(pairs: &[(f32, f32)]) -> Vec<u64> {
        let ns = |ms: f32| (ms * 1e6) as u64;
        pairs.iter().flat_map(|p| [ns(p.0), ns(p.1)]).collect()
    }

    fn assert_times(got: Vec<Option<f32>>, want: &[f32]) {
        assert_eq!(got.len(), want.len(), "{got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g.expect("timed") - w).abs() < 1e-4, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn passes_of_one_name_count_as_their_sum() {
        let names = ["terrain", "sky", "terrain", "", "water"].map(String::from);
        let times = [Some(1.0), Some(2.0), Some(3.0), Some(9.0), None];
        let mut samples: Vec<(String, VecDeque<f32>)> = Vec::new();
        record(&mut samples, names.iter().zip(times));
        let got: Vec<(&str, Vec<f32>)> = samples
            .iter()
            .map(|(n, w)| (n.as_str(), w.iter().copied().collect()))
            .collect();
        assert_eq!(got, [("terrain", vec![4.0]), ("sky", vec![2.0])]);
    }

    #[test]
    fn overlapping_passes_cost_from_the_previous_end() {
        // Pass 2 begins before pass 1 ends; pass 3 was never stamped.
        let stamps = [1_000_000, 3_000_000, 2_000_000, 5_000_000, 0, 0];
        let entries = [(Some(0), None), (Some(1), None), (Some(2), None)];
        assert_eq!(
            entry_times(&stamps, &entries, 1.0),
            [Some(2.0), Some(2.0), None]
        );
    }

    #[test]
    fn parts_chain_inside_their_pass_and_the_next_pass_counts_from_its_end() {
        let stamps = interleave(&[(1.0, 5.0), (1.0, 1.5), (1.5, 4.0), (4.0, 5.0), (4.5, 9.0)]);
        let entries = [
            (Some(0), None),
            (Some(1), Some(0)),
            (Some(2), Some(0)),
            (Some(3), Some(0)),
            (Some(4), None),
        ];
        assert_times(
            entry_times(&stamps, &entries, 1.0),
            &[4.0, 0.5, 2.5, 1.0, 4.0],
        );
    }

    #[test]
    fn a_pass_of_separate_passes_is_the_sum_of_its_parts() {
        let stamps = interleave(&[(1.0, 1.2), (1.1, 3.0), (3.0, 6.0)]);
        let entries = [
            (None, None),
            (Some(0), Some(0)),
            (Some(1), Some(0)),
            (Some(2), None),
        ];
        assert_times(entry_times(&stamps, &entries, 1.0), &[2.0, 0.2, 1.8, 3.0]);
    }

    #[test]
    fn an_unstamped_part_loses_only_its_own_time() {
        let stamps = interleave(&[(1.0, 5.0), (1.0, 1.5), (0.0, 0.0), (4.0, 5.0)]);
        let entries = [
            (Some(0), None),
            (Some(1), Some(0)),
            (Some(2), Some(0)),
            (Some(3), Some(0)),
        ];
        assert_eq!(
            entry_times(&stamps, &entries, 1.0),
            [Some(4.0), Some(0.5), None, Some(1.0)]
        );
    }

    #[test]
    fn a_frame_spans_its_first_stamp_to_its_last_whole_pair() {
        // A pass, its part inside it, a later pass, and one never stamped.
        let stamps = interleave(&[(1.0, 3.0), (1.5, 2.0), (2.5, 5.0), (0.0, 0.0)]);
        let entries = [
            (Some(0), None),
            (Some(1), Some(0)),
            (Some(2), None),
            (Some(3), None),
        ];
        let span = frame_span(&stamps, &entries, 1.0).expect("timed");
        assert!((span - 4.0).abs() < 1e-4, "{span}");
        assert_eq!(frame_span(&stamps, &[(None, None)], 1.0), None);
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
    fn detail_json_has_the_documented_fields_and_nesting() {
        let c = |v, f| Counters {
            vertex_shader_invocations: v,
            clipper_invocations: 2_050_000,
            clipper_primitives_out: 1_200_000,
            fragment_shader_invocations: f,
        };
        let counters = vec![
            (
                "World".to_owned(),
                PassCounters {
                    total: c(4_100_000, 6_300_000),
                    pixels: 2_073_600,
                    parts: vec![
                        ("before translucent".into(), c(3_000_000, 5_100_000)),
                        ("translucent terrain".into(), c(1_100_000, 1_200_000)),
                    ],
                },
            ),
            (
                "Visibility IDs".to_owned(),
                PassCounters {
                    total: c(10, 20),
                    pixels: 100,
                    parts: Vec::new(),
                },
            ),
        ];
        let parts = vec![("GPU culling".to_owned(), vec![("cull".to_owned(), 0.31049)])];
        let got = detail_json(&parts, &counters, support(Some(FragmentUnit::Pixel)));
        let want = json!({
            "gpu_pass_detail": {"parts_mode": "inside_passes", "counters": true, "fragment_unit": "pixel"},
            "gpu_pass_parts": {"GPU culling": {"cull": 0.31}},
            "gpu_counters": {
                "World": {
                    "vertex_shader_invocations": 4_100_000, "clipper_invocations": 2_050_000,
                    "clipper_primitives_out": 1_200_000, "fragment_shader_invocations": 6_300_000,
                    "pixels": 2_073_600, "fragments_per_pixel": 3.04,
                    "parts": {
                        "before translucent": {
                            "vertex_shader_invocations": 3_000_000, "clipper_invocations": 2_050_000,
                            "clipper_primitives_out": 1_200_000, "fragment_shader_invocations": 5_100_000
                        },
                        "translucent terrain": {
                            "vertex_shader_invocations": 1_100_000, "clipper_invocations": 2_050_000,
                            "clipper_primitives_out": 1_200_000, "fragment_shader_invocations": 1_200_000
                        }
                    }
                },
                "Visibility IDs": {
                    "vertex_shader_invocations": 10, "clipper_invocations": 2_050_000,
                    "clipper_primitives_out": 1_200_000, "fragment_shader_invocations": 20,
                    "pixels": 100, "fragments_per_pixel": 0.2
                }
            }
        });
        assert_eq!(got, want);
    }

    #[test]
    fn quad_counting_devices_scale_fragments_per_pixel_and_unknown_ones_omit_it() {
        let counted = vec![(
            "World".to_owned(),
            PassCounters {
                total: Counters {
                    fragment_shader_invocations: 1000,
                    ..Default::default()
                },
                pixels: 2000,
                parts: Vec::new(),
            },
        )];
        let per_pixel =
            |unit| counters_by_pass_json(&counted, unit)["World"]["fragments_per_pixel"].clone();
        assert_eq!(per_pixel(Some(FragmentUnit::Pixel)), json!(0.5));
        assert_eq!(per_pixel(Some(FragmentUnit::Quad)), json!(2.0));
        assert!(per_pixel(None).is_null());
    }

    #[test]
    fn calibration_counts_name_the_unit() {
        let pixels = 64 * 64;
        // 4096 is NVIDIA's pixel count, 1056 RADV's quads of two triangles over a diagonal.
        for (count, unit) in [
            (4096, Some(FragmentUnit::Pixel)),
            (1024, Some(FragmentUnit::Quad)),
            (1056, Some(FragmentUnit::Quad)),
            (2048, None),
            (0, None),
            (8192, None),
        ] {
            assert_eq!(FragmentUnit::of_calibration(count, pixels), unit, "{count}");
        }
    }

    const SHADER: &str = "
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(256) fn light(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x == 0u) { out[0] = 1u; }
}
@compute @workgroup_size(256) fn heavy(@builtin(global_invocation_id) id: vec3<u32>) {
    var a = id.x;
    for (var i = 0u; i < 16384u; i++) { a = (a ^ (a >> 3u)) * 1664525u + i; }
    out[id.x] = a;
}";

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let needed =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
        if !adapter.features().contains(needed) {
            return None;
        }
        eprintln!("pass parts on {}", adapter.get_info().name);
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: needed,
            ..Default::default()
        }))
        .ok()
    }

    /// The pass's median and its parts' medians after 20 timed frames of light, heavy, light2.
    fn timed_parts(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mode: Option<PartsMode>,
    ) -> (f32, Vec<(String, f32)>) {
        let mut timing = match mode {
            Some(mode) => PassTiming::with_parts_mode(device, queue, mode),
            None => PassTiming::new(device, queue),
        }
        .unwrap();
        timing.set_detail(true);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (light, heavy) = (pipeline("light"), pipeline("heavy"));
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 256 * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind = |pipeline: &wgpu::ComputePipeline| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let (light_group, heavy_group) = (bind(&light), bind(&heavy));
        let run = |pass: &mut wgpu::ComputePass<'_>, pipeline, group, n| {
            pass.set_bind_group(0, group, &[]);
            pass.set_pipeline(pipeline);
            pass.dispatch_workgroups(n, 1, 1);
        };
        // A timed frame takes two iterations: record it, then resolve and map its times.
        for _ in 0..60 {
            timing.begin(device, queue);
            let mut encoder = device.create_command_encoder(&Default::default());
            compute_parts(
                &mut encoder,
                Some(&mut timing),
                "parts",
                &mut [
                    ("light", &mut |p| run(p, &light, &light_group, 1)),
                    ("heavy", &mut |p| run(p, &heavy, &heavy_group, 256)),
                    ("light2", &mut |p| run(p, &light, &light_group, 1)),
                ],
            );
            queue.submit([encoder.finish()]);
            timing.submitted(queue);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        let parent = timing.medians().into_iter().find(|m| m.0 == "parts");
        let mut parts = timing.part_medians();
        assert_eq!(parts.len(), 1, "{parts:?}");
        (parent.expect("the pass has a time").1, parts.remove(0).1)
    }

    #[test]
    fn the_heavy_part_of_a_pass_has_the_largest_time() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU with timestamps inside passes: skipped");
            return;
        };
        for mode in [None, Some(PartsMode::SeparatePasses)] {
            let (parent, parts) = timed_parts(&device, &queue, mode);
            let names: Vec<_> = parts.iter().map(|p| p.0.as_str()).collect();
            assert_eq!(names, ["light", "heavy", "light2"], "{mode:?}");
            let heavy = parts[1].1;
            assert!(parts.iter().all(|p| p.1 <= heavy), "{mode:?} {parts:?}");
            let sum: f32 = parts.iter().map(|p| p.1).sum();
            assert!(parent >= sum * 0.95, "{mode:?} {parent} < {parts:?}");
            eprintln!("{mode:?}: parent {parent:.4} ms, parts {parts:?}");
        }
    }

    const QUADS: &str = "
@vertex fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) k: u32) -> @builtin(position) vec4<f32> {
    var xy = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    return vec4<f32>(xy[v], f32(k + 1u) / 9.0, 1.0);
}
@early_depth_test(force)
@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }";
    const QUAD_COUNT: u32 = 8;
    const SIDE: u32 = 64;

    fn stats_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let needed = wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::PIPELINE_STATISTICS_QUERY
            | wgpu::Features::SHADER_EARLY_DEPTH_TEST;
        if adapter.get_info().backend != wgpu::Backend::Vulkan
            || !adapter.features().contains(needed)
        {
            return None;
        }
        eprintln!("pass counters on {}", adapter.get_info().name);
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: needed,
            ..Default::default()
        }))
        .ok()
    }

    struct Scene {
        color: wgpu::TextureView,
        depth: wgpu::TextureView,
        pipeline: wgpu::RenderPipeline,
    }

    impl Scene {
        fn new(device: &wgpu::Device) -> Self {
            let target = |format, label| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: SIDE,
                            height: SIDE,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(QUADS.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                }),
                multiview_mask: None,
                cache: None,
            });
            Self {
                color: target(wgpu::TextureFormat::Rgba8Unorm, "color"),
                depth: target(wgpu::TextureFormat::Depth32Float, "depth"),
                pipeline,
            }
        }

        fn pass<'e>(
            &self,
            encoder: &'e mut wgpu::CommandEncoder,
            timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
        ) -> wgpu::RenderPass<'e> {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                timestamp_writes,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass
        }
    }

    fn near_first(pass: &mut wgpu::RenderPass<'_>) {
        for k in 0..QUAD_COUNT {
            pass.draw(0..6, k..k + 1);
        }
    }

    fn far_first(pass: &mut wgpu::RenderPass<'_>) {
        for k in (0..QUAD_COUNT).rev() {
            pass.draw(0..6, k..k + 1);
        }
    }

    /// Starts the next frame of the wanted kind (frames alternate), and returns it recording.
    fn frame_of_kind(
        timing: &mut PassTiming,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        counts: bool,
    ) {
        for _ in 0..6 {
            timing.begin(device, queue);
            if timing.recording() && (timing.counters_frame() == counts) {
                return;
            }
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        panic!("no frame of the wanted kind");
    }

    fn finish(
        timing: &mut PassTiming,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: wgpu::CommandEncoder,
    ) {
        queue.submit([encoder.finish()]);
        timing.submitted(queue);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }

    #[test]
    fn counters_see_the_overdraw_that_draw_order_makes() {
        let Some((device, queue)) = stats_device() else {
            eprintln!("no GPU with pipeline statistics: skipped");
            return;
        };
        device.on_uncaptured_error(std::sync::Arc::new(|e| panic!("{e}")));
        let scene = Scene::new(&device);
        let mut timing = PassTiming::new(&device, &queue).unwrap();
        timing.set_detail(true);
        let pixels = u64::from(SIDE * SIDE);

        // Out of its frame, a counted pass counts nothing and still ends cleanly.
        frame_of_kind(&mut timing, &device, &queue, false);
        let mut encoder = device.create_command_encoder(&Default::default());
        let pass = scene.pass(&mut encoder, timing.writes("parts frame"));
        let mut pass = counted(Some(&mut timing), pass, pixels, "all");
        assert!(!pass.counting);
        near_first(&mut pass);
        drop(pass);
        finish(&mut timing, &device, &queue, encoder);

        frame_of_kind(&mut timing, &device, &queue, true);
        let mut encoder = device.create_command_encoder(&Default::default());
        let pass = scene.pass(&mut encoder, timing.writes("near first"));
        let mut pass = counted(Some(&mut timing), pass, pixels, "all");
        assert!(pass.counting);
        near_first(&mut pass);
        drop(pass);
        let pass = scene.pass(&mut encoder, timing.writes("far first"));
        let mut pass = counted(Some(&mut timing), pass, pixels, "all");
        far_first(&mut pass);
        drop(pass);
        let pass = scene.pass(&mut encoder, timing.writes("split"));
        let mut pass = counted(Some(&mut timing), pass, pixels, "a");
        far_first(&mut pass);
        pass.split(Some(&mut timing), "b");
        drop(pass);
        finish(&mut timing, &device, &queue, encoder);
        for _ in 0..4 {
            timing.begin(&device, &queue);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }

        let counters = timing.counter_medians();
        let get = |name: &str| {
            let found = counters.iter().find(|c| c.0 == name);
            &found
                .unwrap_or_else(|| panic!("{name} missing from {counters:?}"))
                .1
        };
        let (near, far, split) = (get("near first"), get("far first"), get("split"));
        eprintln!("near first {near:?}\nfar first {far:?}\nsplit {split:?}");
        // RADV counts shaded 2x2 quads (1056 per full-screen layer here), NVIDIA counts pixels.
        let full = u64::from(QUAD_COUNT) * pixels / 4;
        assert!(far.total.fragment_shader_invocations >= full, "{far:?}");
        let unit = timing.detail_support().fragment_unit.expect("calibrated");
        // The unit the calibration draw found must turn this scene's count back into its pixels.
        assert!(
            unit.pixels_of(far.total.fragment_shader_invocations) >= u64::from(QUAD_COUNT) * pixels,
            "{unit:?} {far:?}"
        );
        assert!(
            near.total.fragment_shader_invocations <= 2 * pixels,
            "{near:?}"
        );
        assert!(
            far.total.fragment_shader_invocations
                >= near.total.fragment_shader_invocations * u64::from(QUAD_COUNT) / 2
        );
        assert_eq!(
            near.total.vertex_shader_invocations,
            far.total.vertex_shader_invocations
        );
        assert_eq!(
            near.total.clipper_primitives_out,
            far.total.clipper_primitives_out
        );
        assert!(near.total.clipper_primitives_out >= 2 * u64::from(QUAD_COUNT));
        assert_eq!((near.pixels, near.parts.len()), (pixels, 0));
        let [(a, a_count), (b, b_count)] = &split.parts[..] else {
            panic!("two segments expected: {split:?}");
        };
        assert_eq!((a.as_str(), b.as_str()), ("a", "b"));
        assert!(a_count.fragment_shader_invocations >= full);
        assert_eq!(b_count.fragment_shader_invocations, 0);
        assert_eq!(split.total, *a_count);
    }

    #[test]
    fn stages_split_a_render_pass_in_a_plain_frame_and_kept_frames_hold_them() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let needed = wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
            | wgpu::Features::SHADER_EARLY_DEPTH_TEST;
        if !adapter.features().contains(needed) {
            eprintln!("no GPU with timestamps inside passes: skipped");
            return;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: needed,
            ..Default::default()
        }))
        .unwrap();
        device.on_uncaptured_error(std::sync::Arc::new(|e| panic!("{e}")));
        let scene = Scene::new(&device);
        let mut timing = PassTiming::new(&device, &queue).unwrap();
        timing.keep_frames(true);
        for _ in 0..12 {
            timing.begin(&device, &queue);
            let mut encoder = device.create_command_encoder(&Default::default());
            let pass = scene.pass(&mut encoder, timing.writes("World"));
            let mut pass = counted(Some(&mut timing), pass, 0, "all");
            pass.stage(Some(&mut timing), "near first");
            near_first(&mut pass);
            pass.stage(Some(&mut timing), "far first");
            far_first(&mut pass);
            drop(pass);
            finish(&mut timing, &device, &queue, encoder);
        }
        let parts = timing.part_medians();
        let names: Vec<_> = parts
            .iter()
            .flat_map(|(pass, list)| {
                list.iter()
                    .map(move |(part, _)| (pass.as_str(), part.as_str()))
            })
            .collect();
        assert_eq!(names, [("World", "near first"), ("World", "far first")]);
        let frames = timing.take_frames();
        assert!(!frames.is_empty());
        let f = &frames[0];
        let world = f
            .passes
            .iter()
            .find(|p| p.0 == "World")
            .expect("World timed")
            .1;
        let total = f.total_ms.expect("a total");
        assert!(total >= world * 0.999, "{total} < {world}");
        let staged: f32 = f.parts[0].1.iter().map(|p| p.1).sum();
        assert!(staged <= world * 1.001, "{staged} > {world}");
        assert!(timing.take_frames().is_empty());
    }
}
