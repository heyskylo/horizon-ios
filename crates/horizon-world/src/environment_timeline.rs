//! Timelines (`net.minecraft.world.timeline`): tracks of keyframes that change environment
//! attributes over a clock's ticks (`Timeline`, `AttributeTrack`, `AttributeTrackSampler`), and
//! the keyframe machinery under them (`KeyframeTrack`, `KeyframeTrackSampler`, `EasingType`,
//! `Ease`).
use crate::environment_attributes::{Attribute, Entry, Value, attribute};
use horizon_core::{geometry::trig, jmath};
use serde_json::Value as Json;

/// `Mth.square` / `Mth.cube`.
fn square(x: f32) -> f32 {
    x * x
}
fn cube(x: f32) -> f32 {
    x * x * x
}

/// `Ease.outBounce` (its `d1 = 2.75` is declared but unused).
fn out_bounce(x: f32) -> f32 {
    if x < 0.363_636_37 {
        7.5625 * square(x)
    } else if x < 0.727_272_75 {
        7.5625 * square(x - 0.545_454_56) + 0.75
    } else if f64::from(x) < 0.909_090_909_090_909_1 {
        7.5625 * square(x - 0.818_181_8) + 0.9375
    } else {
        7.5625 * square(x - 0.954_545_44) + 0.984_375
    }
}

const TWO_THIRDS_PI: f64 = (std::f64::consts::PI * 2.0 / 3.0) as f32 as f64;
const HALF_PI: f32 = std::f64::consts::FRAC_PI_2 as f32;
const PI: f32 = std::f64::consts::PI as f32;

/// `EasingType`'s named easings and `Ease`'s functions, with Java's float and double steps.
/// `Ease.inCirc`: `(float)(-Math.sqrt(1.0F - x * x)) + 1.0F`. Outside [-1, 1] the root is HotSpot's NaN (x86's
/// negative default NaN), which `-` makes positive and `+ 1.0F` keeps. LLVM may fold `-r + 1.0` into `1.0 - r`,
/// whose NaN keeps the root's sign, so the NaN is returned before the addition.
fn in_circ(x: f32) -> f32 {
    let negated = (-f64::from(1.0 - x * x).sqrt()) as f32;
    if negated.is_nan() {
        return f32::from_bits(0x7fc0_0000);
    }
    negated + 1.0
}

