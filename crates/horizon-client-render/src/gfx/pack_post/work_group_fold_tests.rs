//! A `workGroups` over wgpu's 65535 per dimension runs, and its programs read the work group
//! built-ins of the dispatch they declared. The programs are original.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

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
    // 70000 x 1 x 2 groups; the last 64 of the second layer each paint one pixel, green when
    // the built-ins agree with that dispatch.
    add(
        "composite.csh",
        "#version 430\nlayout(local_size_x=1) in;\nconst ivec3 workGroups=ivec3(70000,1,2);\n\
         layout(rgba8) writeonly uniform image2D colorimg0;\n\
         void main() {\n    uint first = 70000u - 64u;\n\
             if (gl_WorkGroupID.z != 1u || gl_WorkGroupID.x < first) return;\n\
             uint k = gl_WorkGroupID.x - first;\n\
             bool ok = gl_WorkGroupID.y == 0u && gl_NumWorkGroups == uvec3(70000u, 1u, 2u)\n\
                 && gl_GlobalInvocationID == gl_WorkGroupID;\n\
             imageStore(colorimg0, ivec2(k % 8u, k / 8u), ok ? vec4(0, 1, 0, 1) : vec4(1, 0, 0, 1));\n}\n",
    );
    add(
        "final.vsh",
        "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n",
    );
    add(
        "final.fsh",
        "#version 330 compatibility\nuniform sampler2D colortex0;\nin vec2 uv;\n\
         void main() { gl_FragColor = texelFetch(colortex0, ivec2(gl_FragCoord.xy), 0); }\n",
    );
    let pack =
        horizon_shaders::Pack::from_zip_bytes("original-fold", zip.finish().unwrap().into_inner())
            .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

#[test]
fn a_dispatch_over_the_limit_reads_its_declared_work_groups() {
    let Some(gpu) = Fixture::new() else { return };
    let post = pack();
    assert!(post.notes.is_empty(), "{:?}", post.notes);
    let mut post = gpu.build(post);
    let pixels = gpu.render(&mut post, [8, 8]);
    for (k, got) in pixels.iter().enumerate() {
        assert_eq!(*got, [0, 255, 0, 255], "pixel {k}; notes {:?}", post.notes);
    }
}
