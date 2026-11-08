//! `noiseTextureResolution` sizes the `noisetex` Horizon makes, as Iris 1.11.7 did with probe packs
//! of ours (#10490): a declaration in a fragment shader counts, one in a vertex shader doesn't.
//! Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

/// The width of `noisetex` that `final` sees, with these lines added to `final.vsh` and `final.fsh`.
fn noise_width(vertex_extra: &str, fragment_extra: &str) -> u8 {
    let Some(gpu) = Gpu::new() else { return 0 };
    let vertex = VERTEX.replacen('\n', &format!("\n{vertex_extra}"), 1);
    let files = vec![
        ("final.vsh".into(), vertex),
        (
            "final.fsh".into(),
            format!(
                "#version 130\n{fragment_extra}uniform sampler2D noisetex;\nvarying vec2 uv;\n\
                 void main() {{ gl_FragColor = vec4(float(textureSize(noisetex, 0).x) / 255.0, 0.0, 0.0, 1.0); }}\n"
            ),
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
    assert!(post.notes.is_empty(), "{:?}", post.notes);
    pixels[0][0]
}

#[test]
fn a_fragment_shader_sets_the_noise_resolution() {
    if Gpu::new().is_none() {
        return;
    }
    let declared = "#define NOISE 64\nconst int noiseTextureResolution = NOISE;\n";
    assert_eq!(noise_width("", declared), 64);
    // Not from a vertex shader: the default 256, read as 255 (1.0).
    assert_eq!(
        noise_width("const int noiseTextureResolution = 64;\n", ""),
        255
    );
}
