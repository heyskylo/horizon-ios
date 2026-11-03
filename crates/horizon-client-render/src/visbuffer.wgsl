
// The visibility buffer for solid terrain (gfx/visbuffer.rs, `HORIZON_VISBUFFER=1`), after
// terrain.wgsl in one module. Pass 1 rasterises the flat solid draws writing depth and, per
// pixel, the quad it shows: its arena page and its quad in that page (`vis_id`). Pass 2, one draw for up to 16 pages, runs the shading of `fs_opaque`/`solid_color`
// once per pixel from that ID: it reads the quad's corners from its page as the vertex shaders
// do, runs `terrain_vertex` on the
// triangle the pixel falls in, interpolates its outputs with perspective-correct barycentrics
// and works out the screen gradients `dpdx`/`dpdy` would give, so the texel, mip level, light
// and fog come out as in the normal path.

// Bit 31: terrain is here; bits 22 to 30: the arena page; bits 0 to 21: the quad in the page
// (a page holds at most `arena::PAGE_BYTES` / 24 bytes = 2 796 202 quads, under 2^22).
fn vis_id(page: u32, vertex: u32) -> u32 {
    return 0x80000000u | ((page & 0x1ffu) << 22u) | (vertex >> 2u);
}

// `sprite_at` (terrain.wgsl's `sample_rgss` over `sample_nearest`), sampling only what its
// blends keep: where RGSS has not faded in (the texels span two pixels or more) the nearest
// texel alone, where it has fully, RGSS alone, and its upper mip level only where it blends one
// in. A blend of 0 or 1 gives exactly the one side, so the result is `sprite_at`'s; near
// faces take one tap instead of nine.
fn vis_sprite(tiled: vec2<f32>, layer: u32, du: vec2<f32>, dv: vec2<f32>) -> vec4<f32> {
    let uv = tile_uv(tiled);
    let tiled_face = any(tiled - uv >= vec2(0.5));
    let pixel = vec2(1.0 / globals.filtering.y);
    let texel_screen = sqrt(du * du + dv * dv);
    if u32(globals.filtering.x) != 1u {
        return sample_nearest(uv, layer, pixel, du, dv, texel_screen, tiled_face);
    }
    let max_texel = max(texel_screen.x, texel_screen.y);
    let min_pixel = min(pixel.x, pixel.y);
    let blend = smoothstep(min_pixel, min_pixel * 2.0, max_texel);
    var nearest = vec4(0.0);
    if blend < 1.0 {
        nearest = sample_nearest(uv, layer, pixel, du, dv, texel_screen, tiled_face);
    }
    if blend <= 0.0 {
        return nearest;
    }
    let du_len = length(du);
    let dv_len = length(dv);
    let effective = sqrt(min(du_len, dv_len) * max(du_len, dv_len));
    let exact = max(0.0, log2(effective / min_pixel));
    let low = floor(exact);
    let mip_blend = fract(exact);
    var offsets = array<vec2<f32>, 4>(
        vec2(0.125, 0.375), vec2(-0.125, -0.375), vec2(0.375, -0.125), vec2(-0.375, 0.125));
    var sum_low = vec4(0.0);
    for (var i = 0; i < 4; i++) {
        sum_low += sprite_level(uv + offsets[i] * pixel, layer, low);
    }
    var rgss = sum_low * 0.25;
    if mip_blend > 0.0 {
        var sum_high = vec4(0.0);
        for (var i = 0; i < 4; i++) {
            sum_high += sprite_level(uv + offsets[i] * pixel, layer, low + 1.0);
        }
        rgss = mix(rgss, sum_high * 0.25, mip_blend);
    }
    return mix(nearest, rgss, blend);
}

