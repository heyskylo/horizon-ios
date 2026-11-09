//! A translucent draw reads the preceding scene even when it writes the same color buffer.
//! Original programs and independently calculated colors; no shader-pack assets.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::{TextureFormat as T, util::DeviceExt};

#[test]
fn translucent_draws_sample_the_color_written_by_deferred() {
    let Some(gpu) = Gpu::new() else { return };
    let positions: [[f32; 3]; 6] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    let vertices = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Original translucent quad"),
            contents: bytemuck::cast_slice(&positions),
            usage: wgpu::BufferUsages::VERTEX,
        });
    const ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0=>Float32x3];
    let layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    })];
    // Odd and even numbers of deferred passes leave the current scene on different sides.
    // Exercise two buffer numbers and the legacy sampler aliases too.
    for (buffer, sampler) in [(4, "colortex4"), (7, "gaux4")] {
        for stages in [1, 2] {
            for blended in [false, true] {
                let mut files = vec![
                    ("deferred.vsh".into(), VERTEX.into()),
                    (
                        "deferred.fsh".into(),
                        format!(
                            "#version 120\n/* RENDERTARGETS: {buffer} */\nvoid main(){{gl_FragData[0]=vec4(0.25,0.5,0.75,1.0);}}"
                        ),
                    ),
                    (
                        "gbuffers_water.vsh".into(),
                        "#version 120\nvoid main(){gl_Position=gl_Vertex;}".into(),
                    ),
                    (
                        "gbuffers_water.fsh".into(),
                        format!(
                            "#version 120\n/* RENDERTARGETS: {buffer} */\nuniform sampler2D {sampler};\nvoid main(){{gl_FragData[0]=vec4(texture2D({sampler},vec2(0.5)).rgb+vec3(0.125,0.0,0.0),0.5);}}"
                        ),
                    ),
                    ("final.vsh".into(), VERTEX.into()),
                    (
                        "final.fsh".into(),
                        format!(
                            "#version 120\nuniform sampler2D colortex{buffer};varying vec2 uv;\nvoid main(){{gl_FragColor=vec4(texture2D(colortex{buffer},uv).rgb,1.0);}}"
                        ),
                    ),
                    (
                        "shaders.properties".into(),
                        format!(
                            "blend.gbuffers_water={}\n",
                            if blended {
                                "SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA"
                            } else {
                                "off"
                            }
                        ),
                    ),
                ];
                if stages == 2 {
                    files.extend([
                        ("deferred1.vsh".into(), VERTEX.into()),
                        ("deferred1.fsh".into(), format!(
                            "#version 120\n/* RENDERTARGETS: {buffer} */\nvoid main(){{gl_FragData[0]=vec4(0.5,0.75,0.25,1.0);}}"
                        )),
                    ]);
                }
                let color = gpu.texture(T::Rgba8Unorm);
                let depth = gpu.texture(T::Depth32Float);
                gpu.clear(&color, &depth, 0.0);
                let mut post = gpu.load(
                    files,
                    World {
                        atlas: None,
                        material_maps: None,
                        lightmap: color.clone(),
                        terrain: Some(Geometry {
                            buffers: &layouts,
                            table: None,
                            depth_write: true,
                            atlas: None,
                            material_maps: false,
                            ids: None,
                            cull: false,
                        }),
                        block_ids: &[],
                        sprite_rects: &[],
                        celestial: None,
                        gui_size: SIZE,
                        clouds: None,
                        weather: false,
                        hand: false,
                    },
                    horizon_shaders::GameInputs {
                        terrain: Some(horizon_shaders::GbuffersInputs {
                            declarations: "layout(location=0)in vec3 fixture_position;\n".into(),
                            body: "hz_Vertex=vec4(fixture_position,1.0);\n".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                );
                assert!(post.has_water(), "{:?}", post.notes);
                let mut expected = if stages == 1 {
                    [0.25f32, 0.5, 0.75]
                } else {
                    [0.5, 0.75, 0.25]
                };
                expected[0] += if blended { 0.0625 } else { 0.125 };
                let expected = expected.map(|c| (c * 255.0).round() as u8);
                let hud = HudBatch::new(SIZE);
                for frame in 0..2 {
                    let pixels = gpu.render(
                        &mut post,
                        &color,
                        &depth,
                        &fixture_frame(&hud),
                        &mut |rp, list, _, _| {
                            if list == TerrainList::Translucent {
                                rp.set_vertex_buffer(0, vertices.slice(..));
                                rp.draw(0..6, 0..1);
                            }
                        },
                    );
                    for p in pixels {
                        assert!(
                            p[..3]
                                .iter()
                                .zip(expected)
                                .all(|(&a, b)| a.abs_diff(b) <= 2),
                            "buffer {buffer}, {stages} deferred stages, blended {blended}, frame {frame}: {p:?}, expected {expected:?}"
                        );
                    }
                }
            }
        }
    }
}