const EASINGS: [(&str, fn(f32) -> f32); 32] = [
    ("constant", |_| 0.0),
    ("linear", |x| x),
    ("in_back", |x| square(x) * (2.701_58 * x - 1.701_58)),
    ("in_bounce", |x| 1.0 - out_bounce(1.0 - x)),
    ("in_circ", in_circ),
    ("in_cubic", cube),
    ("in_elastic", |x| {
        if x == 0.0 {
            return 0.0;
        }
        if x == 1.0 {
            return 1.0;
        }
        let x = f64::from(x);
        (-jmath::pow(2.0, 10.0 * x - 10.0) * jmath::sin((x * 10.0 - 10.75) * TWO_THIRDS_PI)) as f32
    }),
    ("in_expo", |x| {
        if x == 0.0 {
            0.0
        } else {
            jmath::pow(2.0, 10.0 * f64::from(x) - 10.0) as f32
        }
    }),
    ("in_quad", |x| x * x),
    ("in_quart", |x| square(square(x))),
    ("in_quint", |x| square(square(x)) * x),
    ("in_sine", |x| 1.0 - trig::cos(f64::from(x * HALF_PI))),
    ("in_out_back", |x| {
        if x < 0.5 {
            4.0 * x * x * (7.189_819 * x - 2.594_909_4) / 2.0
        } else {
            let dt = 2.0 * x - 2.0;
            (dt * dt * (3.594_909_4 * dt + 2.594_909_4) + 2.0) / 2.0
        }
    }),
    ("in_out_bounce", |x| {
        if x < 0.5 {
            (1.0 - out_bounce(1.0 - 2.0 * x)) / 2.0
        } else {
            (1.0 + out_bounce(2.0 * x - 1.0)) / 2.0
        }
    }),
    ("in_out_circ", |x| {
        let x = f64::from(x);
        if x < 0.5 {
            ((1.0 - (1.0 - jmath::pow(2.0 * x, 2.0)).sqrt()) / 2.0) as f32
        } else {
            (((1.0 - jmath::pow(-2.0 * x + 2.0, 2.0)).sqrt() + 1.0) / 2.0) as f32
        }
    }),
    ("in_out_cubic", |x| {
        if x < 0.5 {
            4.0 * cube(x)
        } else {
            (1.0 - jmath::pow(-2.0 * f64::from(x) + 2.0, 3.0) / 2.0) as f32
        }
    }),
    ("in_out_elastic", |x| {
        if x == 0.0 {
            return 0.0;
        }
        if x == 1.0 {
            return 1.0;
        }
        let xd = f64::from(x);
        // The bytecode multiplies by the float `c5 = (float) PI * 4 / 9`, widened.
        let sin = jmath::sin((20.0 * xd - 11.125) * f64::from(PI * 4.0 / 9.0));
        if x < 0.5 {
            (-(jmath::pow(2.0, 20.0 * xd - 10.0) * sin) / 2.0) as f32
        } else {
            (jmath::pow(2.0, -20.0 * xd + 10.0) * sin / 2.0 + 1.0) as f32
        }
    }),
    ("in_out_expo", |x| {
        let xd = f64::from(x);
        if x < 0.5 {
            if x == 0.0 {
                0.0
            } else {
                (jmath::pow(2.0, 20.0 * xd - 10.0) / 2.0) as f32
            }
        } else if x == 1.0 {
            1.0
        } else {
            ((2.0 - jmath::pow(2.0, -20.0 * xd + 10.0)) / 2.0) as f32
        }
    }),
    ("in_out_quad", |x| {
        if x < 0.5 {
            2.0 * square(x)
        } else {
            (1.0 - jmath::pow(-2.0 * f64::from(x) + 2.0, 2.0) / 2.0) as f32
        }
    }),
    ("in_out_quart", |x| {
        if x < 0.5 {
            8.0 * square(square(x))
        } else {
            (1.0 - jmath::pow(-2.0 * f64::from(x) + 2.0, 4.0) / 2.0) as f32
        }
    }),
    ("in_out_quint", |x| {
        if f64::from(x) < 0.5 {
            16.0 * x * x * x * x * x
        } else {
            (1.0 - jmath::pow(-2.0 * f64::from(x) + 2.0, 5.0) / 2.0) as f32
        }
    }),
    ("in_out_sine", |x| {
        -(trig::cos(f64::from(PI * x)) - 1.0) / 2.0
    }),
    ("out_back", |x| {
        1.0 + 2.701_58 * cube(x - 1.0) + 1.701_58 * square(x - 1.0)
    }),
    ("out_bounce", out_bounce),
    ("out_circ", |x| {
        f64::from(1.0 - square(x - 1.0)).sqrt() as f32
    }),
    ("out_cubic", |x| 1.0 - cube(1.0 - x)),
    ("out_elastic", |x| {
        if x == 0.0 {
            return 0.0;
        }
        if x == 1.0 {
            return 1.0;
        }
        let x = f64::from(x);
        (jmath::pow(2.0, -10.0 * x) * jmath::sin((x * 10.0 - 0.75) * TWO_THIRDS_PI) + 1.0) as f32
    }),
    ("out_expo", |x| {
        if x == 1.0 {
            1.0
        } else {
            1.0 - jmath::pow(2.0, -10.0 * f64::from(x)) as f32
        }
    }),
    ("out_quad", |x| 1.0 - square(1.0 - x)),
    ("out_quart", |x| 1.0 - square(square(1.0 - x))),
    ("out_quint", |x| {
        1.0 - jmath::pow(1.0 - f64::from(x), 5.0) as f32
    }),
    ("out_sine", |x| trig::sin(f64::from(x * HALF_PI))),
];

