// GPU culling (gfx/gpu_culling.rs). `apply` writes the records that changed.
// `cull`: one invocation per section id keeps the sections whose column is
// within the render distance, that cave culling's graph reaches (occlusion.rs)
// and whose box is in the frustum, writes each one's origin and the runs of its
// solid quads that can face the eye, a draw each, and counts them in its page,
// per block of 64 ids. `scan`: one workgroup per page turns the blocks' counts
// into where each block's draws start, and the page's count. `scatter`: each
// block writes its sections' draws there, in id order and each section's in its
// runs' order, so a page's draws come in the same order every frame (atomics
// alone would order them as the GPU schedules, and where faces of two sections
// meet at the same depth the later draw keeps the pixel). Nothing here computes
// with floats: the origins are the CPU's bits, copied, the frustum test adds
// integers the CPU computed, and facing compares keys that order as the floats
// the CPU compares. `cull` leaves sections fading in or out to the CPU
// (`FADING`).

struct Params {
    // Ids to test, records `apply` writes, the arena pages the counts and
    // regions cover, and where the regions start in `tables`.
    ids: u32,
    updates: u32,
    pages: u32,
    regions: u32,
    // Bits 3j, 3j + 1 and 3j + 2: frustum plane j's x, y and z are at least 0.
    signs: u32,
    // The render distance in columns, and the camera's column.
    near: i32,
    cam_x: i32,
    cam_z: i32,
    // The tables' first column: x, 0, z, 0.
    first: vec4<i32>,
    // Their entries along x, y and z, and 0.
    cells: vec4<u32>,
    // Each frustum plane's w term, its margin in, then 0, 0.
    w: array<vec4<i32>, 2>,
    // Cave culling's graph (occlusion.rs `Reach`): from x, from z, radius, lowest
    // section y; its height in sections and the level's; REACH, SKY and EYE;
    // where the eye's keys start in `tables`.
    reach: vec4<i32>,
    reach_height: i32,
    reach_sections: i32,
    flags: u32,
    eyes: u32,
}

// gpu_culling.rs `Record`.
struct Section {
    // Section x, y (from the level's bottom), z, and its arena page (-1: a
    // free id).
    key: vec4<i32>,
    // The first vertex of its solid quads, its solid quads (where its last
    // bucket by facing side ends), its box (gpu_culling.rs `rounded_box`), then
    // its flags (`FADING`).
    a: vec4<u32>,
    // Where its first six buckets end (mesher/sides.rs `BUCKETS`): +X, +Z, the
    // other bucket, +Y, -Z, -X.
    ends: array<u32, 6>,
    // What its buckets facing +X, +Z, +Y, -Z, -X and -Y compare the eye's keys
    // with (gpu_culling.rs `Record::facing`).
    keys: array<u32, 6>,
}

