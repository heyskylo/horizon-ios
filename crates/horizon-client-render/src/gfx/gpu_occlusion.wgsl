// GPU culling's occlusion test (gpu_culling.rs `BoxTest`), after
// gpu_culling.wgsl in one module, made only where the depth test can be forced
// before the fragment shader (Vulkan, cull.rs `DepthTest::Early`): one instance
// per box `cull` listed, its section's box drawn front faces only against phase
// 1's depth; then `count2`, `scan2` and `scatter2` write phase 2's draws.
@group(0) @binding(9) var<storage, read> box_list: array<u32>;
@group(0) @binding(10) var<storage, read> box_origins: array<vec4<u32>>;
@group(0) @binding(11) var<storage, read_write> box_runs: array<u32>;
@group(0) @binding(12) var<storage, read> box_sections: array<Section>;

// The corners (bit 0: x, bit 1: y, bit 2: z) of the box's 12 triangles,
// counter-clockwise seen from outside (cull.wgsl's).
const CORNERS = array<u32, 36>(
    0u, 6u, 2u, 0u, 4u, 6u, 1u, 3u, 7u, 1u, 7u, 5u, 0u, 1u, 5u, 0u, 5u, 4u,
    2u, 7u, 3u, 2u, 6u, 7u, 0u, 3u, 1u, 0u, 2u, 3u, 4u, 5u, 7u, 4u, 7u, 6u,
);

struct BoxOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
}

@vertex
fn vs_box(@builtin(vertex_index) v: u32, @builtin(instance_index) t: u32) -> BoxOut {
    let id = box_list[params.occl.y + t];
    let o = bitcast<vec3<f32>>(box_origins[id].xyz);
    var out: BoxOut;
    out.id = id;
    out.position = params.view_proj * vec4(box_corner(o, box_sections[id].a.z, CORNERS[v]), 1.0);
    return out;
}

// Seen at this fragment, which runs only where the depth test passes (forced
// before it: a fragment that failed would still write). Every writer stores
// the same stamp.
@fragment
@early_depth_test(force)
fn fs_box(in: BoxOut) -> @location(0) vec4<f32> {
    let at = in.id * RUN_WORDS + SEEN_WORD;
    if box_runs[at] != params.occl.x {
        box_runs[at] = params.occl.x;
    }
    return vec4(0.0);
}

var<workgroup> tally: array<atomic<u32>, 3>;

// After the test, one workgroup per block of ids: the quads of the sections
// with runs, the tested boxes not seen and their quads, and per page each
// block's draws that phase 2 makes (`drawn_in`'s condition).
@compute @workgroup_size(64)
fn count2(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    if lane < 3u {
        atomicStore(&tally[lane], 0u);
    }
    workgroupBarrier();
    let block = group.y * ROW + group.x;
    let id = block * BLOCK + lane;
    if id < params.ids {
        let at = id * RUN_WORDS;
        let w = runs[at];
        let n = w & 7u;
        if n > 0u {
            var quads = 0u;
            for (var k = 0u; k < n; k++) {
                quads += runs[at + 2u + 2u * k];
            }
            atomicAdd(&tally[0], quads);
            let saw = runs[at + SEEN_WORD] == params.occl.x;
            if (w & TESTED) != 0u && !saw {
                atomicAdd(&tally[1], 1u);
                atomicAdd(&tally[2], quads);
            }
            if (w & PHASE2) != 0u && saw {
                atomicAdd(&counts[block_count(block, w >> PAGE_SHIFT, 1u)], n);
            }
        }
    }
    workgroupBarrier();
    if lane == 0u {
        atomicAdd(&counts[QUADS], atomicLoad(&tally[0]));
        atomicAdd(&counts[HIDDEN], atomicLoad(&tally[1]));
        atomicAdd(&counts[HIDDEN_QUADS], atomicLoad(&tally[2]));
    }
}

@compute @workgroup_size(256)
fn scan2(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    scan_page(group.x, lane, 1u);
}

@compute @workgroup_size(64)
fn scatter2(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    scatter_block(group.y * ROW + group.x, lane, 1u);
}
