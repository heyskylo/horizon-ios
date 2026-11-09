//! The sky disc draws unblended, as vanilla draws it (`RenderPipelines.SKY`): what
//! `gbuffers_skybasic` writes there stays, alpha included. Iris 1.11.7 left exactly that with
//! probe packs of ours (#10490). Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

/// `gbuffers_skybasic` writes (0.2, 0.4, 0.6, 0.25) into colortex1, with `blend` as the
/// properties line; final shows colortex1's colour on the top half and its alpha on the bottom
/// half, each seeing the top of the view.
fn files(blend: &str) -> Vec<(String, String)> {
    vec![
        ("shaders.properties".into(), blend.into()),
        (
            "gbuffers_skybasic.vsh".into(),
            "#version 120\nvoid main() { gl_Position = ftransform(); }\n".into(),
        ),
        (
            "gbuffers_skybasic.fsh".into(),
            "#version 120\n/* DRAWBUFFERS:1 */\n\
             void main() { gl_FragData[0] = vec4(0.2, 0.4, 0.6, 0.25); }\n"
                .into(),
        ),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 120\nuniform sampler2D colortex1;\nvarying vec2 uv;\n\
             void main() {\n\
             \x20   vec4 c = texture2D(colortex1, vec2(uv.x, uv.y > 0.5 ? uv.y : uv.y + 0.5));\n\
             \x20   gl_FragColor = uv.y > 0.5 ? vec4(c.rgb, 1.0) : vec4(c.a, c.a, c.a, 1.0);\n}\n"
                .into(),
        ),
    ]
}

/// The pack's picture of a frame whose world drew nothing.
fn picture(gpu: &Gpu, blend: &str) -> Vec<[u8; 4]> {
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    // The far plane everywhere (reverse-Z).
    gpu.clear(&color, &depth, 0.0);
    let mut post = gpu.load(
        files(blend),
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
        horizon_shaders::GameInputs {
            sky: Some(super::super::pack_sky::inputs()),
            ..Default::default()
        },
    );
    assert!(post.sky.is_some(), "{:?}", post.notes);
    let hud = HudBatch::new(SIZE);
    let mut frame = fixture_frame(&hud);
    frame.title = false;
    frame.skybox = true;
    gpu.render(&mut post, &color, &depth, &frame, &mut |_, _, _, _| {})
}

/// The top four rows see the sky disc (sky_depth_tests); final repeats them in rows 8 to 11.
#[test]
fn the_sky_disc_keeps_what_the_program_wrote() {
    let Some(gpu) = Gpu::new() else { return };
    let width = SIZE[0] as usize;
    let pixels = picture(&gpu, "");
    for (i, p) in pixels[..4 * width].iter().enumerate() {
        let near = |got: u8, want: u8| got.abs_diff(want) <= 1;
        assert!(
            near(p[0], 51) && near(p[1], 102) && near(p[2], 153),
            "pixel {i}: colour {p:?}, expected (0.2, 0.4, 0.6)"
        );
    }
    for (i, p) in pixels[8 * width..12 * width].iter().enumerate() {
        assert!(p[0].abs_diff(64) <= 1, "pixel {i}: alpha {} / 255", p[0]);
    }
    // A pack's own `blend.gbuffers_skybasic` still blends the disc: alpha over the cleared 1.
    let blended = picture(
        &gpu,
        "blend.gbuffers_skybasic = SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ZERO\n",
    );
    assert!(blended[0][0].abs_diff(51) > 8, "{:?}", blended[0]);
    // So does a `blend.gbuffers_skybasic.colortex1` of its own, into that buffer.
    let own = picture(
        &gpu,
        "blend.gbuffers_skybasic.colortex1 = SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ZERO\n",
    );
    assert!(own[0][0].abs_diff(51) > 8, "{:?}", own[0]);
}
