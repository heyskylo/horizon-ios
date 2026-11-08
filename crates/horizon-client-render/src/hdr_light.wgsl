override HDR_WRITE_LIGHT: bool = false;

// The light of colour `f` under the world's mark `a` (the alpha the world left, hdr_mark) where
// the world `shows`, else the colour clipped at white; and whether a light source lifted it.
// `eased` for the last pass, which eases it into the peak itself; the layered frame leaves that
// to the tone curve (fs_tonemap).
struct Lit {
    light: vec3<f32>,
    lifted: bool,
};

fn hdr_mark_light(f: vec3<f32>, a: f32, shows: bool, eased: bool, settings: vec4<f32>, shares: vec4<f32>) -> Lit {
    var lifted = false;
    // Light over 1 (a float frame's) shows only where the world does and carries
    // no mark (alpha 1), up to the peak: light written as light. Marked pixels clip
    // at white before their lift, as on a unorm frame, so light sources, the sun and
    // HDR lighting look as they do there; so do the HUD, menus and HDR Lighting off.
    let c = select(min(f, vec3(1.0)), f, shows && a == 1.0);
    // Gamma 2.2, as SDR displays and the compositors' SDR paths decode it.
    var light = pow(c, vec3(2.2));
    // HDR lighting: alpha between 0.76 and 1 is how much of it the surface gets. Vanilla's
    // light is albedo times lightmap, so this is the lightmap going past 1. It eases into the
    // peak from 60% of it, so bright textures keep their detail. Unmarked light (alpha 1)
    // gets no gain and no knee: the clamp to the peak below is all it needs here.
    if shows && a >= 0.75 && a < 1.0 {
        let top = max(light.r, max(light.g, light.b));
        let gain = 1.0 + (settings.y - 1.0) * (1.0 - a) / 0.24;
        let v = top * gain;
        let knee = max(0.6 * settings.x, 1.0);
        var out = v;
        if eased && v > knee && settings.x > knee {
            out = knee + (settings.x - knee) * (1.0 - exp(-(v - knee) / (settings.x - knee)));
        }
        light *= select(gain, out / max(top, 1e-4), eased && v > knee);
    }
    // The world's alpha is below 0.75 on light sources, the sun and the moon
    // (terrain.wgsl, entity.wgsl): its band of 0.175 is the tier, the lower the
    // brighter, and where in the band how much (hdr_mark).
    if shows && a < 0.75 {
        let band = u32(clamp(floor((a + 0.005) / 0.175), 0.0, 3.0));
        let tier = 3u - band;
        let lift = 1.0 - clamp((a - 0.175 * f32(band)) / 0.165, 0.0, 1.0);
        if lift > 0.0 {
            let top = max(light.r, max(light.g, light.b));
            var gain = 1.0 + (settings.x - 1.0) * shares[tier] * lift;
            if eased {
                gain = min(gain, settings.x / max(top, 1e-4));
            }
            light *= max(gain, 1.0);
            lifted = true;
        }
    }
    // HORIZON_HDR_DEBUG=1: what isn't lifted turns grey, what is keeps its
    // colour with a magenta tint, so a lift shows on any colour.
    if settings.w > 0.5 && settings.z > 0.5 {
        if lifted {
            light *= vec3(1.0, 0.6, 1.0);
        } else {
            light = vec3(dot(light, vec3(0.2126, 0.7152, 0.0722)));
        }
    }
    if eased {
        light = min(light, vec3(settings.x));
    }
    return Lit(light, lifted);
}

// Marks are decoded at the precision of the original 8-bit frame, even in a float target.
fn hdr_quantized_mark(mark: f32) -> f32 {
    return round(saturate(mark) * 255.0) / 255.0;
}

fn hdr_written(color: vec3<f32>, mark: f32, settings: vec4<f32>, shares: vec4<f32>) -> vec3<f32> {
    let shows = settings.z > 0.5 && settings.x > 1.0;
    return pow(hdr_mark_light(max(color, vec3(0.0)), hdr_quantized_mark(mark), shows, false, settings, shares).light, vec3(1.0 / 2.2));
}
