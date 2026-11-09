//! GPU Culling's Auto mode: the setting switched by what limits the frame, measured here.
//!
//! GPU Culling moves the walk over the near sections and the listing of their draws from the
//! render thread to the GPU. That pays where the CPU is the slower of the two and costs where
//! the GPU is, and which a machine is can't be told from its parts: a fast CPU with a modest
//! GPU, a slow one with a strong GPU and every mix between depend on the scene, the render
//! distance and the resolution. So Auto watches the frames' own CPU and GPU times, and only
//! where the CPU is clearly the slower one it tries GPU Culling, measures the frame again, and
//! keeps it if the frame got faster. A trial that didn't help is undone and not repeated for
//! minutes of play, then for longer, then not until the next start, so a machine or a scene it
//! doesn't suit loses a few frames' time a few times, not every half minute.
//!
//! A verdict is the medians of a window of frames (`WINDOW`), which a few slow frames (a chunk
//! load, say) don't move. Nothing here reads the GPU or draws anything: the caller feeds it the
//! frames and applies what it answers.

/// Frames in one verdict.
pub const WINDOW: usize = 240;
/// The CPU's time must be this many times the GPU's for a window to count as CPU-bound.
const CPU_BOUND: f32 = 1.3;
/// Below this CPU time, in milliseconds, a frame is fast enough that neither matters.
const WORTH_TRYING_MS: f32 = 1.5;
/// A trial is kept when the frame's busier side dropped under this share of what it was.
const KEEP_BELOW: f32 = 0.97;
/// Windows of CPU-bound frames in a row before a trial.
const STREAK: u8 = 2;
/// Windows left out after a switch, while its effects settle.
const SETTLE: u8 = 1;
/// How long, in milliseconds of frames watched, after the first and the second trial that didn't
/// help before another; after a third there is none.
const RETRY_AFTER_MS: [f64; 2] = [5.0 * 60_000.0, 20.0 * 60_000.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Off, looking for a CPU-bound frame.
    Watching,
    /// On for a trial: left out `SETTLE` windows, then measured against the frame before.
    Trial,
    /// The trial made the frame faster: on from here.
    Kept,
}

/// What Auto has decided and why, a step at a time.
#[derive(Clone, Debug)]
pub struct GpuCullingAuto {
    cpu: Vec<f32>,
    gpu: Vec<f32>,
    on: bool,
    state: State,
    /// CPU-bound windows in a row.
    streak: u8,
    /// The frame's busier side before the trial, in milliseconds.
    before: f32,
    /// Windows still left out after a switch.
    settling: u8,
    /// The frames watched so far, in milliseconds of their own time.
    clock_ms: f64,
    /// When the next trial may start, on that clock.
    retry_at_ms: f64,
    /// Trials that didn't help.
    rejected: usize,
}

impl Default for GpuCullingAuto {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuCullingAuto {
    pub fn new() -> Self {
        Self {
            cpu: Vec::with_capacity(WINDOW),
            gpu: Vec::with_capacity(WINDOW),
            on: false,
            state: State::Watching,
            streak: 0,
            before: 0.0,
            settling: 0,
            clock_ms: 0.0,
            retry_at_ms: 0.0,
            rejected: 0,
        }
    }

    /// Whether Auto has GPU Culling on now.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Where Auto is, for the status: `watching`, `trial` or `kept`.
    pub fn state(&self) -> &'static str {
        match self.state {
            State::Watching => "watching",
            State::Trial => "trial",
            State::Kept => "kept",
        }
    }

    /// Back to the start: off, with nothing measured (the mode changed, or the world did).
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// One frame's CPU time, a GPU time where its timestamps have arrived, and its whole time,
    /// in milliseconds. Answers the new state when this frame completed a window that changed it.
    pub fn observe(&mut self, cpu_ms: f32, gpu_ms: Option<f32>, frame_ms: f32) -> Option<bool> {
        self.clock_ms += f64::from(frame_ms);
        self.cpu.push(cpu_ms);
        if let Some(gpu) = gpu_ms {
            self.gpu.push(gpu);
        }
        if self.cpu.len() < WINDOW {
            return None;
        }
        let cpu = median(&mut self.cpu);
        // A GPU that can't be timed leaves nothing to compare with: Auto stays as it is.
        let gpu = (self.gpu.len() >= WINDOW / 4).then(|| median(&mut self.gpu));
        self.cpu.clear();
        self.gpu.clear();
        self.verdict(cpu, gpu?)
    }

    fn verdict(&mut self, cpu: f32, gpu: f32) -> Option<bool> {
        if self.settling > 0 {
            self.settling -= 1;
            return None;
        }
        match self.state {
            State::Kept => None,
            State::Watching => {
                if self.clock_ms < self.retry_at_ms {
                    return None;
                }
                let bound = cpu >= WORTH_TRYING_MS && cpu > gpu * CPU_BOUND;
                self.streak = if bound { self.streak + 1 } else { 0 };
                if self.streak < STREAK {
                    return None;
                }
                self.streak = 0;
                self.before = cpu.max(gpu);
                self.state = State::Trial;
                self.settling = SETTLE;
                self.on = true;
                Some(true)
            }
            State::Trial => {
                if cpu.max(gpu) < self.before * KEEP_BELOW {
                    self.state = State::Kept;
                    return None;
                }
                self.state = State::Watching;
                self.retry_at_ms = RETRY_AFTER_MS
                    .get(self.rejected)
                    .map_or(f64::INFINITY, |wait| self.clock_ms + wait);
                self.rejected += 1;
                self.on = false;
                Some(false)
            }
        }
    }
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window of frames at constant times, each as long as its busier side; the last answer.
    fn window(auto: &mut GpuCullingAuto, cpu: f32, gpu: f32) -> Option<bool> {
        (0..WINDOW)
            .filter_map(|_| auto.observe(cpu, Some(gpu), cpu.max(gpu)))
            .last()
    }