/// `EasingType.CubicBezier.CubicCurve`.
#[derive(Clone, Copy, Debug)]
pub struct Curve {
    a: f32,
    b: f32,
    c: f32,
}

impl Curve {
    /// `EasingType.CubicBezier.curveFromControls`.
    fn from_controls(v1: f32, v2: f32) -> Self {
        Self {
            a: 3.0 * v1 - 3.0 * v2 + 1.0,
            b: -6.0 * v1 + 3.0 * v2,
            c: 3.0 * v1,
        }
    }
    /// `EasingType.CubicBezier.CubicCurve.sample`.
    fn sample(self, t: f32) -> f32 {
        ((self.a * t + self.b) * t + self.c) * t
    }
    /// `EasingType.CubicBezier.CubicCurve.sampleGradient`.
    fn gradient(self, t: f32) -> f32 {
        (3.0 * self.a * t + 2.0 * self.b) * t + self.c
    }
}

/// `EasingType`: a named easing or a cubic Bézier.
#[derive(Clone, Copy, Debug)]
pub enum Easing {
    Named(fn(f32) -> f32),
    CubicBezier { x: Curve, y: Curve },
}

impl Easing {
    pub const LINEAR: Self = Self::Named(|x| x);
    const CONSTANT: Self = Self::Named(|_| 0.0);

    /// `EasingType.CODEC`: a name, or `{"cubic_bezier": [x1, y1, x2, y2]}` with x1 and x2 in
    /// 0-1 (`EasingType.CubicBezierControls.validate`).
    pub fn parse(json: &Json) -> Result<Self, String> {
        if let Some(name) = json.as_str() {
            return EASINGS
                .iter()
                .find(|e| e.0 == name)
                .map(|e| Self::Named(e.1))
                .ok_or_else(|| format!("unknown easing {name}"));
        }
        let controls = json
            .get("cubic_bezier")
            .and_then(Json::as_array)
            .filter(|c| c.len() == 4);
        let controls = controls.ok_or_else(|| format!("not an easing: {json}"))?;
        let mut c = [0.0f32; 4];
        for (slot, value) in c.iter_mut().zip(controls) {
            *slot = value.as_f64().ok_or("cubic_bezier needs numbers")? as f32;
        }
        let [x1, y1, x2, y2] = c;
        if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
            return Err("cubic_bezier x1 and x2 must be in [0; 1]".into());
        }
        Ok(Self::CubicBezier {
            x: Curve::from_controls(x1, x2),
            y: Curve::from_controls(y1, y2),
        })
    }

    /// `EasingType.apply`.
    pub fn apply(&self, x: f32) -> f32 {
        match *self {
            Self::Named(f) => f(x),
            Self::CubicBezier { x: xc, y: yc } => yc.sample(solve_t(xc, x)),
        }
    }
}

/// `EasingType.CubicBezier.solveT`: 4 Newton-Raphson steps, then the bisection.
fn solve_t(curve: Curve, x: f32) -> f32 {
    let mut t = x;
    for _ in 0..4 {
        let error = curve.sample(t) - x;
        if error.abs() < 1.0e-5 {
            return t;
        }
        let gradient = curve.gradient(t);
        if gradient < 1.0e-5 {
            break;
        }
        // `Mth.clamp(float)`.
        let step = error / gradient;
        t -= if step < -0.25 {
            -0.25
        } else if step.is_nan() {
            step
        } else {
            step.min(0.25)
        };
    }
    solve_t_bisect(curve, x, t)
}

/// `EasingType.CubicBezier.solveTBisect`: halves 0-1 from `t` until the curve is within 1e-5.
fn solve_t_bisect(curve: Curve, x: f32, mut t: f32) -> f32 {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    while t0 < t1 {
        let error = curve.sample(t) - x;
        if error.abs() < 1.0e-5 {
            return t;
        }
        if error < 0.0 {
            t0 = t;
        } else {
            t1 = t;
        }
        t = (t1 + t0) / 2.0;
    }
    t
}

/// `AttributeTrack`: a modifier and the keyframes of its argument (`KeyframeTrack`).
pub struct Track {
    pub attribute: &'static Attribute,
    keyframes: Vec<(i32, Entry)>,
    easing: Easing,
}

