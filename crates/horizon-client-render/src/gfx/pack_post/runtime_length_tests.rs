//! `.length()` of a runtime-sized array in a named buffer block reads the buffer's own length, as
//! Iris 1.11.7 did with a probe pack of ours (#10490): `bufferObject.0 = 32` holds 8 floats. The
//! program was refused before. Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

#[test]
fn a_runtime_arrays_length_is_the_buffers() {
    let Some(gpu) = Gpu::new() else { return };
    let files = vec![
        ("shaders.properties".into(), "bufferObject.0 = 32\n".into()),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 430 compatibility\nin vec2 uv;\n\
             layout(std430, binding = 0) buffer Values { float v[]; } values;\n\
             void main() { gl_FragColor = vec4(float(values.v.length()) / 32.0, 0.0, 0.0, 1.0); }\n"
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
    // 32 bytes of floats: 8 entries, 8 / 32 = 0.25.
    let p = pixels[0];
    assert!(p[0].abs_diff(64) <= 1, "{p:?}, notes {:?}", post.notes);
}
