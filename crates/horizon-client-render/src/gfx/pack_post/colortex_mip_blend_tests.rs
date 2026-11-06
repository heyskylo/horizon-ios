//! A buffer's mipmaps are read blended between the two nearest levels: our probe packs (#9307)
//! read every colortex at levels 2.5 and 1.25 in composite and final, in Iris 1.11.7, and got the
//! two nearest levels mixed.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

/// composite writes 1.0 into the left half of a 4x4 colortex1 and 0 into the right half, so at
/// (0.25, 0.5) level 1 reads 1.0 and level 2 (1x1) the mean, 0.5. final shows the read at
/// level 1.5 in red and at level 1.25 in green.
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
    add("composite.vsh", VERTEX);
    add(
        "composite.fsh",
        "#version 330 compatibility\n/*\nconst int colortex1Format = RGBA16F;\n*/\n\
         /* RENDERTARGETS: 1 */\nlayout(location = 0) out vec4 o;\n\
         void main() {\n    o = vec4(gl_FragCoord.x < 2.0 ? 1.0 : 0.0, 0.0, 0.0, 1.0);\n}\n",
    );
    add("final.vsh", VERTEX);
    add(
        "final.fsh",
        "#version 330 compatibility\n/*\nconst bool colortex1MipmapEnabled = true;\n*/\n\
         uniform sampler2D colortex1;\n\
         void main() {\n    vec2 at = vec2(0.25, 0.5);\n\
             gl_FragColor = vec4(textureLod(colortex1, at, 1.5).r, textureLod(colortex1, at, 1.25).r, 0.0, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-colortex-mip-blend",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    assert_eq!(post.programs.len(), 2, "{:?}", post.notes);
    post
}

#[test]
fn a_buffer_blends_its_two_nearest_levels() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    let pixels = gpu.render(&mut post, [4, 4]);
    // Level 1.5: 1.0 and 0.5 mixed 1:1, 0.75; level 1.25: mixed 3:1, 0.875.
    let [r, g, ..] = pixels[0];
    assert!(
        r.abs_diff(191) <= 2 && g.abs_diff(223) <= 2,
        "levels 1.5 and 1.25: got red {r}, green {g}, expected 191 and 223"
    );
}
