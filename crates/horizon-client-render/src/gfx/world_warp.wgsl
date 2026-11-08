// A native mod's world warp (gfx/world_warp.rs, docs/native-mods.md "World warps"): what the
// game declares around the mod's WGSL in the warp's own copies of terrain.wgsl and entity.wgsl.
// None of it is in the shaders a frame without a warp draws with.

// What `world_warp` makes of a vertex: where to draw it, from the camera, and what to multiply
// its colour by (terrain only; red under 0 marks its faces cut: `Fragment.cut`).
struct Warped {
    pos: vec3<f32>,
    tint: vec3<f32>,
}
struct WorldWarpUniform {
    // `WorldWarp::params`.
    params: array<vec4<f32>, 4>,
    // xyz: the camera in the world as f32; `camera_rest.xyz` what f32 left out of it, so
    // `(p - warp.camera.xyz) - warp.camera_rest.xyz` is the world's point `p` from the camera,
    // exact far from the world's origin. w: the frame's partial tick, 0 to 1, for motion
    // smoother than the ticks `params` are set in.
    camera: vec4<f32>,
    camera_rest: vec4<f32>,
}
@group(2) @binding(0) var<uniform> warp: WorldWarpUniform;
