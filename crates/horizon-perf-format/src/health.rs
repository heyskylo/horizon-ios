use crate::v1::{HealthSample, HealthWindow, Side};

/// `(step in seconds, samples kept)`: 10 min of 1 s, 1 h of 10 s, 24 h of 60 s, 7 days of 15 min.
pub const TIERS: [(u32, usize); 4] = [(1, 600), (10, 360), (60, 1440), (900, 672)];

/// Durations one second keeps for its percentiles; the count, mean and max stay exact beyond this.
const SECOND_BUFFER: usize = 1024;

const FIELDS: usize = 23;
const TPS: usize = 0;
const MSPT_AVG: usize = 1;
const MSPT_P50: usize = 2;
const MSPT_P95: usize = 3;
const MSPT_MAX: usize = 4;
const FPS: usize = 5;
const FRAME_P50: usize = 6;
const FRAME_P95: usize = 7;
const FRAME_P99: usize = 8;
const FRAME_MAX: usize = 9;
const FRAME_CPU_P50: usize = 10;
const FRAME_GPU_P50: usize = 11;
const CPU_PROCESS: usize = 12;
const CPU_SYSTEM: usize = 13;
const RSS: usize = 14;
const PHYSICAL: usize = 15;
const SWAP: usize = 16;
const ALLOCATOR_COMMIT: usize = 17;
const ALLOCS_PER_S: usize = 18;
const ALLOC_MIB_PER_S: usize = 19;
const PLAYERS: usize = 20;
const CHUNKS: usize = 21;
const ENTITIES: usize = 22;

/// A coarser sample takes the largest value of these fields and the mean of the others.
const MAX_FIELDS: [usize; 2] = [MSPT_MAX, FRAME_MAX];

/// One sample: NaN stands for a field that is not set.
#[derive(Clone, Copy)]
struct Slot([f32; FIELDS]);

const EMPTY: Slot = Slot([f32::NAN; FIELDS]);

/// What the producer measures once a second besides tick or frame times; the latest value holds until replaced.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gauges {
    pub frame_cpu_ms_p50: Option<f32>,
    pub frame_gpu_ms_p50: Option<f32>,
    pub cpu_process_cores: f32,
    pub cpu_system_cores: f32,
    pub rss_mb: f32,
    pub physical_used_mb: f32,
    pub swap_used_mb: f32,
    pub allocator_commit_mb: Option<f32>,
    pub allocs_per_s: Option<f32>,
    pub alloc_mib_per_s: Option<f32>,
    pub players: u32,
    pub chunks: u32,
    pub entities: u32,
}

/// TPS over the last 1, 5 and 15 minutes; None until a sample exists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tps {
    pub one: Option<f32>,
    pub five: Option<f32>,
    pub fifteen: Option<f32>,
}

struct Ring {
    slots: Vec<Slot>,
    capacity: usize,
    pushed: u64,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            capacity,
            pushed: 0,
        }
    }

    fn push(&mut self, slot: Slot) {
        if self.slots.len() < self.capacity {
            self.slots.push(slot);
        } else {
            let at = (self.pushed % self.capacity as u64) as usize;
            self.slots[at] = slot;
        }
        self.pushed += 1;
    }

    fn len(&self) -> usize {
        self.slots.len()
    }

    /// Index (in push order) of the oldest kept sample.
    fn first(&self) -> u64 {
        self.pushed - self.slots.len() as u64
    }

    fn get(&self, index: u64) -> Slot {
        self.slots[(index % self.capacity as u64) as usize]
    }
}

#[derive(Default)]
struct Acc {
    n: u32,
    sum: [f64; FIELDS],
    count: [u32; FIELDS],
    max: [f32; FIELDS],
}

impl Acc {
    fn add(&mut self, slot: &Slot) {
        for (i, &v) in slot.0.iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            self.sum[i] += v as f64;
            self.max[i] = if self.count[i] == 0 {
                v
            } else {
                self.max[i].max(v)
            };
            self.count[i] += 1;
        }
        self.n += 1;
    }

    fn finish(&mut self) -> Slot {
        let mut slot = EMPTY;
        for i in 0..FIELDS {
            if self.count[i] > 0 {
                slot.0[i] = if MAX_FIELDS.contains(&i) {
                    self.max[i]
                } else {
                    (self.sum[i] / self.count[i] as f64) as f32
                };
            }
        }
        *self = Acc::default();
        slot
    }
}

struct Tier {
    step_s: u32,
    /// Samples of the tier below that make one of this tier's.
    ratio: u32,
    ring: Ring,
    acc: Acc,
}

