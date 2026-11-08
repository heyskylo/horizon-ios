//! Environment attributes (`net.minecraft.world.attribute`): the registry of attributes with
//! their types, defaults and ranges (`EnvironmentAttributes`, `AttributeTypes`,
//! `AttributeRange`), and one map entry, a value or a `{modifier, argument}` object, parsed as
//! `EnvironmentAttributeMap.Entry`'s codec does and applied as `AttributeModifier.apply`.
//! Colors are float vectors as in 26.3 (`Vector3fc`, `Vector4fc` with alpha last), and all math
//! is `f32` in Java's order of operations.
use serde_json::Value as Json;

/// `AttributeTypes`. The types without a ported codec are `Other`: their values are kept as
/// JSON and can only be overridden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Boolean,
    Float,
    AngleDegrees,
    RgbColor,
    ArgbColor,
    Other,
}

/// An attribute's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Float(f32),
    Rgb([f32; 3]),
    Argb([f32; 4]),
    Other(Json),
}

enum Default {
    Bool(bool),
    Float(f32),
    Rgb(i32),
    Argb(i32),
    Unported,
}

/// `AttributeRange`: `ofFloat(min, max)` or `any()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Range {
    Any,
    Float(f32, f32),
}

const UNIT: Range = Range::Float(0.0, 1.0);
const NON_NEGATIVE: Range = Range::Float(0.0, f32::INFINITY);

/// One registered attribute (`EnvironmentAttribute`).
pub struct Attribute {
    /// The id without its `minecraft:` namespace.
    pub id: &'static str,
    pub kind: Kind,
    pub range: Range,
    default: Default,
}

const fn a(id: &'static str, kind: Kind, default: Default, range: Range) -> Attribute {
    Attribute {
        id,
        kind,
        range,
        default,
    }
}

