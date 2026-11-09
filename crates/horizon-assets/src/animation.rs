//! Java Edition 26.3 animation metadata and sprite tick selection.
use crate::json::*;
use serde_json::value::RawValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationFrame {
    pub index: i32,
    pub time: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationMetadata {
    pub frames: Option<Vec<AnimationFrame>>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub frame_time: i32,
    pub interpolate: bool,
    /// Polytone's `mode` (wiki "Variant Textures"): what drives the animation instead of the
    /// game's ticks.
    pub mode: Option<ClipMode>,
}

/// What drives a Polytone animation (its `.png.mcmeta` `mode`): the whole animation stretched over
/// one cycle of the time of day, the weather, or an expression's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipMode {
    /// `day_time`: frame 0 at time 0, the last at the cycle's end, then again.
    DayTime { cycle: i64 },
    /// `screen_time`: the ticks the open screen has been open, over the cycle, held on the last
    /// frame once it has run (it doesn't loop, measured on the real mod).
    ScreenTime { cycle: i64 },
    /// `weather`: frame 0 in clear weather, 1 in rain, 2 in a storm (the rain and thunder levels
    /// added up), at most the last.
    Weather,
    /// A Polytone expression: its value's fraction is how far through the animation it is.
    Expression(String),
    /// `game_time`, `season`: not driven; the animation runs on ticks.
    Unsupported(String),
}

/// The values [`ClipMode`]s read this frame.
pub struct ModeInputs<'a> {
    pub day_time: i64,
    /// The ticks the open screen has been open (0 without one).
    pub screen_time: i64,
    /// The rain level plus the thunder level, 0 to 2.
    pub weather: f32,
    /// An expression's value this frame, by its source.
    pub expression: &'a dyn Fn(&str) -> Option<f64>,
}

