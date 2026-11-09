//! Motion: quiet animation for Horizon's HUD overlays, as pure functions of time. An overlay
//! keeps the moments things happened (it opened, the selection moved, a page turned) and asks
//! where a tween is at the frame's [`Clock`]; nothing here holds state or runs on its own, and
//! no motion ever decides what input does.
//!
//! The curves are the design system's (`horizon_theme::QUICK`, `MOVE`): a [`Tween`] takes one
//! of them, at the theme's duration or a shorter one of its own.
//!
//! The game builds the clock once a frame (`Clock::new(now, still)`). A still clock (the game is
//! paused, frame-stepped by `HORIZON_STILL` or a screenshot sequence, or a repro recording or
//! replay runs) shows every tween settled, so recordings and screenshots come out the same each
//! run.
//!
//! ```
//! use horizon_client_render::hud_kit::motion::{self, Clock, Tween};
//! use std::time::{Duration, Instant};
//!
//! let opened = Instant::now();
//! let open = Tween::new(Duration::from_millis(90), horizon_theme::QUICK);
//! // Half way through: partly faded in, partly grown from 94%.
//! let clock = Clock::new(opened + Duration::from_millis(45), false);
//! let alpha = motion::fade(open, clock, opened);
//! let scale = motion::scale_in(open, clock, opened, 0.94);
//! assert!(alpha > 0.0 && alpha < 1.0 && scale > 0.94 && scale < 1.0);
//! // A still clock shows the end.
//! let still = Clock::new(opened, true);
//! assert_eq!(motion::fade(open, still, opened), 1.0);
//! ```
use horizon_theme::Motion;
use std::time::{Duration, Instant};

/// The time a frame shows, built once a frame by the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// The frame's moment (monotonic).
    pub now: Instant,
    /// Motion stands at its end: the game is paused, frame-stepped or recording.
    pub still: bool,
}

impl Clock {
    /// The clock for a frame at `now`; `still` shows every tween settled.
    pub fn new(now: Instant, still: bool) -> Self {
        Self { now, still }
    }

    /// How long ago `since` was, at this frame (zero for a moment after it).
    pub fn since(self, since: Instant) -> Duration {
        self.now.saturating_duration_since(since)
    }
}

/// A duration and one of the design system's easing curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    /// How long it runs.
    pub duration: Duration,
    /// The CSS `cubic-bezier` control points of its curve (a `horizon_theme::Motion`'s).
    pub curve: [f32; 4],
}

impl Tween {
    /// `motion`'s curve over `duration`: the dial's motions are shorter than the launcher's, on
    /// the same curves.
    pub const fn new(duration: Duration, motion: Motion) -> Self {
        Self {
            duration,
            curve: motion.curve,
        }
    }

    /// A theme motion as it is: its own duration and curve.
    pub const fn of(motion: Motion) -> Self {
        Self::new(motion.duration(), motion)
    }

