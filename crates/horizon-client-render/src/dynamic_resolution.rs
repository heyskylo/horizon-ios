//! Dynamic Resolution: the world's resolution scale follows the GPU's frame time toward a target
//! frame rate, between the Resolution Scale setting (the floor) and 100%.
//!
//! The setting is the lowest scale the player accepts; where the GPU has room for more, the
//! scale rises toward 100% a step at a time, and falls back toward the floor where frames take
//! longer than the target allows. A verdict is the median GPU time of a window of frames, which
//! a few slow frames (a chunk load) don't move, and a step up is taken only when the frame would
//! still fit if its cost grew with the pixels: most of a frame doesn't, so it errs toward
//! staying.
//!
//! Each change has a price: new render targets, and history that starts over (TAA, and a
//! shader pack's own: its denoised light, reflections and accumulation). So a shader pack gets
//! the gentle pace (long windows, big steps, a long rest after each change): walking from a
//! heavy scene to a light one changes the scale once, not every few frames. Without a pack the
//! quick pace follows the scene within a second.
//!
//! Nothing here draws or reads the GPU: the caller feeds it the frames and applies the scale it
//! answers.

/// How fast the scale follows the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// A shader pack: its history is expensive to start over.
    Gentle,
    /// Horizon's own renderer.
    Quick,
}

impl Pace {
    /// Milliseconds of frames in one verdict.
    fn window_ms(self) -> f64 {
        match self {
            Pace::Gentle => 3_000.0,
            Pace::Quick => 500.0,
        }
    }
    /// Milliseconds of frames after a change before the next verdict counts, while the new
    /// targets and the restarted history settle.
    fn settle_ms(self) -> f64 {
        match self {
            Pace::Gentle => 8_000.0,
            Pace::Quick => 1_000.0,
        }
    }
    /// Percentage points per change.
    fn step(self) -> u32 {
        match self {
            Pace::Gentle => 10,
            Pace::Quick => 5,
        }
    }
}

/// The frame is too slow when its GPU time passes the target's frame time by this share.
const DOWN_ABOVE: f32 = 1.05;
/// A step up is taken when the frame, its cost grown with the pixels, stays under this share.
const UP_BELOW: f32 = 0.95;
/// Timed frames a window needs for a verdict (a GPU without timestamps gives none).
const MIN_TIMED: usize = 8;

/// The scale in use and what it has measured since the last verdict.
#[derive(Clone, Debug, Default)]
pub struct DynamicResolution {
    gpu: Vec<f32>,
    window_ms: f64,
    since_change_ms: f64,
    /// The scale in use, in percent; `None` until the first change (the floor).
    scale: Option<u32>,
}

impl DynamicResolution {
    pub fn new() -> Self {
        Self::default()
    }

    /// Back to the floor, with nothing measured (turned off, or the world changed).
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// The scale in use, in percent, for a Resolution Scale of `floor`: never below it, never
    /// above 100% unless the floor itself is (then it is the floor).
    pub fn scale(&self, floor: u32) -> u32 {
        self.scale.unwrap_or(floor).clamp(floor, floor.max(100))
    }

