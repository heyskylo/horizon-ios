//! The Wardrobe's live view, without the drawing: where the camera is, the clock items animate
//! by, an emote playing, and when the picture has to be drawn again. The launcher draws with
//! `preview::render` only when `Stage::frame` says so: never while nothing moves, and nothing
//! moves while the window is in the background.
//!
//! - **Animations** run on the game's clock (20 ticks a second), paused while the window is in
//!   the background or nothing animates, so they go on where they stopped.
//! - **At most 20 pictures a second** while something animates (`ANIMATION_FRAME`: one a game
//!   tick, the unit animations are written in), 60 while the player drags or scrolls
//!   (`MOVE_FRAME`); a change sooner than that waits for its turn.
//! - **Emotes** (`BodyMotion`): poses of the player model played once on the same clock; the
//!   items follow their bones as posed (`preview::Draw::item_posed`).

use crate::Pose;
use crate::preview::Camera;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The game's ticks, which item animations count in.
pub const TICKS_PER_SECOND: f32 = 20.0;
/// The shortest gap between two pictures while something animates: 20 a second, one a tick.
pub const ANIMATION_FRAME: Duration = Duration::from_millis(50);
/// The shortest gap between two pictures for a drag or a scroll: 60 a second.
pub const MOVE_FRAME: Duration = Duration::from_millis(16);

/// A body animation played once on the player model (an emote): the poses of its parts
/// (`head`, `body`, `right_arm`…, as `preview::player_model` names them) `ticks` after it
/// started. Parts it leaves out stay at rest.
pub trait BodyMotion: Send + Sync {
    /// How long it plays, in ticks.
    fn length(&self) -> f32;
    /// The parts' poses at `ticks` (0 to `length`).
    fn pose(&self, ticks: f32, set: &mut dyn FnMut(&str, Pose));
}

/// What to draw: the items' animation clock and, during an emote, the player model's poses.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub camera: Camera,
    pub ticks: f32,
    pub body: Vec<(String, Pose)>,
}

/// What `Stage::frame` asks of the view: draw `shot` now, and look again in `wake`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub shot: Option<Shot>,
    pub wake: Option<Duration>,
}

/// Everything the picture depends on; a picture is drawn again only when it changed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    camera: Camera,
    outfit: u64,
    scale: f32,
    ticks: f32,
    emote: Option<usize>,
}

/// A clock that runs only while told to, keeping what it ran before.
#[derive(Clone, Copy, Debug, Default)]
struct Clock {
    banked: Duration,
    since: Option<Instant>,
}

impl Clock {
    fn at(&self, now: Instant) -> Duration {
        self.banked
            + self
                .since
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    fn run(&mut self, on: bool, now: Instant) {
        match (on, self.since) {
            (true, None) => self.since = Some(now),
            (false, Some(_)) => {
                self.banked = self.at(now);
                self.since = None;
            }
            _ => {}
        }
    }

    fn ticks(&self, now: Instant) -> f32 {
        self.at(now).as_secs_f32() * TICKS_PER_SECOND
    }
}

/// The live view's state between pictures.
#[derive(Default)]
pub struct Stage {
    camera: Option<Camera>,
    /// Bumped by every new outfit.
    outfit: u64,
    /// Whether anything worn animates (and animations are wanted).
    animates: bool,
    clock: Clock,
    /// The emote playing, and the clock's ticks when it started.
    emote: Option<(Arc<dyn BodyMotion>, f32)>,
    drawn: Option<(Key, Instant)>,
}

impl Stage {
    pub fn camera(&self) -> Camera {
        self.camera.unwrap_or(Camera::HOME)
    }

    /// Whether the camera is away from `Camera::HOME` (the reset button does something).
    pub fn moved(&self) -> bool {
        self.camera() != Camera::HOME
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = Some(camera);
    }

    /// A new outfit to show; `animates` says whether any of it moves (false under reduced
    /// motion).
    pub fn dress(&mut self, animates: bool) {
        self.outfit += 1;
        self.animates = animates;
    }

    /// Plays `motion` once, from now.
    pub fn play(&mut self, motion: Arc<dyn BodyMotion>, now: Instant) {
        self.emote = Some((motion, self.clock.ticks(now)));
    }

    /// Whether an emote is playing.
    pub fn playing(&self) -> bool {
        self.emote.is_some()
    }