fn optional<'a>(object: &Object<'a>, name: &str) -> Option<&'a RawValue> {
    object.get(name).copied().filter(|v| v.get() != "null")
}
fn bounded_integer(value: &RawValue, minimum: i32) -> Result<i32> {
    // A lenient JsonParser exposes scanner failures as strings, which the
    // uncompressed numeric codec rejects. Model streams instead reject them
    // globally, even inside ignored fields.
    if !native_number_token(value.get()) {
        return Err(error(
            "native reader classifies animation number as a string",
        ));
    }
    let value = integer(value)?;
    if value < minimum {
        return Err(error("animation integer outside codec range"));
    }
    Ok(value)
}
impl AnimationMetadata {
    /// Decode the animation section itself, using the uncompressed JsonOps codec
    /// rules. Numeric primitives truncate and wrap as Java Number.intValue does;
    /// strings and booleans are not numbers. Null optional fields are absent.
    pub fn from_json(text: &str) -> Result<Self> {
        let raw: &RawValue = serde_json::from_str(text)?;
        let fields = object(raw)?;
        let positive = |name| {
            optional(&fields, name)
                .map(|v| bounded_integer(v, 1))
                .transpose()
        };
        let frames = optional(&fields, "frames")
            .map(|v| {
                array(v)?
                    .into_iter()
                    .map(|v| {
                        if v.get().starts_with('{') {
                            let fields = object(v)?;
                            Ok(AnimationFrame {
                                index: bounded_integer(required(&fields, "index")?, 0)?,
                                time: optional(&fields, "time")
                                    .map(|v| bounded_integer(v, 1))
                                    .transpose()?,
                            })
                        } else {
                            Ok(AnimationFrame {
                                index: bounded_integer(v, 0)?,
                                time: None,
                            })
                        }
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        Ok(Self {
            frames,
            width: positive("width")?,
            height: positive("height")?,
            frame_time: positive("frametime")?.unwrap_or(1),
            interpolate: optional(&fields, "interpolate")
                .map(|v| serde_json::from_str(v.get()))
                .transpose()?
                .unwrap_or(false),
            mode: optional(&fields, "mode")
                .and_then(|v| serde_json::from_str::<String>(v.get()).ok())
                .map(|mode| {
                    let cycle = optional(&fields, "time_cycle_duration")
                        .and_then(|v| serde_json::from_str::<f64>(v.get()).ok())
                        .filter(|c| *c >= 1.0)
                        .map_or(24_000, |c| c as i64);
                    match mode.as_str() {
                        "day_time" => ClipMode::DayTime { cycle },
                        "weather" => ClipMode::Weather,
                        "screen_time" => ClipMode::ScreenTime { cycle },
                        "game_time" | "season" => ClipMode::Unsupported(mode),
                        _ => ClipMode::Expression(mode),
                    }
                }),
        })
    }

    pub fn frame_size(&self, image_width: i32, image_height: i32) -> [i32; 2] {
        match (self.width, self.height) {
            (Some(w), Some(h)) => [w, h],
            (Some(w), None) => [w, image_height],
            (None, Some(h)) => [image_width, h],
            (None, None) => [image_width.min(image_height); 2],
        }
    }

    /// Build the animation after image/frame dimensions have been validated by
    /// the resource loader. Out-of-image indices are removed, preserving order
    /// and repeats. Zero or one remaining entries mean static physical frame 0.
    pub fn clip(&self, physical_frames: u32) -> Result<Option<AnimationClip>> {
        if self.frame_time <= 0 {
            return Err(error("nonpositive default animation duration"));
        }
        let sequence: Vec<_> = match &self.frames {
            Some(frames) => frames
                .iter()
                .filter_map(|v| {
                    let time = v.time.unwrap_or(self.frame_time);
                    (v.index >= 0 && (v.index as u32) < physical_frames && time > 0)
                        .then_some((v.index as u32, time as u32))
                })
                .collect(),
            None => (0..physical_frames)
                .map(|i| (i, self.frame_time as u32))
                .collect(),
        };
        if sequence.len() <= 1 {
            return Ok(None);
        }
        let mut duration = 0u64;
        let ends = sequence
            .iter()
            .map(|&(_, time)| {
                duration = duration
                    .checked_add(u64::from(time))
                    .ok_or_else(|| error("animation duration overflow"))?;
                Ok(duration)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(AnimationClip {
            sequence,
            ends,
            duration,
            interpolate: self.interpolate,
            mode: self.mode.clone(),
        }))
    }
}

#[derive(Clone, Debug)]
pub struct AnimationClip {
    sequence: Vec<(u32, u32)>,
    ends: Vec<u64>,
    duration: u64,
    interpolate: bool,
    mode: Option<ClipMode>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationSample {
    pub sequence_index: usize,
    pub sub_frame: u32,
    pub frame: u32,
    pub next_frame: u32,
    pub dirty: bool,
    pub needs_draw: bool,
    /// Native drawToAtlas encodes floor((subFrame / time) * 1000) in its
    /// first-vertex argument. This is not a claim about GPU interpolation pixels.
    pub interpolation_millis: u32,
}
impl AnimationClip {
    pub fn sequence(&self) -> &[(u32, u32)] {
        &self.sequence
    }
    pub fn duration(&self) -> u64 {
        self.duration
    }
    pub fn interpolates(&self) -> bool {
        self.interpolate
    }
    pub fn mode(&self) -> Option<&ClipMode> {
        self.mode.as_ref()
    }

    /// Random access to the state after `tick` native ticks. Binary search avoids
    /// advancing every missed tick when a render frame takes longer than 50 ms.
    pub fn sample(&self, tick: u64) -> AnimationSample {
        let time = tick % self.duration;
        let index = self.ends.partition_point(|end| *end <= time);
        let start = if index == 0 { 0 } else { self.ends[index - 1] };
        let sub_frame = (time - start) as u32;
        let (frame, duration) = self.sequence[index];
        let previous = (index + self.sequence.len() - 1) % self.sequence.len();
        let dirty = tick == 0 || (sub_frame == 0 && frame != self.sequence[previous].0);
        AnimationSample {
            sequence_index: index,
            sub_frame,
            frame,
            next_frame: self.sequence[(index + 1) % self.sequence.len()].0,
            dirty,
            needs_draw: self.interpolate || dirty,
            interpolation_millis: ((sub_frame as f32 / duration as f32) * 1000.0) as u32,
        }
    }
}

impl AnimationClip {
    /// What a Polytone-driven animation shows now (measured on the real mod, Polytone
    /// 26.3-6.8.6: with four frames, times 0, 6000, 12000 and 18000 show frames 0 to 3, an
    /// expression `0.6` frame 2 and `2` frame 0, clear weather frame 0, rain frame 1 and a storm,
    /// even after 15 seconds, frame 2); `None` for a plain animation, which runs on ticks.
    pub fn sample_mode(&self, inputs: &ModeInputs) -> Option<AnimationSample> {
        let fraction = match self.mode.as_ref()? {
            ClipMode::DayTime { cycle } => day_fraction(inputs.day_time, *cycle),
            ClipMode::ScreenTime { cycle } => {
                (inputs.screen_time.max(0) as f64 / *cycle as f64).min(1.0 - f64::EPSILON)
            }
            ClipMode::Weather => {
                let last = self.sequence.len() - 1;
                let index = (inputs.weather.max(0.0) as usize).min(last);
                let start = if index == 0 { 0 } else { self.ends[index - 1] };
                return Some(self.sample(start));
            }
            ClipMode::Expression(source) => (inputs.expression)(source)?.rem_euclid(1.0),
            ClipMode::Unsupported(_) => return None,
        };
        Some(self.sample_fraction(fraction))
    }

    /// The state `fraction` (0 to 1) of the way through the whole animation, mixing within a
    /// frame by how far into it that is.
    fn sample_fraction(&self, fraction: f64) -> AnimationSample {
        let time = fraction.clamp(0.0, 1.0) * self.duration as f64;
        let mut sample = self.sample((time as u64).min(self.duration - 1));
        let (_, frame_time) = self.sequence[sample.sequence_index];
        let start = if sample.sequence_index == 0 {
            0
        } else {
            self.ends[sample.sequence_index - 1]
        };
        let into = ((time - start as f64) / f64::from(frame_time)).clamp(0.0, 1.0);
        sample.interpolation_millis = (into * 1000.0) as u32;
        sample
    }
}

fn day_fraction(day_time: i64, cycle: i64) -> f64 {
    day_time.rem_euclid(cycle) as f64 / cycle as f64
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    fn clip(mcmeta: &str) -> AnimationClip {
        AnimationMetadata::from_json(mcmeta)
            .unwrap()
            .clip(4)
            .unwrap()
            .unwrap()
    }

    fn frame(clip: &AnimationClip, day_time: i64, weather: f32, value: f64) -> u32 {
        let expression = move |_: &str| Some(value);
        clip.sample_mode(&ModeInputs {
            day_time,
            screen_time: day_time,
            weather,
            expression: &expression,
        })
        .unwrap()
        .frame
    }

    #[test]
    fn polytone_modes_pick_the_frames_the_real_mod_shows() {
        let day = clip(r#"{"frametime": 1, "mode": "day_time"}"#);
        let frames: Vec<u32> = [0, 6000, 12_000, 18_000, 30_000]
            .map(|t| frame(&day, t, 0.0, 0.0))
            .into();
        assert_eq!(frames, [0, 1, 2, 3, 1]);
        // 26.3-6.8.6, four frames over 200 ticks: 40 ticks open show the first, and from ten
        // seconds on the last, even after 27 (it doesn't loop).
        let screen = clip(r#"{"frametime": 1, "mode": "screen_time", "time_cycle_duration": 200}"#);
        let frames: Vec<u32> = [0, 40, 200, 360, 540]
            .map(|t| frame(&screen, t, 0.0, 0.0))
            .into();
        assert_eq!(frames, [0, 0, 3, 3, 3]);
        let value = clip(r#"{"frametime": 1, "mode": "0.6"}"#);
        assert_eq!(frame(&value, 0, 0.0, 0.6), 2);
        assert_eq!(frame(&value, 0, 0.0, 2.0), 0);
        let weather = clip(r#"{"frametime": 1, "mode": "weather"}"#);
        let frames: Vec<u32> = [0.0, 1.0, 2.0].map(|w| frame(&weather, 0, w, 0.0)).into();
        assert_eq!(frames, [0, 1, 2]);
        // Three frames: clear, rain and storm are the first, the middle and the last.
        let three = AnimationMetadata::from_json(r#"{"frametime": 1, "mode": "weather"}"#)
            .unwrap()
            .clip(3)
            .unwrap()
            .unwrap();
        let frames: Vec<u32> = [0.0, 1.0, 2.0].map(|w| frame(&three, 0, w, 0.0)).into();
        assert_eq!(frames, [0, 1, 2]);
        // A plain animation and the modes not driven yet run on ticks.
        assert!(
            clip(r#"{"frametime": 1}"#)
                .sample_mode(&ModeInputs {
                    day_time: 0,
                    screen_time: 0,
                    weather: 0.0,
                    expression: &|_| None
                })
                .is_none()
        );
        assert!(
            clip(r#"{"frametime": 1, "mode": "season"}"#)
                .sample_mode(&ModeInputs {
                    day_time: 0,
                    screen_time: 0,
                    weather: 0.0,
                    expression: &|_| None
                })
                .is_none()
        );
    }

    #[test]
    fn a_custom_cycle_stretches_the_day() {
        let day = clip(r#"{"frametime": 1, "mode": "day_time", "time_cycle_duration": 1000}"#);
        assert_eq!(frame(&day, 250, 0.0, 0.0), 1);
        assert_eq!(frame(&day, 1250, 0.0, 0.0), 1);
    }
}
