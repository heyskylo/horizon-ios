//! The sun and moon blend as vanilla's `RenderPipelines.CELESTIAL` (`BlendFunction.OVERLAY`):
//! their colour is added to the sky's, and their own alpha replaces the buffer's. Iris 1.11.7
//! left the sun's alpha in a buffer the sky had written first, with a probe pack of ours
//! (#10490). Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

/// The sky writes alpha 1 into colortex1, then the sun (default blend) writes alpha 0.5 there;
/// final shows colortex1's alpha in red.
fn files() -> Vec<(String, String)> {
    let program = |alpha: &str| {
        format!(
            "#version 120\n/* DRAWBUFFERS:1 */\n\
             void main() {{ gl_FragData[0] = vec4(0.0, 0.0, 0.0, {alpha}); }}\n"
        )
    };
    let vertex = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";
    vec![
        ("gbuffers_skybasic.vsh".into(), vertex.into()),
        ("gbuffers_skybasic.fsh".into(), program("1.0")),
        ("gbuffers_skytextured.vsh".into(), vertex.into()),
        ("gbuffers_skytextured.fsh".into(), program("0.5")),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 120\nuniform sampler2D colortex1;\nvarying vec2 uv;\n\
             void main() { gl_FragColor = vec4(texture2D(colortex1, uv).a, 0.0, 0.0, 1.0); }\n"
                .into(),
        ),
    ]
}

/// A quad across the whole view, `z` blocks in front of the camera.
fn quad(z: f32) -> [crate::mesher::Vertex; 4] {
    [
        [-400.0, -400.0],
        [400.0, -400.0],
        [400.0, 400.0],
        [-400.0, 400.0],
    ]
    .map(|[x, y]| crate::mesher::Vertex {
        pos: [x, y, z],
        uv: [0.0; 2],
        layer: 0,
        color: u32::MAX,
        light: 0,
        normal: [0; 4],
        glint_uv: [0.0; 2],
    })
}

#[test]
fn the_sun_leaves_its_own_alpha_over_the_sky() {
    let Some(gpu) = Gpu::new() else { return };
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    // The world drew nothing: the far plane (reverse-Z) everywhere.
    gpu.clear(&color, &depth, 0.0);
    let sun = gpu.texture(T::Rgba8Unorm);
    let mut post = gpu.load(
        files(),
        World {
            atlas: None,
            material_maps: None,
            lightmap: color.clone(),
            terrain: None,
            block_ids: &[],
            sprite_rects: &[],
            celestial: Some(sun),
            gui_size: SIZE,
            clouds: None,
            weather: false,
            hand: false,
        },
        horizon_shaders::GameInputs {
            sky: Some(super::super::pack_sky::inputs()),
            celestial: Some(super::super::pack_sky::celestial_inputs()),
            ..Default::default()
        },
    );
    assert!(post.sky.is_some(), "{:?}", post.notes);
    let hud = HudBatch::new(SIZE);
    let mut frame = fixture_frame(&hud);
    frame.title = false;
    frame.skybox = true;
    let sun = quad(100.0);
    frame.celestial = &sun;
    let pixels = gpu.render(&mut post, &color, &depth, &frame, &mut |_, _, _, _| {});
    // The top four rows see the sky disc (sky_depth_tests); the sun covers the whole view.
    for (i, p) in pixels[..4 * SIZE[0] as usize].iter().enumerate() {
        assert!(
            p[0].abs_diff(128) <= 1,
            "pixel {i}: colortex1 alpha {} / 255, expected the sun's 0.5",
            p[0]
        );
    }
}
