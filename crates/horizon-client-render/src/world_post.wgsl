// Horizon's world post pass: the world, rendered at the resolution scale,
// copied (bilinear) or anti-aliased with FXAA onto the screen before the
// hand and HUD. Only used when a non-vanilla setting asks for it.
@group(0) @binding(0) var world: texture_2d<f32>;
@group(0) @binding(1) var world_sampler: sampler;

struct PostOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_post(@builtin(vertex_index) i: u32) -> PostOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: PostOut;
    out.clip = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2(p.x, 1.0 - p.y);
    return out;
}

@fragment fn fs_copy(in: PostOut) -> @location(0) vec4<f32> {
    // The alpha carries the emissive mark through to HDR output (hdr.wgsl).
    return textureSampleLevel(world, world_sampler, in.uv, 0.0);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3(0.299, 0.587, 0.114));
}

// FXAA 3.11 (console quality): blend along the local edge direction.
@fragment fn fs_fxaa(in: PostOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(world));
    let centre = textureSampleLevel(world, world_sampler, in.uv, 0.0);
    let rgb_m = centre.rgb;
    let nw = luma(textureSampleLevel(world, world_sampler, in.uv + vec2(-0.5, -0.5) * texel, 0.0).rgb);
    let ne = luma(textureSampleLevel(world, world_sampler, in.uv + vec2(0.5, -0.5) * texel, 0.0).rgb);
    let sw = luma(textureSampleLevel(world, world_sampler, in.uv + vec2(-0.5, 0.5) * texel, 0.0).rgb);
    let se = luma(textureSampleLevel(world, world_sampler, in.uv + vec2(0.5, 0.5) * texel, 0.0).rgb);
    let m = luma(rgb_m);
    let lo = min(m, min(min(nw, ne), min(sw, se)));
    let hi = max(m, max(max(nw, ne), max(sw, se)));
    if hi - lo < max(0.0312, hi * 0.125) {
        return centre;
    }
    var dir = vec2(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.25 * 0.125, 1.0 / 128.0);
    let scale = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * scale, vec2(-8.0), vec2(8.0)) * texel;
    let a = 0.5 * (textureSampleLevel(world, world_sampler, in.uv + dir * (1.0 / 3.0 - 0.5), 0.0).rgb
        + textureSampleLevel(world, world_sampler, in.uv + dir * (2.0 / 3.0 - 0.5), 0.0).rgb);
    let b = a * 0.5 + 0.25 * (textureSampleLevel(world, world_sampler, in.uv - dir * 0.5, 0.0).rgb
        + textureSampleLevel(world, world_sampler, in.uv + dir * 0.5, 0.0).rgb);
    let lb = luma(b);
    if lb < lo || lb > hi {
        return vec4(a, centre.a);
    }
    return vec4(b, centre.a);
}

// Contrast-adaptive sharpening (after AMD's CAS) of TAA's upscaled picture: each
// pixel pushed away from its four neighbours, less where they already differ a lot,
// so edges get their crispness back without halos or lifted grain.
// The Upscaling Sharpness setting, 0..1.
override sharpness: f32 = 0.85;

@fragment fn fs_sharpen(in: PostOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(world));
    let p = clamp(vec2<i32>(in.uv * vec2<f32>(size)), vec2(0), size - 1);
    let centre = textureLoad(world, p, 0);
    let c = centre.rgb;
    let n = textureLoad(world, clamp(p + vec2(0, -1), vec2(0), size - 1), 0).rgb;
    let s = textureLoad(world, clamp(p + vec2(0, 1), vec2(0), size - 1), 0).rgb;
    let w = textureLoad(world, clamp(p + vec2(-1, 0), vec2(0), size - 1), 0).rgb;
    let e = textureLoad(world, clamp(p + vec2(1, 0), vec2(0), size - 1), 0).rgb;
    let lo = min(c, min(min(n, s), min(w, e)));
    let hi = max(c, max(max(n, s), max(w, e)));
    // How much room there is before clipping, per channel: little at strong edges.
    let room = clamp(min(lo, 2.0 - hi) / max(hi, vec3(0.0001)), vec3(0.0), vec3(1.0));
    // Up to -1/5, the most CAS allows (the Upscaling Sharpness setting's 100%).
    let lobe = -sqrt(room) * 0.2 * sharpness;
    let sharp = (c + lobe * (n + s + w + e)) / (1.0 + 4.0 * lobe);
    // The alpha carries the emissive mark through to HDR output (hdr.wgsl).
    return vec4(clamp(sharp, lo, hi), centre.a);
}

// TAA (`world_post.rs`): blend this frame into the history, reprojected from the
// depth buffer through the camera's motion and clamped to this frame's 3x3
// neighbourhood so that what moved or appeared does not ghost.
struct Taa {
    reproject: mat4x4<f32>,
    // xy: this frame's jitter in clip space; z: 1 when the history is unusable.
    jitter: vec4<f32>,
};
@group(0) @binding(2) var history: texture_2d<f32>;
// The world's depth as an unfilterable float texture: naga's GLSL backend
// cannot emit `textureLoad` from a depth texture, and a depth view bound as
// `TextureSampleType::Float { filterable: false }` gives the same texel on
// every backend.
@group(0) @binding(3) var depth: texture_2d<f32>;
@group(0) @binding(4) var<uniform> taa: Taa;

