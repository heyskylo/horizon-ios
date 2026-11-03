
// The visibility buffer for solid terrain (gfx/visbuffer.rs, `HORIZON_VISBUFFER=1`), after
// terrain.wgsl in one module. Pass 1 rasterises the flat solid draws writing depth and, per
// pixel, the quad it shows: its arena page and its quad in that page (`vis_id`). Pass 2, once
// a page, runs the shading of `fs_opaque`/`solid_color` once per pixel from that ID: it reads
// the quad's corners from the arena as the vertex shaders do, runs `terrain_vertex` on the
// triangle the pixel falls in, interpolates its outputs with perspective-correct barycentrics
// and works out the screen gradients `dpdx`/`dpdy` would give, so the texel, mip level, light
// and fog come out as in the normal path.

// Bit 31: terrain is here; bits 22 to 30: the arena page; bits 0 to 21: the quad in the page
// (a page holds at most `arena::PAGE_BYTES` / 24 bytes = 2 796 202 quads, under 2^22).
fn vis_id(page: u32, vertex: u32) -> u32 {
    return 0x80000000u | ((page & 0x1ffu) << 22u) | (vertex >> 2u);
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
    let alpha = sprite(in.uv, in.layer & 0x7fffu).a * in.alpha;
    if alpha < 0.5 && (in.layer & 0x8000u) == 0u { discard; }
    return in.id;
}

// Pass 2. The page's vertices (mesher.rs's TerrainVertex, 24 bytes: position, UV as two u16,
// layer and light as u16 each, RGBA8 colour) and pass 1's IDs, beside the page's block table
// and the origins by section id at bindings 0 and 1 (terrain.wgsl's flat draws).
@group(1) @binding(2) var<storage, read> vis_vertices: array<u32>;
@group(1) @binding(3) var vis_ids: texture_2d<u32>;

struct VisResolveOut {
    @builtin(position) clip: vec4<f32>,
    // The arena page this draw resolves: its instance (`Visbuffer::resolve`).
    @location(0) @interpolate(flat) page: u32,
}
@vertex fn vs_vis_resolve(@builtin(vertex_index) i: u32,
                          @builtin(instance_index) page: u32) -> VisResolveOut {
    let p = vec2(f32(i32(i & 1u) * 4 - 1), f32(i32(i >> 1u) * 4 - 1));
    var out: VisResolveOut;
    out.clip = vec4(p, 0.0, 1.0);
    out.page = page;
    return out;
}

