//! In gbuffers programs a sampler the game gives no texture reads texture unit 0, the atlas
//! (OptiFine's shaders.txt: `gtexture` is unit 0 there), and so do `colortex0` to `colortex3`,
//! which aren't gbuffers samplers in that table. Our probe packs (#9307) read the block atlas
//! through a sampler name of their own, and through each of `colortex0` to `colortex3` and
//! `gnormal`, in gbuffers_terrain in Iris 1.11.7.
use super::depth_test::{Gpu, SIZE, VERTEX, fixture_frame};
use super::*;
use crate::hud::HudBatch;
use wgpu::{TextureFormat as T, util::DeviceExt};

const ATLAS: [u8; 4] = [10, 200, 30, 255];

#[test]
fn unknown_samplers_and_colortex0_to_3_in_gbuffers_terrain_read_the_atlas() {
    let Some(gpu) = Gpu::new() else { return };
    let files = vec![
        ("final.vsh".to_owned(), VERTEX.to_owned()),
        (
            "final.fsh".to_owned(),
            "#version 120\nuniform sampler2D colortex0;varying vec2 uv;\
             void main(){gl_FragColor=vec4(texture2D(colortex0,uv).rgb,1.0);}"
                .to_owned(),
        ),
        (
            "gbuffers_terrain.vsh".to_owned(),
            "#version 120\nvoid main(){gl_Position=vec4(gl_Vertex.xy,0.0,1.0);}".to_owned(),
        ),
        (
            "gbuffers_terrain.fsh".to_owned(),
            "#version 120\nuniform sampler2D probeOwnName;\nuniform sampler2D colortex0;\n\
             uniform sampler2D gdepth;\n/* DRAWBUFFERS:0 */\nvoid main(){\n\
             \x20   float x = gl_FragCoord.x;\n\
             \x20   gl_FragData[0] = x < 6.0 ? texture2D(probeOwnName, vec2(0.5))\n\
             \x20       : x < 11.0 ? texture2D(colortex0, vec2(0.5)) : texture2D(gdepth, vec2(0.5));\n}"
                .to_owned(),
        ),
    ];
    let terrain = horizon_shaders::GbuffersInputs {
        declarations: "layout(location=0)in vec3 fixture_position;\n".into(),
        body: "hz_Vertex=vec4(fixture_position,1.0);hz_Color=vec4(1.0);hz_MultiTexCoord0=vec4(0.0);\
               hz_MultiTexCoord1=vec4(240.0);hz_MultiTexCoord2=vec4(240.0);hz_Normal=vec3(0.0,0.0,1.0);\n"
            .into(),
        ..Default::default()
    };
    let inputs = horizon_shaders::GameInputs {
        terrain: Some(terrain),
        ..Default::default()
    };
    const ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0=>Float32x3];
    let layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    })];
    let atlas = gpu
        .device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("Original one-colour atlas"),
                size: wgpu::Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: T::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &ATLAS.repeat(4),
        )
        .create_view(&Default::default());
    let color = gpu.texture(T::Rgba8Unorm);
    let depth = gpu.texture(T::Depth32Float);
    let mut post = gpu.load(
        files,
        World {
            atlas: Some((atlas, [2, 2])),
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
        inputs,
    );
    assert!(post.has_terrain(), "{:?}", post.notes);
    let corners = [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let vertices: Vec<f32> = [0, 1, 2, 0, 2, 3]
        .into_iter()
        .flat_map(|c| [corners[c][0], corners[c][1], 0.0])
        .collect();
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Original full-screen quad"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let hud = HudBatch::new(SIZE);
    let frame = fixture_frame(&hud);
    let pixels = gpu.render(&mut post, &color, &depth, &frame, &mut |rp, list, _, _| {
        if list == TerrainList::Solid {
            rp.set_vertex_buffer(0, buffer.slice(..));
            rp.draw(0..6, 0..1);
        }
    });
    // Columns 0-5 read the pack's own name, 6-10 colortex0 (which the program also writes),
    // 11-15 gdepth (colortex1).
    for (i, p) in pixels.iter().enumerate() {
        assert_eq!(*p, ATLAS, "column {}", i % SIZE[0] as usize);
    }
}