// Catmull-Rom over the history in 5 bilinear taps: sharper than one under motion.
fn sample_history(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(history));
    let pos = uv * size;
    let centre = floor(pos - 0.5) + 0.5;
    let f = pos - centre;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = (centre - 1.0) / size;
    let t3 = (centre + 2.0) / size;
    let t12 = (centre + w2 / w12) / size;
    let c = textureSampleLevel(history, world_sampler, vec2(t12.x, t0.y), 0.0).rgb * (w12.x * w0.y)
        + textureSampleLevel(history, world_sampler, vec2(t0.x, t12.y), 0.0).rgb * (w0.x * w12.y)
        + textureSampleLevel(history, world_sampler, t12, 0.0).rgb * (w12.x * w12.y)
        + textureSampleLevel(history, world_sampler, vec2(t3.x, t12.y), 0.0).rgb * (w3.x * w12.y)
        + textureSampleLevel(history, world_sampler, vec2(t12.x, t3.y), 0.0).rgb * (w12.x * w3.y);
    let weight = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    return max(c / weight, vec3(0.0));
}

fn to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3(0.25 * (c.r + 2.0 * c.g + c.b), 0.5 * (c.r - c.b), 0.25 * (2.0 * c.g - c.r - c.b));
}

fn from_ycocg(c: vec3<f32>) -> vec3<f32> {
    return max(vec3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z), vec3(0.0));
}

// Moves `c` toward the box's centre until it lies inside (clipping keeps the
// colour's hue where a per-channel clamp would shift it).
fn clip_box(c: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> vec3<f32> {
    let mid = 0.5 * (hi + lo);
    let half_size = max(0.5 * (hi - lo), vec3(0.0001));
    let d = c - mid;
    let unit = abs(d / half_size);
    let far = max(unit.x, max(unit.y, unit.z));
    if far > 1.0 {
        return mid + d / far;
    }
    return c;
}

// It runs at the history's size, which upscaling makes larger than the world's.
// The history is unjittered: each output pixel takes the world pixel whose
// jittered sample lands nearest its centre, weighted by how near it lands.
@fragment fn fs_taa(in: PostOut) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(world));
    // The jitter moved the scene by `jitter` in clip space: where this pixel's
    // point was drawn, in world pixels.
    let drawn = (in.uv + taa.jitter.xy * vec2(0.5, -0.5)) * size;
    let last = vec2<i32>(size) - 1;
    let p = clamp(vec2<i32>(floor(drawn)), vec2(0), last);
    let centre = textureLoad(world, p, 0);
    var lo = to_ycocg(centre.rgb);
    var hi = lo;
    // Mean and spread of the 3x3 (YCoCg): the history is clipped to a box around
    // the mean a few deviations wide, tighter than min..max, so that what changed
    // does not linger as a smear.
    var m1 = vec3(0.0);
    var m2 = vec3(0.0);
    // Reversed depth: the nearest of the 3x3 is the largest, and its motion keeps
    // the edges of what is in front from trailing.
    var near = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let q = clamp(p + vec2(x, y), vec2(0), last);
            let c = to_ycocg(textureLoad(world, q, 0).rgb);
            lo = min(lo, c);
            hi = max(hi, c);
            m1 += c;
            m2 += c * c;
            near = max(near, textureLoad(depth, q, 0).r);
        }
    }
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let previous = taa.reproject * vec4(ndc, near, 1.0);
    let uv = vec2(0.5, -0.5) * previous.xy / previous.w + 0.5;
    if taa.jitter.z > 0.0 || any(uv != clamp(uv, vec2(0.0), vec2(1.0))) {
        return vec4(centre.rgb, centre.a);
    }
    // How far this pixel moved since the last frame, in output pixels.
    let speed = length((uv - in.uv) * vec2<f32>(textureDimensions(history)));
    let moving = clamp(speed / 6.0, 0.0, 1.0);
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3(0.0)));
    let wide = mix(1.25, 0.75, moving) * sigma;
    let past = from_ycocg(clip_box(to_ycocg(sample_history(uv)), max(lo, mean - wide), min(hi, mean + wide)));
    // How far the sample landed from this pixel's centre, in output pixels: a
    // Gaussian of it scales the new frame's share, so that frames whose sample
    // fell far away (most of them when upscaling) barely move the pixel. Under
    // motion the history is resampled every frame and blurs, so new frames weigh more.
    let off = (drawn - (vec2<f32>(p) + 0.5)) * vec2<f32>(textureDimensions(history)) / size;
    let share = mix(0.1, 0.3, moving) * exp(-2.0 * dot(off, off));
    // The alpha carries the emissive mark through to HDR output (hdr.wgsl).
    return vec4(mix(past, centre.rgb, share), centre.a);
}