    /// A machine whose CPU limits the frame is tried, and keeps GPU Culling when the frame
    /// gets faster.
    #[test]
    fn a_cpu_bound_frame_tries_it_and_keeps_it_when_it_helps() {
        let mut auto = GpuCullingAuto::new();
        assert!(!auto.on());
        // One CPU-bound window is not yet a verdict.
        assert_eq!(window(&mut auto, 5.0, 2.0), None);
        assert_eq!(window(&mut auto, 5.0, 2.0), Some(true));
        assert!(auto.on());
        assert_eq!(auto.state(), "trial");
        // The window after the switch is left out, whatever it shows.
        assert_eq!(window(&mut auto, 9.0, 9.0), None);
        // The frame's busier side went from 5.0 to 3.0 ms.
        assert_eq!(window(&mut auto, 3.0, 2.6), None);
        assert_eq!(auto.state(), "kept");
        assert!(auto.on());
        // Kept stays kept, even if a later scene is GPU-bound.
        assert_eq!(window(&mut auto, 1.0, 8.0), None);
        assert!(auto.on());
    }

    /// Where it didn't help, it is undone, and not tried again for 5 minutes, then 20, then
    /// not at all: a scene it doesn't suit is not tried every half minute.
    #[test]
    fn a_trial_that_does_not_help_is_undone_and_waits_longer_each_time() {
        let mut auto = GpuCullingAuto::new();
        // One window is 240 frames of 5 ms: 1.2 s. A trial, and its rejection:
        let reject = |auto: &mut GpuCullingAuto| {
            window(auto, 5.0, 2.0);
            assert_eq!(window(auto, 5.0, 2.0), Some(true));
            window(auto, 5.0, 2.0);
            // The GPU took over the frame: 5.0 ms before, 5.4 after.
            assert_eq!(window(auto, 2.5, 5.4), Some(false));
            assert!(!auto.on());
            assert_eq!(auto.state(), "watching");
        };
        reject(&mut auto);
        // CPU-bound as ever, and left alone for 5 minutes of frames: 250 windows.
        let mut waited = 0;
        while window(&mut auto, 5.0, 2.0).is_none() {
            waited += 1;
        }
        assert!((248..=252).contains(&waited), "{waited}");
        // That window started the second trial; it is rejected too.
        window(&mut auto, 5.0, 2.0);
        assert_eq!(window(&mut auto, 2.5, 5.4), Some(false));
        // Now 20 minutes: 1000 windows.
        let mut waited = 0;
        while window(&mut auto, 5.0, 2.0).is_none() {
            waited += 1;
        }
        assert!((998..=1002).contains(&waited), "{waited}");
        window(&mut auto, 5.0, 2.0);
        assert_eq!(window(&mut auto, 2.5, 5.4), Some(false));
        // A third rejection is for good.
        for _ in 0..5000 {
            assert_eq!(window(&mut auto, 5.0, 2.0), None);
        }
        assert!(!auto.on());
    }

    /// A GPU-bound machine is never switched: its frame is the GPU's.
    #[test]
    fn a_gpu_bound_frame_is_left_alone() {
        let mut auto = GpuCullingAuto::new();
        for _ in 0..20 {
            assert_eq!(window(&mut auto, 0.5, 3.6), None);
        }
        assert!(!auto.on());
    }

    /// Within the margin neither side limits the frame, and a single CPU-bound window among
    /// others breaks no streak into a trial.
    #[test]
    fn neither_a_balanced_nor_an_occasional_cpu_bound_frame_is_tried() {
        let mut auto = GpuCullingAuto::new();
        for _ in 0..10 {
            assert_eq!(window(&mut auto, 3.0, 2.6), None);
        }
        for _ in 0..10 {
            assert_eq!(window(&mut auto, 5.0, 2.0), None);
            assert_eq!(window(&mut auto, 2.0, 2.0), None);
        }
        assert!(!auto.on());
    }

    /// Fast frames aren't worth a trial, bound or not.
    #[test]
    fn a_frame_under_the_floor_is_not_tried() {
        let mut auto = GpuCullingAuto::new();
        for _ in 0..10 {
            assert_eq!(window(&mut auto, 1.2, 0.2), None);
        }
        assert!(!auto.on());
    }

    /// Without GPU times (a GPU that can't be timed) there is nothing to compare.
    #[test]
    fn frames_without_gpu_times_decide_nothing() {
        let mut auto = GpuCullingAuto::new();
        for _ in 0..WINDOW * 10 {
            assert_eq!(auto.observe(8.0, None, 8.0), None);
        }
        assert!(!auto.on());
    }

    /// A spike in a window doesn't move its verdict: the median ignores it.
    #[test]
    fn a_few_slow_frames_do_not_make_a_window_cpu_bound() {
        let mut auto = GpuCullingAuto::new();
        for _ in 0..4 {
            let mut answer = None;
            for i in 0..WINDOW {
                // One frame in ten is a 30 ms chunk load, the rest are GPU-bound.
                let cpu = if i % 10 == 0 { 30.0 } else { 0.8 };
                answer = answer.or(auto.observe(cpu, Some(3.0), 3.0));
            }
            assert_eq!(answer, None);
        }
        assert!(!auto.on());
    }

    #[test]
    fn reset_starts_over() {
        let mut auto = GpuCullingAuto::new();
        window(&mut auto, 5.0, 2.0);
        window(&mut auto, 5.0, 2.0);
        assert!(auto.on());
        auto.reset();
        assert!(!auto.on());
        assert_eq!(auto.state(), "watching");
        assert_eq!(window(&mut auto, 5.0, 2.0), None);
    }
}
