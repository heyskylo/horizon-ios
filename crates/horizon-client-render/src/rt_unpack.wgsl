// Appended to terrain.wgsl (gfx/ray_query.rs's `Unpacker`): the solid quads of sections
// about to get a BLAS, written out as plain vertices (`TerrainVertex`, six words each), so
// ray tracing keeps the section arena in its compact layout. `section_corner` reads a corner
// exactly as the solid draws do, from a record or a vertex, out of the page bound at
// `arena_page`.
struct RtJob {
    // The draw vertex index of the section's first solid quad's first corner (`solid_base`).
    base: u32,
    // Its solid quads.
    quads: u32,
    // Where its first vertex goes in `rt_out`.
    out: u32,
    pad: u32,
}
@group(2) @binding(0) var<storage, read> rt_jobs: array<RtJob>;
@group(2) @binding(1) var<storage, read_write> rt_out: array<u32>;

// One invocation per vertex: x over a section's vertices, y over the page's sections.
@compute @workgroup_size(64) fn cs_rt_unpack(@builtin(global_invocation_id) id: vec3<u32>) {
    let job = rt_jobs[id.y];
    if id.x >= job.quads * 4u {
        return;
    }
    let c = section_corner(job.base + id.x).c;
    let o = (job.out + id.x) * 6u;
    rt_out[o] = c.p.x;
    rt_out[o + 1u] = c.p.y;
    rt_out[o + 2u] = c.p.z;
    rt_out[o + 3u] = c.r.x;
    rt_out[o + 4u] = c.r.y;
    rt_out[o + 5u] = c.r.z;
}
