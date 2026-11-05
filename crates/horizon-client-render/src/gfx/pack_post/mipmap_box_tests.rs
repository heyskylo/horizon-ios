//! A buffer's mip chain is a box filter of the level above, odd sides included, so a small
//! bright spot keeps its share of every level down to the last, the buffer's mean: OpenGL 4.6
//! (8.14.4) recommends a box filter for generated mipmaps, and Iris 1.11.7's colortex mipmaps
//! kept a 26x22-pixel spot of a 1280x720 buffer in its last level (probe packs of ours).
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

/// composite writes 1.0 into one texel of colortex1, its top right corner, and 0 elsewhere;
/// final shows level 2 (the 1x1 level of a 5x3 buffer) in red and texel (1, 0) of level 1 (2x1)
/// in green, both scaled so that the box filter's values read 0.5.
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
         void main() {\n    ivec2 p = ivec2(gl_FragCoord.xy);\n\
             o = vec4(p == ivec2(4, 2) ? 1.0 : 0.0, 0.0, 0.0, 1.0);\n}\n",
    );
    add("final.vsh", VERTEX);
    add(
        "final.fsh",
        "#version 330 compatibility\n/*\nconst bool colortex1MipmapEnabled = true;\n*/\n\
         uniform sampler2D colortex1;\n\
         void main() {\n    float mean = textureLod(colortex1, vec2(0.5), 2.0).r;\n\
             float right = texelFetch(colortex1, ivec2(1, 0), 1).r;\n\
             gl_FragColor = vec4(mean * 7.5, right * 3.75, 0.0, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-mipmap-box",
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
fn a_bright_texel_keeps_its_share_of_odd_sized_levels() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    let pixels = gpu.render(&mut post, [5, 3]);
    // The 1x1 level is the mean of the 15 texels, 1/15; texel (1, 0) of the 2x1 level covers
    // columns 2, 3 and 4 (weights 1, 2, 2 of 5) of all three rows (1 of 3 each): 2/15.
    let [r, g, ..] = pixels[0];
    assert!(
        r.abs_diff(128) <= 2 && g.abs_diff(128) <= 2,
        "mean and level-1 texel scaled to 0.5: got red {r}, green {g}, expected 128 and 128"
    );
}
