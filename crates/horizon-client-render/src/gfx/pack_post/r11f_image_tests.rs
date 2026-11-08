//! An `R11F_G11F_B10F` image that a compute program declares `layout(r11f_g11f_b10f)` is stored
//! into and read back, as Iris 1.11.7 did with a probe pack of ours (#10490): its program is no
//! longer refused when the image is the `RGBA16F` stand-in. Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

#[test]
fn an_r11f_image_is_stored_into_and_read_back() {
    let Some(gpu) = Gpu::new() else { return };
    let files = vec![
        (
            "shaders.properties".into(),
            "image.imgA = texA RGBA R11F_G11F_B10F FLOAT false false 4 4\n".into(),
        ),
        (
            "composite.csh".into(),
            "#version 430\nlayout(local_size_x = 1) in;\nconst ivec3 workGroups = ivec3(1, 1, 1);\n\
             layout(r11f_g11f_b10f) writeonly uniform image2D imgA;\n\
             void main() { imageStore(imgA, ivec2(1, 1), vec4(0.25, 1.5, 3.0, 1.0)); }\n"
                .into(),
        ),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 130\nuniform sampler2D texA;\nvarying vec2 uv;\n\
             void main() { gl_FragColor = vec4(texelFetch(texA, ivec2(1, 1), 0).rgb / 4.0, 1.0); }\n"
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
    // (0.25, 1.5, 3.0) / 4 = (0.0625, 0.375, 0.75).
    let p = pixels[0];
    assert!(
        p[0].abs_diff(16) <= 1 && p[1].abs_diff(96) <= 1 && p[2].abs_diff(191) <= 1,
        "{p:?}, notes {:?}",
        post.notes
    );
}