struct VisIdOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
}
// The quads with no cutout that are not fading (`opaque_quad`): no discard, so a tiled GPU
// keeps hiding what lies behind them. `page`: the draw's instance, its arena page
// (`Flat::draw_paged`).
@vertex fn vs_vis_opaque(
    @builtin(vertex_index) vertex: u32, @builtin(instance_index) page: u32,
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>
) -> VisIdOut {
    var out: VisIdOut;
    let origin = slot_origins[block_slots[vertex >> 8u]];
    if !opaque_quad(layer, origin.w) {
        out.clip = vec4(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    out.clip = fade_depth(globals.view_proj * vec4(position + origin.xyz, 1.0), origin.w);
    out.id = vis_id(page, vertex);
    return out;
}
@fragment fn fs_vis_id(in: VisIdOut) -> @location(0) u32 {
    return in.id;
}

struct VisCutoutOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) layer: u32,
    // The vertex colour's alpha (`terrain_vertex`'s `color.a`: the lightmap's).
    @location(3) alpha: f32,
    @location(4) @interpolate(flat) fade: u32,
    @location(5) local: vec3<f32>,
}
// The other quads: `solid_color`'s fade dither and alpha cutout, with no shading.
@vertex fn vs_vis_cutout(
    @builtin(vertex_index) vertex: u32, @builtin(instance_index) page: u32,
    @location(0) position: vec3<f32>, @location(1) uv: vec2<u32>,
    @location(2) layer: u32, @location(3) color: vec4<f32>, @location(4) light: vec2<u32>
) -> VisCutoutOut {
    var out: VisCutoutOut;
    let origin = slot_origins[block_slots[vertex >> 8u]];
    if opaque_quad(layer, origin.w) {
        out.clip = vec4(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let pos = position + origin.xyz;
    out.clip = fade_depth(globals.view_proj * vec4(pos, 1.0), origin.w);
    out.id = vis_id(page, vertex);
    out.uv = terrain_uv(uv, origin.w);
    out.layer = layer;
    let block = max(f32(light.y), dynamic_block_light(pos));
    out.alpha = sample_lightmap(f32(light.x) / 16.0, block / 16.0).a;
    out.fade = fade_code(origin.w);
    out.local = position;
    return out;
}
@fragment fn fs_vis_cutout(in: VisCutoutOut) -> @location(0) u32 {
    let dx = dpdx(in.local);
    let dy = dpdy(in.local);
    if faded(in.fade, in.local, dx, dy) { discard; }
    let alpha = vis_sprite(in.uv, in.layer & 0x7fffu, dpdx(in.uv), dpdy(in.uv)).a * in.alpha;
    if alpha < 0.5 && (in.layer & 0x8000u) == 0u { discard; }
    return in.id;
}

// Pass 2. Up to 16 arena pages (mesher.rs's TerrainVertex, 24 bytes: position, UV as two u16,
// layer and light as u16 each, RGBA8 colour), a quad's 96 bytes as six vec4s; at bindings 0 and
// 1 the block tables of all pages, page after page, `VIS_BLOCKS_PER_PAGE` entries each
// (`Visbuffer::resolve` copies them from the flat draws' own), and the origins by section id
// (terrain.wgsl's flat draws); pass 1's IDs.
override VIS_BLOCKS_PER_PAGE: u32 = 1u;
@group(1) @binding(2) var<storage, read> vis_page_0: array<vec4<u32>>;
@group(1) @binding(3) var<storage, read> vis_page_1: array<vec4<u32>>;
@group(1) @binding(4) var<storage, read> vis_page_2: array<vec4<u32>>;
@group(1) @binding(5) var<storage, read> vis_page_3: array<vec4<u32>>;
@group(1) @binding(6) var<storage, read> vis_page_4: array<vec4<u32>>;
@group(1) @binding(7) var<storage, read> vis_page_5: array<vec4<u32>>;
@group(1) @binding(8) var<storage, read> vis_page_6: array<vec4<u32>>;
@group(1) @binding(9) var<storage, read> vis_page_7: array<vec4<u32>>;
@group(1) @binding(10) var<storage, read> vis_page_8: array<vec4<u32>>;
@group(1) @binding(11) var<storage, read> vis_page_9: array<vec4<u32>>;
@group(1) @binding(12) var<storage, read> vis_page_10: array<vec4<u32>>;
@group(1) @binding(13) var<storage, read> vis_page_11: array<vec4<u32>>;
@group(1) @binding(14) var<storage, read> vis_page_12: array<vec4<u32>>;
@group(1) @binding(15) var<storage, read> vis_page_13: array<vec4<u32>>;
@group(1) @binding(16) var<storage, read> vis_page_14: array<vec4<u32>>;
@group(1) @binding(17) var<storage, read> vis_page_15: array<vec4<u32>>;
@group(1) @binding(18) var vis_ids: texture_2d<u32>;

// A quad's 96 bytes, six 16-byte words from `i` on, of the page in `slot`: one switch for
// all of them, so a thread group whose pixels show several pages walks its cases once.
struct VisQuad {
    q: array<vec4<u32>, 6>,
}
fn vis_quad(slot: u32, i: u32) -> VisQuad {
    switch slot {
        case 0u: {
            return VisQuad(array(vis_page_0[i], vis_page_0[i + 1u], vis_page_0[i + 2u],
                vis_page_0[i + 3u], vis_page_0[i + 4u], vis_page_0[i + 5u]));
        }
        case 1u: {
            return VisQuad(array(vis_page_1[i], vis_page_1[i + 1u], vis_page_1[i + 2u],
                vis_page_1[i + 3u], vis_page_1[i + 4u], vis_page_1[i + 5u]));
        }
        case 2u: {
            return VisQuad(array(vis_page_2[i], vis_page_2[i + 1u], vis_page_2[i + 2u],
                vis_page_2[i + 3u], vis_page_2[i + 4u], vis_page_2[i + 5u]));
        }
        case 3u: {
            return VisQuad(array(vis_page_3[i], vis_page_3[i + 1u], vis_page_3[i + 2u],
                vis_page_3[i + 3u], vis_page_3[i + 4u], vis_page_3[i + 5u]));
        }
        case 4u: {
            return VisQuad(array(vis_page_4[i], vis_page_4[i + 1u], vis_page_4[i + 2u],
                vis_page_4[i + 3u], vis_page_4[i + 4u], vis_page_4[i + 5u]));
        }
        case 5u: {
            return VisQuad(array(vis_page_5[i], vis_page_5[i + 1u], vis_page_5[i + 2u],
                vis_page_5[i + 3u], vis_page_5[i + 4u], vis_page_5[i + 5u]));
        }
        case 6u: {
            return VisQuad(array(vis_page_6[i], vis_page_6[i + 1u], vis_page_6[i + 2u],
                vis_page_6[i + 3u], vis_page_6[i + 4u], vis_page_6[i + 5u]));
        }
        case 7u: {
            return VisQuad(array(vis_page_7[i], vis_page_7[i + 1u], vis_page_7[i + 2u],
                vis_page_7[i + 3u], vis_page_7[i + 4u], vis_page_7[i + 5u]));
        }
        case 8u: {
            return VisQuad(array(vis_page_8[i], vis_page_8[i + 1u], vis_page_8[i + 2u],
                vis_page_8[i + 3u], vis_page_8[i + 4u], vis_page_8[i + 5u]));
        }
        case 9u: {
            return VisQuad(array(vis_page_9[i], vis_page_9[i + 1u], vis_page_9[i + 2u],
                vis_page_9[i + 3u], vis_page_9[i + 4u], vis_page_9[i + 5u]));
        }
        case 10u: {
            return VisQuad(array(vis_page_10[i], vis_page_10[i + 1u], vis_page_10[i + 2u],
                vis_page_10[i + 3u], vis_page_10[i + 4u], vis_page_10[i + 5u]));
        }
        case 11u: {
            return VisQuad(array(vis_page_11[i], vis_page_11[i + 1u], vis_page_11[i + 2u],
                vis_page_11[i + 3u], vis_page_11[i + 4u], vis_page_11[i + 5u]));
        }
        case 12u: {
            return VisQuad(array(vis_page_12[i], vis_page_12[i + 1u], vis_page_12[i + 2u],
                vis_page_12[i + 3u], vis_page_12[i + 4u], vis_page_12[i + 5u]));
        }
        case 13u: {
            return VisQuad(array(vis_page_13[i], vis_page_13[i + 1u], vis_page_13[i + 2u],
                vis_page_13[i + 3u], vis_page_13[i + 4u], vis_page_13[i + 5u]));
        }
        case 14u: {
            return VisQuad(array(vis_page_14[i], vis_page_14[i + 1u], vis_page_14[i + 2u],
                vis_page_14[i + 3u], vis_page_14[i + 4u], vis_page_14[i + 5u]));
        }
        default: {
            return VisQuad(array(vis_page_15[i], vis_page_15[i + 1u], vis_page_15[i + 2u],
                vis_page_15[i + 3u], vis_page_15[i + 4u], vis_page_15[i + 5u]));
        }
    }
}

struct VisResolveOut {
    @builtin(position) clip: vec4<f32>,
    // The group of 16 pages this draw resolves: its instance (`Visbuffer::resolve`).
    @location(0) @interpolate(flat) chunk: u32,
}
@vertex fn vs_vis_resolve(@builtin(vertex_index) i: u32,
                          @builtin(instance_index) chunk: u32) -> VisResolveOut {
    let p = vec2(f32(i32(i & 1u) * 4 - 1), f32(i32(i >> 1u) * 4 - 1));
    var out: VisResolveOut;
    out.clip = vec4(p, 0.0, 1.0);
    out.chunk = chunk;
    return out;
}

// A framebuffer position's NDC (the world pass draws to the whole target).
fn vis_ndc(at: vec2<f32>, size: vec2<f32>) -> vec2<f32> {
    return vec2(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
}

// Perspective-correct barycentrics of the point at NDC `p` on the triangle with clip-space
// corners a, b, c (x, y, w): homogeneous 2D rasterisation (Olano and Greer). With M the matrix
// of the corners as columns, the weights are M^-1 (p, 1) normalised, and the rows of M^-1 are
// the corners' cross products over the determinant, which the normalisation cancels. Corners
// behind the camera need no clipping. The rows are given (`vis_rows`), as three points share
// them.
fn vis_bary(rows: mat3x3<f32>, p: vec2<f32>) -> vec3<f32> {
    let l = vec3(p, 1.0) * rows;
    let sum = l.x + l.y + l.z;
    if abs(sum) < 1e-30 {
        return vec3(-1.0);
    }
    return l / sum;
}
fn vis_rows(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> mat3x3<f32> {
    return mat3x3(cross(b, c), cross(c, a), cross(a, b));
}

// A corner's `terrain_vertex` from its six words: position, UV, layer and light, colour (the
// vertex attributes `TERRAIN_ATTRIBUTES` in gfx.rs give: Float32x3, Uint16x2, Uint16, Uint8x2
// with sky in the low byte, Unorm8x4).
fn vis_corner(position: vec3<u32>, rest: vec3<u32>, origin: vec4<f32>) -> TerrainOut {
    let uv = vec2(rest.x & 0xffffu, rest.x >> 16u);
    let light = vec2((rest.y >> 16u) & 0xffu, rest.y >> 24u);
    return terrain_vertex(bitcast<vec3<f32>>(position), terrain_uv(uv, origin.w),
        rest.y & 0xffffu, unpack4x8unorm(rest.z), light, origin.xyz, origin.w);
}

@fragment fn fs_vis_resolve(in: VisResolveOut) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(in.clip.xy));
    let id = textureLoad(vis_ids, pixel, 0).x;
    let page = (id >> 22u) & 0x1ffu;
    if (id & 0x80000000u) == 0u || page / 16u != in.chunk {
        discard;
    }
    let slot = page % 16u;
    let quad = id & 0x3fffffu;
    let at = 6u * quad;
    // The quad's block (256 vertices, 64 quads) names its section, as for `vs_terrain_flat`.
    let origin = slot_origins[block_slots[page * VIS_BLOCKS_PER_PAGE + (quad >> 6u)]];
    // Corner k is words 6k to 6k + 5 of the quad's 24.
    let words = vis_quad(slot, at).q;
    let q0 = words[0];
    let q1 = words[1];
    let q2 = words[2];
    let q3 = words[3];
    let q4 = words[4];
    let q5 = words[5];
    let p0 = q0.xyz;
    let r0 = vec3(q0.w, q1.x, q1.y);
    let p1 = vec3(q1.z, q1.w, q2.x);
    let r1 = q2.yzw;
    let p2 = q3.xyz;
    let r2 = vec3(q3.w, q4.x, q4.y);
    let p3 = vec3(q4.z, q4.w, q5.x);
    let r3 = q5.yzw;
    let c0 = (globals.view_proj * vec4(bitcast<vec3<f32>>(p0) + origin.xyz, 1.0)).xyw;
    let c1 = (globals.view_proj * vec4(bitcast<vec3<f32>>(p1) + origin.xyz, 1.0)).xyw;
    let c2 = (globals.view_proj * vec4(bitcast<vec3<f32>>(p2) + origin.xyz, 1.0)).xyw;
    let c3 = (globals.view_proj * vec4(bitcast<vec3<f32>>(p3) + origin.xyz, 1.0)).xyw;
    // The quad's triangles are 0 1 2 and 0 2 3 (`Renderer::quad_indices`, flatten.wgsl),
    // either side of the diagonal 0 2. Corner 1's weight in 0 1 2 is `vis_bary`'s row
    // cross(c2, c0) over the sum of the rows, whose sign is that of the triangle's determinant
    // (the sum is one over the point's w), dot(c1, cross(c2, c0)): the pixel lies in 0 1 2 when
    // the two agree. A degenerate 0 1 2 (no area, no pixels) leaves it to 0 2 3.
    let size = vec2<f32>(textureDimensions(vis_ids));
    let centre = in.clip.xy;
    let p = vis_ndc(centre, size);
    let diagonal = cross(c2, c0);
    let det = dot(c1, diagonal);
    let second = dot(vec3(p, 1.0), diagonal) * det < 0.0 || det == 0.0;
    let rows = vis_rows(c0, select(c1, c2, second), select(c2, c3, second));
    let b = vis_bary(rows, p);
    // dpdxFine and dpdyFine: the difference to the other pixel of this one's 2x2 quad in its
    // row and column, odd minus even, over the same triangle's plane.
    let odd = vec2<f32>(pixel & vec2(1));
    let toward = vec2(1.0) - 2.0 * odd;
    let bx = vis_bary(rows, vis_ndc(centre + vec2(toward.x, 0.0), size));
    let by = vis_bary(rows, vis_ndc(centre + vec2(0.0, toward.y), size));
    // `terrain_vertex` on the triangle's corners; the first is the provoking one, whose flat
    // outputs (layer, emission, fade, emissive layer) the rasteriser keeps.
    let t0 = vis_corner(p0, r0, origin);
    let t1 = vis_corner(select(p1, p2, second), select(r1, r2, second), origin);
    let t2 = vis_corner(select(p2, p3, second), select(r2, r3, second), origin);
    var t = t0;
    t.uv = b.x * t0.uv + b.y * t1.uv + b.z * t2.uv;
    t.color = b.x * t0.color + b.y * t1.color + b.z * t2.color;
    t.spherical = b.x * t0.spherical + b.y * t1.spherical + b.z * t2.spherical;
    t.cylindrical = b.x * t0.cylindrical + b.y * t1.cylindrical + b.z * t2.cylindrical;
    t.scene = b.x * t0.scene + b.y * t1.scene + b.z * t2.scene;
    t.local = b.x * t0.local + b.y * t1.local + b.z * t2.local;
    t.visibility = b.x * t0.visibility + b.y * t1.visibility + b.z * t2.visibility;
    let du = (bx.x * t0.uv + bx.y * t1.uv + bx.z * t2.uv - t.uv) * toward.x;
    let dv = (by.x * t0.uv + by.y * t1.uv + by.z * t2.uv - t.uv) * toward.y;
    let dlx = (bx.x * t0.local + bx.y * t1.local + bx.z * t2.local - t.local) * toward.x;
    let dly = (by.x * t0.local + by.y * t1.local + by.z * t2.local - t.local) * toward.y;
    // `fs_opaque` and `solid_color` past their discards, which pass 1 already made.
    hdr_normal = normalize(cross(dly, dlx));
    let texel = vis_sprite(t.uv, t.layer & 0x7fffu, du, dv);
    var color = texel * t.color;
    color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, color.a), color, t.visibility);
    return solid_finish_at(t, texel, color, du, dv);
}