/// `EnvironmentAttributes`, in registration order.
pub static ATTRIBUTES: [Attribute; 51] = {
    use Default as D;
    use Kind::*;
    [
        a("visual/fog_color", RgbColor, D::Rgb(0), Range::Any),
        a(
            "visual/fog_start_distance",
            Float,
            D::Float(0.0),
            Range::Any,
        ),
        a(
            "visual/fog_end_distance",
            Float,
            D::Float(1024.0),
            NON_NEGATIVE,
        ),
        a(
            "visual/sky_fog_end_distance",
            Float,
            D::Float(512.0),
            NON_NEGATIVE,
        ),
        a(
            "visual/cloud_fog_end_distance",
            Float,
            D::Float(2048.0),
            NON_NEGATIVE,
        ),
        a(
            "visual/water_fog_color",
            RgbColor,
            D::Rgb(-16_448_205),
            Range::Any,
        ),
        a(
            "visual/water_fog_start_distance",
            Float,
            D::Float(-8.0),
            Range::Any,
        ),
        a(
            "visual/water_fog_end_distance",
            Float,
            D::Float(96.0),
            NON_NEGATIVE,
        ),
        a("visual/sky_color", RgbColor, D::Rgb(0), Range::Any),
        a(
            "visual/sunrise_sunset_color",
            ArgbColor,
            D::Argb(0),
            Range::Any,
        ),
        a("visual/cloud_color", ArgbColor, D::Argb(0), Range::Any),
        a("visual/cloud_height", Float, D::Float(192.33), Range::Any),
        a("visual/sun_angle", AngleDegrees, D::Float(0.0), Range::Any),
        a("visual/moon_angle", AngleDegrees, D::Float(0.0), Range::Any),
        a("visual/star_angle", AngleDegrees, D::Float(0.0), Range::Any),
        a("visual/moon_phase", Other, D::Unported, Range::Any),
        a("visual/star_brightness", Float, D::Float(0.0), UNIT),
        a(
            "visual/block_light_tint",
            RgbColor,
            D::Rgb(-10_100),
            Range::Any,
        ),
        a("visual/sky_light_color", RgbColor, D::Rgb(-1), Range::Any),
        a("visual/sky_light_factor", Float, D::Float(1.0), UNIT),
        a(
            "visual/night_vision_color",
            RgbColor,
            D::Rgb(-6_710_887),
            Range::Any,
        ),
        a(
            "visual/ambient_light_color",
            RgbColor,
            D::Rgb(-16_777_216),
            Range::Any,
        ),
        a(
            "visual/default_dripstone_particle",
            Other,
            D::Unported,
            Range::Any,
        ),
        a("visual/ambient_particles", Other, D::Unported, Range::Any),
        a("audio/background_music", Other, D::Unported, Range::Any),
        a("audio/music_volume", Float, D::Float(1.0), UNIT),
        a("audio/ambient_sounds", Other, D::Unported, Range::Any),
        a(
            "audio/firefly_bush_sounds",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a(
            "gameplay/sky_light_level",
            Float,
            D::Float(15.0),
            Range::Float(0.0, 15.0),
        ),
        a(
            "gameplay/can_start_raid",
            Boolean,
            D::Bool(true),
            Range::Any,
        ),
        a(
            "gameplay/water_evaporates",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a("gameplay/bed_rule", Other, D::Unported, Range::Any),
        a("gameplay/straw_bed_rule", Other, D::Unported, Range::Any),
        a(
            "gameplay/respawn_anchor_works",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a(
            "gameplay/nether_portal_spawns_piglin",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a("gameplay/fast_lava", Boolean, D::Bool(false), Range::Any),
        a(
            "gameplay/increased_fire_burnout",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a("gameplay/eyeblossom_open", Other, D::Unported, Range::Any),
        a(
            "gameplay/turtle_egg_hatch_chance",
            Float,
            D::Float(0.002),
            UNIT,
        ),
        a(
            "gameplay/piglins_zombify",
            Boolean,
            D::Bool(true),
            Range::Any,
        ),
        a(
            "gameplay/snow_golem_melts",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a(
            "gameplay/creaking_active",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a(
            "gameplay/surface_slime_spawn_chance",
            Float,
            D::Float(0.0),
            UNIT,
        ),
        a(
            "gameplay/cat_waking_up_gift_chance",
            Float,
            D::Float(0.0),
            UNIT,
        ),
        a(
            "gameplay/bees_stay_in_hive",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a(
            "gameplay/monsters_burn",
            Boolean,
            D::Bool(false),
            Range::Any,
        ),
        a(
            "gameplay/can_pillager_patrol_spawn",
            Boolean,
            D::Bool(true),
            Range::Any,
        ),
        a(
            "gameplay/natural_mob_spawns",
            Other,
            D::Unported,
            Range::Any,
        ),
        a(
            "gameplay/creature_world_gen_spawn_probability",
            Float,
            D::Float(0.1),
            Range::Float(0.0, 0.999_999_9),
        ),
        a("gameplay/villager_activity", Other, D::Unported, Range::Any),
        a(
            "gameplay/baby_villager_activity",
            Other,
            D::Unported,
            Range::Any,
        ),
    ]
};

/// The attribute an id names, with or without its `minecraft:` namespace (`Identifier.parse`).
pub fn attribute(id: &str) -> Option<&'static Attribute> {
    let path = match id.split_once(':') {
        Some(("minecraft", path)) => path,
        Some(_) => return None,
        None => id,
    };
    ATTRIBUTES.iter().find(|a| a.id == path)
}

/// `ARGB.from8BitChannel` of each byte: `vector3fFromRGB24` / `vector4fFromARGB32`.
fn channels(color: i32) -> [f32; 4] {
    let c = color as u32;
    let byte = |shift: u32| (c >> shift & 255) as f32 / 255.0;
    [byte(16), byte(8), byte(0), byte(24)]
}

impl Attribute {
    /// `EnvironmentAttribute.defaultValue`; `None` for the types not ported yet.
    pub fn default_value(&self) -> Option<Value> {
        Some(match self.default {
            Default::Bool(b) => Value::Bool(b),
            Default::Float(f) => Value::Float(f),
            Default::Rgb(c) => {
                let [r, g, b, _] = channels(c);
                Value::Rgb([r, g, b])
            }
            Default::Argb(c) => Value::Argb(channels(c)),
            Default::Unported => return None,
        })
    }

    /// `AttributeRange.sanitize`: a float outside the range goes through `Mth.clamp`
    /// (`v < min ? min : Math.min(v, max)`, so NaN stays NaN).
    pub fn sanitize(&self, value: Value) -> Value {
        match (self.range, value) {
            (Range::Float(min, max), Value::Float(v)) if !(v >= min && v <= max) => {
                Value::Float(if v < min { min } else { java_min(v, max) })
            }
            (_, value) => value,
        }
    }

    /// `EnvironmentAttribute.valueCodec`: the type's codec, then `AttributeRange.validate`.
    fn value(&self, json: &Json) -> Result<Value, String> {
        let value = match self.kind {
            Kind::Boolean => Value::Bool(boolean(json)?),
            Kind::Float | Kind::AngleDegrees => Value::Float(float(json)?),
            Kind::RgbColor => Value::Rgb(rgb(json)?),
            Kind::ArgbColor => Value::Argb(argb(json)?),
            // These types' modifiers (`append`, `overlay`) aren't ported yet.
            Kind::Other if json.get("modifier").is_some() => {
                return Err("modifiers on this attribute are not ported".into());
            }
            Kind::Other => Value::Other(json.clone()),
        };
        if let (Range::Float(min, max), Value::Float(v)) = (self.range, &value)
            && !(*v >= min && *v <= max)
        {
            return Err(format!("{v} is not in range [{min}; {max}]"));
        }
        Ok(value)
    }
}

/// `Codec.BOOL` through `JsonOps.getBooleanValue`: only a JSON boolean.
fn boolean(json: &Json) -> Result<bool, String> {
    json.as_bool()
        .ok_or_else(|| format!("not a boolean: {json}"))
}

/// `Codec.FLOAT` through `JsonOps.getNumberValue`: any JSON number, as a float. (Gson rounds the
/// decimal text to a float once; this rounds through a double, which differs only for decimals
/// a hair from a float midpoint.)
fn float(json: &Json) -> Result<f32, String> {
    json.as_f64()
        .map(|f| f as f32)
        .ok_or_else(|| format!("not a number: {json}"))
}

/// `Codec.INT` of a JSON number (`Number.intValue`: truncated).
fn int(json: &Json) -> Option<i32> {
    let n = json.as_number()?;
    n.as_i64()
        .map(|i| i as i32)
        .or_else(|| n.as_f64().map(|f| f as i64 as i32))
}

/// `ExtraCodecs.hexColor`: `#` and exactly `digits` hex digits.
fn hex(json: &Json, digits: usize) -> Option<i32> {
    let hex = json.as_str()?.strip_prefix('#')?;
    if hex.len() != digits || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(|v| v as i32)
}

/// `Codec.FLOAT.listOf()` of exactly `N` numbers (`VECTOR3F`, `VECTOR4F`).
fn vector<const N: usize>(json: &Json) -> Option<[f32; N]> {
    let list = json.as_array().filter(|l| l.len() == N)?;
    let mut out = [0.0; N];
    for (slot, item) in out.iter_mut().zip(list) {
        *slot = float(item).ok()?;
    }
    Some(out)
}

/// `ExtraCodecs.STRING_RGB_VEC3_COLOR`: `#rrggbb`, else `[r, g, b]`, else an int.
fn rgb(json: &Json) -> Result<[f32; 3], String> {
    let from_int = |c: i32| {
        let [r, g, b, _] = channels(c);
        [r, g, b]
    };
    hex(json, 6)
        .map(from_int)
        .or_else(|| vector(json))
        .or_else(|| int(json).map(from_int))
        .ok_or_else(|| format!("not an RGB color: {json}"))
}

/// `ExtraCodecs.STRING_ARGB_VEC4_COLOR`: `#aarrggbb`, else `[r, g, b, a]`, else an int.
fn argb(json: &Json) -> Result<[f32; 4], String> {
    hex(json, 8)
        .map(channels)
        .or_else(|| vector(json))
        .or_else(|| int(json).map(channels))
        .ok_or_else(|| format!("not an ARGB color: {json}"))
}

/// `AttributeModifier.OperationId`, with the argument each operation reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// `override`: the argument replaces the value.
    Override(Value),
    /// `FloatModifier.ALPHA_BLEND`: `Mth.lerp(alpha, subject, value)`.
    AlphaBlendFloat {
        value: f32,
        alpha: f32,
    },
    Add(Value),
    Subtract(Value),
    Multiply(Value),
    Minimum(f32),
    Maximum(f32),
    /// `ColorModifier.ALPHA_BLEND_RGB` / `_ARGB`, by an ARGB color.
    AlphaBlendColor([f32; 4]),
    /// `ColorModifier.BLEND_TO_GRAY_RGB` / `_ARGB`.
    BlendToGray {
        brightness: f32,
        factor: f32,
    },
    Boolean(BoolOp, bool),
    /// A modifier of a type not ported yet (`ListModifier.append`, `MobSpawnSettingsModifier`
    /// `overlay`): kept, and applying it leaves the value as it was.
    Unported(Json),
    /// An entry computed when it applies, from the value so far (not vanilla: a resource pack's
    /// expression). `None` leaves the value as it was.
    Dynamic(Dynamic),
}

/// The function behind [`Entry::Dynamic`]; two are equal when they are the same function.
#[derive(Clone)]
pub struct Dynamic(pub std::sync::Arc<dyn Fn(&Value) -> Option<Entry> + Send + Sync>);

impl std::fmt::Debug for Dynamic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dynamic")
    }
}

impl PartialEq for Dynamic {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// `BooleanModifier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
}

/// `Codec.floatRange(0, 1)`.
fn unit(json: &Json, name: &str) -> Result<f32, String> {
    let v = float(json)?;
    if (0.0..=1.0).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{name} {v} is outside [0, 1]"))
    }
}

impl Entry {
    /// `EnvironmentAttributeMap.Entry.createCodec`: the attribute's value (an override), else
    /// `{"modifier": op, "argument": ...}` with an operation the attribute's type allows.
    pub fn parse(attribute: &Attribute, json: &Json) -> Result<Self, String> {
        let value_error = match attribute.value(json) {
            Ok(value) => return Ok(Self::Override(value)),
            Err(e) => e,
        };
        let Some(object) = json.as_object() else {
            return Err(value_error);
        };
        if attribute.kind == Kind::Other && object.contains_key("modifier") {
            return Ok(Self::Unported(json.clone()));
        }
        let op = object
            .get("modifier")
            .and_then(Json::as_str)
            .ok_or_else(|| format!("{value_error}; and no modifier"))?;
        let argument = object.get("argument").ok_or("no argument")?;
        Self::with_modifier(attribute, op, argument)
    }

    /// `EnvironmentAttributeMap.Entry.createFullCodec`: the entry `op` makes of `argument` on
    /// this attribute, with the modifier's argument codec (also a timeline keyframe's value,
    /// under its track's modifier).
    pub fn with_modifier(attribute: &Attribute, op: &str, argument: &Json) -> Result<Self, String> {
        let kind = attribute.kind;
        let color = matches!(kind, Kind::RgbColor | Kind::ArgbColor);
        let simple = |argument: &Json| -> Result<Value, String> {
            Ok(match kind {
                Kind::Float | Kind::AngleDegrees => Value::Float(float(argument)?),
                // `RgbModifier`s read an RGB argument, also on ARGB attributes; `MULTIPLY_ARGB`
                // reads ARGB (handled below).
                Kind::RgbColor | Kind::ArgbColor => Value::Rgb(rgb(argument)?),
                _ => return Err(format!("{op} is not allowed on this attribute")),
            })
        };
        let bool_op = |op: BoolOp| -> Result<Self, String> {
            match kind {
                Kind::Boolean => Ok(Self::Boolean(op, boolean(argument)?)),
                _ => Err("boolean modifiers need a boolean attribute".into()),
            }
        };
        Ok(match op {
            "override" => Self::Override(attribute.value(argument)?),
            "add" => Self::Add(simple(argument)?),
            "subtract" => Self::Subtract(simple(argument)?),
            "multiply" if kind == Kind::ArgbColor => Self::Multiply(Value::Argb(argb(argument)?)),
            "multiply" => Self::Multiply(simple(argument)?),
            "minimum" | "maximum" if matches!(kind, Kind::Float | Kind::AngleDegrees) => {
                let v = float(argument)?;
                if op == "minimum" {
                    Self::Minimum(v)
                } else {
                    Self::Maximum(v)
                }
            }
            "alpha_blend" if color => Self::AlphaBlendColor(argb(argument)?),
            "alpha_blend" if matches!(kind, Kind::Float | Kind::AngleDegrees) => match argument {
                // `FloatWithAlpha.CODEC`: a float, or `{value, alpha = 1}`.
                Json::Object(o) => Self::AlphaBlendFloat {
                    value: float(o.get("value").ok_or("no value")?)?,
                    alpha: o.get("alpha").map_or(Ok(1.0), |a| unit(a, "alpha"))?,
                },
                _ => Self::AlphaBlendFloat {
                    value: float(argument)?,
                    alpha: 1.0,
                },
            },
            "blend_to_gray" if color => Self::BlendToGray {
                brightness: unit(
                    argument.get("brightness").ok_or("no brightness")?,
                    "brightness",
                )?,
                factor: unit(argument.get("factor").ok_or("no factor")?, "factor")?,
            },
            "and" => bool_op(BoolOp::And)?,
            "nand" => bool_op(BoolOp::Nand)?,
            "or" => bool_op(BoolOp::Or)?,
            "nor" => bool_op(BoolOp::Nor)?,
            "xor" => bool_op(BoolOp::Xor)?,
            "xnor" => bool_op(BoolOp::Xnor)?,
            _ => return Err(format!("modifier {op} is not allowed on this attribute")),
        })
    }

    /// `AttributeTrack.bakeSampler`'s argument lerp: two keyframes of one track (same
    /// modifier) blended at `alpha`. Overrides use the type's keyframe lerp; float arguments
    /// `Mth.lerp`; color arguments `ARGB.srgbLerp`; `FloatWithAlpha` and
    /// `ColorModifier.BlendToGray.lerp` each field; boolean arguments step at 1
    /// (as `AttributeType.ofNotInterpolated` keyframes do).
    pub fn keyframe_lerp(attribute: &Attribute, alpha: f32, from: &Self, to: &Self) -> Self {
        let float = |a: f32, b: f32| a + alpha * (b - a);
        let value = |a: &Value, b: &Value| match (a, b) {
            (Value::Float(a), Value::Float(b)) => Value::Float(float(*a, *b)),
            (Value::Rgb(a), Value::Rgb(b)) => {
                Value::Rgb(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
            }
            (Value::Argb(a), Value::Argb(b)) => {
                Value::Argb(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
            }
            _ => {
                if alpha >= 1.0 {
                    b.clone()
                } else {
                    a.clone()
                }
            }
        };
        match (from, to) {
            (Self::Override(a), Self::Override(b)) => {
                Self::Override(attribute.lerp(Lerp::Keyframe, alpha, a, b))
            }
            (
                Self::AlphaBlendFloat {
                    value: v1,
                    alpha: a1,
                },
                Self::AlphaBlendFloat {
                    value: v2,
                    alpha: a2,
                },
            ) => Self::AlphaBlendFloat {
                value: float(*v1, *v2),
                alpha: float(*a1, *a2),
            },
            (Self::Add(a), Self::Add(b)) => Self::Add(value(a, b)),
            (Self::Subtract(a), Self::Subtract(b)) => Self::Subtract(value(a, b)),
            (Self::Multiply(a), Self::Multiply(b)) => Self::Multiply(value(a, b)),
            (Self::Minimum(a), Self::Minimum(b)) => Self::Minimum(float(*a, *b)),
            (Self::Maximum(a), Self::Maximum(b)) => Self::Maximum(float(*a, *b)),
            (Self::AlphaBlendColor(a), Self::AlphaBlendColor(b)) => {
                Self::AlphaBlendColor(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
            }
            (
                Self::BlendToGray {
                    brightness: b1,
                    factor: f1,
                },
                Self::BlendToGray {
                    brightness: b2,
                    factor: f2,
                },
            ) => Self::BlendToGray {
                brightness: float(*b1, *b2),
                factor: float(*f1, *f2),
            },
            _ => {
                if alpha >= 1.0 {
                    to.clone()
                } else {
                    from.clone()
                }
            }
        }
    }

    /// `AttributeModifier.apply(subject, argument)`.
    pub fn apply(&self, subject: &Value) -> Value {
        use Value::{Argb, Float, Rgb};
        let rgb_lerp = |s: [f32; 3], t: [f32; 3], f: f32| -> [f32; 3] {
            std::array::from_fn(|i| (t[i] - s[i]) * f + s[i])
        };
        match (self, subject) {
            (Self::Override(v), _) => v.clone(),
            (Self::Dynamic(f), subject) => match (f.0)(subject) {
                Some(entry) => entry.apply(subject),
                None => subject.clone(),
            },
            (Self::AlphaBlendFloat { value, alpha }, Float(s)) => Float(s + alpha * (value - s)),
            (Self::Add(Float(b)), Float(a)) => Float(a + b),
            (Self::Subtract(Float(b)), Float(a)) => Float(a - b),
            (Self::Multiply(Float(b)), Float(a)) => Float(a * b),
            (Self::Minimum(b), Float(a)) => Float(java_min(*a, *b)),
            (Self::Maximum(b), Float(a)) => Float(java_max(*a, *b)),
            // `ARGB.addRgb` / `subtractRgb`: clamped channels; alpha kept.
            (Self::Add(Rgb(b)), Rgb(a)) => Rgb(std::array::from_fn(|i| java_min(a[i] + b[i], 1.0))),
            (Self::Add(Rgb(b)), Argb(a)) => Argb(std::array::from_fn(|i| {
                if i == 3 {
                    a[3]
                } else {
                    java_min(a[i] + b[i], 1.0)
                }
            })),
            (Self::Subtract(Rgb(b)), Rgb(a)) => {
                Rgb(std::array::from_fn(|i| java_max(a[i] - b[i], 0.0)))
            }
            (Self::Subtract(Rgb(b)), Argb(a)) => Argb(std::array::from_fn(|i| {
                if i == 3 {
                    a[3]
                } else {
                    java_max(a[i] - b[i], 0.0)
                }
            })),
            // `ARGB.multiply`: component-wise.
            (Self::Multiply(Rgb(b)), Rgb(a)) => Rgb(std::array::from_fn(|i| a[i] * b[i])),
            (Self::Multiply(Argb(b)), Argb(a)) => Argb(std::array::from_fn(|i| a[i] * b[i])),
            // `ARGB.alphaBlend(Vector3fc, Vector4fc)`: lerp toward the source by its alpha.
            (Self::AlphaBlendColor(src), Rgb(dst)) => {
                if src[3] == 0.0 {
                    Rgb(*dst)
                } else {
                    Rgb(rgb_lerp(*dst, [src[0], src[1], src[2]], src[3]))
                }
            }
            // `ARGB.alphaBlend(Vector4fc, Vector4fc)`.
            (Self::AlphaBlendColor(src), Argb(dst)) => {
                let (sa, da) = (src[3], dst[3]);
                if sa == 1.0 {
                    Argb(*src)
                } else if sa == 0.0 {
                    Argb(*dst)
                } else {
                    let out = sa + da * (1.0 - sa);
                    let channel = |i: usize| (src[i] * sa + dst[i] * (out - sa)) / out;
                    Argb([channel(0), channel(1), channel(2), out])
                }
            }
            // `scaleRGB(greyscale(subject), brightness)`, then `srgbLerp(factor, subject, grey)`.
            (Self::BlendToGray { brightness, factor }, Rgb(s)) => {
                Rgb(blend_to_gray(*s, *brightness, *factor))
            }
            (Self::BlendToGray { brightness, factor }, Argb(s)) => {
                let [r, g, b] = blend_to_gray([s[0], s[1], s[2]], *brightness, *factor);
                // The grey keeps the subject's alpha, then lerps with it: equal unless infinite.
                let grey_alpha = s[3];
                Argb([r, g, b, (grey_alpha - s[3]) * factor + s[3]])
            }
            (Self::Boolean(op, arg), Value::Bool(s)) => Value::Bool(match op {
                BoolOp::And => *arg && *s,
                BoolOp::Nand => !*arg || !*s,
                BoolOp::Or => *arg || *s,
                BoolOp::Nor => !*arg && !*s,
                BoolOp::Xor => arg ^ s,
                BoolOp::Xnor => arg == s,
            }),
            // A parsed entry always matches its attribute's type.
            (_, subject) => subject.clone(),
        }
    }
}

fn blend_to_gray(s: [f32; 3], brightness: f32, factor: f32) -> [f32; 3] {
    let grey = s[0] * 0.3 + s[1] * 0.59 + s[2] * 0.11;
    // Java's `Math.clamp(v, 0, 1)`: `Math.min(1, Math.max(v, 0))`, so -0.0 becomes 0.0.
    let scaled = java_min(1.0, java_max(grey * brightness, 0.0));
    std::array::from_fn(|i| (scaled - s[i]) * factor + s[i])
}

/// Java's `Math.min(float, float)`: NaN wins, and -0.0 is below 0.0.
fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b && a == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// Java's `Math.max(float, float)`.
fn java_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b && a == 0.0 {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `EnvironmentAttributeMap`: at most one entry per attribute.
#[derive(Clone, Default)]
pub struct AttributeMap(Vec<(&'static Attribute, Entry)>);

impl std::fmt::Debug for AttributeMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(a, _)| a.id))
            .finish()
    }
}

impl AttributeMap {
    /// `EnvironmentAttributeMap.CODEC`: a JSON object from attribute id to entry.
    pub fn parse(json: &Json) -> Result<Self, String> {
        let map = json.as_object().ok_or("attributes are not an object")?;
        map.iter()
            .map(|(id, entry)| {
                let attribute = attribute(id).ok_or_else(|| format!("unknown attribute {id}"))?;
                let entry = Entry::parse(attribute, entry).map_err(|e| format!("{id}: {e}"))?;
                Ok((attribute, entry))
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// A map built in code (`EnvironmentAttributeMap.Builder`); a later entry for the same
    /// attribute replaces the earlier one.
    pub fn from_entries(entries: impl IntoIterator<Item = (&'static Attribute, Entry)>) -> Self {
        let mut map = Self::default();
        for (attribute, entry) in entries {
            map.0.retain(|(a, _)| a.id != attribute.id);
            map.0.push((attribute, entry));
        }
        map
    }

    /// Sets the attribute's entry, replacing the one it had.
    pub fn set(&mut self, attribute: &'static Attribute, entry: Entry) {
        self.0.retain(|(a, _)| a.id != attribute.id);
        self.0.push((attribute, entry));
    }

    /// Drops the attribute's entry.
    pub fn remove(&mut self, attribute: &Attribute) {
        self.0.retain(|(a, _)| a.id != attribute.id);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, attribute: &Attribute) -> Option<&Entry> {
        self.0
            .iter()
            .find(|(a, _)| a.id == attribute.id)
            .map(|(_, e)| e)
    }

    /// The attributes it has an entry for.
    pub fn attributes(&self) -> impl Iterator<Item = &'static Attribute> + '_ {
        self.0.iter().map(|(a, _)| *a)
    }

    /// `EnvironmentAttributeMap.applyModifier`: the entry applied to `base`, or `base`.
    pub fn apply(&self, attribute: &Attribute, base: &Value) -> Value {
        self.get(attribute)
            .map_or_else(|| base.clone(), |e| e.apply(base))
    }
}

/// What an `AttributeType` lerps for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lerp {
    /// Between a timeline's keyframes (`keyframeLerp`).
    Keyframe,
    /// Into rain or thunder (`stateChangeLerp`).
    StateChange,
    /// Across biomes (`spatialLerp`).
    Spatial,
}

impl Attribute {
    /// `EnvironmentAttribute.isSpatiallyInterpolated`: mixed across nearby biomes.
    pub fn spatially_interpolated(&self) -> bool {
        self.id.starts_with("visual/")
            && !matches!(
                self.id,
                "visual/moon_phase"
                    | "visual/default_dripstone_particle"
                    | "visual/ambient_particles"
            )
    }

    /// `EnvironmentAttribute.isFullResolutionBiomes`: read from the block's biome, not the
    /// noise biome.
    pub fn full_resolution_biomes(&self) -> bool {
        self.id == "gameplay/natural_mob_spawns"
    }

    /// `AttributeType`'s lerps. Floats and angles use `Mth.lerp`, colors `ARGB.srgbLerp`
    /// (`(to - from) * alpha + from` per channel); the types `ofNotInterpolated` step to `to`
    /// once alpha reaches 1 (keyframes), 0 (state changes) or 0.5 (biomes).
    pub fn lerp(&self, purpose: Lerp, alpha: f32, from: &Value, to: &Value) -> Value {
        match (from, to) {
            (Value::Float(a), Value::Float(b)) => Value::Float(a + alpha * (b - a)),
            (Value::Rgb(a), Value::Rgb(b)) => {
                Value::Rgb(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
            }
            (Value::Argb(a), Value::Argb(b)) => {
                Value::Argb(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
            }
            _ => {
                let threshold = match purpose {
                    Lerp::Keyframe => 1.0,
                    Lerp::StateChange => 0.0,
                    Lerp::Spatial => 0.5,
                };
                if alpha >= threshold {
                    to.clone()
                } else {
                    from.clone()
                }
            }
        }
    }
}
