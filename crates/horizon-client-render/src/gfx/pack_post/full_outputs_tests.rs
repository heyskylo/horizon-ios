//! A shadow program writing as many vertex outputs as the GPU takes leaves no room for the boats'
//! water patches (`GbuffersInputs::water_patches`, one output more): translucent terrain gets
//! the program without them, not no shadow program. The programs here are original.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

fn pack(outputs: u32) -> Post {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, source: &str| {
        zip.start_file(
            format!("shaders/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    };
    let declared: String = (0..outputs)
        .map(|i| format!("varying vec4 v{i};\n"))
        .collect();
    let written: String = (0..outputs).map(|i| format!("v{i}=vec4({i}.0);")).collect();
    let read: String = (0..outputs).map(|i| format!("+v{i}")).collect();
    add(
        "shadow.vsh",
        &format!(
            "#version 120\n{declared}void main() {{ gl_Position=vec4(gl_Vertex.xy,0.5,1.0); {written} }}\n"
        ),
    );
    add(
        "shadow.fsh",
        &format!("#version 120\n{declared}void main() {{ gl_FragData[0]=vec4(0.0){read}; }}\n"),
    );
    add(
        "final.vsh",
        "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n",
    );
    add(
        "final.fsh",
        "#version 120\nconst int shadowMapResolution=16;\nuniform sampler2D shadowcolor0;\n\
         varying vec2 uv;\nvoid main() { gl_FragColor=texture2D(shadowcolor0,uv); }\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-full-outputs",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let inputs = horizon_shaders::GameInputs {
        terrain: Some(horizon_shaders::GbuffersInputs {
            parts: None,
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
fn a_shadow_program_with_every_output_drops_the_boats_patches_not_its_water() {
    let Some(gpu) = Fixture::new() else { return };
    const ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0=>Float32x3];
    let layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    })];
    let geometry = || {
        Some(Geometry {
            material_maps: false,
            buffers: &layouts,
            table: None,
            depth_write: true,
            atlas: None,
            ids: None,
            cull: true,
        })
    };
    let outputs = gpu.device.limits().max_inter_stage_shader_variables;
    let post = gpu.build_with_terrain(pack(outputs), geometry());
    assert!(post.has_shadow(), "{:?}", post.notes);
    assert!(post.shadow_water.is_some(), "{:?}", post.notes);
    assert!(
        post.notes
            .iter()
            .any(|n| n.contains("without the boats' water patches")),
        "{:?}",
        post.notes
    );
    // With room for them, the patches stay and nothing is said.
    let post = gpu.build_with_terrain(pack(outputs - 1), geometry());
    assert!(post.shadow_water.is_some(), "{:?}", post.notes);
    assert!(
        !post.notes.iter().any(|n| n.contains("water patches")),
        "{:?}",
        post.notes
    );
}
