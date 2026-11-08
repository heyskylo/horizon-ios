// Terrain, sky and outline shaders, ported from the pinned vanilla
// terrain.vsh/fsh, fog.glsl and sample_lightmap.glsl.
struct Globals {
    view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    // environmental start/end, render-distance start/end
    fog: vec4<f32>,
    sky_color: vec4<f32>,
    // x: sky darkening, y: game time, z: aspect, w: tan(fov/2)
    misc: vec4<f32>,
    inverse_view: mat4x4<f32>,
    sunrise: vec4<f32>,
    // xyz: sun direction, w: star brightness
    sun: vec4<f32>,
    // x: sky fog end, y: cloud fog end, z: dark disc flag
    fog_ends: vec4<f32>,
    // Atlas rects (u0, v0, width, height) of end_sky.png and end_portal.png.
    end_portal_sprites: array<vec4<f32>, 2>,
    // x: vanilla's GameTime ((game time % 24000 + partial tick) / 24000), z: how many
    // dynamic_lights are set
    time: vec4<f32>,
    // Atlas rect of the current destroy stage (read by entity.wgsl).
    crumbling_sprite: vec4<f32>,
    outline_params: vec4<f32>,
    // Horizon Extras' dynamic lights: camera-relative position and level (dynamic_light.rs).
    dynamic_lights: array<vec4<f32>, 16>,
    colored_lights: array<vec4<f32>, 32>,
    colored_count: vec4<u32>,
    emissive_layers: array<vec4<u32>, 128>,
    emissive_count: vec4<u32>,
    // The sunrise fan's 16 triangles (gfx/sunrise_fan.rs): xyz the apex weight's normal, w the
    // triangle's volume with the camera; then each rim vertex's normal against the apex.
    sunrise_fan: array<vec4<f32>, 32>,
    // x: Options.textureFiltering (0 none, 1 RGSS, 2 anisotropic), y: the sprite arrays' width
    // in texels (vanilla's TextureSize), z: the mip level at which a sprite is one texel when
    // the views reach it, else 0 (`rotated_grid`).
    filtering: vec4<f32>,
    // HDR output's tier windows (hdr.rs `Hdr::windows`): x..w low to very bright,
    // [0] where the lift starts, [1] where it is full.
    hdr_windows: array<vec4<f32>, 2>,
    // HDR lighting: x how strongly the sky lights the world now (`Sky::sky_light_factor`),
    // y what the sky and fog get: the same, but none under water; z 1 with HDR output on.
    // w: apply the no-pack HDR gain; HDR_WRITE_LIGHT independently keeps float alpha as coverage.
    hdr_light: vec4<f32>,
    hdr_settings: vec4<f32>,
    hdr_shares: vec4<f32>,
}
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var sprites: texture_2d_array<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;
@group(0) @binding(3) var lightmap: texture_2d<f32>;
@group(0) @binding(4) var lightmap_sampler: sampler;
// Layers past the first array's SPRITE_SPLIT (2048, gfx.rs): mods' sprites.
@group(0) @binding(5) var sprites_1: texture_2d_array<f32>;
@group(0) @binding(6) var sprites_2: texture_2d_array<f32>;
@group(0) @binding(7) var sprites_3: texture_2d_array<f32>;
@group(0) @binding(8) var sprites_4: texture_2d_array<f32>;
@group(0) @binding(9) var sprites_5: texture_2d_array<f32>;
@group(0) @binding(10) var sprites_6: texture_2d_array<f32>;
@group(0) @binding(11) var sprites_7: texture_2d_array<f32>;
// Per block sprite (x the layer in its array, y the array), the brightness its
// pixels must reach to shine in HDR output: r where the bright parts shine, g
// where all of it does (gfx/hdr_stats.rs, measured from the sprite itself).
@group(0) @binding(12) var emission_thresholds: texture_2d<f32>;
// How many sprite arrays hold layers (a pipeline constant): 1 for vanilla,
// which then samples exactly as before.
override SPRITE_ARRAYS: u32 = 1u;
override SPRITE_SPLIT: u32 = 2048u;
// Whether the solid shaders carry HDR output's lighting (a pipeline constant): the
// renderer builds its solid terrain without it for the frames HDR is off, where
// `hdr_light.z` is 0, since even a branch never taken costs Apple GPUs ~15% of a
// solid pixel's time (gfx.rs's `SolidPipeline`).
override HDR_LIGHTING: bool = true;
// A sprite sampled with the given gradients, from whichever array holds its layer.
fn sprite_grad(uv: vec2<f32>, layer: u32, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let l = i32(layer % SPRITE_SPLIT);
    if SPRITE_ARRAYS == 1u {
        return textureSampleGrad(sprites, sprite_sampler, uv, l, dx, dy);
    }
    switch layer / SPRITE_SPLIT {
        case 0u: { return textureSampleGrad(sprites, sprite_sampler, uv, l, dx, dy); }
        case 1u: { return textureSampleGrad(sprites_1, sprite_sampler, uv, l, dx, dy); }
        case 2u: { return textureSampleGrad(sprites_2, sprite_sampler, uv, l, dx, dy); }
        case 3u: { return textureSampleGrad(sprites_3, sprite_sampler, uv, l, dx, dy); }
        case 4u: { return textureSampleGrad(sprites_4, sprite_sampler, uv, l, dx, dy); }
        case 5u: { return textureSampleGrad(sprites_5, sprite_sampler, uv, l, dx, dy); }
        case 6u: { return textureSampleGrad(sprites_6, sprite_sampler, uv, l, dx, dy); }
        default: { return textureSampleGrad(sprites_7, sprite_sampler, uv, l, dx, dy); }
    }
}
// The same at an explicit mip level.
fn sprite_level(uv: vec2<f32>, layer: u32, lod: f32) -> vec4<f32> {
    let l = i32(layer % SPRITE_SPLIT);
    if SPRITE_ARRAYS == 1u {
        return textureSampleLevel(sprites, sprite_sampler, uv, l, lod);
    }
    switch layer / SPRITE_SPLIT {
        case 0u: { return textureSampleLevel(sprites, sprite_sampler, uv, l, lod); }
        case 1u: { return textureSampleLevel(sprites_1, sprite_sampler, uv, l, lod); }
        case 2u: { return textureSampleLevel(sprites_2, sprite_sampler, uv, l, lod); }
        case 3u: { return textureSampleLevel(sprites_3, sprite_sampler, uv, l, lod); }
        case 4u: { return textureSampleLevel(sprites_4, sprite_sampler, uv, l, lod); }
        case 5u: { return textureSampleLevel(sprites_5, sprite_sampler, uv, l, lod); }
        case 6u: { return textureSampleLevel(sprites_6, sprite_sampler, uv, l, lod); }
        default: { return textureSampleLevel(sprites_7, sprite_sampler, uv, l, lod); }
    }
}
// texture_sampling.glsl's sampleNearest: the sprite's texels stay square pixels, blending only
// across the one screen pixel where two meet, so a linear (and anisotropic) sampler keeps them crisp.
// `tiled`: a distant tile's face takes its sprite again per block, so its gradients span many
// sprites while `uv` stays inside one (see `tile_uv` below).
fn sample_nearest(uv: vec2<f32>, layer: u32, pixel: vec2<f32>, du: vec2<f32>, dv: vec2<f32>,
                  texel_screen: vec2<f32>, tiled: bool) -> vec4<f32> {
    let texel_coords = uv / pixel;
    let center = round(texel_coords) - 0.5;
    var offset = texel_coords - center;
    offset = (offset - 0.5) * pixel / max(texel_screen, vec2(1e-12)) + 0.5;
    offset = clamp(offset, vec2(0.0), vec2(1.0));
    let at = (center + offset) * pixel;
    if tiled {
        // An anisotropic sampler lays its taps along those gradients, past the sprite's edge,
        // where the clamped sampler reads that edge texel again: long streaks on the far tiles.
        // Asking for the level the gradients give takes the one tap an anisotropy of 1 would.
        let level = max(length(du), length(dv)) / min(pixel.x, pixel.y);
        return sprite_level(at, layer, max(0.0, log2(level)));
    }
    return sprite_grad(at, layer, du, dv);
}
// sampleRGSS: four rotated-grid samples of the two nearest mip levels, faded in as the texels
// shrink below two screen pixels, over sample_nearest.
fn sample_rgss(uv: vec2<f32>, layer: u32, pixel: vec2<f32>, du: vec2<f32>, dv: vec2<f32>,
               texel_screen: vec2<f32>, tiled: bool) -> vec4<f32> {
    let max_texel = max(texel_screen.x, texel_screen.y);
    let min_pixel = min(pixel.x, pixel.y);
    let blend = smoothstep(min_pixel, min_pixel * 2.0, max_texel);
    // Only the side of the mix that has weight is sampled: near texels (blend 0) take
    // sample_nearest's one tap alone, far ones (blend 1) the rotated grid alone.
    if blend <= 0.0 {
        return sample_nearest(uv, layer, pixel, du, dv, texel_screen, tiled);
    }
    let rgss = rotated_grid(uv, layer, pixel, du, dv);
    if blend >= 1.0 {
        return rgss;
    }
    return mix(sample_nearest(uv, layer, pixel, du, dv, texel_screen, tiled), rgss, blend);
}
fn rotated_grid(uv: vec2<f32>, layer: u32, pixel: vec2<f32>, du: vec2<f32>,
                dv: vec2<f32>) -> vec4<f32> {
    let min_pixel = min(pixel.x, pixel.y);
    let du_len = length(du);
    let dv_len = length(dv);
    let effective = sqrt(min(du_len, dv_len) * max(du_len, dv_len));
    let exact = max(0.0, log2(effective / min_pixel));
    let low = floor(exact);
    let mip_blend = fract(exact);
    // From this level on the sprite is a single texel (0: the views stop before one is), and
    // the sampler clamps any level past it to it: there every tap reads the same value, so
    // one tap gives the four's mean. Distant tiles sit there.
    let single = globals.filtering.z;
    if single > 0.0 && low >= single {
        return sprite_level(uv, layer, low);
    }
    let high_single = single > 0.0 && low + 1.0 >= single;
    var offsets = array<vec2<f32>, 4>(
        vec2(0.125, 0.375), vec2(-0.125, -0.375), vec2(0.375, -0.125), vec2(-0.375, 0.125));
    var sum_low = vec4(0.0);
    var sum_high = vec4(0.0);
    for (var i = 0; i < 4; i++) {
        let p = uv + offsets[i] * pixel;
        sum_low += sprite_level(p, layer, low);
        if !high_single {
            sum_high += sprite_level(p, layer, low + 1.0);
        }
    }
    if high_single {
        sum_high = 4.0 * sprite_level(uv, layer, low + 1.0);
    }
    return mix(sum_low * 0.25, sum_high * 0.25, mip_blend);
}
// Distant terrain tiles (lod/mesh.rs) count their UVs in blocks past 1: each block takes the
// sprite again, as separate faces would under the clamped sampler. Sprite UVs in [0, 1] stay.
fn tile_uv(uv: vec2<f32>) -> vec2<f32> {
    return uv - max(ceil(uv) - 1.0, vec2(0.0));
}
fn sprite(tiled: vec2<f32>, layer: u32) -> vec4<f32> {
    // The gradients come from the unwrapped UV, so the mip level holds across block seams.
    return sprite_at(tiled, layer, dpdx(tiled), dpdy(tiled));
}
// `sprite` with the unwrapped UV's screen gradients given: the visibility buffer's resolve
// (visbuffer.wgsl) works them out from the quad, as its neighbours may be other quads.
fn sprite_at(tiled: vec2<f32>, layer: u32, du: vec2<f32>, dv: vec2<f32>) -> vec4<f32> {
    // terrain.fsh's two sampling paths: sampleRGSS for RGSS, sampleNearest otherwise.
    let uv = tile_uv(tiled);
    // The wrap shifts a face's UVs by whole blocks, so half of one tells a tile's face from a
    // block's (whose UVs stay inside its sprite).
    let tiled_face = any(tiled - uv >= vec2(0.5));
    let pixel = vec2(1.0 / globals.filtering.y);
    let texel_screen = sqrt(du * du + dv * dv);
    if u32(globals.filtering.x) == 1u {
        return sample_rgss(uv, layer, pixel, du, dv, texel_screen, tiled_face);
    }
    return sample_nearest(uv, layer, pixel, du, dv, texel_screen, tiled_face);
}