    /// Called each time the view is drawn, at `now`, `scale` device pixels a point; `active` is
    /// whether the window is in front. Says whether to draw the picture again, and when to look
    /// again (never, while nothing moves).
    pub fn frame(&mut self, now: Instant, scale: f32, active: bool) -> Frame {
        let ticks = self.clock.ticks(now);
        if self
            .emote
            .as_ref()
            .is_some_and(|(m, from)| ticks - from >= m.length())
        {
            self.emote = None;
        }
        let moving = active && (self.animates || self.emote.is_some());
        self.clock.run(moving, now);
        let key = Key {
            camera: self.camera(),
            outfit: self.outfit,
            scale,
            ticks,
            emote: self
                .emote
                .as_ref()
                .map(|(m, _)| Arc::as_ptr(m) as *const () as usize),
        };
        let camera_moved = self
            .drawn
            .as_ref()
            .is_some_and(|(d, _)| d.camera != self.camera());
        let gap = if camera_moved {
            MOVE_FRAME
        } else if moving {
            ANIMATION_FRAME
        } else {
            MOVE_FRAME
        };
        match self.drawn {
            Some((drawn, _)) if drawn == key => Frame {
                shot: None,
                wake: moving.then_some(ANIMATION_FRAME),
            },
            Some((_, at)) if now.saturating_duration_since(at) < gap => Frame {
                shot: None,
                wake: Some(gap - now.saturating_duration_since(at)),
            },
            _ => {
                self.drawn = Some((key, now));
                let mut body = Vec::new();
                if let Some((m, from)) = &self.emote {
                    m.pose((ticks - from).clamp(0.0, m.length()), &mut |name, pose| {
                        body.push((name.to_owned(), pose))
                    });
                }
                Frame {
                    shot: Some(Shot {
                        camera: key.camera,
                        ticks,
                        body,
                    }),
                    wake: moving.then_some(ANIMATION_FRAME),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// Calls `frame` every `every` for `span` from `start`; the pictures drawn.
    fn run(
        stage: &mut Stage,
        start: Instant,
        span: Duration,
        every: Duration,
        active: bool,
    ) -> usize {
        let mut drawn = 0;
        let mut t = Duration::ZERO;
        while t < span {
            drawn += usize::from(stage.frame(start + t, 2.0, active).shot.is_some());
            t += every;
        }
        drawn
    }

    #[test]
    fn nothing_is_drawn_again_while_nothing_moves() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(false);
        let first = stage.frame(t0, 2.0, true);
        assert!(first.shot.is_some());
        assert_eq!(first.wake, None);
        // A minute of frames (the page redrawn for other reasons, hovers…), focused or not:
        // nothing drawn, nothing asked for.
        for active in [true, false] {
            for k in 1..600 {
                let f = stage.frame(t0 + k * 100 * MS, 2.0, active);
                assert_eq!(f, Frame::default());
            }
        }
    }

    #[test]
    fn a_drag_draws_at_most_60_pictures_a_second() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(false);
        stage.frame(t0, 2.0, true);
        // The pointer moves every 4 ms for a second, the view asked each time.
        let mut drawn = 0;
        let mut waits = Vec::new();
        for k in 1..=250 {
            let now = t0 + k * 4 * MS;
            stage.set_camera(stage.camera().dragged(1.0, 0.0));
            let f = stage.frame(now, 2.0, true);
            drawn += usize::from(f.shot.is_some());
            waits.extend(f.wake);
        }
        assert!((55..=63).contains(&drawn), "{drawn}");
        // A change that waits says when its turn comes, never later than a frame.
        assert!(waits.iter().all(|w| *w <= MOVE_FRAME), "{waits:?}");
        // The drag's end is drawn when the wait is over, then nothing more.
        let end = t0 + 1000 * MS + MOVE_FRAME;
        assert!(stage.frame(end, 2.0, true).shot.is_some());
        assert_eq!(stage.frame(end + 500 * MS, 2.0, true), Frame::default());
    }

    #[test]
    fn dragging_while_animating_draws_at_60_pictures_a_second() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(true);
        stage.frame(t0, 2.0, true);
        let mut drawn = 0;
        let mut throttled_waits = Vec::new();
        for k in 1..=250 {
            let now = t0 + k * 4 * MS;
            stage.set_camera(stage.camera().dragged(1.0, 0.0));
            let f = stage.frame(now, 2.0, true);
            drawn += usize::from(f.shot.is_some());
            if f.shot.is_none() {
                throttled_waits.extend(f.wake);
            }
        }
        assert!((55..=63).contains(&drawn), "{drawn}");
        assert!(
            throttled_waits.iter().all(|w| *w <= MOVE_FRAME),
            "{throttled_waits:?}"
        );
    }

    #[test]
    fn animations_play_at_20_pictures_a_second_only_in_front() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(true);
        let first = stage.frame(t0, 2.0, true);
        assert_eq!(first.wake, Some(ANIMATION_FRAME));
        // Asked as often as it wants for a second: about 20 pictures.
        let drawn = 1 + run(&mut stage, t0 + MS, Duration::from_secs(1), MS, true);
        assert!((19..=21).contains(&drawn), "{drawn}");
        // In the background: the frozen frame drawn at most once more, then nothing and no
        // timer, however long.
        let t1 = t0 + Duration::from_secs(1);
        let last = run(&mut stage, t1, 100 * MS, 10 * MS, false);
        assert!(last <= 1, "{last}");
        assert_eq!(stage.frame(t1 + 100 * MS, 2.0, false), Frame::default());
        assert_eq!(
            run(&mut stage, t1, Duration::from_secs(60), 100 * MS, false),
            0
        );
        // Back in front an hour later, the animations go on where they stopped: the clock ran
        // one second (20 ticks), not an hour.
        let back = t1 + Duration::from_secs(3600);
        assert_eq!(stage.frame(back, 2.0, true).wake, Some(ANIMATION_FRAME));
        let shot = stage
            .frame(back + 60 * MS, 2.0, true)
            .shot
            .expect("drawn again in front");
        assert!((shot.ticks - 21.2).abs() < 0.1, "{}", shot.ticks);
        let later = stage.frame(back + 1060 * MS, 2.0, true).shot.unwrap();
        assert!(
            (later.ticks - shot.ticks - 20.0).abs() < 1e-3,
            "{}",
            later.ticks
        );
    }

