//! A program writing a float `vec4` into an integer buffer draws: its other buffers get what it
//! wrote, and the integer buffer the floats' bits, as Iris 1.11.7 on NVIDIA 615.78.08 did with
//! a probe pack of ours (#10490). Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

#[test]
fn a_float_output_into_an_integer_buffer_writes_its_bits() {
    let Some(gpu) = Gpu::new() else { return };
    let files = vec![
        ("composite.vsh".into(), VERTEX.into()),
        (
            "composite.fsh".into(),
            "#version 130\n/*\nconst int colortex3Format = RGBA32UI;\n*/\n/* DRAWBUFFERS:03 */\n\
             void main() {\n\
             \x20   gl_FragData[0] = vec4(0.25, 0.5, 0.75, 1.0);\n\
             \x20   gl_FragData[1] = vec4(1.5, 2.0, 7.0, 0.25);\n}\n"
                .into(),
        ),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 130\nuniform sampler2D colortex0;\nuniform usampler2D colortex3;\n\
             varying vec2 uv;\n\
             void main() {\n\
             \x20   uvec4 q = texelFetch(colortex3, ivec2(gl_FragCoord.xy), 0);\n\
             \x20   bvec4 bits = equal(q, uvec4(0x3FC00000u, 0x40000000u, 0x40E00000u, 0x3E800000u));\n\
             \x20   gl_FragColor = uv.y > 0.5 ? texture2D(colortex0, uv)\n\
             \x20       : vec4(float(bits.x && bits.y), float(bits.z), float(bits.w), 1.0);\n}\n"
                .into(),
        ),
    ];
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    gpu.clear(&color, &depth, 0.0);
    let mut post = gpu.load(
        files,
        World {
            atlas: None,
            material_maps: None,
            lightmap: color.clone(),
            terrain: None,
            block_ids: &[],
            sprite_rects: &[],
            celestial: None,
            gui_size: SIZE,
            clouds: None,
            weather: false,
            hand: false,
        },
        horizon_shaders::GameInputs::default(),
    );
    let hud = HudBatch::new(SIZE);
    let mut frame = fixture_frame(&hud);
    frame.title = false;
    let pixels = gpu.render(&mut post, &color, &depth, &frame, &mut |_, _, _, _| {});
    let width = SIZE[0] as usize;
    let (top, bottom) = (pixels[0], pixels[(SIZE[1] as usize - 1) * width]);
    assert!(
        top[0].abs_diff(64) <= 1 && top[1].abs_diff(128) <= 1 && top[2].abs_diff(191) <= 1,
        "colortex0 {top:?}, notes {:?}",
        post.notes
    );
    assert_eq!(bottom, [255, 255, 255, 255], "colortex3's bits");
}
