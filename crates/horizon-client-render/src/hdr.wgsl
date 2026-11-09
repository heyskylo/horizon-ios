// Horizon's HDR output pass: the frame, drawn as without HDR in vanilla's gamma
// space, decoded to linear light at the paper white and written to the swapchain:
// HDR10 (BT.2020, PQ) or scRGB (1.0 = 80 nits). Bright pixels of light sources that still show the
// world as it was before the hand and HUD are expanded towards the peak.
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var world: texture_2d<f32>;

struct Params {
    // Paper white in scRGB units (nits / 80).
    paper: f32,
    // Peak brightness relative to paper white; 1 expands nothing.
    peak: f32,
    // 1 when this frame drew a world.
    world: f32,
    // 1 greys what isn't expanded and tints what is (HORIZON_HDR_DEBUG=1).
    debug: f32,
    // 1 for an HDR10 swapchain, 0 for scRGB.
    pq: f32,
    // The display's black level in nits; 0 leaves the picture as it is.
    black: f32,
    // The HUD and menus' brightness as a share of paper white.
    ui: f32,
    // The compatibility frame's lift (`lit`; the float frame's writers have theirs from
    // Hdr::draw_params). HDR lighting: what the strongest light multiplies a surface by, 1 for
    // none.
    scene: f32,
    // Per tier, low to very bright (hdr.rs `COMPAT_SHARES`): the share of the headroom
    // its fully lifted pixels get.
    share: vec4<f32>,
    // HDR calibration (hdr_calibration.rs): x its step (0 none, 1 black level,
    // 2 peak, 3 paper white), y the slider in nits, z where its controls start,
    // a share of the height.
    calibration: vec4<f32>,
    // The scene's peak in units of paper white where Horizon lifts the world (hdr.rs
    // `SCENE_PEAK`), else 0: what the tone curve maps onto the display's peak (`tone_scale`).
    scene_peak: f32,
};
@group(0) @binding(2) var<uniform> params: Params;
// Horizon's logo, 1 inside, 0 outside, for calibration's test pictures.
@group(0) @binding(3) var logo: texture_2d<f32>;
// The float frame with HUD coverage (Hdr::layered): the frame keeps the world as light
// directly from its writers; the HUD draws on a picture of its own (`hud`, fs_keep), and `coverage` says how
// much of the world it hides there (gfx/hud_coverage.rs).
@group(0) @binding(4) var coverage: texture_2d<f32>;
@group(0) @binding(5) var hud: texture_2d<f32>;

// Light sources are lifted evenly rather than by a highlight curve: vanilla's
// textures were drawn for SDR, so a curve on their brightest pixels blows out
// near-white spots (glowstone) and leaves the rest (lava, a jack o'lantern's
// face) nearly as it was. Each tier's pixels brighter than its `full` get its
// `share` of the headroom, dimmer ones less down to none at its `start`
// (lava's crust, a lantern's metal), and no channel goes past the peak. That
// brightness is the texel's own, before lighting: the shaders that mark the
// frame (hdr_mark) work it out, so a light source shines the same at any light
// level and on every face. Brightness is the mean of luminance and the
// brightest channel, so saturated light counts as bright.

// Whether `c` still shows the world pixel `w`: the same, or darkened evenly
// across its channels, as the vignette and other screen-wide tints darken it.
// The hand, HUD and text replace pixels instead, so they never pass. The
// channels are held to one factor, the ratio of their sums, so a channel at 0
// stays 0; a step of 8-bit rounding is allowed on each.
fn shows_world(c: vec3<f32>, w: vec3<f32>) -> bool {
    if all(c == w) {
        return true;
    }
    let f = (c.r + c.g + c.b) / max(w.r + w.g + w.b, 1.0 / 255.0);
    return f <= 1.01 && f > 0.02 && all(abs(c - f * w) <= 0.03 * w + 1.0 / 255.0);
}

// A texel with its alpha as an 8-bit frame holds it. A 16-bit frame
// (Renderer::frame_format) keeps alpha between 8-bit steps, where the marks below
// would read differently (an alpha of 0.001 would count as the hand); its colour
// keeps the gradients 8 bits would band, and a float frame's (LINEAR_FRAME) the
// light over 1 that blends added past white. On an 8-bit frame nothing changes.
fn frame_texel(v: vec4<f32>) -> vec4<f32> {
    return vec4(max(v.rgb, vec3(0.0)), round(saturate(v.a) * 255.0) / 255.0);
}

