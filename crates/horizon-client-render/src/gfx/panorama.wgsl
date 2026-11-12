// CubeMap's position-only cube and panorama.vsh/fsh: sample its untransformed direction.
override HDR_WRITE_LIGHT: bool = false;
struct Projection { view_proj: mat4x4<f32> }
@group(0) @binding(0) var<uniform> projection: Projection;
@group(0) @binding(1) var panorama: texture_cube<f32>;
@group(0) @binding(2) var panorama_sampler: sampler;
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) direction: vec3<f32>,
}
// The cube's corner of each of its 36 vertices (CubeMap's indices): 4 7 6, 4 6 5, 5 6 2,
// 5 2 1, 1 2 3, 1 3 0, 0 3 7, 0 7 4, 7 3 2, 7 2 6, 0 4 5, 0 5 1.
// Packed four bits each, eight a word, read through a switch: a constant array indexed by the
// vertex would be a private array, which Adreno's Vulkan compiler (build 0676.41) fails to link.
fn cube_corner(v: u32) -> u32 {
    var word: u32;
    switch v / 8u {
        case 0u: { word = 0x65564674u; }
        case 1u: { word = 0x13211252u; }
        case 2u: { word = 0x47073003u; }
        case 3u: { word = 0x40627237u; }
        default: { word = 0x00001505u; }
    }
    return (word >> (v % 8u * 4u)) & 0xfu;
}
@vertex fn vs_panorama(@builtin(vertex_index) index: u32) -> Out {
    // CubeMap's corners: -1 or 1 on each axis, x 1 at corners 1, 2, 5 and 6, y at 2, 3, 6
    // and 7, z at 4 to 7.
    let c = cube_corner(index);
    let high = vec3((c ^ (c >> 1u)) & 1u, c & 2u, c & 4u) != vec3(0u);
    let position = select(vec3(-1.0), vec3(1.0), high);
    var out: Out;
    out.clip = projection.view_proj * vec4(position, 1.0);
    out.direction = position;
    return out;
}
@fragment fn fs_panorama(in: Out) -> @location(0) vec4<f32> {
    let color = textureSample(panorama, panorama_sampler, in.direction);
    return vec4(color.rgb, select(color.a, 1.0, HDR_WRITE_LIGHT));
}