struct VisVertex {
    position: vec3<f32>,
    uv: vec2<u32>,
    layer: u32,
    light: vec2<u32>,
    color: vec4<f32>,
}
// The vertex attributes `TERRAIN_ATTRIBUTES` (gfx.rs) give: Float32x3, Uint16x2, Uint16,
// Uint8x2 (sky in the low byte), Unorm8x4.
fn vis_vertex(v: u32) -> VisVertex {
    let b = v * 6u;
    var out: VisVertex;
    out.position = bitcast<vec3<f32>>(vec3(vis_vertices[b], vis_vertices[b + 1u],
        vis_vertices[b + 2u]));
    let uv = vis_vertices[b + 3u];
    out.uv = vec2(uv & 0xffffu, uv >> 16u);
    let packed = vis_vertices[b + 4u];
    out.layer = packed & 0xffffu;
    out.light = vec2((packed >> 16u) & 0xffu, packed >> 24u);
    out.color = unpack4x8unorm(vis_vertices[b + 5u]);
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
// behind the camera need no clipping.
fn vis_bary(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, p: vec2<f32>) -> vec3<f32> {
    let q = vec3(p, 1.0);
    let l = vec3(dot(cross(b, c), q), dot(cross(c, a), q), dot(cross(a, b), q));
    let sum = l.x + l.y + l.z;
    if abs(sum) < 1e-30 {
        return vec3(-1.0);
    }
    return l / sum;
}

fn vis_corner(v: VisVertex, origin: vec4<f32>) -> TerrainOut {
    return terrain_vertex(v.position, terrain_uv(v.uv, origin.w), v.layer, v.color, v.light,
        origin.xyz, origin.w);
}

@fragment fn fs_vis_resolve(in: VisResolveOut) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(in.clip.xy));
    let id = textureLoad(vis_ids, pixel, 0).x;
    if (id & 0x80000000u) == 0u || ((id >> 22u) & 0x1ffu) != in.page {
        discard;
    }
    let base = (id & 0x3fffffu) * 4u;
    let origin = slot_origins[block_slots[base >> 8u]];
    var corners: array<VisVertex, 4>;
    var clip: array<vec3<f32>, 4>;
    for (var k = 0u; k < 4u; k++) {
        corners[k] = vis_vertex(base + k);
        let c = globals.view_proj * vec4(corners[k].position + origin.xyz, 1.0);
        clip[k] = c.xyw;
    }
    // The quad's triangles are 0 1 2 and 0 2 3 (`Renderer::quad_indices`, flatten.wgsl): the
    // pixel lies in the one whose smallest weight is larger (edges round either way).
    let size = vec2<f32>(textureDimensions(vis_ids));
    let at = in.clip.xy;
    let p = vis_ndc(at, size);
    let ba = vis_bary(clip[0], clip[1], clip[2], p);
    let bb = vis_bary(clip[0], clip[2], clip[3], p);
    let second = min(bb.x, min(bb.y, bb.z)) > min(ba.x, min(ba.y, ba.z));
    let i1 = select(1u, 2u, second);
    let i2 = select(2u, 3u, second);
    let b = select(ba, bb, second);
    // dpdxFine and dpdyFine: the difference to the other pixel of this one's 2x2 quad in its
    // row and column, odd minus even, over the same triangle's plane.
    let odd = vec2<f32>(pixel & vec2(1));
    let toward = vec2(1.0) - 2.0 * odd;
    let bx = vis_bary(clip[0], clip[i1], clip[i2], vis_ndc(at + vec2(toward.x, 0.0), size));
    let by = vis_bary(clip[0], clip[i1], clip[i2], vis_ndc(at + vec2(0.0, toward.y), size));
    // `terrain_vertex` on the triangle's corners; the first is the provoking one, whose flat
    // outputs (layer, emission, fade, emissive layer) the rasteriser keeps.
    let t0 = vis_corner(corners[0], origin);
    let t1 = vis_corner(corners[i1], origin);
    let t2 = vis_corner(corners[i2], origin);
    var t = t0;
    t.uv = b.x * t0.uv + b.y * t1.uv + b.z * t2.uv;
    t.color = b.x * t0.color + b.y * t1.color + b.z * t2.color;
    t.spherical = b.x * t0.spherical + b.y * t1.spherical + b.z * t2.spherical;
    t.cylindrical = b.x * t0.cylindrical + b.y * t1.cylindrical + b.z * t2.cylindrical;
    t.scene = b.x * t0.scene + b.y * t1.scene + b.z * t2.scene;
    t.local = b.x * t0.local + b.y * t1.local + b.z * t2.local;
    t.visibility = b.x * t0.visibility + b.y * t1.visibility + b.z * t2.visibility;
    t.height = b.x * t0.height + b.y * t1.height + b.z * t2.height;
    let du = (bx.x * t0.uv + bx.y * t1.uv + bx.z * t2.uv - t.uv) * toward.x;
    let dv = (by.x * t0.uv + by.y * t1.uv + by.z * t2.uv - t.uv) * toward.y;
    let dlx = (bx.x * t0.local + bx.y * t1.local + bx.z * t2.local - t.local) * toward.x;
    let dly = (by.x * t0.local + by.y * t1.local + by.z * t2.local - t.local) * toward.y;
    // `fs_opaque` and `solid_color` past their discards, which pass 1 already made.
    hdr_normal = normalize(cross(dly, dlx));
    let texel = sprite_at(t.uv, t.layer & 0x7fffu, du, dv);
    var color = texel * t.color;
    color = mix(globals.fog_color * vec4(1.0, 1.0, 1.0, color.a), color, t.visibility);
    return solid_finish_at(t, texel, color, du, dv);
}