// gpu_culling.rs `Update`: the record for id `id.x`.
struct Update {
    id: vec4<u32>,
    section: Section,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> sections: array<Section>;
// gpu_culling.rs `tables`: the origins' f32 bits along x, y and z (n of them);
// per frustum plane its terms for those origins, n each; per plane and axis its
// terms for the 32 box codes; with an eye (`EYE`), the keys of the eye less
// each origin, n of them; then each page's region of `draws` (its first draw
// and its size).
@group(0) @binding(2) var<storage, read> tables: array<u32>;
@group(0) @binding(3) var<storage, read> updates: array<Update>;
// Sections kept, draws, each page's draws, then per block of ids and page its
// draws (`cull`), turned into where they start (`scan`).
@group(0) @binding(4) var<storage, read_write> counts: array<atomic<u32>>;
// DrawIndexedIndirectArgs, five words each.
@group(0) @binding(5) var<storage, read_write> draws: array<u32>;
// Per id the instance it is drawn with: origin and UV step, as f32 bits.
@group(0) @binding(6) var<storage, read_write> origins: array<vec4<u32>>;
// Per id `RUN_WORDS`: its page << PAGE_SHIFT | its draws when it is drawn this
// frame, else 0; then each draw's first quad and quads.
@group(0) @binding(7) var<storage, read_write> runs: array<u32>;
// occlusion.rs `Reach::bits`: whether the search reached each cell of its box,
// a bit each (32 a word, lowest first).
@group(0) @binding(8) var<storage, read> reached_cells: array<u32>;

// gfx.rs `section_instance`'s w for a whole section: 1 / mesher::UV_STEPS.
const STEP_BITS: u32 = 0x38000000u;
// Workgroups along x when the ids need more than one row of them.
const ROW: u32 = 65535u;
// Ids per block.
const BLOCK: u32 = 64u;
// Counts before the pages'.
const STATS: u32 = 2u;
// gpu_culling.rs `rounded_box` for a box past what its codes hold: no plane
// rejects it.
const UNBOUNDED: u32 = 0x80000000u;
// In `params.flags`: cave culling's graph is in `params.reach`, and its search
// reached the box's top layer (the sky band).
const REACH: u32 = 1u;
const SKY: u32 = 2u;
// In a section's `a.w`: it fades in or out, and the CPU draws it with its fade.
const FADING: u32 = 1u;
// In `params.flags`: solid quads facing away from the eye are left out, its
// keys in `tables` from `params.eyes`.
const EYE: u32 = 4u;
// Words per id in `runs`, and where its page starts in the first, its draws
// (at most 4: gpu_culling.rs `RUNS`) below.
const RUN_WORDS: u32 = 9u;
const PAGE_SHIFT: u32 = 3u;

// The count of `block`'s draws in `page` (`cull`), then where they start (`scan`).
fn block_count(block: u32, page: u32) -> u32 {
    return STATS + params.pages + block * params.pages + page;
}

// `page`'s region of `draws`: its first draw and its size.
fn region(page: u32) -> vec2<u32> {
    let at = params.regions + 2u * page;
    return vec2<u32>(tables[at], tables[at + 1u]);
}

// streaming.rs `in_view_distance` around the camera's column. A gap at or past
// the distance cannot pass, so testing it first keeps the squares in range.
fn near(x: i32, z: i32) -> bool {
    let r = params.near;
    let gx = max(abs(x - params.cam_x) - 1, 0);
    let gz = max(abs(z - params.cam_z) - 1, 0);
    if gx >= r || gz >= r {
        return false;
    }
    return gx * gx + gz * gz < r * r;
}

// occlusion.rs `Occlusion::contains` (`Graph::contains`): with no graph, or with
// cave culling off, every section; in the searched box, whether the search
// reached it; above a clamped box, over its columns and within the level, the
// sky band (all of it or none); else none. `Reach::contains` on the CPU.
fn reached(key: vec3<i32>) -> bool {
    if (params.flags & REACH) == 0u {
        return true;
    }
    let r = params.reach.z;
    let dx = key.x - params.reach.x;
    let dz = key.z - params.reach.y;
    let dy = key.y - params.reach.w;
    if dx < -r || dx > r || dz < -r || dz > r || dy < 0 {
        return false;
    }
    if dy >= params.reach_height {
        return (params.flags & SKY) != 0u && key.y >= 0 && key.y <= params.reach_sections;
    }
    let i = u32(((dz + r) * (2 * r + 1) + dx + r) * params.reach_height + dy);
    return ((reached_cells[i >> 5u] >> (i & 31u)) & 1u) != 0u;
}

fn term(at: u32) -> i32 {
    return bitcast<i32>(tables[at]);
}

// gfx.rs `Frustum::intersects` for the box `b` of the section in cell `c` of the
// tables, in integers (gpu_culling.rs `tables`): it keeps every box the CPU keeps.
fn in_frustum(c: vec3<u32>, b: u32) -> bool {
    if (b & UNBOUNDED) != 0u {
        return true;
    }
    let nx = params.cells.x;
    let ny = params.cells.y;
    let n = nx + ny + params.cells.z;
    for (var j = 0u; j < 6u; j++) {
        // The box's codes at the corner the plane looks at.
        let s = params.signs >> (3u * j);
        let kx = (b >> select(0u, 15u, (s & 1u) != 0u)) & 31u;
        let ky = (b >> select(5u, 20u, (s & 2u) != 0u)) & 31u;
        let kz = (b >> select(10u, 25u, (s & 4u) != 0u)) & 31u;
        let origin_terms = n + j * n;
        let code_terms = 7u * n + 96u * j;
        let sum = params.w[j / 4u][j % 4u] + term(origin_terms + c.x)
            + term(origin_terms + nx + c.y) + term(origin_terms + nx + ny + c.z)
            + term(code_terms + kx) + term(code_terms + 32u + ky) + term(code_terms + 64u + kz);
        if sum < 0 {
            return false;
        }
    }
    return true;
}

// Where bucket `b` of section `id` ends (mesher/sides.rs `BUCKETS` order): its
// solid quads for the last.
fn bucket_end(id: u32, b: u32) -> u32 {
    if b == 6u {
        return sections[id].a.y;
    }
    return sections[id].ends[b];
}

// mesher/sides.rs `Sides::visible_from` for bucket `b` of section `id` (+X, +Z,
// the other bucket, +Y, -Z, -X, -Y), with `eye` the keys of the eye less its
// origin: whether its quads can face the eye. -Y compares the eye's own y, as a
// GPU frame has no water mirror. The keys order as the floats the CPU compares.
fn seen(id: u32, b: u32, eye: vec3<u32>) -> bool {
    if b == 2u {
        return true;
    }
    let t = sections[id].keys[b - select(0u, 1u, b > 2u)];
    switch b {
        case 0u: { return eye.x > t; }
        case 1u: { return eye.z > t; }
        case 3u: { return eye.y > t; }
        case 4u: { return eye.z < t; }
        case 5u: { return eye.x < t; }
        default: { return eye.y < t; }
    }
}

// One invocation per changed record; the CPU lists each id once.
@compute @workgroup_size(64)
fn apply(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < params.updates {
        let u = updates[gid.x];
        sections[u.id.x] = u.section;
    }
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) gid: vec3<u32>) {
    let id = gid.y * (ROW * BLOCK) + gid.x;
    // Past the table's end the records are stale: the CPU never writes them free.
    if id >= params.ids {
        return;
    }
    let at = id * RUN_WORDS;
    runs[at] = 0u;
    let s = sections[id];
    if s.key.w < 0 || s.a.y == 0u || (s.a.w & FADING) != 0u || !near(s.key.x, s.key.z)
        || !reached(s.key.xyz) {
        return;
    }
    let page = u32(s.key.w);
    // The tables hold every column within the distance; a key before their
    // first wraps past their end.
    let c = vec3<u32>(s.key.xyz - params.first.xyz);
    if page >= params.pages || any(c >= params.cells.xyz) || !in_frustum(c, s.a.z) {
        return;
    }
    // Kept, whether or not a quad of it can face the eye.
    atomicAdd(&counts[0], 1u);
    let nx = params.cells.x;
    let ny = params.cells.y;
    origins[id] = vec4<u32>(tables[c.x], tables[nx + c.y], tables[nx + ny + c.z], STEP_BITS);
    // gfx.rs `facing_runs`: the runs of its buckets that can face the eye, an
    // empty bucket breaking none, a draw each; without an eye, its solid quads.
    var n = 0u;
    if (params.flags & EYE) == 0u {
        runs[at + 1u] = 0u;
        runs[at + 2u] = s.a.y;
        n = 1u;
    } else {
        let e = params.eyes;
        let eye = vec3<u32>(tables[e + c.x], tables[e + nx + c.y], tables[e + nx + ny + c.z]);
        var start = 0u;
        var first = 0u;
        var open = false;
        for (var b = 0u; b < 7u; b++) {
            let end = bucket_end(id, b);
            if end == start {
                continue;
            }
            let shown = seen(id, b, eye);
            if shown && !open {
                open = true;
                first = start;
            } else if !shown && open {
                runs[at + 1u + 2u * n] = first;
                runs[at + 2u + 2u * n] = start - first;
                n += 1u;
                open = false;
            }
            start = end;
        }
        if open {
            runs[at + 1u + 2u * n] = first;
            runs[at + 2u + 2u * n] = start - first;
            n += 1u;
        }
    }
    if n == 0u {
        return;
    }
    runs[at] = (page << PAGE_SHIFT) | n;
    atomicAdd(&counts[block_count(id / BLOCK, page)], n);
}