// The terrain stages' values, as the code reads them. What crosses from the vertex stage to the
// fragment stage is `TerrainWire`: the same values in fewer interpolant locations.
struct TerrainOut {
    clip: vec4<f32>,
    uv: vec2<f32>,
    layer: u32,
    color: vec4<f32>,
    spherical: f32,
    cylindrical: f32,
    // How the face shines in HDR output (mesher.rs's `EMISSION_BITS`): 0 not
    // at all, 1 to 4 the tier with its bright parts, 5 to 8 the tier all over.
    emission: u32,
    // HDR lighting: block and sky light, as levels 0 to 15.
    scene: vec2<f32>,
    fade: u32,
    // A distant tile's tile-relative position, for its fade (`faded`).
    local: vec3<f32>,
    // terrain.vsh's chunkVisibility: the section's fade-in, from 16 blocks out.
    visibility: f32,
    // The camera-relative height, for the water mirror's cut at the plane.
    height: f32,
    emissive_layer: u32,
    // [world warp varyings]
    // (a native mod's world warp adds its own here, and in `TerrainWire`, `to_wire` and
    // `from_wire`: gfx/world_warp.rs)
    // [end world warp varyings]
}
// `TerrainOut` in 5 interpolant locations instead of 12. A GPU's vertex stage pays for each
// location it exports (AMD's parameter cache gives one a whole slot, however few of its four
// components are used): on an RX 580 the 12 cost 3.2 ns per quad and these 5 cost 1.6. Every
// component is still interpolated on its own (and the integers stay flat), so each pixel gets
// the very values the separate locations gave it.
struct TerrainWire {
    @builtin(position) clip: vec4<f32>,
    // The colour.
    @location(0) a: vec4<f32>,
    // `local` and `visibility`.
    @location(1) b: vec4<f32>,
    // `uv` and `scene`.
    @location(2) c: vec4<f32>,
    // `spherical`, `cylindrical` and `height`.
    @location(3) d: vec4<f32>,
    // `layer`, `emission`, `fade` and `emissive_layer`.
    @location(4) @interpolate(flat) e: vec4<u32>,
    // [world warp wire]
    // [end world warp wire]
}
fn to_wire(o: TerrainOut) -> TerrainWire {
    var w: TerrainWire;
    w.clip = o.clip;
    w.a = o.color;
    w.b = vec4(o.local, o.visibility);
    w.c = vec4(o.uv, o.scene);
    w.d = vec4(o.spherical, o.cylindrical, o.height, 0.0);
    w.e = vec4(o.layer, o.emission, o.fade, o.emissive_layer);
    // [world warp to wire]
    // [end world warp to wire]
    return w;
}
fn from_wire(w: TerrainWire) -> TerrainOut {
    var o: TerrainOut;
    o.clip = w.clip;
    o.color = w.a;
    o.local = w.b.xyz;
    o.visibility = w.b.w;
    o.uv = w.c.xy;
    o.scene = w.c.zw;
    o.spherical = w.d.x;
    o.cylindrical = w.d.y;
    o.height = w.d.z;
    o.layer = w.e.x;
    o.emission = w.e.y;
    o.fade = w.e.z;
    o.emissive_layer = w.e.w;
    // [world warp from wire]
    // [end world warp from wire]
    return o;
}
// Whether a fading tile or section leaves this pixel out (gfx/lod.rs's
// `fade_code`, `fade_out_code`). It fades in, or out, chunk by chunk: each
// column of its own grid (`local`, the tile-relative position: 16 blocks,
// 2^level for coarse tiles) starts in turn over the first half of the fade
// and fades over the second, as a 4x4 ordered pattern in the face's plane,
// its unit the power of two of a block nearest 1.5 pixels, so it stays on
// the terrain as the camera moves. `dx` and `dy`: `local`'s screen
// derivatives.
fn faded(code: u32, local: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>) -> bool {
    if code == 0u { return false; }
    // Sixty-fourths of the fade: 1 to 63 fading in, 65 to 128 fading out.
    let step = (code - 1u) % 129u;
    let out = step >= 64u;
    let progress = f32(select(step, step - 64u, out)) / 64.0;
    let size = exp2(f32(max((code - 1u) / 129u, 4u)));
    let cell = bitcast<vec2<u32>>(vec2<i32>(floor(local.xz / size)));
    var h = cell.x * 73856093u ^ cell.y * 19349663u;
    h = (h ^ (h >> 13u)) * 0x5bd1e995u;
    let start = f32((h ^ (h >> 15u)) & 1023u) / 2048.0;
    let ramp = clamp((progress - start) * 2.0, 0.0, 1.0);
    let shown = select(ramp, 1.0 - ramp, out);
    let n = abs(cross(dx, dy));
    var plane = local.xy;
    if n.x >= n.y && n.x >= n.z {
        plane = local.yz;
    } else if n.y >= n.z {
        plane = local.xz;
    }
    let pixel = max(length(dx), length(dy));
    let unit = exp2(ceil(log2(max(pixel * 1.5, 1.0 / 16.0))));
    let p = vec2<u32>(bitcast<vec2<u32>>(vec2<i32>(floor(plane / unit)))) & vec2(3u);
    let bayer = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    return (f32(bayer[p.y * 4u + p.x]) + 0.5) / 16.0 >= shown;
}
// A fading tile or section is drawn a hair toward the camera (depth is
// reversed: nearer is larger), the newer the more, so it wins where it draws
// over the solid tiles it replaces. Distant tiles go back by 0.2% of their
// distance, so near chunks win over tiles still under them (gfx/lod.rs's
// `previous`, columns not complete yet): near water lies a little below a
// tile's (vanilla's fluid corner heights), and under a smaller step the
// tile's drew over it, two waters, opaque. A tile's UV step is 1/32, a
// section's 1/32768 (`terrain_uv`).
fn fade_depth(clip: vec4<f32>, w: f32) -> vec4<f32> {
    let code = fade_code(w);
    let tile = instance_step(w) > 0.001;
    var scale = select(1.0, 1.0 - 2e-3, tile);
    if code != 0u && (code - 1u) % 129u < 64u {
        scale *= 1.0 + 5e-6 * f32(64u - (code - 1u) % 129u);
    } else if code != 0u && !tile {
        // A section leaving (gfx/lod.rs's `HELD`, `fade_out_code`) goes 2% of
        // its distance toward the camera: far terrain's coarser blocks stand a
        // little out of the real ones, and the tile coming in under it hid it
        // at once, before its fade.
        scale *= 1.02;
    }
    return vec4(clip.xy, clip.z * scale, clip.w);
}
// Horizon Extras' dynamic lights (dynamic_light.rs): the block light, in the vertex's units
// (16 per level), a vertex at camera-relative `pos` gets from the frame's light sources, one
// level less per block. time.z of them are set: none when the extra is off, so no loop runs.
fn dynamic_block_light(pos: vec3<f32>) -> f32 {
    let count = min(u32(globals.time.z), 16u);
    var level = 0.0;
    for (var i = 0u; i < count; i++) {
        let l = globals.dynamic_lights[i];
        level = max(level, l.w - distance(pos, l.xyz));
    }
    return level * 16.0;
}
fn native_colored_light(pos: vec3<f32>) -> vec3<f32> {
    var rgb = vec3(0.0);
    for (var i = 0u; i < min(globals.colored_count.x, 16u); i++) {
        let at = globals.colored_lights[i * 2u];
        let color = globals.colored_lights[i * 2u + 1u];
        rgb += color.rgb * max(0.0, 1.0 - distance(pos, at.xyz) / at.w) * color.a;
    }
    return min(rgb, vec3(1.0));
}
fn sample_lightmap(sky: f32, block: f32) -> vec4<f32> {
    let uv = vec2(block, sky) * 16.0;
    return textureSampleLevel(lightmap, lightmap_sampler,
        clamp(uv / 256.0 + 0.5 / 16.0, vec2(0.5 / 16.0), vec2(15.5 / 16.0)), 0.0);
}
// mesher.rs's TerrainVertex holds the UV in fixed point; the instance's
// origin.w is the step: 1/32768 per unit for sections (a power of two on an
// integer below 2^24, so it decodes exactly), 1/32 per block for distant
// terrain tiles (TerrainVertex::tiled). A fading tile negates it past its
// dither code (gfx/lod.rs's `instance`): the step is the fraction either way.
fn terrain_uv(packed: vec2<u32>, w: f32) -> vec2<f32> {
    return vec2<f32>(packed) * instance_step(w);
}
// A whole section's step (a power of two) carries its fade-in in the mantissa's 23 bits, as
// `1 - visibility` (gfx.rs's `section_instance`): masked off, the step; the rest is
// `RenderSection.getVisibility`. A fading one (negative) is fully visible.
fn instance_step(w: f32) -> f32 {
    if w > 0.0 {
        return bitcast<f32>(bitcast<u32>(w) & 0xff800000u);
    }
    return fract(abs(w));
}
fn instance_visibility(w: f32) -> f32 {
    if w > 0.0 {
        return 1.0 - f32(bitcast<u32>(w) & 0x7fffffu) / 8388608.0;
    }
    return 1.0;
}
// A fading distant tile's dither code (gfx/lod.rs's `fade_code`), 0 when whole.
fn fade_code(w: f32) -> u32 {
    return u32(floor(max(-w, 0.0)));
}
@vertex fn vs_terrain(
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>,
    @location(5) origin: vec4<f32>) -> TerrainWire {
    return to_wire(terrain_vertex(position, terrain_uv(uv, origin.w), layer, color, light, origin.xyz,
        origin.w));
}
// Flat section draws (gfx/flat.rs): one draw per arena page over the frame's
// index list. A section's vertices fill whole 256-vertex blocks of its page:
// the block gives its resident slot, the slot its camera-relative origin.
@group(1) @binding(0) var<storage, read> block_slots: array<u32>;
@group(1) @binding(1) var<storage, read> slot_origins: array<vec4<f32>>;
@vertex fn vs_terrain_flat(
    @builtin(vertex_index) vertex: u32,
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>
) -> TerrainWire {
    let origin = slot_origins[block_slots[vertex >> 8u]];
    return to_wire(terrain_vertex(position, terrain_uv(uv, origin.w), layer, color, light, origin.xyz,
        origin.w));
}
// Solid terrain draws in two passes, each over its own part of every mesh's quads
// (`mesher::Part`): first the quads with no cutout that are not fading, through
// `fs_opaque`, which never discards, so a tiled GPU (Apple's) keeps hiding the pixels
// behind them; then the rest through `fs_solid`. One discard anywhere in a pipeline's shader turned that hiding off for
// all the terrain it drew. The draw lists give each pass only its own part; a mesh that
// was not sorted (`Sides::unsorted`) gives both passes all of its quads, so each pass
// still leaves out the other's: decided before any of the vertex's work, outside the
// clip volume, it makes no pixel.
fn opaque_quad(layer: u32, w: f32) -> bool {
    return (layer & 0x8000u) != 0u && fade_code(w) == 0u;
}
fn left_out() -> TerrainOut {
    var out: TerrainOut;
    out.clip = vec4(2.0, 2.0, 2.0, 1.0);
    return out;
}
@vertex fn vs_terrain_opaque(
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>,
    @location(5) origin: vec4<f32>) -> TerrainWire {
    if !opaque_quad(layer, origin.w) { return to_wire(left_out()); }
    return to_wire(terrain_vertex(position, terrain_uv(uv, origin.w), layer, color, light, origin.xyz,
        origin.w));
}
@vertex fn vs_terrain_cutout(
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>,
    @location(5) origin: vec4<f32>) -> TerrainWire {
    if opaque_quad(layer, origin.w) { return to_wire(left_out()); }
    return to_wire(terrain_vertex(position, terrain_uv(uv, origin.w), layer, color, light, origin.xyz,
        origin.w));
}
@vertex fn vs_terrain_flat_opaque(
    @builtin(vertex_index) vertex: u32,
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>
) -> TerrainWire {
    let origin = slot_origins[block_slots[vertex >> 8u]];
    if !opaque_quad(layer, origin.w) { return to_wire(left_out()); }
    return to_wire(terrain_vertex(position, terrain_uv(uv, origin.w), layer, color, light, origin.xyz,
        origin.w));
}
@vertex fn vs_terrain_flat_cutout(
    @builtin(vertex_index) vertex: u32,
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>
) -> TerrainWire {
    let origin = slot_origins[block_slots[vertex >> 8u]];
    if opaque_quad(layer, origin.w) { return to_wire(left_out()); }
    return to_wire(terrain_vertex(position, terrain_uv(uv, origin.w), layer, color, light, origin.xyz,
        origin.w));
}
// Solid section draws that read their vertices from the arena page (gfx.rs's `pulled`
// pipelines; `mesher/packed.rs`): a draw's vertex indices below PACKED_BASE name vertices
// (`TerrainVertex`, six words, as `TERRAIN_ATTRIBUTES` reads them), from PACKED_BASE on a
// corner of the quad whose record is at that slot (`PACKED_BASE + 4 * slot + corner`).
// Either way they come out as the six words of the vertex, so the work after is the same.
// Binding 19: visbuffer.wgsl, in the same module, takes 2 to 18 of its own group 1.
@group(1) @binding(19) var<storage, read> arena_page: array<vec2<u32>>;
const PACKED_BASE: u32 = 0x10000000u;
// Far tiles' own compact layout (`lod/packed.rs`), read apart from a section's by the base
// its draws use: from FAR_BASE on, a corner of the quad whose far record is at that slot.
const FAR_BASE: u32 = 0x20000000u;
// `TerrainVertex::tiled`: a far tile's UVs are in blocks from the face's corner, this many
// fixed-point steps each (`mesher::TILED_UV_STEPS`).
const TILED_UV_STEPS: f32 = 32.0;
// A vertex's words: position bits, then UV, layer with light, colour.
struct ArenaCorner {
    p: vec3<u32>,
    r: vec3<u32>,
}
fn arena_vertex(v: u32) -> ArenaCorner {
    let a = arena_page[v * 3u];
    let b = arena_page[v * 3u + 1u];
    let c = arena_page[v * 3u + 2u];
    return ArenaCorner(vec3(a.x, a.y, b.x), vec3(b.y, c.x, c.y));
}
// The offset a block corner adds on one component: `side` on the face's axis (rel 0), `du`
// on the next axis (rel 1), `dw` on the one after (rel 2).
fn corner_offset(rel: u32, side: u32, du: u32, dw: u32) -> u32 {
    return select(select(dw, du, rel == 1u), side, rel == 0u);
}
// `packed::packed_vertex`: corner `corner` of the record whose words are lo (0 to 2) and
// hi (3 to 5).
fn packed_corner(lo: vec3<u32>, hi: vec3<u32>, corner: u32) -> ArenaCorner {
    let orient = (hi.z >> 12u) & 0x3ffffu;
    let block = vec3(orient & 15u, (orient >> 4u) & 15u, (orient >> 8u) & 15u);
    let axis = (orient >> 12u) & 3u;
    let side = (orient >> 14u) & 1u;
    let start = (orient >> 15u) & 3u;
    let dir = (orient >> 17u) & 1u;
    let step = (start + select(corner, 4u - corner, dir == 1u)) & 3u;
    let du = select(0u, 1u, step == 1u || step == 2u);
    let dw = select(0u, 1u, step >= 2u);
    // No dynamic component stores: FXC (DirectX 12) rejects them (X3500). A component's offset
    // from the axis (0: the axis, 1: the next, 2: the one after) picks what it gets. Scalar
    // selects only: a vector select becomes GLSL's mix(uvec3, uvec3, bvec3), which old AMD
    // OpenGL drivers (Radeon HD 7400M, driver 15.301) resolve to the float overload and reject.
    let offset = vec3(
        corner_offset((3u - axis) % 3u, side, du, dw),
        corner_offset((4u - axis) % 3u, side, du, dw),
        corner_offset((5u - axis) % 3u, side, du, dw),
    );
    let pos = block + offset;
    let sel = lo.z >> 24u;
    let us = (sel >> (2u * corner)) & 1u;
    let vs = (sel >> (2u * corner + 1u)) & 1u;
    let u = (lo.z >> (6u * us)) & 63u;
    let v = (lo.z >> (12u + 6u * vs)) & 63u;
    var light = hi.z;
    if corner == 0u {
        light = hi.x >> 16u;
    } else if corner == 1u {
        light = hi.y;
    } else if corner == 2u {
        light = hi.y >> 12u;
    }
    light &= 0xfffu;
    let packed_light = ((light & 63u) << 2u) | (((light >> 6u) & 63u) << 10u);
    let g = (lo.y >> (8u * corner)) & 255u;
    let t = lo.x;
    let color = (g * ((t >> 16u) & 255u) / 255u) | ((g * ((t >> 8u) & 255u) / 255u) << 8u)
        | ((g * (t & 255u) / 255u) << 16u) | (t & 0xff000000u);
    return ArenaCorner(bitcast<vec3<u32>>(vec3<f32>(pos)),
        vec3((u << 10u) | (v << 26u), (hi.x & 0xffffu) | (packed_light << 16u), color));
}
// `lod::packed::Face::vertices`: corner `corner` of the far record whose words are lo (0 to
// 2) and hi (3 to 5). A far quad is a rectangle in an axis-aligned plane with one colour,
// one light and one layer, so the record holds the rectangle and this rebuilds the four
// corners and the UVs `lod::mesh::emit_quad` derived from them. The two have to agree, and
// `lod::mesh`'s `every_quad_the_mesher_emits_packs_and_comes_back` is what holds them to it.
fn far_corner(lo: vec3<u32>, hi: vec3<u32>, corner: u32) -> ArenaCorner {
    let axis = hi.z & 3u;
    let reversed = (hi.z >> 2u) & 1u;
    let plane = f32(bitcast<i32>(lo.z));
    let u0 = f32(bitcast<i32>(hi.x));
    let v0 = f32(bitcast<i32>(hi.y));
    let u1 = u0 + f32((hi.z >> 3u) & 0x7ffu);
    let v1 = v0 + f32((hi.z >> 14u) & 0x7ffu);
    // `points.reverse()`, which is what the mesher does for Down, South and East.
    let c = select(corner, 3u - corner, reversed == 1u);
    let far_v = select(v0, v1, c <= 1u);
    // Axis X runs its u the other way round, as that function's `points` do.
    let first = c == 0u || c == 3u;
    let far_u = select(select(u1, u0, first), select(u0, u1, first), axis == 2u);
    // 0: the plane is y, u is x and v is z. 1: the plane is z, u is x, v is y.
    // 2: the plane is x, u is z and v is y.
    var pos = vec3(far_u, plane, far_v);
    if axis == 1u {
        pos = vec3(far_u, far_v, plane);
    } else if axis == 2u {
        pos = vec3(plane, far_v, far_u);
    }
    // The UV is `[u, -v]` of the face counted from its corner, which is `[u0, -v1]`: every
    // coordinate is whole, so the mesher's `floor` of the least corner is the corner itself.
    let uv = vec2<u32>(vec2(far_u - u0, v1 - far_v) * TILED_UV_STEPS);
    // Word 1 is the layer with the light, word 0 the colour, as a vertex holds them.
    return ArenaCorner(bitcast<vec3<u32>>(pos), vec3(uv.x | (uv.y << 16u), lo.y, lo.x));
}
// A solid draw's vertex `vertex`, and the slot its block is found by (`block_slots`, 256
// slots a block): the vertex's own, or its quad's record's. `packed`: it has a record.
struct SectionCorner {
    c: ArenaCorner,
    slot: u32,
    packed: bool,
}
fn section_corner(vertex: u32) -> SectionCorner {
    if vertex < PACKED_BASE {
        return SectionCorner(arena_vertex(vertex), vertex, false);
    }
    if vertex >= FAR_BASE {
        let far_slot = (vertex - FAR_BASE) >> 2u;
        let a = arena_page[far_slot * 3u];
        let b = arena_page[far_slot * 3u + 1u];
        let c = arena_page[far_slot * 3u + 2u];
        // `FULL`: the quad's four vertices, word 0 slots on, as a section's spill.
        if (c.y & 0x80000000u) != 0u {
            return SectionCorner(arena_vertex(far_slot + a.x + (vertex & 3u)), far_slot, true);
        }
        return SectionCorner(far_corner(vec3(a.x, a.y, b.x), vec3(b.y, c.x, c.y), vertex & 3u),
            far_slot, true);
    }
    let slot = (vertex - PACKED_BASE) >> 2u;
    let corner = vertex & 3u;
    let a = arena_page[slot * 3u];
    let b = arena_page[slot * 3u + 1u];
    let c = arena_page[slot * 3u + 2u];
    // `packed::FULL`: the quad's four vertices, word 0 slots on.
    if (c.y & 0x80000000u) != 0u {
        return SectionCorner(arena_vertex(slot + a.x + corner), slot, true);
    }
    return SectionCorner(packed_corner(vec3(a.x, a.y, b.x), vec3(b.y, c.x, c.y), corner), slot,
        true);
}
// The colour as the vertex fetch converts a Unorm8x4 attribute, bit for bit (Apple's M1:
// times 1/255; `unpack4x8unorm` and the correctly rounded c / 255 each differ from it for
// some bytes, and moved a few pixels' colour or alpha cutout).
fn unorm8x4(c: u32) -> vec4<f32> {
    return vec4<f32>(vec4(c & 255u, (c >> 8u) & 255u, (c >> 16u) & 255u, c >> 24u))
        * (1.0 / 255.0);
}
fn corner_layer(c: ArenaCorner) -> u32 {
    return c.r.y & 0xffffu;
}
fn corner_vertex(c: ArenaCorner, origin: vec4<f32>) -> TerrainOut {
    let uv = vec2(c.r.x & 0xffffu, c.r.x >> 16u);
    let light = vec2((c.r.y >> 16u) & 0xffu, c.r.y >> 24u);
    return terrain_vertex(bitcast<vec3<f32>>(c.p), terrain_uv(uv, origin.w), corner_layer(c),
        unorm8x4(c.r.z), light, origin.xyz, origin.w);
}
@vertex fn vs_section(@builtin(vertex_index) vertex: u32,
    @location(5) origin: vec4<f32>) -> TerrainWire {
    return to_wire(corner_vertex(section_corner(vertex).c, origin));
}
@vertex fn vs_section_opaque(@builtin(vertex_index) vertex: u32,
    @location(5) origin: vec4<f32>) -> TerrainWire {
    let s = section_corner(vertex);
    if !opaque_quad(corner_layer(s.c), origin.w) { return to_wire(left_out()); }
    return to_wire(corner_vertex(s.c, origin));
}
@vertex fn vs_section_cutout(@builtin(vertex_index) vertex: u32,
    @location(5) origin: vec4<f32>) -> TerrainWire {
    let s = section_corner(vertex);
    if opaque_quad(corner_layer(s.c), origin.w) { return to_wire(left_out()); }
    return to_wire(corner_vertex(s.c, origin));
}
@vertex fn vs_section_flat(@builtin(vertex_index) vertex: u32) -> TerrainWire {
    let s = section_corner(vertex);
    let origin = slot_origins[block_slots[s.slot >> 8u]];
    return to_wire(corner_vertex(s.c, origin));
}
@vertex fn vs_section_flat_opaque(@builtin(vertex_index) vertex: u32) -> TerrainWire {
    let s = section_corner(vertex);
    let origin = slot_origins[block_slots[s.slot >> 8u]];
    if !opaque_quad(corner_layer(s.c), origin.w) { return to_wire(left_out()); }
    return to_wire(corner_vertex(s.c, origin));
}
@vertex fn vs_section_flat_cutout(@builtin(vertex_index) vertex: u32) -> TerrainWire {
    let s = section_corner(vertex);
    let origin = slot_origins[block_slots[s.slot >> 8u]];
    if opaque_quad(corner_layer(s.c), origin.w) { return to_wire(left_out()); }
    return to_wire(corner_vertex(s.c, origin));
}
// HDR lighting: how far a surface's light carries it above paper white, 0 to 1. Only the
// strongest light does, so dark stays as dark as in SDR; sky light by how the face meets the sun.
fn hdr_scene(block: f32, sky: f32, facing: f32) -> f32 {
    let b = smoothstep(11.0, 15.0, block);
    let s = smoothstep(11.0, 15.0, sky) * globals.hdr_light.x * facing;
    return max(b, s);
}
// The frame's alpha for it, between 1 (none) and the light sources' marks below 0.75.
fn hdr_scene_alpha(scene: f32) -> f32 {
    return 1.0 - 0.24 * scene;
}
// The face's normal, from the fragment's derivatives, for how it faces the sun.
var<private> hdr_normal: vec3<f32>;
fn hdr_facing() -> f32 {
    return mix(0.45, 1.0, max(dot(hdr_normal, globals.sun.xyz), 0.0));
}
fn terrain_vertex(position: vec3<f32>, uv: vec2<f32>, layer: u32, color: vec4<f32>,
    light: vec2<u32>, origin: vec3<f32>, w: f32) -> TerrainOut {
    // [world warp vertex]
    // (a world warp's pipelines put the point through the mod's `world_warp`: gfx/world_warp.rs)
    let pos = position + origin;
    // [end world warp vertex]
    var out: TerrainOut;
    out.clip = fade_depth(globals.view_proj * vec4(pos, 1.0), w);
    out.local = position;
    out.uv = uv;
    out.layer = layer;
    out.emissive_layer = 0xffffffffu;
    // The pairs are sorted by base layer: stop at the first past this one.
    let sprite_layer = layer & 0x7fffu;
    for (var i = 0u; i < min(globals.emissive_count.x, 128u); i++) {
        let pair = globals.emissive_layers[i];
        if pair.x == sprite_layer {
            out.emissive_layer = pair.y;
            break;
        }
        if pair.x > sprite_layer {
            break;
        }
    }
    let block = max(f32(light.y), dynamic_block_light(pos));
    out.color = vec4(color.rgb, 1.0) * sample_lightmap(f32(light.x) / 16.0, block / 16.0);
    out.scene = vec2(block / 16.0, f32(light.x) / 16.0);
    if globals.colored_count.x > 0u {
        out.color = vec4(min(out.color.rgb + color.rgb * native_colored_light(pos), vec3(1.0)), out.color.a);
    }
    out.emission = 15u - (u32(round(color.a * 255.0)) & 15u);
    out.height = pos.y;
    out.spherical = length(pos);
    out.cylindrical = max(length(pos.xz), abs(pos.y));
    out.fade = fade_code(w);
    const CHUNK_FULLY_VISIBLE_RANGE: f32 = 16.0;
    out.visibility = mix(1.0, instance_visibility(w),
        clamp((length(pos) - CHUNK_FULLY_VISIBLE_RANGE) / CHUNK_FULLY_VISIBLE_RANGE, 0.0, 1.0));
    // [world warp out]
    // [end world warp out]
    return out;
}
fn linear_fog(d: f32, start: f32, end: f32) -> f32 {
    if d <= start { return 0.0; }
    if d >= end { return 1.0; }
    return (d - start) / (end - start);
}
fn apply_fog(color: vec4<f32>, spherical: f32, cylindrical: f32) -> vec4<f32> {
    let f = max(linear_fog(spherical, globals.fog.x, globals.fog.y),
                linear_fog(cylindrical, globals.fog.z, globals.fog.w));
    return vec4(mix(color.rgb, globals.fog_color.rgb, f * globals.fog_color.a), color.a);
}
// hdr.wgsl's brightness: the mean of luminance and the brightest channel, in
// linear light.
fn brightness(c: vec3<f32>) -> f32 {
    let l = pow(max(c, vec3(0.0)), vec3(2.2));
    return 0.5 * (dot(l, vec3(0.2126, 0.7152, 0.0722)) + max(l.r, max(l.g, l.b)));
}
// HDR output's mark (hdr.wgsl): the frame's alpha below 0.75, a band of 0.175 per
// tier (1 low .. 4 very bright, from 0.525 down to 0) and in it how much the pixel
// lifts, from its own brightness `y` before lighting through the tier's window.
fn hdr_mark(tier: u32, y: f32) -> f32 {
    let t = clamp(tier, 1u, 4u) - 1u;
    let lift = smoothstep(globals.hdr_windows[0][t], globals.hdr_windows[1][t], y);
    return 0.175 * f32(3u - t) + 0.165 * (1.0 - lift);
}
@fragment fn fs_solid(wire: TerrainWire) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    return solid_color(in);
}
// `fs_solid` for the opaque pass (`opaque_quad`): no fade, no cutout, no discard.
@fragment fn fs_opaque(wire: TerrainWire) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    if HDR_LIGHTING && globals.hdr_light.z != 0.0 {
        hdr_normal = normalize(cross(dpdy(in.local), dpdx(in.local)));
    }
    let texel = sprite(in.uv, in.layer & 0x7fffu);
    var color = texel * in.color;
    color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, color.a), color, in.visibility);
    return solid_finish(in, texel, color);
}
// The water mirror (gfx.rs's reflection pass): what lies below the water plane
// (`fog_ends.w`, camera-relative) would show above it in the mirror, so it is cut away.
@fragment fn fs_solid_mirror(wire: TerrainWire) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    if in.height < globals.fog_ends.w { discard; }
    return solid_color(in);
}
fn solid_color(in: TerrainOut) -> vec4<f32> {
    let dx = dpdx(in.local);
    let dy = dpdy(in.local);
    if HDR_LIGHTING && globals.hdr_light.z != 0.0 {
        hdr_normal = normalize(cross(dy, dx));
    }
    if faded(in.fade, in.local, dx, dy) { discard; }
    // Bit 15 of the layer marks opaque leaves (forceOpaque): no alpha cutout.
    let texel = sprite(in.uv, in.layer & 0x7fffu);
    var color = texel * in.color;
    // terrain.fsh: the fade-in mixes toward the fog colour.
    color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, color.a), color, in.visibility);
    if color.a < 0.5 && (in.layer & 0x8000u) == 0u { discard; }
    return solid_finish(in, texel, color);
}
// The rest of a solid pixel's colour once it is kept: emission and fog.
fn solid_finish(in: TerrainOut, texel: vec4<f32>, color: vec4<f32>) -> vec4<f32> {
    return solid_light(solid_finish_at(in, texel, color, dpdx(in.uv), dpdy(in.uv)));
}
// A finished solid pixel as the frame keeps it: on the float frame its colour, alpha 1, lit by
// HDR's gain where it applies (`hdr_light.w`); else its mark in alpha.
fn solid_light(finished: vec4<f32>) -> vec4<f32> {
    if HDR_WRITE_LIGHT {
        if globals.hdr_light.w != 0.0 {
            return vec4(hdr_written(finished.rgb, finished.a, globals.hdr_settings, globals.hdr_shares), 1.0);
        }
        return vec4(finished.rgb, 1.0);
    }
    return finished;
}
// `solid_finish` with the UV's screen gradients given (`sprite_at`).
fn solid_finish_at(in: TerrainOut, texel: vec4<f32>, color: vec4<f32>, du: vec2<f32>,
                   dv: vec2<f32>) -> vec4<f32> {
    // Below 0.75, alpha tells HDR output (hdr.wgsl) a light source's pixel
    // shines, at which tier (mesher::hdr_tier) and how much (hdr_mark), by the
    // texel's own brightness, so its light level and face shading don't change
    // it. Only pixels as bright as their sprite's threshold may: its bright
    // parts, or all of it. Nothing else reads the frame's alpha.
    // Fog turns a far surface into the fog's colour: it takes the fog's light with it, or a
    // fully fogged mountain would stand out from the fog behind it.
    // Without HDR output nothing reads the alpha, so its lighting is not worked out.
    var emissive = 1.0;
    if HDR_LIGHTING && globals.hdr_light.z != 0.0 {
        let fogged = max(linear_fog(in.spherical, globals.fog.x, globals.fog.y),
            linear_fog(in.cylindrical, globals.fog.z, globals.fog.w)) * globals.fog_color.a;
        emissive = hdr_scene_alpha(mix(hdr_scene(in.scene.x, in.scene.y, hdr_facing()),
            globals.hdr_light.y, fogged));
        if in.emission > 0u {
            // A light source's own face shines as one or stays as drawn, without the lighting.
            emissive = 1.0;
            let whole = in.emission > 4u;
            let tier = select(in.emission, in.emission - 4u, whole);
            let l = in.layer & 0x7fffu;
            let t = textureLoad(emission_thresholds, vec2(i32(l % SPRITE_SPLIT), i32(l / SPRITE_SPLIT)), 0);
            let y = brightness(texel.rgb);
            // All over means all over: the sprite's range is one frame's, and an animated sprite's
            // other frames dip below it. Fog hides a light source as it hides everything else.
            if whole || y >= t.r {
                emissive = hdr_mark(tier, y * (1.0 - fogged));
            }
        }
    }
    if in.emissive_layer != 0xffffffffu {
        // The high bit: an OptiFine overlay, drawn cutout (opaque where alpha >= 0.1).
        var mask = sprite_at(in.uv, in.emissive_layer & 0x7fffffffu, du, dv);
        if (in.emissive_layer & 0x80000000u) != 0u {
            mask.a = select(0.0, 1.0, mask.a >= 0.1);
        }
        return apply_fog(vec4(mix(color.rgb, mask.rgb, mask.a), mix(emissive, hdr_mark(3u, 1.0), mask.a)), in.spherical, in.cylindrical);
    }
    return apply_fog(vec4(color.rgb, emissive), in.spherical, in.cylindrical);
}
// HDR lighting on what's mostly opaque among translucent faces (water), drawn again after
// them with only alpha written: `fs_translucent` blends its alpha as coverage, so a lake's
// value would be diluted. Clear glass leaves what's behind it, light sources included, alone.
@fragment fn fs_translucent_mark(wire: TerrainWire) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    let dx = dpdx(in.local);
    let dy = dpdy(in.local);
    hdr_normal = normalize(cross(dy, dx));
    if sprite(in.uv, in.layer & 0x7fffu).a < 0.6 { discard; }
    let fogged = max(linear_fog(in.spherical, globals.fog.x, globals.fog.y),
        linear_fog(in.cylindrical, globals.fog.z, globals.fog.w)) * globals.fog_color.a;
    let scene = mix(hdr_scene(in.scene.x, in.scene.y, hdr_facing()), globals.hdr_light.y, fogged);
    return vec4(0.0, 0.0, 0.0, hdr_scene_alpha(scene));
}
// Water, glass and other see-through faces draw whole as they fade in, the
// layer drawn first in front (gfx.rs's water tiers): dithered, the faded-out
// pixels showed the layer behind through the one in front, twice the water.
@fragment fn fs_translucent(wire: TerrainWire) -> @location(0) vec4<f32> {
    let in = from_wire(wire);
    if HDR_WRITE_LIGHT && globals.hdr_light.w != 0.0 {
        hdr_normal = normalize(cross(dpdy(in.local), dpdx(in.local)));
    }
    let texel = sprite(in.uv, in.layer & 0x7fffu);
    var color = texel * in.color;
    color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, color.a), color, in.visibility);
    if color.a < 0.01 { discard; }
    let finished = apply_fog(color, in.spherical, in.cylindrical);
    if HDR_WRITE_LIGHT && globals.hdr_light.w != 0.0 {
        // The surface's own light, whatever its texel's alpha: what is behind it is light already.
        // (The marked frame keeps one mark a pixel, so there `fs_translucent_mark` skips clear
        // texels to leave what is behind them its own.)
        let fogged = max(linear_fog(in.spherical, globals.fog.x, globals.fog.y),
            linear_fog(in.cylindrical, globals.fog.z, globals.fog.w)) * globals.fog_color.a;
        let mark = hdr_scene_alpha(mix(hdr_scene(in.scene.x, in.scene.y, hdr_facing()), globals.hdr_light.y, fogged));
        return vec4(hdr_written(finished.rgb, mark, globals.hdr_settings, globals.hdr_shares), finished.a);
    }
    return finished;
}