// Linear BT.709 to BT.2020 (ITU-R BT.2087), columns first.
const BT709_TO_BT2020 = mat3x3<f32>(
    vec3(0.6274, 0.0691, 0.0164),
    vec3(0.3293, 0.9195, 0.0880),
    vec3(0.0433, 0.0114, 0.8956),
);

// SMPTE ST 2084: nits (up to 10000) to the PQ signal.
fn pq(nits: vec3<f32>) -> vec3<f32> {
    let y = pow(clamp(nits / 10000.0, vec3(0.0), vec3(1.0)), vec3(0.1593017578125));
    return pow((0.8359375 + 18.8515625 * y) / (1.0 + 18.6875 * y), vec3(78.84375));
}

// ST 2084's inverse: the PQ signal to nits.
fn pq_nits(e: vec3<f32>) -> vec3<f32> {
    let p = pow(clamp(e, vec3(0.0), vec3(1.0)), vec3(1.0 / 78.84375));
    let y = max(p - 0.8359375, vec3(0.0)) / (18.8515625 - 18.6875 * p);
    return 10000.0 * pow(y, vec3(1.0 / 0.1593017578125));
}

// ITU-R BT.2390's black level lift (its EETF without the knee) on the PQ signal
// `e`: black moves up to the display's black level, the peak stays, and the lift
// fades out by the fourth power on the way up, so only the shadows move. A display
// that can't go darker than its black level would crush everything below it.
fn lift_black(e: vec3<f32>) -> vec3<f32> {
    let top = pq(vec3(params.peak * params.paper * 80.0)).x;
    let n = min(e / top, vec3(1.0));
    return e + pq(vec3(params.black)).x * pow(vec3(1.0) - n, vec3(4.0));
}

// Whether `p` falls on the logo drawn `size` pixels wide around `centre`.
fn on_logo(p: vec2<f32>, centre: vec2<f32>, size: f32) -> bool {
    let uv = (p - centre) / size + 0.5;
    if any(uv < vec2(0.0)) || any(uv >= vec2(1.0)) {
        return false;
    }
    let texels = vec2<f32>(textureDimensions(logo));
    return textureLoad(logo, vec2<i32>(uv * texels), 0).r > 0.5;
}

// A calibration step's test picture at `p`, in nits, the way display calibration
// tools draw them: nothing in it is bright over a large area, so the display
// doesn't dim itself while it is judged.
fn calibration_nits(p: vec2<f32>, size: vec2<f32>) -> f32 {
    let top = size.y * params.calibration.z;
    let centre = vec2(size.x, top) / 2.0;
    let v = params.calibration.y;
    switch u32(params.calibration.x) {
        // Black level: the logo at the slider on 0. Lowered until it disappears,
        // it sits on the darkest the display shows.
        case 1u: {
            return select(0.0, v, on_logo(p, centre, top * 0.4));
        }
        // Peak: a tenth of the screen at 10 000 nits, which the display clips at
        // its brightest, the logo inside at the slider. Raised until it
        // disappears, it sits on that brightest.
        case 2u: {
            let side = sqrt(0.1 * size.x * size.y);
            if all(abs(p - centre) < vec2(side / 2.0)) {
                return select(10000.0, v, on_logo(p, centre, side * 0.7));
            }
            return 0.0;
        }
        // Paper white: a white card at the slider, on a dim grey, to match a
        // sheet of paper held beside the screen.
        default: {
            let card = vec2(top * 0.9, top * 0.6);
            return select(3.0, v, all(abs(p - centre) < card / 2.0));
        }
    }
}

@vertex fn vs_hdr(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4(p * 2.0 - 1.0, 0.0, 1.0);
}

// Hdr::keep_world: once the world is kept, the frame's alpha starts at 0 again.
@fragment fn fs_clear_alpha() -> @location(0) vec4<f32> {
    return vec4(0.0);
}

fn lit(f: vec3<f32>, a: f32, shows: bool, eased: bool) -> Lit {
    return hdr_mark_light(f, a, shows, eased, vec4(params.peak, params.scene, params.world, params.debug), (params.peak - 1.0) * params.share);
}

// Calibration: above its controls the test picture, straight in nits, past
// paper white, the lift and the black level.
fn calibrating(pos: vec4<f32>) -> bool {
    let size = vec2<f32>(textureDimensions(frame));
    return params.calibration.x > 0.5 && pos.y < size.y * params.calibration.z;
}

fn calibration_picture(pos: vec4<f32>) -> vec4<f32> {
    let nits = vec3(calibration_nits(pos.xy, vec2<f32>(textureDimensions(frame))));
    if params.pq > 0.5 {
        return vec4(pq(nits), 1.0);
    }
    return vec4(nits / 80.0, 1.0);
}

