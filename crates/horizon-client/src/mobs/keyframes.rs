//! Vanilla keyframe animations (`AnimationDefinition`, `AnimationChannel`,
//! `KeyframeAnimation`) exported from the client's animation definitions,
//! applied to a pose map exactly like `KeyframeAnimation.Entry.apply`.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Position,
    Rotation,
    Scale,
}
#[derive(Clone, Copy, Debug)]
pub struct Keyframe {
    pub time: f32,
    pub pre: [f32; 3],
    pub post: [f32; 3],
    pub catmull: bool,
}
#[derive(Clone, Debug)]
pub struct Channel {
    pub target: Target,
    pub keyframes: Vec<Keyframe>,
}
#[derive(Clone, Debug)]
pub struct Animation {
    pub length: f32,
    pub looping: bool,
    pub bones: Vec<(String, Vec<Channel>)>,
}
fn v3(v: &Value) -> [f32; 3] {
    let a = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    [a(0), a(1), a(2)]
}
impl Animation {
    pub fn parse(v: &Value) -> Self {
        let bones = v["bones"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, channels)| {
                let channels = channels
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| Channel {
                        target: match c["target"].as_str() {
                            Some("position") => Target::Position,
                            Some("scale") => Target::Scale,
                            _ => Target::Rotation,
                        },
                        keyframes: c["keyframes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|k| Keyframe {
                                time: k[0].as_f64().unwrap_or(0.0) as f32,
                                pre: v3(&k[1]),
                                post: v3(&k[2]),
                                catmull: k[3].as_str() == Some("catmullrom"),
                            })
                            .collect(),
                    })
                    .collect();
                (name.clone(), channels)
            })
            .collect();
        Self {
            length: v["length"].as_f64().unwrap_or(1.0) as f32,
            looping: v["looping"].as_bool().unwrap_or(false),
            bones,
        }
    }
    /// KeyframeAnimation.apply(millis, scale): offsets added to each bone's pose.
    pub fn apply(&self, millis: i64, scale: f32, poses: &mut super::anim::Poses) {
        let mut seconds = millis as f32 / 1000.0;
        if self.looping && self.length > 0.0 {
            seconds %= self.length;
        }
        for (bone, channels) in &self.bones {
            for c in channels {
                let v = sample(&c.keyframes, seconds, scale);
                let p = poses.get(bone);
                match c.target {
                    // ModelPart.offsetPos / offsetRotation / offsetScale.
                    Target::Position => {
                        p[0] += v[0];
                        p[1] += v[1];
                        p[2] += v[2];
                    }
                    Target::Rotation => {
                        p[3] += v[0];
                        p[4] += v[1];
                        p[5] += v[2];
                    }
                    Target::Scale => {
                        p[6] += v[0];
                        p[7] += v[1];
                        p[8] += v[2];
                    }
                }
            }
        }
    }
    /// KeyframeAnimation.applyWalk(pos, speed, speedFactor, scaleFactor).
    pub fn apply_walk(
        &self,
        pos: f32,
        speed: f32,
        speed_factor: f32,
        scale_factor: f32,
        poses: &mut super::anim::Poses,
    ) {
        let time = (pos * 50.0 * speed_factor) as i64;
        let scale = (speed * scale_factor).min(1.0);
        self.apply(time, scale, poses);
    }
}
fn catmullrom(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
}
/// KeyframeAnimation.Entry.apply interpolation.
fn sample(k: &[Keyframe], seconds: f32, scale: f32) -> [f32; 3] {
    if k.is_empty() {
        return [0.0; 3];
    }
    // Mth.binarySearch(0, len, i -> seconds <= k[i].time) - 1
    let first = k.iter().position(|f| seconds <= f.time).unwrap_or(k.len());
    let prev = first.saturating_sub(1);
    let next = (prev + 1).min(k.len() - 1);
    let delta = seconds - k[prev].time;
    let alpha = if next != prev {
        (delta / (k[next].time - k[prev].time)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if k[next].catmull {
        let p0 = k[prev.saturating_sub(1)].post;
        let p1 = k[prev].post;
        let p2 = k[next].post;
        let p3 = k[(next + 1).min(k.len() - 1)].post;
        std::array::from_fn(|i| catmullrom(alpha, p0[i], p1[i], p2[i], p3[i]) * scale)
    } else {
        let a = k[prev].post;
        let b = k[next].pre;
        std::array::from_fn(|i| (a[i] + (b[i] - a[i]) * alpha) * scale)
    }
}
/// AnimationState: a start time in ticks while started.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimationState {
    pub started: Option<f32>,
}
impl AnimationState {
    pub fn start(&mut self, age: f32) {
        self.started = Some(age);
    }
    pub fn start_if_stopped(&mut self, age: f32) {
        if self.started.is_none() {
            self.started = Some(age);
        }
    }
    pub fn stop(&mut self) {
        self.started = None;
    }
    /// `AnimationState.fastForward`: the animation is `ticks` further on.
    pub fn fast_forward(&mut self, ticks: i32) {
        if let Some(s) = &mut self.started {
            *s -= ticks as f32;
        }
    }
    pub fn animate_when(&mut self, condition: bool, age: f32) {
        if condition {
            self.start_if_stopped(age)
        } else {
            self.stop()
        }
    }
    /// Milliseconds since the start at render time `age` (ticks + partial).
    pub fn millis(&self, age: f32) -> Option<i64> {
        self.started.map(|s| ((age - s) * 50.0) as i64)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linear_keyframes_interpolate_post_to_pre() {
        let k = vec![
            Keyframe {
                time: 0.0,
                pre: [0.0; 3],
                post: [0.0; 3],
                catmull: false,
            },
            Keyframe {
                time: 1.0,
                pre: [2.0, 4.0, 6.0],
                post: [2.0, 4.0, 6.0],
                catmull: false,
            },
        ];
        let v = sample(&k, 0.5, 1.0);
        assert_eq!(v, [1.0, 2.0, 3.0]);
        // Past the end it holds the last value.
        assert_eq!(sample(&k, 3.0, 0.5), [1.0, 2.0, 3.0]);
    }
    #[test]
    fn catmullrom_passes_through_keyframes() {
        let k = vec![
            Keyframe {
                time: 0.0,
                pre: [0.0; 3],
                post: [0.0; 3],
                catmull: true,
            },
            Keyframe {
                time: 1.0,
                pre: [1.0; 3],
                post: [1.0; 3],
                catmull: true,
            },
            Keyframe {
                time: 2.0,
                pre: [0.0; 3],
                post: [0.0; 3],
                catmull: true,
            },
        ];
        assert!((sample(&k, 1.0, 1.0)[0] - 1.0).abs() < 1e-6);
    }
    #[test]
    fn looping_animation_applies_walk_without_overflow() {
        let anim = Animation {
            length: 1.25,
            looping: true,
            bones: vec![(
                "body".to_string(),
                vec![Channel {
                    target: Target::Position,
                    keyframes: vec![
                        Keyframe {
                            time: 0.0,
                            pre: [0.0; 3],
                            post: [0.0, 1.0, 0.0],
                            catmull: false,
                        },
                        Keyframe {
                            time: 1.25,
                            pre: [0.0, 1.0, 0.0],
                            post: [0.0, 1.0, 0.0],
                            catmull: false,
                        },
                    ],
                }],
            )],
        };
        let mut poses = super::super::anim::Poses::default();
        anim.apply_walk(50.0, 1.0, 1.0, 2.5, &mut poses);
        assert!((poses.get("body")[1] - 1.0).abs() < 1e-6);
    }
}