// Native still-fluid boxes use block sprites and lightmap without terrain AO/emission
// packing or entity-model diffuse shading. The absolute sprite layer occupies glint UV x.
struct NativeFluidOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) sprite: u32,
    @location(3) spherical: f32,
    @location(4) cylindrical: f32,
}
@vertex fn vs_native_fluid(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>,
    @location(3) color: vec4<f32>, @location(4) light: vec4<u32>,
    @location(6) sprite: vec2<f32>) -> NativeFluidOut {
    let block = max(f32(light.y), dynamic_block_light(position));
    var lit = color * sample_lightmap(f32(light.x) / 16.0, block / 16.0);
    if globals.colored_count.x > 0u {
        lit = vec4(min(lit.rgb + color.rgb * native_colored_light(position), vec3(1.0)), lit.a);
    }
    return NativeFluidOut(globals.view_proj * vec4(position, 1.0), uv, lit, u32(sprite.x),
        length(position), max(length(position.xz), abs(position.y)));
}
@fragment fn fs_native_fluid(in: NativeFluidOut) -> @location(0) vec4<f32> {
    // The source translucent shader has no alpha discard; transparent texels write depth too.
    let color = sprite_grad(in.uv, in.sprite, dpdx(in.uv), dpdy(in.uv)) * in.color;
    return apply_fog(color, in.spherical, in.cylindrical);
}