// Light in units of paper white, as the swapchain takes it: HDR10 or scRGB, the black level
// lifted.
fn display(light: vec3<f32>) -> vec4<f32> {
    let scrgb = light * params.paper;
    if params.pq > 0.5 {
        let e = pq(BT709_TO_BT2020 * scrgb * 80.0);
        return vec4(select(e, lift_black(e), params.black > 0.0), 1.0);
    }
    if params.black > 0.0 {
        return vec4(pq_nits(lift_black(pq(scrgb * 80.0))) / 80.0, 1.0);
    }
    return vec4(scrgb, 1.0);
}

@fragment fn fs_hdr(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    if calibrating(pos) {
        return calibration_picture(pos);
    }
    let at = vec2<i32>(pos.xy);
    let f = frame_texel(textureLoad(frame, at, 0));
    // What covers the world here (Hdr::keep_world): nothing at 0, the hand at
    // 1/255 (Hdr::mark_hand: drawn at the world's brightness), else the HUD or a
    // menu, by how much.
    let covered = f.a > 0.0;
    let hand = covered && f.a < 1.5 / 255.0;
    let ui = select(f.a, 0.0, hand);
    let w = frame_texel(textureLoad(world, at, 0));
    // The world's marks lift only where nothing was drawn over them since: grey text over
    // the white sun still looks like the sun darkened, but it raised the frame's alpha. The
    // hand writes its own alpha there (Hdr::hand_light_view): 1, or a light source it holds.
    let shows = params.world > 0.5 && params.peak > 1.0
        && (hand || (!covered && shows_world(f.rgb, w.rgb)));
    var light = lit(f.rgb, w.a, shows, true).light;
    // The HUD and menus at their own brightness, by how much they cover.
    light *= mix(1.0, params.ui, ui);
    return display(light);
}

// Where the scene's peak is unknown (`tone_scale`), the curve's knee: up to it light stays as it
// is. Where HDR lighting eased into the peak before the curve (`lit`), 60% of it, so bright
// textures keep their detail; at least paper white.
fn tone_knee(headroom: f32) -> f32 {
    return max(0.6 * headroom, 1.0);
}

// The tone curve on light whose brightest channel is `top`, in units of paper white `paper` nits:
// what to scale the light by, so it ends at most `headroom` (T). ITU-R BT.2390's EETF in PQ,
// without the black level lift (`display` does that): the same up to its knee, then a Hermite
// spline reaching T at the source's peak, past which light shows at T. The brightest channel sets
// the scale for all three, so light keeps its hue (lava stays orange) and no channel passes T.
//
// The source is the scene's peak S where Horizon lifts the world (`scene_peak`, hdr.rs
// `SCENE_PEAK`): the world's light is the same on every display, light past S is held at S, and
// where T reaches S that is all, so a display above S shows the world as one at S. Where the
// scene's range is unknown (`scene_peak` 0: a 0.2 pack's own light, HDR Lighting off), the
// source follows the display, as the 0.2 contract has it: the peak that puts the EETF's knee at
// `tone_knee`, PQ's 10 000 nits at most.
fn tone_scale(top: f32, scene_peak: f32, headroom: f32, paper: f32) -> f32 {
    let peak = pq(vec3(headroom * paper)).x;
    var source: f32;
    var e: f32;
    var held = 1.0;
    if scene_peak > 0.0 {
        held = min(1.0, scene_peak / max(top, 1e-6));
        if scene_peak <= headroom {
            return held;
        }
        source = pq(vec3(scene_peak * paper)).x;
        e = pq(vec3(min(top, scene_peak) * paper)).x / source;
    } else {
        if top <= tone_knee(headroom) {
            return 1.0;
        }
        source = min(3.0 * peak - 2.0 * pq(vec3(tone_knee(headroom) * paper)).x, 1.0);
        e = pq(vec3(top * paper)).x / source;
    }
    let max_lum = peak / source;
    // At least 0: past 3 to 1 in PQ the knee would leave the source's range.
    let ks = max(1.5 * max_lum - 0.5, 0.0);
    if e >= 1.0 {
        return headroom / top;
    }
    if e <= ks {
        return held;
    }
    let t = (e - ks) / (1.0 - ks);
    let t2 = t * t;
    let t3 = t2 * t;
    let p = (2.0 * t3 - 3.0 * t2 + 1.0) * ks + (t3 - 2.0 * t2 + t) * (1.0 - ks) + (3.0 * t2 - 2.0 * t3) * max_lum;
    return pq_nits(vec3(p * source)).x / paper / top;
}