var<workgroup> partial: array<u32, 256>;

// One workgroup per page: each lane sums a stretch of blocks, the lanes'
// sums are scanned, and each block's sum becomes where its draws start.
@compute @workgroup_size(256)
fn scan(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let page = group.x;
    let blocks = (params.ids + BLOCK - 1u) / BLOCK;
    let stretch = (blocks + 255u) / 256u;
    let first = min(lane * stretch, blocks);
    let last = min(first + stretch, blocks);
    var sum = 0u;
    for (var b = first; b < last; b++) {
        sum += atomicLoad(&counts[block_count(b, page)]);
    }
    partial[lane] = sum;
    workgroupBarrier();
    for (var step = 1u; step < 256u; step <<= 1u) {
        var before = 0u;
        if lane >= step {
            before = partial[lane - step];
        }
        workgroupBarrier();
        partial[lane] += before;
        workgroupBarrier();
    }
    var start = 0u;
    if lane > 0u {
        start = partial[lane - 1u];
    }
    for (var b = first; b < last; b++) {
        let count = atomicLoad(&counts[block_count(b, page)]);
        atomicStore(&counts[block_count(b, page)], start);
        start += count;
    }
    if lane == 255u {
        // A region has room for every draw of its page's records; were one
        // short, its draws past it go.
        let total = min(partial[255], region(page).y);
        atomicStore(&counts[STATS + page], total);
        atomicAdd(&counts[1], total);
    }
}

var<workgroup> held: array<u32, 64>;

// One workgroup per block of ids: each section's draws go after those of the
// block's earlier sections in its page, in its runs' order.
@compute @workgroup_size(64)
fn scatter(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let block = group.y * ROW + group.x;
    let id = block * BLOCK + lane;
    var mine = 0u;
    if id < params.ids {
        mine = runs[id * RUN_WORDS];
    }
    held[lane] = mine;
    workgroupBarrier();
    // Its draws below PAGE_SHIFT, its page above.
    let n = mine & 7u;
    if n == 0u {
        return;
    }
    let page = mine >> PAGE_SHIFT;
    var slot = atomicLoad(&counts[block_count(block, page)]);
    for (var other = 0u; other < lane; other++) {
        // A section not drawn holds 0: no draws.
        if (held[other] >> PAGE_SHIFT) == page {
            slot += held[other] & 7u;
        }
    }
    let r = region(page);
    let at = id * RUN_WORDS;
    let base = sections[id].a.x;
    for (var k = 0u; k < n; k++) {
        if slot + k >= r.y {
            return;
        }
        // A run of its solid quads, from its first vertex, with its origin.
        let d = (r.x + slot + k) * 5u;
        draws[d] = runs[at + 2u + 2u * k] * 6u;
        draws[d + 1u] = 1u;
        draws[d + 2u] = 0u;
        draws[d + 3u] = base + runs[at + 1u + 2u * k] * 4u;
        draws[d + 4u] = id;
    }
}
