//! Emotes: an item of the `emote` slot whose `emote` block keyframes the player model's bones over
//! time (rotations in degrees, offsets in pixels), one-shot or looping, with optional props: the
//! item's own parts, boxes riding a bone like any item's, which the keyframes may move too.
//!
//! Time is in game ticks from the emote's start. Between two keys a channel eases as the first key
//! says (`smooth`: a Catmull-Rom curve through the neighbouring keys, the default; `linear`;
//! `in_out`; `step`); a loop wraps from its last key to its first at `length`. The emote fades in
//! over `blend` ticks from the pose the player had, and a one-shot fades out over its last `blend`.

use crate::item::{Bone, Error};
use crate::pose::Pose;
use glam::{EulerRot, Quat, Vec3};
use serde::Deserialize;

/// The longest emote (ticks): a one-shot's length, a loop's period.
pub const MAX_LENGTH: f32 = 200.0;
/// Tracks per emote: the six bones, the root, and props.
pub const MAX_TRACKS: usize = 16;
/// Keys per track, and in all.
pub const MAX_KEYS: usize = 32;
pub const MAX_ALL_KEYS: usize = 192;
/// How far a key may turn a bone or prop about one axis (degrees); the root may turn twice round.
pub const MAX_TURN: f32 = 360.0;
pub const MAX_ROOT_TURN: f32 = 720.0;
/// How far a key may move a bone or prop from its rest (pixels), and the whole body (a hop of a
/// block at most).
pub const MAX_MOVE: f32 = 8.0;
pub const MAX_ROOT_MOVE: f32 = 16.0;

/// The player model's bones an emote poses, in the order `Emote::pose_body` takes them.
pub const BODY: [Bone; 6] = [
    Bone::Head,
    Bone::Body,
    Bone::RightArm,
    Bone::LeftArm,
    Bone::RightLeg,
    Bone::LeftLeg,
];

/// The root's pivot: the feet, 24 pixels below the neck in the player model's space (y down).
const FEET: Vec3 = Vec3::new(0.0, 24.0, 0.0);

/// What a track moves.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Bone(Bone),
    /// The whole body, about the feet.
    Root,
    /// One of the item's own parts, on top of its rest pose.
    Prop(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Smooth,
    Linear,
    InOut,
    Step,
}

/// A key: where its track is at `t` (rotation in radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub t: f32,
    pub offset: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub target: Target,
    pub keys: Vec<Key>,
}

/// A checked emote.
#[derive(Clone, Debug, PartialEq)]
pub struct Emote {
    pub length: f32,
    pub looping: bool,
    pub blend: f32,
    pub tracks: Vec<Track>,
}

/// Where a track is at a time: an offset (pixels), a rotation (radians), a scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub offset: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmoteFile {
    length: f32,
    #[serde(default, rename = "loop")]
    looping: bool,
    #[serde(default = "four")]
    blend: f32,
    tracks: Vec<TrackFile>,
}