impl Track {
    /// `AttributeTrack.createCodec`: `{"modifier"?, "keyframes": [{"ticks", "value"}],
    /// "ease"?}`, the modifier `override` by default, with `KeyframeTrack.validateKeyframes`.
    pub fn parse(attribute: &'static Attribute, json: &Json) -> Result<Self, String> {
        let op = json
            .get("modifier")
            .map_or(Some("override"), Json::as_str)
            .ok_or("modifier is not a string")?;
        let list = json
            .get("keyframes")
            .and_then(Json::as_array)
            .ok_or("no keyframes")?;
        let keyframes = list
            .iter()
            .map(|k| {
                let ticks = k
                    .get("ticks")
                    .and_then(Json::as_i64)
                    .ok_or("keyframe without ticks")?;
                let ticks = i32::try_from(ticks)
                    .ok()
                    .filter(|t| *t >= 0)
                    .ok_or("ticks must be non-negative")?;
                let value = k.get("value").ok_or("keyframe without value")?;
                Ok((ticks, Entry::with_modifier(attribute, op, value)?))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if keyframes.is_empty() {
            return Err("Keyframes must not be empty".into());
        }
        if keyframes.windows(2).any(|w| w[1].0 < w[0].0) {
            return Err("Keyframes must be ordered by ticks field".into());
        }
        if keyframes.len() > 1 {
            // As vanilla counts it, from the last keyframe's ticks.
            let (mut repeat, mut last) = (0, keyframes[keyframes.len() - 1].0);
            for &(ticks, _) in &keyframes {
                if ticks == last {
                    repeat += 1;
                    if repeat > 2 {
                        return Err(format!("More than 2 keyframes on same tick: {ticks}"));
                    }
                } else {
                    repeat = 0;
                }
                last = ticks;
            }
        }
        let easing = json.get("ease").map_or(Ok(Easing::LINEAR), Easing::parse)?;
        Ok(Self {
            attribute,
            keyframes,
            easing,
        })
    }

    /// `KeyframeTrack.validatePeriod`: every keyframe within `[0; period]`.
    pub fn validate_period(&self, period: i32) -> Result<(), String> {
        match self.keyframes.iter().find(|k| k.0 < 0 || k.0 > period) {
            Some((ticks, _)) => Err(format!(
                "Keyframe at tick {ticks} must be in range [0; {period}]"
            )),
            None => Ok(()),
        }
    }

    /// `KeyframeTrackSampler.bakeSegments`: with a period, the last keyframe also leads into
    /// the first across the loop.
    pub fn sampler(&self, period: Option<i32>) -> TrackSampler {
        let k = &self.keyframes;
        let segment =
            |from: &(i32, Entry), from_ticks: i32, to: &(i32, Entry), to_ticks: i32| Segment {
                easing: self.easing,
                from: from.1.clone(),
                from_ticks,
                to: to.1.clone(),
                to_ticks,
            };
        let mut segments = Vec::new();
        if k.len() == 1 {
            segments.push(Segment {
                easing: Easing::CONSTANT,
                from: k[0].1.clone(),
                from_ticks: 0,
                to: k[0].1.clone(),
                to_ticks: 0,
            });
        } else {
            let (first, last) = (&k[0], &k[k.len() - 1]);
            if let Some(p) = period {
                segments.push(segment(last, last.0 - p, first, first.0));
            }
            self.add_segments_from_keyframes(&mut segments);
            if let Some(p) = period {
                segments.push(segment(last, last.0, first, first.0 + p));
            }
        }
        TrackSampler {
            attribute: self.attribute,
            period,
            segments,
        }
    }
}

impl Track {
    /// `KeyframeTrackSampler.addSegmentsFromKeyframes`: one segment per pair of keyframes.
    fn add_segments_from_keyframes(&self, out: &mut Vec<Segment>) {
        for w in self.keyframes.windows(2) {
            out.push(Segment {
                easing: self.easing,
                from: w[0].1.clone(),
                from_ticks: w[0].0,
                to: w[1].1.clone(),
                to_ticks: w[1].0,
            });
        }
    }
}

struct Segment {
    easing: Easing,
    from: Entry,
    from_ticks: i32,
    to: Entry,
    to_ticks: i32,
}

/// `AttributeTrackSampler` over a `KeyframeTrackSampler`: the track's modifier, with the
/// argument sampled at a clock's total ticks.
pub struct TrackSampler {
    pub attribute: &'static Attribute,
    period: Option<i32>,
    segments: Vec<Segment>,
}

impl TrackSampler {
    /// `KeyframeTrackSampler.sample`: the argument at `ticks`, looped by the period
    /// (`KeyframeTrackSampler.loopTicks`: Java's floor modulo).
    pub fn sample(&self, ticks: i64) -> Entry {
        let t = self
            .period
            .map_or(ticks, |p| ticks.rem_euclid(i64::from(p)));
        let segment = self
            .segments
            .iter()
            .find(|s| t < i64::from(s.to_ticks))
            .unwrap_or_else(|| &self.segments[self.segments.len() - 1]);
        if t <= i64::from(segment.from_ticks) {
            return segment.from.clone();
        }
        if t >= i64::from(segment.to_ticks) {
            return segment.to.clone();
        }
        let alpha = (t - i64::from(segment.from_ticks)) as f32
            / (segment.to_ticks - segment.from_ticks) as f32;
        Entry::keyframe_lerp(
            self.attribute,
            segment.easing.apply(alpha),
            &segment.from,
            &segment.to,
        )
    }

    /// `AttributeTrackSampler.applyTimeBased`.
    pub fn apply(&self, base: &Value, ticks: i64) -> Value {
        self.sample(ticks).apply(base)
    }
}

/// `Timeline`: tracks on one clock, optionally looping every `period_ticks`.
pub struct Timeline {
    /// The `WorldClock` it reads.
    pub clock: String,
    pub period: Option<i32>,
    pub tracks: Vec<Track>,
}

impl Timeline {
    /// `Timeline.DIRECT_CODEC` with `Timeline.validateInternal`: time markers inside the
    /// period, and every track's keyframes within it. Tracks of attributes that aren't
    /// registered fail.
    pub fn parse(json: &Json) -> Result<Self, String> {
        let clock = json
            .get("clock")
            .and_then(Json::as_str)
            .ok_or("no clock")?
            .to_owned();
        let period = match json.get("period_ticks") {
            None => None,
            Some(p) => Some(
                p.as_i64()
                    .and_then(|p| i32::try_from(p).ok())
                    .filter(|p| *p > 0)
                    .ok_or("period_ticks must be positive")?,
            ),
        };
        let mut tracks = Vec::new();
        for (id, track) in json
            .get("tracks")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let attribute = attribute(id).ok_or_else(|| format!("unknown attribute {id}"))?;
            tracks.push(Track::parse(attribute, track).map_err(|e| format!("{id}: {e}"))?);
        }
        if let Some(period) = period {
            for (id, marker) in json
                .get("time_markers")
                .and_then(Json::as_object)
                .into_iter()
                .flatten()
            {
                let ticks = marker
                    .as_i64()
                    .or_else(|| marker.get("ticks").and_then(Json::as_i64))
                    .unwrap_or(-1);
                if ticks < 0 || ticks >= i64::from(period) {
                    return Err(format!("Time Marker {id} must be in range [0; {period})"));
                }
            }
            for track in &tracks {
                track.validate_period(period)?;
            }
        }
        Ok(Self {
            clock,
            period,
            tracks,
        })
    }

    /// `Timeline.getCurrentTicks`: the clock's total ticks, within the period when there is one.
    pub fn current_ticks(&self, total: i64) -> i64 {
        self.period.map_or(total, |p| total % i64::from(p))
    }

    /// `Timeline.getPeriodCount`: whole periods the clock has run.
    pub fn period_count(&self, total: i64) -> i32 {
        self.period.map_or(0, |p| (total / i64::from(p)) as i32)
    }
}