// Native terrain models retain floating-point transformed normals. Fullbright still receives
// entity directional shading; the material's alpha test and mip policy are independent.
// Diffuse/normal policy: Flywheel 1.0.6, copyright2021-2024 Jozufozu (MIT);
// full permission/warranty notice in THIRD_PARTY_NOTICES.md.
override NATIVE_MODEL_ALPHA_CUTOFF: f32 = 0.0;
struct NativeModelOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) sprite: u32,
    @location(3) spherical: f32,
    @location(4) cylindrical: f32,
    @location(5) normal: vec3<f32>,
}
@vertex fn vs_native_model(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec4<u32>,
    @location(5) normal_z: u32, @location(6) normal_xy: vec2<f32>) -> NativeModelOut {
    let block = max(f32(light.y), dynamic_block_light(position));
    var lit = color * sample_lightmap(f32(light.x) / 16.0, block / 16.0);
    if globals.colored_count.x > 0u {
        lit = vec4(min(lit.rgb + color.rgb * native_colored_light(position), vec3(1.0)), lit.a);
    }
    return NativeModelOut(globals.view_proj * vec4(position, 1.0), uv, lit, layer >> 16u,
        length(position), max(length(position.xz), abs(position.y)),
        vec3(normal_xy, bitcast<f32>(normal_z)));
}
@fragment fn fs_native_model(in: NativeModelOut) -> @location(0) vec4<f32> {
    var color = sprite_grad(in.uv, in.sprite, dpdx(in.uv), dpdy(in.uv)) * in.color;
    if NATIVE_MODEL_ALPHA_CUTOFF > 0.0 && color.a < NATIVE_MODEL_ALPHA_CUTOFF { discard; }
    if any(in.normal != vec3(0.0)) {
        let light0 = normalize(vec3(0.2, 1.0, -0.7));
        let light1 = normalize(vec3(-0.2, select(1.0, -1.0, globals.time.y > 0.5), 0.7));
        let shade = min(1.0, (max(0.0, dot(light0, in.normal))
            + max(0.0, dot(light1, in.normal))) * 0.6 + 0.4);
        color = vec4(color.rgb * shade, color.a);
    }
    let finished = apply_fog(color, in.spherical, in.cylindrical);
    return vec4(finished.rgb, select(finished.a, 1.0, HDR_WRITE_LIGHT));
}