/// The durations of the ticks or frames inside the second being recorded.
struct Second {
    n: u32,
    sum: f64,
    max: f32,
    durations: Vec<f32>,
}

impl Second {
    fn new() -> Self {
        Self {
            n: 0,
            sum: 0.0,
            max: 0.0,
            durations: Vec::with_capacity(SECOND_BUFFER),
        }
    }

    /// Nearest-rank percentile of the sorted durations.
    fn percentile(&self, p: f64) -> f32 {
        let rank = ((p * self.durations.len() as f64).ceil() as usize).max(1);
        self.durations[rank - 1]
    }
}

/// The always-on health history of one side: four fixed-size tiers, about 300 KB however long the game runs.
pub struct HealthRecorder {
    side: Side,
    base: f64,
    tiers: [Tier; 4],
    /// Index (from `base`) of the second being recorded.
    current: u64,
    second: Second,
    gauges: Gauges,
}

impl HealthRecorder {
    /// `start` is the recorder's clock reading in seconds when it begins; every time passed in later is on that clock.
    pub fn new(side: Side, start: f64) -> Self {
        let mut ratio = 1;
        let tiers = TIERS.map(|(step_s, cap)| {
            let tier = Tier {
                step_s,
                ratio: step_s / ratio,
                ring: Ring::new(cap),
                acc: Acc::default(),
            };
            ratio = step_s;
            tier
        });
        Self {
            side,
            base: start,
            tiers,
            current: 0,
            second: Second::new(),
            gauges: Gauges::default(),
        }
    }

    pub fn side(&self) -> Side {
        self.side
    }

    /// Bytes the rings hold, from their real capacities.
    pub fn memory_bytes(&self) -> usize {
        self.tiers
            .iter()
            .map(|t| t.ring.slots.capacity() * size_of::<Slot>())
            .sum::<usize>()
            + self.second.durations.capacity() * size_of::<f32>()
    }

    /// Replaces the gauges that seconds closed from now on carry.
    pub fn set_gauges(&mut self, gauges: Gauges) {
        self.gauges = gauges;
    }

    /// Closes every second that ended before `t`; seconds with no tick or frame count as zero TPS or FPS.
    pub fn advance(&mut self, t: f64) {
        let target = (t - self.base).max(0.0).floor() as u64;
        while self.current < target {
            self.close_second();
            self.current += 1;
        }
    }

    /// One tick (server) or frame (client) that took `ms`, finished at `t`.
    pub fn record(&mut self, t: f64, ms: f32) {
        self.advance(t);
        let s = &mut self.second;
        s.n += 1;
        s.sum += ms as f64;
        s.max = if s.n == 1 { ms } else { s.max.max(ms) };
        if s.durations.len() < SECOND_BUFFER {
            s.durations.push(ms);
        }
    }

    fn close_second(&mut self) {
        let mut slot = EMPTY;
        let s = &mut self.second;
        let rate = s.n as f32;
        s.durations.sort_by(f32::total_cmp);
        let timed = s.n > 0;
        let (avg, p50, p95, p99, max) = if timed {
            (
                (s.sum / s.n as f64) as f32,
                s.percentile(0.50),
                s.percentile(0.95),
                s.percentile(0.99),
                s.max,
            )
        } else {
            (f32::NAN, f32::NAN, f32::NAN, f32::NAN, f32::NAN)
        };
        if self.side == Side::Client {
            slot.0[FPS] = rate;
            slot.0[FRAME_P50] = p50;
            slot.0[FRAME_P95] = p95;
            slot.0[FRAME_P99] = p99;
            slot.0[FRAME_MAX] = max;
        } else {
            slot.0[TPS] = rate;
            slot.0[MSPT_AVG] = avg;
            slot.0[MSPT_P50] = p50;
            slot.0[MSPT_P95] = p95;
            slot.0[MSPT_MAX] = max;
        }
        s.n = 0;
        s.sum = 0.0;
        s.max = 0.0;
        s.durations.clear();
        let g = &self.gauges;
        let opt = |v: Option<f32>| v.unwrap_or(f32::NAN);
        slot.0[FRAME_CPU_P50] = opt(g.frame_cpu_ms_p50);
        slot.0[FRAME_GPU_P50] = opt(g.frame_gpu_ms_p50);
        slot.0[CPU_PROCESS] = g.cpu_process_cores;
        slot.0[CPU_SYSTEM] = g.cpu_system_cores;
        slot.0[RSS] = g.rss_mb;
        slot.0[PHYSICAL] = g.physical_used_mb;
        slot.0[SWAP] = g.swap_used_mb;
        slot.0[ALLOCATOR_COMMIT] = opt(g.allocator_commit_mb);
        slot.0[ALLOCS_PER_S] = opt(g.allocs_per_s);
        slot.0[ALLOC_MIB_PER_S] = opt(g.alloc_mib_per_s);
        slot.0[PLAYERS] = g.players as f32;
        slot.0[CHUNKS] = g.chunks as f32;
        slot.0[ENTITIES] = g.entities as f32;
        self.push_fine(slot);
    }

