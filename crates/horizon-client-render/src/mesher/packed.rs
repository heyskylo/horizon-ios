//! The compact layout of a section's solid quads in the arena: most of them are whole faces
//! of a block's cube on the block grid, and those take one 24-byte record each instead of
//! four 24-byte vertices. terrain.wgsl's `section_corner` expands a record into the very
//! vertex words `TerrainVertex` would hold (`packed_vertex` here is the same on the CPU, and
//! a record is kept only when it gives back the quad's four vertices bit for bit), so every
//! pass draws the same picture from either layout.
//!
//! A mesh's solid range is then one record per quad in mesh order (so `Sides` and the draws
//! count quads as before), padding to a multiple of 4 slots, and the quads no record fits as
//! their four vertices: their record holds the distance to those (`FULL`). A record:
//!
//! - word 0: the tint (0xRRGGBB as the mesher multiplies it) and the colour's alpha byte;
//! - word 1: each corner's grey level before the tint;
//! - word 2: the UV rectangle in 1/32 of the sprite (u0, u1, v0, v1, 6 bits each) and, per
//!   corner, which edge of it its u and v take;
//! - word 3: the layer, and corner 0's light (sky and block light over 4, 6 bits each);
//! - word 4: corners 1 and 2's light;
//! - word 5: corner 3's light, the face (block x, y, z, the axis it faces along, which side
//!   of the block, and where and which way its corners go round), and `FULL` clear.
use super::TerrainVertex;
use std::sync::atomic::{AtomicBool, Ordering};

/// A solid quad's tint the mesher does not know (fluids): only untinted colours pack.
pub const UNKNOWN_TINT: u32 = u32::MAX;
/// The tint of an untinted quad, whose colour is its grey level.
pub const WHITE: u32 = 0xFF_FFFF;
/// Word 5's bit of a quad kept as four vertices; word 0 is then the slots from the record
/// to its first vertex.
pub const FULL: u32 = 1 << 31;
/// The draws' base vertex of a compact mesh: `PACKED_BASE + 4 * slot + corner` is a corner of
/// the quad whose record is at `slot`; below it, vertex indices name vertices.
pub const PACKED_BASE: u32 = 1 << 28;

/// Whether meshes sorted from now on get the compact layout: set by the renderer while its
/// passes read it (no shader pack).
static PACK: AtomicBool = AtomicBool::new(false);

pub fn pack_quads(on: bool) {
    PACK.store(on, Ordering::Relaxed);
}

pub fn packing() -> bool {
    PACK.load(Ordering::Relaxed)
}

/// The order corners go round a face: (du, dw) of each step.
const ROUND: [[u32; 2]; 4] = [[0, 0], [1, 0], [1, 1], [0, 1]];

/// `quad`'s record, if one gives back its four vertices exactly. `tint`: what the mesher
/// multiplied its grey levels by (`UNKNOWN_TINT` when it does not say).
pub fn pack_quad(quad: &[TerrainVertex; 4], tint: u32) -> Option<[u32; 6]> {
    // Corners on the block grid of the section.
    let mut p = [[0u32; 3]; 4];
    for (to, v) in p.iter_mut().zip(quad) {
        for (c, &x) in to.iter_mut().zip(&v.pos) {
            if !(0.0..=16.0).contains(&x) || x.fract() != 0.0 {
                return None;
            }
            *c = x as u32;
        }
    }
    let axis = (0..3).find(|&a| p.iter().all(|c| c[a] == p[0][a]))?;
    let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
    let lo = [u, w].map(|k| p.iter().map(|c| c[k]).min().unwrap_or(0));
    if lo.iter().any(|&l| l > 15) {
        return None;
    }
    let mut steps = [0u32; 4];
    for (step, c) in steps.iter_mut().zip(&p) {
        let d = [c[u] - lo[0], c[w] - lo[1]];
        *step = ROUND.iter().position(|r| *r == d)? as u32;
    }
    let (start, dir) = (0..8u32).map(|n| (n & 3, n >> 2)).find(|&(start, dir)| {
        (0..4u32).all(|i| {
            let step = if dir == 1 { 4 - i } else { i };
            steps[i as usize] == (start + step) & 3
        })
    })?;
    let plane = p[0][axis];
    let mut block = [0u32; 3];
    block[axis] = plane.min(15);
    block[u] = lo[0];
    block[w] = lo[1];
    let side = plane - block[axis];
    let orient = block[0]
        | block[1] << 4
        | block[2] << 8
        | (axis as u32) << 12
        | side << 14
        | start << 15
        | dir << 17;
    // UVs in 1/32 of the sprite, at most two values each way.
    let mut ranges = [[u32::MAX, 0u32]; 2];
    let mut uvs = [[0u32; 2]; 4];
    for (uv, v) in uvs.iter_mut().zip(quad) {
        for k in 0..2 {
            let raw = u32::from(v.uv[k]);
            if raw & 1023 != 0 {
                return None;
            }
            uv[k] = raw >> 10;
            ranges[k] = [ranges[k][0].min(uv[k]), ranges[k][1].max(uv[k])];
        }
    }
    let mut select = 0u32;
    for (i, uv) in uvs.iter().enumerate() {
        for k in 0..2 {
            if uv[k] == ranges[k][1] && uv[k] != ranges[k][0] {
                select |= 1 << (2 * i + k);
            } else if uv[k] != ranges[k][0] {
                return None;
            }
        }
    }
    let rect = ranges[0][0] | ranges[0][1] << 6 | ranges[1][0] << 12 | ranges[1][1] << 18;
    // Light in steps of 4 (smooth light averages four levels of 16).
    let mut light = [0u32; 4];
    for (l, v) in light.iter_mut().zip(quad) {
        let (sky, block) = (u32::from(v.light & 0xFF), u32::from(v.light >> 8));
        if sky & 3 != 0 || block & 3 != 0 {
            return None;
        }
        *l = sky >> 2 | (block >> 2) << 6;
    }
    // The colour: each channel the grey level times the tint's, over 255.
    let tint = if tint == UNKNOWN_TINT {
        WHITE
    } else {
        tint & WHITE
    };
    let alpha = quad[0].color & 0xFF00_0000;
    let mut grays = 0u32;
    for (i, v) in quad.iter().enumerate() {
        grays |= gray_of(v.color, tint)? << (8 * i);
    }
    let words = [
        tint | alpha,
        grays,
        rect | select << 24,
        u32::from(quad[0].layer) | light[0] << 16,
        light[1] | light[2] << 12,
        light[3] | orient << 12,
    ];
    let exact = (0..4).all(|c| {
        let (a, b) = (packed_vertex(&words, c), quad[c]);
        bytemuck::bytes_of(&a) == bytemuck::bytes_of(&b)
    });
    exact.then_some(words)
}

