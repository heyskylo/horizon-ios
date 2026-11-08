//! In the End, while the dragon fight's world fog leaves out vanilla's sky, a pack's
//! `gbuffers_skybasic` draws a fan in the fog colour, as our probe packs read it in Iris 1.11.7.
//! Original fixture, no pack files.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::TextureFormat as T;

/// `gbuffers_skybasic` writes its vertex colour into colortex0 and `gl_Vertex` into colortex1
/// (32-bit floats). final shows, on the picture's top half, `gl_Vertex.y` of the view's lower
/// half as a 24-bit integer ((y + 64) * 65536), and on its bottom half colortex0 of the view's
/// upper half.
fn files() -> Vec<(String, String)> {
    vec![
        ("shaders.properties".into(), "blend.gbuffers_skybasic=off\n".into()),
        (
            "gbuffers_skybasic.vsh".into(),
            "#version 120\nvarying vec4 color;\nvarying vec3 at;\nvoid main() {\n\
             \x20   gl_Position = ftransform();\n    color = gl_Color;\n    at = gl_Vertex.xyz;\n}\n"
                .into(),
        ),
        (
            "gbuffers_skybasic.fsh".into(),
            "#version 120\nvarying vec4 color;\nvarying vec3 at;\n/* DRAWBUFFERS:01 */\n\
             void main() {\n    gl_FragData[0] = color;\n    gl_FragData[1] = vec4(at, 1.0);\n}\n"
                .into(),
        ),
        ("final.vsh".into(), VERTEX.into()),
        (
            "final.fsh".into(),
            "#version 130\n/*\nconst int colortex1Format = RGBA32F;\n*/\n\
             uniform sampler2D colortex0;\nuniform sampler2D colortex1;\nvarying vec2 uv;\n\
             void main() {\n\
             \x20   float y = texelFetch(colortex1, ivec2(gl_FragCoord.xy) - ivec2(0, 8), 0).y;\n\
             \x20   uint n = min(uint((y + 64.0) * 65536.0 + 0.5), 16777215u);\n\
             \x20   vec4 at = vec4(uvec4(n >> 16u, (n >> 8u) & 255u, n & 255u, 255u)) / 255.0;\n\
             \x20   gl_FragColor = uv.y > 0.5 ? at : texelFetch(colortex0, ivec2(gl_FragCoord.xy) + ivec2(0, 8), 0);\n}\n"
                .into(),
        ),
    ]
}

/// The pack's picture of the fixture's frame (16 pixels square, 90° field of view, looking level
/// toward +Z) in the End, the world having left the far plane everywhere.
fn picture(gpu: &Gpu, boss_world_fog: bool) -> Vec<[u8; 4]> {
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    gpu.clear(&color, &depth, 0.0);
    let mut post = gpu.load(
        files(),
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
    frame.end_sky = true;
    frame.boss_world_fog = boss_world_fog;
    frame.detail_distance = 192.0;
    frame.fog = Some(crate::fog::FogData {
        color: [0.2, 0.4, 0.6, 1.0],
        environmental_start: 0.0,
        environmental_end: 96.0,
        render_distance_start: 172.8,
        render_distance_end: 192.0,
        sky_end: 96.0,
        cloud_end: 96.0,
    });
    gpu.render(&mut post, &color, &depth, &frame, &mut |_, _, _, _| {})
}

/// Where the view ray through pixel (`column`, `row`) meets the fan: from (0, -16, 0) to eight
/// points 16 above the eye at radius 192, every 45° from +Z. None when it passes above the rim.
fn fan_height(column: usize, row: usize) -> Option<f64> {
    let side = |i: usize| (i as f64 + 0.5) / 8.0 - 1.0;
    // The fan is mirrored across x = 0, so either sign of the view's x does.
    let ray = glam::DVec3::new(side(column), -side(row), 1.0);
    let azimuth = ray.x.atan2(ray.z).to_degrees();
    let edge = (azimuth / 45.0).floor();
    let rim = |k: f64| {
        let a = (k * 45.0).to_radians();
        glam::DVec3::new(192.0 * a.sin(), 16.0, 192.0 * a.cos())
    };
    let apex = glam::DVec3::new(0.0, -16.0, 0.0);
    let normal = (rim(edge) - apex).cross(rim(edge + 1.0) - apex);
    let along = apex.dot(normal) / ray.dot(normal);
    let hit = ray * along;
    (along > 0.0 && hit.y <= 16.0).then_some(hit.y)
}

#[test]
fn the_end_under_the_dragons_fog_has_the_packs_fan() {
    let Some(gpu) = Gpu::new() else { return };
    let pixels = picture(&gpu, true);
    let width = SIZE[0] as usize;
    // Below the horizon: every pixel sees the fan, at the height the planes give (to 0.02:
    // the GPU interpolates `gl_Vertex` across triangles 192 blocks wide; it came within 0.008).
    for row in 8..16 {
        for column in 0..width {
            let p = pixels[(row - 8) * width + column];
            let got = f64::from(u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
                / 65536.0
                - 64.0;
            let expected = fan_height(column, row).expect("below the horizon");
            assert!(
                (got - expected).abs() < 0.02,
                "row {row}, column {column}: gl_Vertex.y {got}, expected {expected}"
            );
        }
    }
    // Above it: only the row just over the horizon reaches the fan (the rim stands 4.8° up),
    // drawn in the fog colour; above that, the world's grey stays.
    for row in 0..8 {
        for column in 0..width {
            let p = pixels[(row + 8) * width + column];
            let expected = match fan_height(column, row) {
                Some(_) => [51, 102, 153, 255],
                None => [16, 16, 16, 255],
            };
            assert_eq!(p, expected, "row {row}, column {column}");
        }
        assert_eq!(fan_height(0, row).is_some(), row == 7, "row {row}");
    }
}

/// The End without the fog: vanilla's End sky, which no `gbuffers_skybasic` draws.
#[test]
fn the_end_without_the_fog_has_no_fan() {
    let Some(gpu) = Gpu::new() else { return };
    let pixels = picture(&gpu, false);
    let width = SIZE[0] as usize;
    for (i, p) in pixels[8 * width..].iter().enumerate() {
        assert_eq!(*p, [16, 16, 16, 255], "pixel {i}");
    }
}