    #[test]
    fn a_new_outfit_or_scale_is_drawn_once() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(false);
        stage.frame(t0, 2.0, true);
        stage.dress(false);
        assert!(stage.frame(t0 + 100 * MS, 2.0, true).shot.is_some());
        assert!(stage.frame(t0 + 200 * MS, 1.0, true).shot.is_some());
        assert_eq!(stage.frame(t0 + 300 * MS, 1.0, true), Frame::default());
    }

    /// Raises the right arm over 20 ticks.
    struct Wave;
    impl BodyMotion for Wave {
        fn length(&self) -> f32 {
            20.0
        }
        fn pose(&self, ticks: f32, set: &mut dyn FnMut(&str, Pose)) {
            let x = -std::f32::consts::PI * ticks / 20.0;
            set("right_arm", [-5.0, 2.0, 0.0, x, 0.0, 0.0, 1.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn an_emote_plays_once_then_the_view_rests() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(false);
        stage.frame(t0, 2.0, true);
        stage.play(Arc::new(Wave), t0);
        let start = stage.frame(t0 + 50 * MS, 2.0, true);
        let shot = start.shot.expect("drawn when it starts");
        assert_eq!(shot.body.len(), 1);
        assert_eq!(start.wake, Some(ANIMATION_FRAME));
        // Half a second in (10 ticks), the arm is half way up.
        let mid = stage.frame(t0 + 550 * MS, 2.0, true).shot.unwrap();
        let x = mid.body[0].1[3];
        assert!((x + std::f32::consts::FRAC_PI_2).abs() < 0.1, "{x}");
        // Over after a second: drawn once more at rest, then nothing.
        let end = stage.frame(t0 + 1100 * MS, 2.0, true);
        assert!(end.shot.as_ref().is_some_and(|s| s.body.is_empty()));
        assert_eq!(end.wake, None);
        assert!(!stage.playing());
        assert_eq!(stage.frame(t0 + 2000 * MS, 2.0, true), Frame::default());
    }

    #[test]
    fn an_emote_waits_while_the_window_is_in_the_background() {
        let t0 = Instant::now();
        let mut stage = Stage::default();
        stage.dress(false);
        stage.play(Arc::new(Wave), t0);
        stage.frame(t0, 2.0, true);
        stage.frame(t0 + 250 * MS, 2.0, false);
        assert_eq!(stage.frame(t0 + 10_000 * MS, 2.0, false), Frame::default());
        assert!(stage.playing());
        // Back in front, it goes on from five ticks in.
        stage.frame(t0 + 20_000 * MS, 2.0, true);
        let back = stage.frame(t0 + 20_060 * MS, 2.0, true).shot.unwrap();
        assert!((back.ticks - 6.2).abs() < 0.1, "{}", back.ticks);
    }

    #[test]
    fn the_camera_starts_home_and_says_when_it_moved() {
        let mut stage = Stage::default();
        assert_eq!(stage.camera(), Camera::HOME);
        assert!(!stage.moved());
        stage.set_camera(stage.camera().zoomed(1.0));
        assert!(stage.moved());
        stage.set_camera(Camera::HOME);
        assert!(!stage.moved());
    }
}