    /// One frame: its GPU time where its timestamps have arrived, and its whole time, in
    /// milliseconds. Answers the new scale, in percent, when this frame completed a window that
    /// changed it.
    pub fn observe(
        &mut self,
        gpu_ms: Option<f32>,
        frame_ms: f32,
        floor: u32,
        target_fps: u32,
        pace: Pace,
    ) -> Option<u32> {
        self.window_ms += f64::from(frame_ms);
        self.since_change_ms += f64::from(frame_ms);
        if let Some(gpu) = gpu_ms {
            self.gpu.push(gpu);
        }
        // A window lasts its time and holds enough timed frames: a slow scene (or timestamps that
        // arrive with gaps) keeps gathering past the time rather than starting over with too few,
        // which would never judge the scenes that most need a lower scale. A GPU that can't be
        // timed gathers nothing and so leaves the scale alone.
        if self.window_ms < pace.window_ms() || self.gpu.len() < MIN_TIMED {
            return None;
        }
        self.window_ms = 0.0;
        let timed = std::mem::take(&mut self.gpu);
        // At or above 100% there is nothing to vary; a change still settling isn't judged yet.
        if floor >= 100 || self.since_change_ms < pace.settle_ms() {
            return None;
        }
        let gpu = median(timed);
        let target = 1000.0 / target_fps.max(1) as f32;
        let now = self.scale(floor);
        let next = if gpu > target * DOWN_ABOVE && now > floor {
            now.saturating_sub(pace.step()).max(floor)
        } else if now < 100 {
            let up = (now + pace.step()).min(100);
            let grown = (up as f32 / now as f32).powi(2);
            if gpu * grown < target * UP_BELOW {
                up
            } else {
                now
            }
        } else {
            now
        };
        if next == now {
            return None;
        }
        self.scale = Some(next);
        self.since_change_ms = 0.0;
        Some(next)
    }
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ms` milliseconds of frames, each `gpu` long on the GPU and `frame` long in all; the last
    /// answer.
    fn run(
        d: &mut DynamicResolution,
        ms: f64,
        gpu: f32,
        frame: f32,
        floor: u32,
        pace: Pace,
    ) -> Option<u32> {
        let mut answer = None;
        let mut t = 0.0;
        while t < ms {
            if let Some(scale) = d.observe(Some(gpu), frame, floor, 60, pace) {
                answer = Some(scale);
            }
            t += f64::from(frame);
        }
        answer
    }

    #[test]
    fn it_starts_at_the_floor_and_climbs_while_there_is_room() {
        let mut d = DynamicResolution::new();
        assert_eq!(d.scale(70), 70);
        // 8 ms of a 16.7 ms budget: room to grow, one step per window and settle.
        assert_eq!(run(&mut d, 1_200.0, 8.0, 8.0, 70, Pace::Quick), Some(75));
        assert_eq!(run(&mut d, 30_000.0, 8.0, 8.0, 70, Pace::Quick), Some(100));
        assert_eq!(d.scale(70), 100);
    }

    #[test]
    fn a_slow_frame_steps_down_but_never_below_the_floor() {
        let mut d = DynamicResolution::new();
        run(&mut d, 30_000.0, 8.0, 8.0, 70, Pace::Quick);
        assert_eq!(d.scale(70), 100);
        // 25 ms against 16.7: down a step at a time, to the floor and no further.
        run(&mut d, 60_000.0, 25.0, 25.0, 70, Pace::Quick);
        assert_eq!(d.scale(70), 70);
    }

    #[test]
    fn a_slow_scene_still_steps_down() {
        let mut d = DynamicResolution::new();
        run(&mut d, 30_000.0, 8.0, 8.0, 70, Pace::Quick);
        assert_eq!(d.scale(70), 100);
        // 10 FPS: five frames in a 500 ms window, fewer than a verdict needs; the window keeps
        // gathering until it has them.
        run(&mut d, 60_000.0, 100.0, 100.0, 70, Pace::Quick);
        assert_eq!(d.scale(70), 70);
    }

    #[test]
    fn a_frame_near_the_target_stays_put() {
        let mut d = DynamicResolution::new();
        // 15 ms: under the target, but a step up (75 -> 80%) would make it 17 ms.
        assert_eq!(run(&mut d, 60_000.0, 15.0, 16.0, 75, Pace::Quick), None);
        assert_eq!(d.scale(75), 75);
    }

    #[test]
    fn the_gentle_pace_takes_big_steps_and_long_rests() {
        let mut d = DynamicResolution::new();
        // The first window (3 s) can't count yet: a change rests 8 s, and the first counts
        // from the start.
        assert_eq!(run(&mut d, 7_000.0, 8.0, 8.0, 70, Pace::Gentle), None);
        assert_eq!(run(&mut d, 3_000.0, 8.0, 8.0, 70, Pace::Gentle), Some(80));
        // Then nothing for the 8 s rest.
        assert_eq!(run(&mut d, 7_000.0, 8.0, 8.0, 70, Pace::Gentle), None);
        assert_eq!(d.scale(70), 80);
    }

    #[test]
    fn nothing_changes_at_or_above_100_or_without_gpu_times() {
        let mut d = DynamicResolution::new();
        assert_eq!(run(&mut d, 30_000.0, 30.0, 30.0, 100, Pace::Quick), None);
        assert_eq!(d.scale(120), 120);
        let mut d = DynamicResolution::new();
        for _ in 0..1_000 {
            assert_eq!(d.observe(None, 8.0, 70, 60, Pace::Quick), None);
        }
        assert_eq!(d.scale(70), 70);
    }

    #[test]
    fn reset_goes_back_to_the_floor() {
        let mut d = DynamicResolution::new();
        run(&mut d, 30_000.0, 8.0, 8.0, 70, Pace::Quick);
        d.reset();
        assert_eq!(d.scale(70), 70);
    }
}