    /// The linear progress, 0 to 1, `since` a start at this clock; 1 on a still clock.
    pub fn linear(self, clock: Clock, since: Instant) -> f32 {
        if clock.still || self.duration.is_zero() {
            return 1.0;
        }
        (clock.since(since).as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// The eased progress, 0 to 1, on the tween's curve.
    pub fn progress(self, clock: Clock, since: Instant) -> f32 {
        horizon_theme::bezier(self.curve, self.linear(clock, since))
    }

    /// Whether it has finished (always, on a still clock).
    pub fn done(self, clock: Clock, since: Instant) -> bool {
        self.linear(clock, since) >= 1.0
    }

    /// From `from` to `to` along the eased progress.
    pub fn lerp(self, clock: Clock, since: Instant, from: f32, to: f32) -> f32 {
        from + (to - from) * self.progress(clock, since)
    }
}

/// An alpha from 0 to 1 as something fades in.
pub fn fade(tween: Tween, clock: Clock, since: Instant) -> f32 {
    tween.progress(clock, since)
}

/// An alpha from 1 to 0 as something fades out.
pub fn fade_out(tween: Tween, clock: Clock, since: Instant) -> f32 {
    1.0 - tween.progress(clock, since)
}

/// A scale growing from `from` (0.94: 94%) to 1.
pub fn scale_in(tween: Tween, clock: Clock, since: Instant, from: f32) -> f32 {
    tween.lerp(clock, since, from, 1.0)
}

/// An offset in GUI px from `distance` back to 0: something arriving from `distance` away.
pub fn slide(tween: Tween, clock: Clock, since: Instant, distance: f32) -> f32 {
    tween.lerp(clock, since, distance, 0.0)
}

/// A flash's strength, 1 at its start falling to 0: how much white goes over what flashed.
pub fn flash(tween: Tween, clock: Clock, since: Instant) -> f32 {
    1.0 - tween.linear(clock, since)
}

/// From angle `from` to `to` (radians) the short way round, along the eased progress: a lit
/// sector easing to the next slot never sweeps the long way.
pub fn turn(tween: Tween, clock: Clock, since: Instant, from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let delta = (to - from + PI).rem_euclid(TAU) - PI;
    from + delta * tween.progress(clock, since)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_4, PI};

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn at(start: Instant, ms: u64) -> Clock {
        Clock::new(start + MS(ms), false)
    }

    #[test]
    fn a_tween_runs_from_its_start_to_its_end_and_holds() {
        let t0 = Instant::now();
        let tween = Tween::new(MS(80), horizon_theme::MOVE);
        assert_eq!(tween.progress(at(t0, 0), t0), 0.0);
        assert_eq!(tween.progress(at(t0, 80), t0), 1.0);
        assert_eq!(tween.progress(at(t0, 500), t0), 1.0);
        // MOVE eases out: well past half way at half the time.
        assert!(tween.progress(at(t0, 40), t0) > 0.75);
        // A frame before the start (a moment recorded later this frame) is the start.
        assert_eq!(tween.progress(Clock::new(t0, false), t0 + MS(5)), 0.0);
        assert!(!tween.done(at(t0, 79), t0) && tween.done(at(t0, 80), t0));
    }

    #[test]
    fn a_still_clock_shows_every_motion_settled() {
        let t0 = Instant::now();
        let still = Clock::new(t0, true);
        let tween = Tween::of(horizon_theme::QUICK);
        assert_eq!(tween.duration, horizon_theme::QUICK.duration());
        assert_eq!(fade(tween, still, t0), 1.0);
        assert_eq!(fade_out(tween, still, t0), 0.0);
        assert_eq!(scale_in(tween, still, t0, 0.94), 1.0);
        assert_eq!(slide(tween, still, t0, 8.0), 0.0);
        assert_eq!(flash(tween, still, t0), 0.0);
        assert!((turn(tween, still, t0, 0.0, 1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_helpers_start_where_they_say() {
        let t0 = Instant::now();
        let tween = Tween::new(MS(100), horizon_theme::QUICK);
        let c = at(t0, 0);
        assert_eq!(fade(tween, c, t0), 0.0);
        assert_eq!(fade_out(tween, c, t0), 1.0);
        assert_eq!(scale_in(tween, c, t0, 0.94), 0.94);
        assert_eq!(slide(tween, c, t0, -8.0), -8.0);
        assert_eq!(flash(tween, c, t0), 1.0);
        // A flash falls evenly.
        assert!((flash(tween, at(t0, 25), t0) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_turn_takes_the_short_way_round() {
        let t0 = Instant::now();
        let tween = Tween::new(MS(80), horizon_theme::MOVE);
        // From up-left (7/8 of a turn) to up-right: through up, not round the bottom.
        let from = 7.0 * FRAC_PI_4;
        let half = turn(tween, at(t0, 1), t0, from, FRAC_PI_4);
        assert!(half > from && half < from + PI / 2.0 + 1e-4, "{half}");
        let end = turn(tween, at(t0, 80), t0, from, FRAC_PI_4);
        assert!((end - (2.0 * PI + FRAC_PI_4)).abs() < 1e-4, "{end}");
    }
}
