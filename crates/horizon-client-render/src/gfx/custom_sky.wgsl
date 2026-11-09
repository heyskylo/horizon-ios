// OptiFine custom sky layers (gfx/custom_sky.rs): one skybox per draw, its six faces cut from a
// 3x2 texture, mixed with the sky under it by the pipeline's blend. The fragment's output is
// chosen so that blend gives the layer's documented mode at its brightness, and a layer at
// brightness 0 leaves the sky as it was.
struct Layer {
    // Projection * view (camera-relative) * the layer's rotation.
    clip_from_sky: mat4x4<f32>,
    brightness: f32,
    // horizon_optifine::sky::Blend, in gfx/custom_sky.rs's MODES order.
    mode: u32,
    pad: vec2<u32>,
    hdr_settings: vec4<f32>,
    hdr_shares: vec4<f32>,
    hdr_light: vec4<f32>,
}
@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var sky_texture: texture_2d<f32>;
@group(0) @binding(2) var sky_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // The face's cell (u0, v0, u1, v1): sampling stays half a texel inside it.
    @location(1) @interpolate(flat) cell: vec4<f32>,
}

@vertex fn vs_custom_sky(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cell: vec4<f32>,
) -> Out {
    var out: Out;
    out.clip = layer.clip_from_sky * vec4(position * 100.0, 1.0);
    out.uv = uv;
    out.cell = cell;
    return out;
}

const ADD: u32 = 0u;
const SUBTRACT: u32 = 1u;
const MULTIPLY: u32 = 2u;
const DODGE: u32 = 3u;
const BURN: u32 = 4u;
const SCREEN: u32 = 5u;
const REPLACE: u32 = 6u;
const OVERLAY: u32 = 7u;
const ALPHA: u32 = 8u;

fn custom_sky_color(in: Out) -> vec4<f32> {
    let half = 0.5 / vec2<f32>(textureDimensions(sky_texture));
    let uv = clamp(in.uv, in.cell.xy + half, in.cell.zw - half);
    let t = textureSample(sky_texture, sky_sampler, uv);
    let k = t.a * layer.brightness;
    var color = t.rgb;
    if HDR_WRITE_LIGHT && layer.hdr_light.w != 0.0 &&
        (layer.mode == ADD || layer.mode == SUBTRACT || layer.mode == REPLACE || layer.mode == ALPHA) {
        color = hdr_written(color, 1.0 - 0.24 * layer.hdr_light.y, layer.hdr_settings, layer.hdr_shares);
    }
    // On a frame of linear light (`FRAME_LINEAR`) the layer's colour is decoded as it is stored.
    switch layer.mode {
        // dst * src: white leaves the sky alone. Decoded, the product stays exact:
        // (dst * src)^2.2 = dst^2.2 * src^2.2.
        case MULTIPLY: { return frame_stored(vec4(mix(vec3(1.0), t.rgb, k), 1.0)); }
        // 2 * dst * src: grey leaves it alone. Decoded as entity.wgsl's crumbling, (2 src)^2.2 / 2,
        // so it stays exact too.
        case OVERLAY: {
            let factor = mix(vec3(0.5), t.rgb, k);
            if FRAME_LINEAR { return vec4(pow(2.0 * factor, vec3(2.2)) * 0.5, 1.0); }
            return vec4(factor, 1.0);
        }
        case REPLACE: { return frame_stored(vec4(color, 1.0)); }
        // mix(dst, src, alpha): alpha is coverage, kept.
        case ALPHA: { return frame_stored(vec4(color, k)); }
        // add, subtract, dodge, burn, screen: the layer premultiplied, black leaves it alone. On
        // linear light add and subtract add and take away light, as `"linear"` means to; dodge,
        // burn and screen come out near their gamma results, not exactly.
        default: { return vec4(frame_stored(vec4(color, 1.0)).rgb * k, 1.0); }
    }
}

@fragment fn fs_custom_sky(in: Out) -> @location(0) vec4<f32> {
    return custom_sky_color(in);
}

@fragment fn fs_custom_sky_screen_light(in: Out) -> @location(0) vec4<f32> {
    let color = custom_sky_color(in);
    return vec4(hdr_written(color.rgb, 1.0 - 0.24 * layer.hdr_light.y, layer.hdr_settings, layer.hdr_shares), color.a);
}