fn four() -> f32 {
    4.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrackFile {
    part: String,
    keys: Vec<KeyFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFile {
    t: f32,
    #[serde(default)]
    offset: [f32; 3],
    /// Degrees.
    #[serde(default)]
    rotation: [f32; 3],
    #[serde(default)]
    scale: Option<f32>,
    #[serde(default)]
    ease: Ease,
}

fn invalid(text: impl Into<String>) -> Error {
    Error::Invalid(text.into())
}

fn bone_named(name: &str) -> Option<Bone> {
    BODY.into_iter().find(|b| b.part() == name)
}

impl EmoteFile {
    /// Checks the emote; `props` names the item's own parts.
    pub(crate) fn check(self, props: &[String]) -> Result<Emote, Error> {
        let numbers = [self.length, self.blend];
        if !numbers.iter().all(|v| v.is_finite()) {
            return Err(invalid("the emote's length is not a number"));
        }
        if !(4.0..=MAX_LENGTH).contains(&self.length) {
            return Err(invalid(format!("an emote lasts 4 to {MAX_LENGTH} ticks")));
        }
        if !(0.0..=self.length / 2.0).contains(&self.blend) {
            return Err(invalid("an emote's blend is at most half its length"));
        }
        if self.tracks.is_empty() || self.tracks.len() > MAX_TRACKS {
            return Err(invalid(format!("an emote has 1 to {MAX_TRACKS} tracks")));
        }
        let all: usize = self.tracks.iter().map(|t| t.keys.len()).sum();
        if all > MAX_ALL_KEYS {
            return Err(invalid(format!("{all} keys, at most {MAX_ALL_KEYS}")));
        }
        let mut tracks: Vec<Track> = Vec::new();
        for file in self.tracks {
            let target = match file.part.as_str() {
                "root" => Target::Root,
                name => match bone_named(name) {
                    Some(bone) => Target::Bone(bone),
                    None if props.iter().any(|p| p == name) => Target::Prop(name.to_owned()),
                    None => return Err(invalid(format!("an emote moves no part {name:?}"))),
                },
            };
            if tracks.iter().any(|t| t.target == target) {
                return Err(invalid(format!("two tracks move {:?}", file.part)));
            }
            if file.keys.is_empty() || file.keys.len() > MAX_KEYS {
                return Err(invalid(format!("a track has 1 to {MAX_KEYS} keys")));
            }
            let (turn, reach) = match target {
                Target::Root => (MAX_ROOT_TURN, MAX_ROOT_MOVE),
                _ => (MAX_TURN, MAX_MOVE),
            };
            let mut keys = Vec::with_capacity(file.keys.len());
            for k in file.keys {
                let scale = k.scale.unwrap_or(1.0);
                let values = [k.t, scale].into_iter().chain(k.offset).chain(k.rotation);
                if !values.into_iter().all(f32::is_finite) {
                    return Err(invalid("an emote's key is not a number"));
                }
                if keys.last().is_some_and(|last: &Key| k.t <= last.t)
                    || !(0.0..=self.length).contains(&k.t)
                {
                    return Err(invalid("keys go forward in time within the emote"));
                }
                if k.rotation.iter().any(|r| r.abs() > turn)
                    || k.offset.iter().any(|o| o.abs() > reach)
                {
                    return Err(invalid(format!(
                        "a key of {:?} turns or moves too far",
                        file.part
                    )));
                }
                // Only a prop grows or shrinks (it may pop in from nothing).
                if k.scale.is_some() && !matches!(target, Target::Prop(_))
                    || !(0.0..=2.0).contains(&scale)
                {
                    return Err(invalid("only a prop's key scales, from 0 to 2"));
                }
                keys.push(Key {
                    t: k.t,
                    offset: k.offset,
                    rotation: k.rotation.map(f32::to_radians),
                    scale,
                    ease: k.ease,
                });
            }
            tracks.push(Track { target, keys });
        }
        Ok(Emote {
            length: self.length,
            looping: self.looping,
            blend: self.blend,
            tracks,
        })
    }
}

/// Catmull-Rom through `p1` to `p2`, `p0` and `p3` their neighbours.
fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, s: f32) -> f32 {
    let s2 = s * s;
    0.5 * (2.0 * p1
        + (p2 - p0) * s
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * s2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * s2 * s)
}

impl Track {
    /// The track at `t` ticks into an emote of `length` (wrapped if `looping`).
    pub fn sample(&self, t: f32, length: f32, looping: bool) -> Sample {
        let keys = &self.keys;
        let n = keys.len();
        let at = |k: &Key| Sample {
            offset: k.offset,
            rotation: k.rotation,
            scale: k.scale,
        };
        let t = if looping {
            t.rem_euclid(length)
        } else {
            t.clamp(0.0, length)
        };
        // The segment from key i to the next: wrapping round in a loop, held at the ends else.
        let i = match keys.iter().rposition(|k| k.t <= t) {
            Some(i) => i,
            None if looping => n - 1,
            None => return at(&keys[0]),
        };
        if i == n - 1 && !looping {
            return at(&keys[i]);
        }
        let next = (i + 1) % n;
        let (t0, mut t1) = (keys[i].t, keys[next].t);
        let mut now = t;
        if next <= i {
            t1 += length;
            if now < t0 {
                now += length;
            }
        }
        let span = t1 - t0;
        let s = if span > 0.0 {
            ((now - t0) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (k0, k1) = (&keys[i], &keys[next]);
        let neighbour = |j: isize| -> &Key {
            if looping {
                &keys[j.rem_euclid(n as isize) as usize]
            } else {
                &keys[j.clamp(0, n as isize - 1) as usize]
            }
        };
        let (kp, kn) = (neighbour(i as isize - 1), neighbour(i as isize + 2));
        let mix = |a: f32, b: f32, pa: f32, nb: f32| match k0.ease {
            Ease::Step => a,
            Ease::Linear => a + (b - a) * s,
            Ease::InOut => a + (b - a) * (s * s * (3.0 - 2.0 * s)),
            Ease::Smooth => catmull(pa, a, b, nb, s),
        };
        let three = |f: fn(&Key) -> [f32; 3]| -> [f32; 3] {
            let (a, b, p, q) = (f(k0), f(k1), f(kp), f(kn));
            std::array::from_fn(|c| mix(a[c], b[c], p[c], q[c]))
        };
        Sample {
            offset: three(|k| k.offset),
            rotation: three(|k| k.rotation),
            scale: mix(k0.scale, k1.scale, kp.scale, kn.scale),
        }
    }
}

/// `smoothstep`, clamped.
fn smooth01(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn quat(r: [f32; 3]) -> Quat {
    Quat::from_euler(EulerRot::ZYX, r[2], r[1], r[0])
}

impl Emote {
    /// How much of the emote shows at `t` ticks in: fading in over `blend`, and out over the last
    /// `blend` of a one-shot. Past a one-shot's end: 0.
    pub fn weight(&self, t: f32) -> f32 {
        if t < 0.0 || !self.looping && t >= self.length {
            return 0.0;
        }
        if self.blend <= 0.0 {
            return 1.0;
        }
        let fade_in = smooth01(t / self.blend);
        if self.looping {
            fade_in
        } else {
            fade_in.min(smooth01((self.length - t) / self.blend))
        }
    }

    /// Poses the player model's bones (`BODY`'s order) `t` ticks in: `rest` is the model at
    /// rest, `poses` what its animation made of it, blended toward the emote by `weight` (1: the
    /// emote's own pose). A bone the emote doesn't key keeps its animation; the root moves all
    /// six about the feet.
    pub fn pose_body(&self, t: f32, weight: f32, rest: &[Pose; 6], poses: &mut [Pose; 6]) {
        if weight <= 0.0 {
            return;
        }
        let mut target = *poses;
        for track in &self.tracks {
            if let Target::Bone(bone) = track.target {
                let i = BODY.iter().position(|b| *b == bone).unwrap_or(0);
                let s = track.sample(t, self.length, self.looping);
                let r = rest[i];
                target[i] = [
                    r[0] + s.offset[0],
                    r[1] + s.offset[1],
                    r[2] + s.offset[2],
                    s.rotation[0],
                    s.rotation[1],
                    s.rotation[2],
                    r[6],
                    r[7],
                    r[8],
                ];
            }
        }
        if let Some(root) = self.tracks.iter().find(|t| t.target == Target::Root) {
            let s = root.sample(t, self.length, self.looping);
            let turn = quat(s.rotation);
            let shift = Vec3::from_array(s.offset);
            for p in &mut target {
                let at = FEET + turn * (Vec3::new(p[0], p[1], p[2]) - FEET) + shift;
                let (z, y, x) = (turn * quat([p[3], p[4], p[5]])).to_euler(EulerRot::ZYX);
                p[..6].copy_from_slice(&[at.x, at.y, at.z, x, y, z]);
            }
        }
        // Fully in (most of an emote): the target as it is, no blending to pay for.
        if weight >= 1.0 {
            for (p, goal) in poses.iter_mut().zip(target) {
                p[..6].copy_from_slice(&goal[..6]);
            }
            return;
        }
        let w = weight;
        for (p, goal) in poses.iter_mut().zip(target) {
            let at = Vec3::new(p[0], p[1], p[2]).lerp(Vec3::new(goal[0], goal[1], goal[2]), w);
            let turned = quat([p[3], p[4], p[5]]).slerp(quat([goal[3], goal[4], goal[5]]), w);
            let (z, y, x) = turned.to_euler(EulerRot::ZYX);
            p[..6].copy_from_slice(&[at.x, at.y, at.z, x, y, z]);
        }
    }

    /// The props' poses `t` ticks in, each from its rest pose (`rest`, by part name).
    pub fn pose_props(
        &self,
        t: f32,
        rest: impl Fn(&str) -> Option<Pose>,
        mut set: impl FnMut(&str, Pose),
    ) {
        for track in &self.tracks {
            let Target::Prop(name) = &track.target else {
                continue;
            };
            let Some(mut pose) = rest(name) else {
                continue;
            };
            let s = track.sample(t, self.length, self.looping);
            for c in 0..3 {
                pose[c] += s.offset[c];
                pose[3 + c] += s.rotation[c];
                pose[6 + c] *= s.scale;
            }
            set(name, pose);
        }
    }

    /// Times worth checking the props' reach at: every key, and 32 points over the length.
    pub(crate) fn sample_times(&self) -> impl Iterator<Item = f32> + '_ {
        let keys = self.tracks.iter().flat_map(|t| t.keys.iter().map(|k| k.t));
        keys.chain((0..32).map(|k| self.length * k as f32 / 32.0))
    }
}

/// An emote someone plays: the item, and when it started and stopped (ticks of the clock the
/// game samples it with, such as the player's age).
#[derive(Clone, Debug)]
pub struct Playing {
    pub item: std::sync::Arc<crate::Item>,
    pub start: f32,
    /// Cancelled at this time: it fades out over its blend from there.
    pub stop: Option<f32>,
}

impl Playing {
    /// The emote's own time and weight at `now`, or `None` once it is over.
    pub fn at(&self, now: f32) -> Option<(f32, f32)> {
        let emote = self.item.emote.as_ref()?;
        let t = now - self.start;
        let mut weight = emote.weight(t);
        if let Some(stop) = self.stop {
            let fade = if emote.blend > 0.0 {
                smooth01(1.0 - (now - stop) / emote.blend)
            } else {
                0.0
            };
            weight = weight.min(fade);
        }
        let over = !emote.looping && t >= emote.length || self.stop.is_some() && weight <= 0.0;
        (!over).then_some((t, weight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Item;
    use crate::pose::tests::png;
    use std::f32::consts::{FRAC_PI_2, PI};

    /// A wave: the right arm up over 8 ticks, waved twice, down by 40; a balloon prop in the left
    /// hand that pops in.
    fn wave(edit: impl Fn(String) -> String) -> String {
        let file = format!(
            r#"{{"format": 1, "id": "wave", "slot": "emote",
              "texture": {{"png": "{}"}},
              "parts": [{{"name": "balloon", "bone": "left_arm", "pivot": [1, 10, 0],
                "cubes": [{{"origin": [-2, -4, -2], "size": [4, 4, 4], "uv": [0, 0]}}]}}],
              "emote": {{"length": 40, "blend": 4, "tracks": [
                {{"part": "right_arm", "keys": [
                  {{"t": 0}},
                  {{"t": 8, "rotation": [0, 0, 150], "ease": "linear"}},
                  {{"t": 16, "rotation": [0, 0, 120], "ease": "step"}},
                  {{"t": 24, "rotation": [0, 0, 150]}},
                  {{"t": 40}}]}},
                {{"part": "balloon", "keys": [
                  {{"t": 0, "scale": 0, "ease": "in_out"}}, {{"t": 10, "offset": [0, -4, 0]}}]}}]}}}}"#,
            png(16, 16)
        );
        edit(file)
    }

    fn parse(file: &str) -> Result<Item, crate::Error> {
        Item::parse(file.as_bytes())
    }

    fn emote(file: &str) -> Emote {
        parse(file).unwrap().emote.unwrap()
    }

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }

    #[test]
    fn keys_are_hit_and_each_ease_runs_between_them() {
        let e = emote(&wave(|f| f));
        let arm = &e.tracks[0];
        assert_eq!(arm.target, Target::Bone(crate::Bone::RightArm));
        let z = |t: f32| arm.sample(t, e.length, e.looping).rotation[2].to_degrees();
        // A smooth curve passes through its keys.
        near(z(0.0), 0.0);
        near(z(8.0), 150.0);
        near(z(24.0), 150.0);
        near(z(40.0), 0.0);
        // Linear from 8 to 16: halfway is the mean; step from 16 holds until 24.
        near(z(12.0), 135.0);
        near(z(20.0), 120.0);
        near(z(23.9), 120.0);
        // Held after the end of a one-shot.
        near(z(55.0), 0.0);
    }

    #[test]
    fn smooth_keys_leave_no_corner() {
        // Smooth through 0, 90, 0 at 0, 10, 20: the slope is the same on both sides of each inner
        // key, and the curve tops out at the key (Catmull-Rom's tangent there is flat).
        let file = wave(|f| {
            f.replace(
                r#"{"t": 8, "rotation": [0, 0, 150], "ease": "linear"},
                  {"t": 16, "rotation": [0, 0, 120], "ease": "step"},
                  {"t": 24, "rotation": [0, 0, 150]},"#,
                r#"{"t": 10, "rotation": [0, 0, 90]}, {"t": 20}, {"t": 30, "rotation": [0, 0, -90]},"#,
            )
        });
        let e = emote(&file);
        let z = |t: f32| e.tracks[0].sample(t, e.length, false).rotation[2];
        for key in [10.0, 20.0] {
            let (h, k) = (1e-2, key);
            let left = (z(k) - z(k - h)) / h;
            let right = (z(k + h) - z(k)) / h;
            assert!((left - right).abs() < 0.02, "{key}: {left} vs {right}");
        }
        assert!(z(9.5) < z(10.0) && z(10.5) < z(10.0));
    }

    #[test]
    fn a_loop_wraps_from_its_last_key_to_its_first() {
        let file = wave(|f| {
            f.replace(
                r#""length": 40, "blend": 4"#,
                r#""length": 40, "loop": true"#,
            )
            .replace(
                r#"{"t": 40}"#,
                r#"{"t": 32, "rotation": [0, 0, 40], "ease": "linear"}"#,
            )
        });
        let e = emote(&file);
        let z = |t: f32| e.tracks[0].sample(t, e.length, true).rotation[2].to_degrees();
        // From 40° at 32 back to 0° at 40 (= 0), linearly: 20° at 36.
        near(z(36.0), 20.0);
        // A period later, the same.
        near(z(76.0), 20.0);
        near(z(48.0), 150.0);
        // A loop fades in, then stays.
        near(e.weight(0.0), 0.0);
        near(e.weight(2.0), 0.5);
        near(e.weight(400.0), 1.0);
    }

    #[test]
    fn a_one_shot_fades_in_and_out_and_a_cancel_fades_it_out() {
        let item = std::sync::Arc::new(parse(&wave(|f| f)).unwrap());
        let e = item.emote.as_ref().unwrap();
        near(e.weight(2.0), 0.5);
        near(e.weight(20.0), 1.0);
        near(e.weight(38.0), 0.5);
        assert_eq!(e.weight(40.0), 0.0);
        let mut playing = Playing {
            item: item.clone(),
            start: 100.0,
            stop: None,
        };
        assert_eq!(playing.at(120.0), Some((20.0, 1.0)));
        assert_eq!(playing.at(140.0), None);
        // Cancelled at 110: half gone 2 ticks later, over after the blend.
        playing.stop = Some(110.0);
        let (t, w) = playing.at(112.0).unwrap();
        near(t, 12.0);
        near(w, 0.5);
        assert_eq!(playing.at(114.0), None);
    }

    /// The player model's bones at rest (`BODY`'s order) and as walking left them.
    fn bodies() -> ([Pose; 6], [Pose; 6]) {
        let at = |x: f32, y: f32| [x, y, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let rest = [
            at(0.0, 0.0),
            at(0.0, 0.0),
            at(-5.0, 2.0),
            at(5.0, 2.0),
            at(-1.9, 12.0),
            at(1.9, 12.0),
        ];
        let mut walking = rest;
        walking[0][4] = 0.3; // the head looks aside
        walking[2][3] = 0.5; // the right arm swings
        (rest, walking)
    }

    #[test]
    fn keyed_bones_take_the_emotes_pose_and_others_keep_theirs() {
        let e = emote(&wave(|f| f));
        let (rest, walking) = bodies();
        let mut poses = walking;
        e.pose_body(8.0, 1.0, &rest, &mut poses);
        // The right arm: rest position, 150° about z, its swing gone.
        near(poses[2][0], -5.0);
        near(poses[2][3], 0.0);
        near(poses[2][5], 150f32.to_radians());
        // The head still looks aside; the legs don't move.
        near(poses[0][4], 0.3);
        assert_eq!(poses[4], walking[4]);
        // Half blended: halfway between the swing and the emote's pose (one axis each).
        let mut half = walking;
        half[2][3] = 0.0;
        e.pose_body(8.0, 0.5, &rest, &mut half);
        near(half[2][5], 75f32.to_radians());
    }

    #[test]
    fn the_root_turns_and_lifts_the_whole_body_about_the_feet() {
        let file = wave(|f| {
            f.replace(
                r#"{"part": "balloon","#,
                r#"{"part": "root", "keys": [{"t": 0, "offset": [0, -8, 0], "rotation": [0, 180, 0]}]},
                {"part": "balloon","#,
            )
        });
        let e = emote(&file);
        let (rest, _) = bodies();
        let mut poses = rest;
        e.pose_body(0.0, 1.0, &rest, &mut poses);
        // Half a turn about the vertical through the feet swaps the arms' sides, then 8 px up.
        near(poses[3][0], -5.0);
        near(poses[3][1], 2.0 - 8.0);
        near(poses[4][0], 1.9);
        near(poses[4][1], 12.0 - 8.0);
        // Each bone is turned with it (180° about y, whatever Euler angles say so).
        let q = quat([poses[0][3], poses[0][4], poses[0][5]]);
        assert!(q.angle_between(Quat::from_rotation_y(PI)) < 1e-3);
        // A quarter turn about x tips the head (24 px above the feet) forward along -z... or +z:
        // (0, -24) from the feet turned 90° about x is (0, 0, -24) with y down, z forward.
        let tip = wave(|f| {
            f.replace(
                r#"{"part": "balloon","#,
                r#"{"part": "root", "keys": [{"t": 0, "rotation": [90, 0, 0]}]}, {"part": "balloon","#,
            )
        });
        let mut tipped = rest;
        emote(&tip).pose_body(0.0, 1.0, &rest, &mut tipped);
        let head = Vec3::new(tipped[0][0], tipped[0][1], tipped[0][2]);
        let expected = FEET + Quat::from_rotation_x(FRAC_PI_2) * (Vec3::ZERO - FEET);
        assert!((head - expected).length() < 1e-3, "{head} {expected}");
    }

    #[test]
    fn props_pop_in_and_move_from_their_rest() {
        let item = parse(&wave(|f| f)).unwrap();
        let e = item.emote.as_ref().unwrap();
        let mut got = Vec::new();
        e.pose_props(5.0, |n| item.rest(n), |n, p| got.push((n.to_owned(), p)));
        let (name, pose) = &got[0];
        assert_eq!(name, "balloon");
        // In-out from scale 0 at 0 to 1 at 10: half at 5; moved half of 4 px up.
        near(pose[6], 0.5);
        near(pose[1], 10.0 - 2.0);
    }

    #[test]
    fn refuses_emotes_past_the_limits() {
        assert!(parse(&wave(|f| f)).is_ok());
        let bad = [
            (r#""length": 40"#, r#""length": 400"#),
            (r#""length": 40"#, r#""length": 2"#),
            (r#""blend": 4"#, r#""blend": 30"#),
            (r#"{"t": 16,"#, r#"{"t": 4,"#),
            (r#"{"t": 40}"#, r#"{"t": 41}"#),
            (r#""part": "right_arm""#, r#""part": "tail""#),
            (
                r#""part": "balloon", "keys""#,
                r#""part": "right_arm", "keys""#,
            ),
            (
                r#"[0, 0, 150], "ease": "linear""#,
                r#"[0, 0, 400], "ease": "linear""#,
            ),
            (r#"{"t": 0}"#, r#"{"t": 0, "scale": 2}"#),
            (r#"{"t": 0}"#, r#"{"t": 0, "offset": [9, 0, 0]}"#),
            (r#""ease": "step""#, r#""ease": "bounce""#),
        ];
        for (from, to) in bad {
            let file = wave(|f| f.replacen(from, to, 1));
            assert_ne!(file, wave(|f| f), "{from}");
            assert!(parse(&file).is_err(), "{to}");
        }
        // The balloon carried past the reach: 18 px down its arm, then 8 more and twice as big.
        let far = wave(|f| {
            f.replace("[1, 10, 0]", "[1, 18, 0]").replace(
                r#""offset": [0, -4, 0]"#,
                r#""offset": [0, 8, 0], "scale": 2"#,
            )
        });
        assert!(matches!(parse(&far), Err(crate::Error::TooFar(_))));
        let many: Vec<String> = (0..=MAX_KEYS).map(|k| format!(r#"{{"t": {k}}}"#)).collect();
        let file = wave(|f| f.replacen(r#"{"t": 0}"#, &many.join(","), 1));
        assert!(parse(&file).is_err());
    }

    #[test]
    fn only_an_emote_has_one_and_an_emote_needs_a_texture_only_for_props() {
        // A hat with an emote, an emote without one.
        let hat = wave(|f| f.replace(r#""slot": "emote""#, r#""slot": "head""#));
        assert!(parse(&hat).is_err());
        let mut bare = wave(|f| f);
        let emote_at = bare.find(r#""emote": {"#).unwrap();
        let start = bare[..emote_at].rfind(',').unwrap();
        bare.replace_range(start..bare.len() - 1, "");
        assert!(parse(&bare).is_err());
        // No props: no texture needed. Props: needed.
        let no_props = r#"{"format": 1, "id": "nod", "slot": "emote", "parts": [],
            "emote": {"length": 20, "tracks": [{"part": "head", "keys": [{"t": 0}, {"t": 10, "rotation": [30, 0, 0]}]}]}}"#;
        let item = parse(no_props).unwrap();
        assert_eq!((item.slot, item.vertices), (crate::Slot::Emote, 0));
        let untextured = wave(|f| {
            let (a, b) = (
                f.find(r#""texture""#).unwrap(),
                f.find(r#""parts""#).unwrap(),
            );
            format!("{}{}", &f[..a], &f[b..])
        });
        assert!(matches!(parse(&untextured), Err(crate::Error::Texture(_))));
    }
}