// Sky (SkyRenderer): the 10-vertex sky disc fan at y = 16 (radius 512) in
// the sky colour with sky fog (sky.fsh: fog 0..FogSkyEnd on the spherical
// distance, a step at FogSkyEnd on the cylindrical one), evaluated per pixel
// along the view ray with the fan's interpolated vertex distances; the
// clear colour (fog colour) elsewhere; the sunrise/sunset fan blended over
// it; the dark disc (y = -4) below the horizon when the camera is low.
struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}
@vertex fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = vec2(f32(i32(i & 1u) * 4 - 1), f32(i32(i >> 1u) * 4 - 1));
    var out: SkyOut;
    out.clip = vec4(p, 0.0, 1.0);
    out.ndc = p;
    return out;
}
fn view_ray(ndc: vec2<f32>) -> vec3<f32> {
    return normalize((globals.inverse_view * vec4(ndc.x * globals.misc.z * globals.misc.w,
        ndc.y * globals.misc.w, -1.0, 0.0)).xyz);
}
const PI: f32 = 3.14159265;
// Interpolated (spherical, cylindrical) distance where the ray meets a disc
// fan at height y, or negative when it misses.
fn disc_distance(d: vec3<f32>, y: f32) -> vec2<f32> {
    if d.y * y <= 0.0 { return vec2(-1.0); }
    let p = d * (y / d.y);
    let h = length(p.xz);
    let phi = atan2(p.z, p.x);
    let sector = PI / 4.0;
    let delta = phi - (floor(phi / sector) * sector + sector * 0.5);
    let edge = 512.0 * cos(sector * 0.5) / cos(delta);
    if h > edge { return vec2(-1.0); }
    let s = h / edge;
    let ay = abs(y);
    let rim = sqrt(512.0 * 512.0 + y * y);
    return vec2(ay + s * (rim - ay), ay + s * (512.0 - ay));
}
fn sky_fog(color: vec3<f32>, dist: vec2<f32>) -> vec3<f32> {
    let end = globals.fog_ends.x;
    let f = max(linear_fog(dist.x, 0.0, end), linear_fog(dist.y, end, end));
    return mix(color, globals.fog_color.rgb, f * globals.fog_color.a);
}
// Weight of the fan centre where the ray crosses the sunrise fan (0 when missed):
// Moller-Trumbore from the origin along d, over normals built once a frame
// (gfx/sunrise_fan.rs). The barycentrics are apex/s, k1/s and -k0/s; s is -det.
fn sunrise_weight_full(d: vec3<f32>) -> f32 {
    if globals.sunrise.a <= 0.001 { return 0.0; }
    var best = 0.0;
    var k0 = dot(d, globals.sunrise_fan[16].xyz);
    for (var i = 0; i < 16; i++) {
        let triangle = globals.sunrise_fan[i];
        let k1 = dot(d, globals.sunrise_fan[16 + (i + 1) % 16].xyz);
        // A hit needs s on the side of w, so k0 and k1 on either side of 0.
        if k0 * triangle.w <= 0.0 && k1 * triangle.w >= 0.0 {
            let apex = dot(d, triangle.xyz);
            let s = apex + k1 - k0;
            if abs(s) >= 1e-6 && s * triangle.w > 0.0 && apex * s >= 0.0 && k1 * s >= 0.0
                && k0 * s <= 0.0 {
                best = max(best, apex / s);
            }
        }
        k0 = k1;
    }
    return best;
}
// The rim's YZ projection is an ellipse. Comparing its unsquashed X/Y
// magnitudes with tan(PI/8) selects one of sixteen sectors without atan2.
fn sunrise_triangle_weight(d: vec3<f32>, i: u32) -> f32 {
    let k0 = dot(d, globals.sunrise_fan[16u+i].xyz);
    let k1 = dot(d, globals.sunrise_fan[16u+(i+1u)%16u].xyz);
    let triangle = globals.sunrise_fan[i];
    let apex = dot(d, triangle.xyz);
    let s = apex + k1 - k0;
    if abs(s) >= 1e-6 && s * triangle.w > 0.0 && apex * s >= 0.0
        && k1 * s >= 0.0 && k0 * s <= 0.0 {
        return apex / s;
    }
    return 0.0;
}
fn sunrise_weight(d: vec3<f32>) -> f32 {
    if globals.sunrise.a <= 0.001 { return 0.0; }
    if dot(d.yz, d.yz) < 1e-10 { return sunrise_weight_full(d); }
    // The camera lies in the rim plane. A forward ray can reach the fan
    // only on the apex side. Keep a guard for rounded coplanar normals.
    let apex = dot(d, globals.sunrise_fan[0].xyz);
    if apex * globals.sunrise_fan[0].w < -1e-4 { return 0.0; }
    let y = dot(d, globals.sunrise_fan[16].xyz);
    let x = -dot(d, globals.sunrise_fan[20].xyz);
    let ax = abs(x);
    let ay = abs(y);
    let tangent = 0.414213562373095;
    let quadrant_sector = select(select(0u, 1u, ay > ax * tangent),
        select(2u, 3u, ax < ay * tangent), ay > ax);
    let sector = select(select(8u + quadrant_sector, 7u - quadrant_sector, y >= 0.0),
        select(15u - quadrant_sector, quadrant_sector, y >= 0.0), x >= 0.0);
    var best = max(sunrise_triangle_weight(d, sector),
        sunrise_triangle_weight(d, (sector + 8u) % 16u));
    // Test adjacent sectors only at a rounded boundary; their exact rim
    // normals remain the authority for hit tests and interpolation weights.
    let k0 = dot(d, globals.sunrise_fan[16u+sector].xyz);
    let k1 = dot(d, globals.sunrise_fan[16u+(sector+1u)%16u].xyz);
    if min(abs(k0), abs(k1)) < max(ax, ay) * 0.001 {
        best = max(best, sunrise_triangle_weight(d, (sector + 15u) % 16u));
        best = max(best, sunrise_triangle_weight(d, (sector + 1u) % 16u));
        best = max(best, sunrise_triangle_weight(d, (sector + 7u) % 16u));
        best = max(best, sunrise_triangle_weight(d, (sector + 9u) % 16u));
    }
    return best;
}
// The sky's colour along the view ray `d` (a shader pack's sky stage gets it as its input:
// gfx/native_sky.rs builds that stage from this section).
fn sky_along_ray(d: vec3<f32>) -> vec3<f32> {
    var color = sky_along_ray_base(d);
    let w = sunrise_weight(d);
    if w > 0.0 {
        color = mix(color, globals.sunrise.rgb, w * globals.sunrise.a);
    }
    return color;
}
// The sky without the sunrise fan: the renderer rasterizes the fan's sixteen
// triangles over this colour instead of testing them in every sky pixel.
fn sky_along_ray_base(d: vec3<f32>) -> vec3<f32> {
    var color = globals.fog_color.rgb;
    let dist = disc_distance(d, 16.0);
    if dist.x >= 0.0 {
        color = sky_fog(globals.sky_color.rgb, dist);
    }
    return color;
}
fn sky_finished(color: vec3<f32>) -> vec4<f32> {
    let mark = 1.0 - 0.24 * globals.hdr_light.y;
    if HDR_WRITE_LIGHT {
        if globals.hdr_light.w != 0.0 {
            return vec4(hdr_written(color, mark, globals.hdr_settings, globals.hdr_shares), 1.0);
        }
        return vec4(color, 1.0);
    }
    return vec4(color, mark);
}
@fragment fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    // `hdr_scene_alpha`, spelled out: this part of the file also goes into shader packs' skies.
    return sky_finished(sky_along_ray(view_ray(in.ndc)));
}
@fragment fn fs_sky_base(in: SkyOut) -> @location(0) vec4<f32> {
    return sky_finished(sky_along_ray_base(view_ray(in.ndc)));
}
// The sunrise/sunset fan as geometry: the clip position is projected by the
// CPU with the sky's unjittered view, and the apex weight (1 at the apex, 0 at
// the rim) interpolates exactly the barycentric the per-pixel test computed.
struct SunriseOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) weight: f32,
}
@vertex fn vs_sunrise(@location(0) clip: vec4<f32>, @location(1) weight: f32) -> SunriseOut {
    return SunriseOut(clip, weight);
}
@fragment fn fs_sunrise(in: SunriseOut) -> @location(0) vec4<f32> {
    return vec4(sky_finished(globals.sunrise.rgb).rgb, in.weight * globals.sunrise.a);
}
// The first-person hand's depth clear inside the world pass: `vs_sky`'s triangle lies at the
// far plane, written over the whole depth target with no colour.
@fragment fn fs_depth_reset(in: SkyOut) -> @location(0) vec4<f32> {
    return vec4(0.0);
}
@fragment fn fs_dark_disc(in: SkyOut) -> @location(0) vec4<f32> {
    let d = view_ray(in.ndc);
    let dist = disc_distance(d, -4.0);
    if dist.x < 0.0 { discard; }
    return vec4(sky_fog(vec3(0.0), dist), 1.0);
}

