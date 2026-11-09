//! The shadow pass draws terrain faces from both sides, as Iris 1.11.7 does (#9033: a probe pack
//! of ours writing `gl_FrontFacing` into `shadowcolor0` found back faces in Iris's shadow map and
//! none in Horizon's). The programs here are original.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};
use wgpu::util::DeviceExt;

const FINAL_VSH: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

fn pack() -> Post {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(
            format!("shaders/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    // The terrain's positions are already in clip space.
    add(
        "shadow.vsh",
        "#version 120\nvoid main() { gl_Position=vec4(gl_Vertex.xy,0.5,1.0); }\n",
    );
    // Red where a front face was drawn, blue where a back face was.
    add(
        "shadow.fsh",
        "#version 120\nvoid main() {\n\
         gl_FragData[0]=gl_FrontFacing ? vec4(1.0,0.0,0.0,1.0) : vec4(0.0,0.0,1.0,1.0);\n}\n",
    );
    add("final.vsh", FINAL_VSH);
    add(
        "final.fsh",
        "#version 120\nconst int shadowMapResolution=16;\nuniform sampler2D shadowcolor0;\n\
         varying vec2 uv;\nvoid main() { gl_FragColor=texture2D(shadowcolor0,uv); }\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-shadow-faces",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let inputs = horizon_shaders::GameInputs {
        terrain: Some(horizon_shaders::GbuffersInputs {
            item_id_declared: false,
            far_depth: false,
            paged_maps: false,
            water_mask: false,
            water_patches: false,
            declarations: "layout(location=0)in vec3 fixture_position;\n".into(),
            body: "hz_Vertex=vec4(fixture_position,1.0);hz_Color=vec4(1.0);\
                   hz_MultiTexCoord0=vec4(0.0);hz_MultiTexCoord1=vec4(240.0);\
                   hz_MultiTexCoord2=vec4(240.0);hz_Normal=vec3(0.0,0.0,1.0);\n"
                .into(),
            attributes: vec![],
            after: String::new(),
            alpha_test: None,
        }),
        ..Default::default()
    };
    horizon_shaders::load_post(&pack, "", &properties, &macros, &inputs)
}

#[test]
fn shadow_pass_draws_terrain_back_faces() {
    let Some(gpu) = Fixture::new() else { return };
    const ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0=>Float32x3];
    let layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    })];
    // Culled as the game's own terrain is (`Renderer` hands packs `cull: true`).
    let mut post = gpu.build_with_terrain(
        pack(),
        Some(Geometry {
            material_maps: false,
            buffers: &layouts,
            table: None,
            depth_write: true,
            atlas: None,
            ids: None,
            cull: true,
        }),
    );
    assert!(post.has_shadow(), "{:?}", post.notes);
    // GL clip space: the left half counter-clockwise (a front face), the right half
    // clockwise (a back face).
    let positions: [[f32; 3]; 12] = [
        [-1., -1., 0.],
        [0., -1., 0.],
        [0., 1., 0.],
        [-1., -1., 0.],
        [0., 1., 0.],
        [-1., 1., 0.],
        [0., -1., 0.],
        [1., 1., 0.],
        [1., -1., 0.],
        [0., -1., 0.],
        [0., 1., 0.],
        [1., 1., 0.],
    ];
    let vertices = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Original shadow faces geometry"),
            contents: bytemuck::cast_slice(&positions),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let size = [8, 8];
    let pixels = gpu.render_with_terrain(&mut post, size, &mut |rp, list, _, _| {
        if list == TerrainList::ShadowSolid {
            rp.set_vertex_buffer(0, vertices.slice(..));
            rp.draw(0..12, 0..1);
        }
    });
    let near = |got: [u8; 4], want: [u8; 4]| got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1);
    for (i, got) in pixels.iter().enumerate() {
        let x = i as u32 % size[0];
        let want = if x < size[0] / 2 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        assert!(
            near(*got, want),
            "pixel ({x}, {}): {got:?}, expected {want:?} (red: front face, blue: back face)",
            i as u32 / size[0]
        );
    }
}
