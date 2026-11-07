//! Color functions on packed ARGB numbers (`red`, `color`, `lerpColor`, `hsb`...). Colors are
//! Java ints: `color` returns a signed one, and every function reads either sign. Channels go
//! through vanilla's `ARGB` rules: a float becomes `floor(v * 255)`, colors blend per channel by
//! `Mth.lerpInt`.

fn argb(v: f64) -> u32 {
    v as i64 as u32
}

fn pack(c: u32) -> f64 {
    f64::from(c as i32)
}

/// Vanilla `ARGB.as8BitChannel`, with the value first clamped to 0-1.
fn channel(v: f64) -> u32 {
    ((v.clamp(0.0, 1.0) as f32) * 255.0).floor() as u32
}

fn component(c: f64, shift: u32) -> f64 {
    f64::from(argb(c) >> shift & 255) / 255.0
}

pub(super) fn red(a: &[f64]) -> f64 {
    component(a[0], 16)
}
pub(super) fn green(a: &[f64]) -> f64 {
    component(a[0], 8)
}
pub(super) fn blue(a: &[f64]) -> f64 {
    component(a[0], 0)
}
pub(super) fn alpha(a: &[f64]) -> f64 {
    component(a[0], 24)
}

fn from_channels(a: f64, r: f64, g: f64, b: f64) -> f64 {
    pack(channel(a) << 24 | channel(r) << 16 | channel(g) << 8 | channel(b))
}

/// `color(r, g, b, ?a)`: alpha defaults to 1.
pub(super) fn color(a: &[f64]) -> f64 {
    from_channels(a.get(3).copied().unwrap_or(1.0), a[0], a[1], a[2])
}

pub(super) fn with_alpha(a: &[f64]) -> f64 {
    pack(argb(a[0]) & 0x00ff_ffff | channel(a[1]) << 24)
}

/// Vanilla `ARGB.srgbLerp`: each channel `p0 + floor(t * (p1 - p0))`, in floats.
pub(super) fn lerp_color(a: &[f64]) -> f64 {
    let (from, to, t) = (argb(a[0]), argb(a[1]), a[2] as f32);
    let mix = |shift: u32| {
        let (p0, p1) = ((from >> shift & 255) as i32, (to >> shift & 255) as i32);
        ((p0 + (t * (p1 - p0) as f32).floor() as i32).clamp(0, 255) as u32) << shift
    };
    pack(mix(24) | mix(16) | mix(8) | mix(0))
}

/// Hue, saturation and brightness (all 0-1) of a color.
fn to_hsb(c: f64) -> (f64, f64, f64) {
    let (r, g, b) = (component(c, 16), component(c, 8), component(c, 0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let saturation = if max > 0.0 { delta / max } else { 0.0 };
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (hue, saturation, max)
}

/// An opaque color from hue (wrapping), saturation and brightness.
fn from_hsb(h: f64, s: f64, v: f64, alpha: u32) -> f64 {
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let h = h.rem_euclid(1.0) * 6.0;
    let f = h - h.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match h.floor() as u32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |x: f64| (x * 255.0).round() as u32;
    pack(alpha << 24 | byte(r) << 16 | byte(g) << 8 | byte(b))
}

pub(super) fn hsb(a: &[f64]) -> f64 {
    from_hsb(a[0], a[1], a[2], 255)
}
pub(super) fn hue(a: &[f64]) -> f64 {
    to_hsb(a[0]).0
}
pub(super) fn saturation(a: &[f64]) -> f64 {
    to_hsb(a[0]).1
}
pub(super) fn brightness(a: &[f64]) -> f64 {
    to_hsb(a[0]).2
}

/// Blends hue, saturation and brightness, the hue the short way around the wheel; alpha blends
/// like [`lerp_color`].
pub(super) fn lerp_hsb(a: &[f64]) -> f64 {
    let ((h0, s0, v0), (h1, s1, v1), t) = (to_hsb(a[0]), to_hsb(a[1]), a[2]);
    let dh = (h1 - h0 + 0.5).rem_euclid(1.0) - 0.5;
    let alpha = argb(lerp_color(a)) >> 24;
    from_hsb(h0 + dh * t, s0 + (s1 - s0) * t, v0 + (v1 - v0) * t, alpha)
}

pub(super) fn dist_square(a: &[f64]) -> f64 {
    let (dx, dy) = (a[2] - a[0], a[3] - a[1]);
    dx * dx + dy * dy
}
pub(super) fn dist(a: &[f64]) -> f64 {
    dist_square(a).sqrt()
}
