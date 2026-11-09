// Native mods' world primitives (gfx/world_primitives.rs): quads from the shared atlas and lines
// with a width in pixels, camera-relative, at the end of the world pass.

struct Globals {
    // Camera-relative world to clip space, with TAA's jitter (what the world was drawn with).
    view_proj: mat4x4<f32>,
    // The frame's size in pixels (line widths are in its pixels).
    viewport: vec4<f32>,
}
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct QuadOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // A beam's sprite v range (start, length), else 0.
    @location(2) @interpolate(flat) wrap: vec2<f32>,
}

// `Vertex` (mesher.rs): position, atlas UV, layer, RGBA colour, light.
@vertex fn vs_quad(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: u32) -> QuadOut {
    var out: QuadOut;
    out.clip = globals.view_proj * vec4(pos, 1.0);
    out.uv = uv;
    out.color = color;
    // Layer 6 is a beacon beam's: its v repeats inside the sprite, whose range rides in the
    // light bytes (block_entity_render::beam).
    if (layer & 0xffffu) == 6u {
        out.wrap = vec2(f32(light & 0xffffu), f32(light >> 16u)) / 65535.0;
    } else {
        out.wrap = vec2(0.0);
    }
    return out;
}

@fragment fn fs_quad(in: QuadOut) -> @location(0) vec4<f32> {
    var uv = in.uv;
    if in.wrap.y > 0.0 { uv.y = in.wrap.x + fract(in.uv.y) * in.wrap.y; }
    let color = textureSample(atlas, atlas_sampler, uv) * in.color;
    if color.a <= 0.0 { discard; }
    return color;
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

// One instance per line (gizmos::camera_relative): its ends, ARGB colour and width in pixels,
// a strip of 4 vertices across it, as terrain.wgsl's `vs_debug_line`.
@vertex fn vs_line(@builtin(vertex_index) vertex: u32,
    @location(0) start: vec3<f32>, @location(1) end: vec3<f32>,
    @location(2) argb: vec4<f32>, @location(3) width: f32) -> LineOut {
    let position = select(start, end, vertex >= 2u);
    let view_shrink = 1.0 - 1.0 / 256.0;
    let line_start = globals.view_proj * vec4(position * view_shrink, 1.0);
    let line_end = globals.view_proj * vec4((position + (end - start)) * view_shrink, 1.0);
    let ndc1 = line_start.xyz / line_start.w;
    let ndc2 = line_end.xyz / line_end.w;
    let screen = globals.viewport.xy;
    let direction = normalize((ndc2.xy - ndc1.xy) * screen);
    var offset = vec2(-direction.y, direction.x) * width / screen;
    if offset.x < 0.0 { offset = -offset; }
    if vertex % 2u != 0u { offset = -offset; }
    var out: LineOut;
    out.clip = vec4((ndc1 + vec3(offset, 0.0)) * line_start.w, line_start.w);
    // ARGB in a little-endian u32 reads as B, G, R, A.
    out.color = argb.zyxw;
    return out;
}

@fragment fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}