    fn push_fine(&mut self, slot: Slot) {
        self.tiers[0].ring.push(slot);
        let mut carry = slot;
        for tier in self.tiers.iter_mut().skip(1) {
            tier.acc.add(&carry);
            if tier.acc.n < tier.ratio {
                return;
            }
            carry = tier.acc.finish();
            tier.ring.push(carry);
        }
    }

    /// The tiers as `Capture.history` windows, oldest sample first, with `t0` relative to `origin`; empty tiers are left out.
    pub fn history(&self, origin: f64) -> Vec<HealthWindow> {
        self.tiers
            .iter()
            .filter(|t| t.ring.len() > 0)
            .map(|t| {
                let first = t.ring.first();
                HealthWindow {
                    side: self.side as i32,
                    step_s: t.step_s as f32,
                    t0: self.base + (first * t.step_s as u64) as f64 - origin,
                    samples: (first..t.ring.pushed)
                        .map(|i| self.to_proto(&t.ring.get(i), 0.0))
                        .collect(),
                }
            })
            .collect()
    }

    /// The 1 s samples closed since `cursor` (start from 0), as `Capture.health` entries with `t` relative to `origin`.
    pub fn samples_since(&self, cursor: u64, origin: f64) -> (Vec<HealthSample>, u64) {
        let ring = &self.tiers[0].ring;
        let from = cursor.max(ring.first());
        let samples = (from..ring.pushed)
            .map(|i| self.to_proto(&ring.get(i), self.base + i as f64 - origin))
            .collect();
        (samples, ring.pushed)
    }

    /// Mean TPS over the last 1, 5 and 15 minutes of closed seconds, each from the finest tier that spans it.
    pub fn tps(&self) -> Tps {
        let over = |window_s: u32| {
            let tier = self
                .tiers
                .iter()
                .find(|t| t.step_s * t.ring.capacity as u32 >= window_s)?;
            let want = (window_s / tier.step_s) as u64;
            let first = tier.ring.first().max(tier.ring.pushed.saturating_sub(want));
            let (sum, n) = (first..tier.ring.pushed)
                .map(|i| tier.ring.get(i).0[TPS])
                .filter(|v| !v.is_nan())
                .fold((0.0f64, 0u32), |(s, n), v| (s + v as f64, n + 1));
            (n > 0).then(|| (sum / n as f64) as f32)
        };
        Tps {
            one: over(60),
            five: over(300),
            fifteen: over(900),
        }
    }

    fn to_proto(&self, slot: &Slot, t: f64) -> HealthSample {
        let f = &slot.0;
        let opt = |i: usize| (!f[i].is_nan()).then_some(f[i]);
        let num = |i: usize| if f[i].is_nan() { 0.0 } else { f[i] };
        HealthSample {
            side: self.side as i32,
            t,
            tps: opt(TPS),
            mspt_avg: opt(MSPT_AVG),
            mspt_p50: opt(MSPT_P50),
            mspt_p95: opt(MSPT_P95),
            mspt_max: opt(MSPT_MAX),
            fps: opt(FPS),
            frame_ms_p50: opt(FRAME_P50),
            frame_ms_p95: opt(FRAME_P95),
            frame_ms_p99: opt(FRAME_P99),
            frame_ms_max: opt(FRAME_MAX),
            frame_cpu_ms_p50: opt(FRAME_CPU_P50),
            frame_gpu_ms_p50: opt(FRAME_GPU_P50),
            cpu_process_cores: num(CPU_PROCESS),
            cpu_system_cores: num(CPU_SYSTEM),
            rss_mb: num(RSS),
            physical_used_mb: num(PHYSICAL),
            swap_used_mb: num(SWAP),
            allocator_commit_mb: opt(ALLOCATOR_COMMIT),
            allocs_per_s: opt(ALLOCS_PER_S),
            alloc_mib_per_s: opt(ALLOC_MIB_PER_S),
            players: num(PLAYERS).round() as u32,
            chunks: num(CHUNKS).round() as u32,
            entities: num(ENTITIES).round() as u32,
        }
    }
}