// rendertype_lines.vsh: screen-space stroke and camera-space depth separation.
fn outline_clip(vertex: u32, start: vec3<f32>, end: vec3<f32>, width: f32) -> vec4<f32> {
    let position = select(start, end, vertex >= 2u);
    let normal = normalize(end - start);
    let view_shrink = 1.0 - 1.0 / 256.0;
    let line_start = globals.view_proj * vec4((position * globals.outline_params.w) * view_shrink, 1.0);
    let line_end = globals.view_proj * vec4(((position + normal) * globals.outline_params.w) * view_shrink, 1.0);
    let ndc1 = line_start.xyz / line_start.w;
    let ndc2 = line_end.xyz / line_end.w;
    let screen = globals.outline_params.xy;
    let direction = normalize((ndc2.xy - ndc1.xy) * screen);
    var offset = vec2(-direction.y, direction.x) * width / screen;
    if offset.x < 0.0 { offset = -offset; }
    if vertex % 2u != 0u { offset = -offset; }
    return vec4((ndc1 + vec3(offset, 0.0)) * line_start.w, line_start.w);
}
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) spherical: f32,
    @location(1) cylindrical: f32,
    @location(2) color: vec4<f32>,
}
@vertex fn vs_line(@builtin(vertex_index) vertex: u32,
    @location(0) start: vec3<f32>, @location(1) end: vec3<f32>,
    @location(2) argb: vec4<f32>, @location(3) width: f32) -> LineOut {
    let position = select(start, end, vertex >= 2u);
    var out: LineOut;
    out.clip = outline_clip(vertex, start, end, width);
    out.spherical = length(position);
    out.cylindrical = max(length(position.xz), abs(position.y));
    // ARGB in a little-endian u32 reads as B, G, R, A.
    out.color = argb.zyxw;
    return out;
}
@fragment fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return apply_fog(in.color, in.spherical, in.cylindrical);
}
// Gizmo lines (rendertype_lines with a per-vertex colour and LineWidth in
// pixels; no layering transform).
struct DebugLineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) spherical: f32,
    @location(1) cylindrical: f32,
    @location(2) color: vec4<f32>,
}
@vertex fn vs_debug_line(@builtin(vertex_index) vertex: u32,
    @location(0) start: vec3<f32>, @location(1) end: vec3<f32>,
    @location(2) argb: vec4<f32>, @location(3) width: f32) -> DebugLineOut {
    let position = select(start, end, vertex >= 2u);
    let view_shrink = 1.0 - 1.0 / 256.0;
    let line_start = globals.view_proj * vec4(position * view_shrink, 1.0);
    let line_end = globals.view_proj * vec4((position + (end - start)) * view_shrink, 1.0);
    let ndc1 = line_start.xyz / line_start.w;
    let ndc2 = line_end.xyz / line_end.w;
    let screen = globals.outline_params.xy;
    let direction = normalize((ndc2.xy - ndc1.xy) * screen);
    var offset = vec2(-direction.y, direction.x) * width / screen;
    if offset.x < 0.0 { offset = -offset; }
    if vertex % 2u != 0u { offset = -offset; }
    var out: DebugLineOut;
    out.clip = vec4((ndc1 + vec3(offset, 0.0)) * line_start.w, line_start.w);
    out.spherical = length(position);
    out.cylindrical = max(length(position.xz), abs(position.y));
    // ARGB in a little-endian u32 reads as B, G, R, A.
    out.color = argb.zyxw;
    return out;
}
@fragment fn fs_debug_line(in: DebugLineOut) -> @location(0) vec4<f32> {
    return apply_fog(in.color, in.spherical, in.cylindrical);
}