// Hdr::tonemap, after the post effects and before the camera overlays (docs/design/
// hdr-linear-light.md, `[tonemap]`): the light (`world` holds the frame) fitted to the display, in
// gamma, where scaling the light by s scales the stored colour by s^(1/2.2). Light the curve
// leaves alone stays exactly as stored. With no headroom and no scene peak, a clip at white.
@fragment fn fs_tonemap(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let g = textureLoad(world, vec2<i32>(pos.xy), 0).rgb;
    if params.scene_peak == 0.0 && params.peak <= 1.0 {
        return vec4(min(g, vec3(1.0)), 1.0);
    }
    let top = pow(max(max(g.r, max(g.g, g.b)), 0.0), 2.2);
    let s = tone_scale(top, params.scene_peak, params.peak, params.paper * 80.0);
    return vec4(select(g * pow(s, 1.0 / 2.2), g, s == 1.0), 1.0);
}

// Hdr::keep_world: the HUD's backdrop, the world clipped to white with alpha 0, the frame as
// the HUD found it before. The HUD's target is 8-bit, as that frame was.
@fragment fn fs_keep(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let w = textureLoad(frame, vec2<i32>(pos.xy), 0).rgb;
    return vec4(clamp(w, vec3(0.0), vec3(1.0)), 0.0);
}

// The backdrop `fs_keep` left in the HUD's 8-bit target for world `w`: where no HUD draws, the
// target holds exactly this, so the HUD's own part below is exactly 0 there.
fn backdrop(w: vec3<f32>) -> vec3<f32> {
    return round(clamp(w, vec3(0.0), vec3(1.0)) * 255.0) / 255.0;
}

// The HUD over the world in gamma, the space the HUD blends in: its own part, what its backdrop
// B doesn't explain (H = P - (1 - c) B, exact for the layering blends), at UI Brightness, over
// (1 - c) of the world, light above white included. Where the world is at or below white, at
// 100%, that is the HUD's picture as drawn.
fn layered(at: vec2<i32>) -> vec3<f32> {
    let w = max(textureLoad(frame, at, 0).rgb, vec3(0.0));
    let p = textureLoad(hud, at, 0).rgb;
    let c = textureLoad(coverage, at, 0).r;
    let h = max(p - (1.0 - c) * backdrop(w), vec3(0.0));
    return h * pow(params.ui, 1.0 / 2.2) + (1.0 - c) * w;
}

// Hdr::composite: the picture so far, stored as the frame stores light, for the menu blur.
@fragment fn fs_composite(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(layered(vec2<i32>(pos.xy)), 1.0);
}

// Screenshots keep midtones at their drawn brightness, with a shoulder starting at 0.6
// paper white in linear light. Unlike the display curve (which ends at the display's peak),
// this has an unbounded source range: k + (1-k)*(top-k)/(top+1-2*k) approaches SDR
// white smoothly, with slope 1 at k. One scale keeps the world's linear RGB ratios.
fn screenshot_world(w: vec3<f32>) -> vec3<f32> {
    let top = pow(max(w.r, max(w.g, w.b)), 2.2);
    if top <= 0.6 {
        return w;
    }
    let mapped = 0.6 + 0.4 * (top - 0.6) / (top - 0.2);
    return w * pow(mapped / top, 1.0 / 2.2);
}

override screenshot_world_only: bool = false;
// Off where Horizon lifts nothing (HDR Lighting off, or a shader pack's picture): the world is
// clipped at white as SDR draws it.
override screenshot_shoulder: bool = true;

// The ordinary PNG, or the world's icon before the HUD: H as drawn, at 100%, over
// the SDR world's light. This never changes the targets used by HDR output or its copy.
@fragment fn fs_sdr_screenshot(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(pos.xy);
    let w = max(textureLoad(frame, at, 0).rgb, vec3(0.0));
    let mapped = select(min(w, vec3(1.0)), screenshot_world(w), screenshot_shoulder);
    if screenshot_world_only {
        return vec4(mapped, 1.0);
    }
    let p = textureLoad(hud, at, 0).rgb;
    let c = textureLoad(coverage, at, 0).r;
    let h = max(p - (1.0 - c) * backdrop(w), vec3(0.0));
    return vec4(h + (1.0 - c) * mapped, 1.0);
}

@fragment fn fs_hdr_layered(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    if calibrating(pos) {
        return calibration_picture(pos);
    }
    return display(min(pow(layered(vec2<i32>(pos.xy)), vec3(2.2)), vec3(params.peak)));
}
