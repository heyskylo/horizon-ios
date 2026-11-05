//! The sky's programs read their fragments' own depth in `gl_FragCoord.z`, as our probe packs
//! read it in Iris 1.11.7: the sky disc at its distance; and
//! the sky still shows only where the world left the far plane. Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

/// `program` writes its fragment's `gl_FragCoord.z` into colortex1 (32-bit floats) and red into
/// colortex0; final shows colortex1's value as a 24-bit integer on the top half, and the top
/// half of colortex0 on the bottom half.
fn files(program: &str) -> Vec<(String, String)> {
    vec![
        (
            "shaders.properties".into(),
            format!("blend.{program}=off\n"),
        ),
        (
            format!("{program}.vsh"),
            "#version 120\nvoid main() { gl_Position = ftransform(); }\n".into(),
        ),
        (
            format!("{program}.fsh"),
            "#version 120\n/* DRAWBUFFERS:01 */\nvoid main() {\n\
             \x20   gl_FragData[0] = vec4(1.0, 0.0, 0.0, 1.0);\n\
             \x20   gl_FragData[1] = vec4(gl_FragCoord.z);\n}\n"
                .into(),
        ),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 130\n/*\nconst int colortex1Format = RGBA32F;\n*/\n\
             uniform sampler2D colortex0;\nuniform sampler2D colortex1;\nvarying vec2 uv;\n\
             void main() {\n\
             \x20   uint n = min(uint(texture(colortex1, uv).r * 16777215.0 + 0.5), 16777215u);\n\
             \x20   vec4 depth = vec4(uvec4(n >> 16u, (n >> 8u) & 255u, n & 255u, 255u)) / 255.0;\n\
             \x20   gl_FragColor = uv.y > 0.5 ? depth : texture(colortex0, uv + vec2(0.0, 0.5));\n}\n"
                .into(),
        ),
    ]
}

/// GL window depth of a point `distance` in front of the camera along its view axis, for the
/// fixture's perspective: near 0.05, far `depth_far`.
fn window_depth(distance: f64, far: f64) -> f64 {
    let near = 0.05;
    let ndc = (far + near) / (far - near) - 2.0 * far * near / ((far - near) * distance);
    ndc * 0.5 + 0.5
}

/// The pack's picture of a frame of the fixture, whose world drew nothing but left
/// `world_depth` (GL window depth) everywhere.
fn picture(
    gpu: &Gpu,
    program: &str,
    world_depth: f32,
    celestial: &[crate::mesher::Vertex],
) -> Vec<[u8; 4]> {
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    // The world's depth is reverse-Z.
    gpu.clear(&color, &depth, 1.0 - world_depth);
    let sun = gpu.texture(T::Rgba8Unorm);
    let mut post = gpu.load(
        files(program),
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
    frame.celestial = celestial;
    gpu.render(&mut post, &color, &depth, &frame, &mut |_, _, _, _| {})
}

fn decode(p: [u8; 4]) -> f64 {
    f64::from(u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2])) / 16_777_215.0
}

/// The sky disc lies 16 blocks above the eye: the fixture looks level with a 90° field of view,
/// so a pixel `row` rows below the top of the 16-pixel picture sees it along the view axis at
/// 16 / tan(elevation) = 16 / (1 - (row + 0.5) / 8) blocks.
#[test]
fn the_sky_disc_reads_its_own_depth() {
    let Some(gpu) = Gpu::new() else { return };
    let pixels = picture(&gpu, "gbuffers_skybasic", 1.0, &[]);
    for row in 0..4 {
        let along = 16.0 / (1.0 - (row as f64 + 0.5) / 8.0);
        let expected = window_depth(along, 1000.0);
        for x in 0..SIZE[0] as usize {
            let got = decode(pixels[row * SIZE[0] as usize + x]);
            assert!(
                (got - expected).abs() < 2e-5,
                "row {row}, column {x}: gl_FragCoord.z {got}, expected {expected}"
            );
        }
    }
}

/// The sun's quad (and the moon's behind the camera), 100 blocks away across the view axis:
/// one depth everywhere.
#[test]
fn the_sun_reads_its_own_depth() {
    let Some(gpu) = Gpu::new() else { return };
    let quad = |z: f32| {
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
    };
    let quads: Vec<_> = quad(100.0).into_iter().chain(quad(-100.0)).collect();
    let pixels = picture(&gpu, "gbuffers_skytextured", 1.0, &quads);
    let expected = window_depth(100.0, 1000.0);
    for (i, &p) in pixels[..pixels.len() / 2].iter().enumerate() {
        let got = decode(p);
        assert!(
            (got - expected).abs() < 2e-5,
            "pixel {i}: gl_FragCoord.z {got}, expected {expected}"
        );
    }
}

/// Where the world left a depth nearer than the far plane, even one farther away than the sky
/// disc, the world shows (the fixture's grey), not the sky's red.
#[test]
fn the_sky_shows_only_where_the_world_left_the_far_plane() {
    let Some(gpu) = Gpu::new() else { return };
    let half = SIZE[0] as usize * SIZE[1] as usize / 2;
    let far = picture(&gpu, "gbuffers_skybasic", 1.0, &[]);
    let near = picture(&gpu, "gbuffers_skybasic", 0.9999, &[]);
    // The sky disc covers the upper half of the view (row 7 sees it 256 blocks away, at a
    // window depth of 0.9998).
    for (i, (f, n)) in far[half..].iter().zip(&near[half..]).enumerate() {
        assert_eq!(*f, [255, 0, 0, 255], "far plane, pixel {i}");
        assert_eq!(*n, [16, 16, 16, 255], "depth 0.9999, pixel {i}");
    }
}