/// The tint's channels, as the mesher takes them for red, green and blue.
fn channels(tint: u32) -> [u32; 3] {
    [tint >> 16 & 255, tint >> 8 & 255, tint & 255]
}

/// A grey level that `tint` turns into `color`'s red, green and blue.
fn gray_of(color: u32, tint: u32) -> Option<u32> {
    let want = [color & 255, color >> 8 & 255, color >> 16 & 255];
    let t = channels(tint);
    let gives = |g: u32| (0..3).all(|k| g * t[k] / 255 == want[k]);
    let k = (0..3).max_by_key(|&k| t[k]).unwrap_or(0);
    if t[k] == 0 {
        return gives(0).then_some(0);
    }
    let lo = (want[k] * 255).div_ceil(t[k]);
    let hi = ((want[k] * 255 + 254) / t[k]).min(255);
    (lo..=hi).find(|&g| gives(g))
}

/// Corner `corner` of the quad `words` holds, as terrain.wgsl's `packed_corner` expands it.
pub fn packed_vertex(words: &[u32; 6], corner: usize) -> TerrainVertex {
    let [w0, w1, w2, w3, w4, w5] = *words;
    let orient = w5 >> 12 & 0x3_FFFF;
    let block = [orient & 15, orient >> 4 & 15, orient >> 8 & 15];
    let axis = (orient >> 12 & 3) as usize;
    let side = orient >> 14 & 1;
    let (start, dir) = (orient >> 15 & 3, orient >> 17 & 1);
    let i = corner as u32;
    let step = (start + if dir == 1 { 4 - i } else { i }) & 3;
    let [du, dw] = ROUND[step as usize];
    let mut pos = block;
    pos[axis] += side;
    pos[(axis + 1) % 3] += du;
    pos[(axis + 2) % 3] += dw;
    let select = w2 >> 24;
    let us = select >> (2 * i) & 1;
    let vs = select >> (2 * i + 1) & 1;
    let uv = [w2 >> (6 * us) & 63, w2 >> (12 + 6 * vs) & 63].map(|k| (k << 10) as u16);
    let light = [w3 >> 16, w4, w4 >> 12, w5][corner] & 0xFFF;
    let light = ((light & 63) << 2 | (light >> 6 & 63) << 10) as u16;
    let g = w1 >> (8 * i) & 255;
    let [r, gr, b] = channels(w0).map(|t| g * t / 255);
    TerrainVertex {
        pos: pos.map(|c| c as f32),
        uv,
        layer: (w3 & 0xFFFF) as u16,
        light,
        color: r | gr << 8 | b << 16 | (w0 & 0xFF00_0000),
    }
}

/// The compact layout of sorted solid quads with their tints (`SectionMesh::packed`), or
/// nothing when it would be no smaller than their vertices.
pub fn layout(solid: &[TerrainVertex], tints: &[u32]) -> Vec<[u32; 6]> {
    let quads = solid.as_chunks::<4>().0;
    let records: Vec<Option<[u32; 6]>> = quads
        .iter()
        .zip(tints)
        .map(|(q, &t)| pack_quad(q, t))
        .collect();
    let n = quads.len();
    let packed = records.iter().filter(|r| r.is_some()).count();
    let full = n - packed;
    let pad = if full > 0 {
        n.next_multiple_of(4) - n
    } else {
        0
    };
    if n + pad + 4 * full >= 4 * n {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n + pad + 4 * full);
    let mut next = n + pad;
    for (i, record) in records.iter().enumerate() {
        out.push(record.unwrap_or_else(|| {
            let at = next;
            next += 4;
            [(at - i) as u32, 0, 0, 0, 0, FULL]
        }));
    }
    out.resize(n + pad, [0; 6]);
    for (q, record) in quads.iter().zip(&records) {
        if record.is_none() {
            out.extend(
                q.iter()
                    .map(|v| bytemuck::cast::<TerrainVertex, [u32; 6]>(*v)),
            );
        }
    }
    out
}

/// Quads `layout` gave a record of their own.
pub fn packed_quads(layout: &[[u32; 6]], quads: usize) -> usize {
    layout
        .iter()
        .take(quads)
        .filter(|w| w[5] & FULL == 0)
        .count()
}
