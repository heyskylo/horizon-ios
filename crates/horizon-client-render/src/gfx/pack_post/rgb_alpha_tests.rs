//! Three-channel buffers read alpha 1, as in Iris 1.11.7: written with alpha 0, cleared with
//! alpha 0, or never written. The values come from probe pack p8106 run in Iris (the PR has
//! them); the programs here are the probe's, cut down.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

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
        "#version 120\n/* RENDERTARGETS: 1,5,7 */\n\
         /*\nconst int colortex0Format = RGB16F;\nconst int colortex1Format = RGB16F;\n\
         const int colortex2Format = RGB8;\nconst int colortex3Format = RGB32F;\n\
         const int colortex4Format = RGB32UI;\nconst int colortex5Format = RGBA16F;\n\
         const int colortex7Format = RGB16;\n*/\n\
         const vec4 colortex2ClearColor = vec4(0.25, 0.5, 0.75, 0.0);\n\
         const bool colortex3Clear = false;\n\
         void main() {\n    gl_FragData[0] = vec4(0.25, 0.5, 0.75, 0.0);\n\
         gl_FragData[1] = vec4(0.25, 0.5, 0.75, 0.0);\n    gl_FragData[2] = vec4(0.25, 0.5, 0.75, 0.0);\n}\n",
    );
    add("composite1.vsh", VERTEX);
    add(
        "composite1.fsh",
        "#version 330 compatibility\n/* RENDERTARGETS: 4 */\nlayout(location = 0) out uvec4 o;\n\
         void main() { o = uvec4(5u, 6u, 7u, 0u); }\n",
    );
    add("final.vsh", VERTEX);
    // Pixel column x shows buffer x % 8: its alpha in red, its green in green.
    add(
        "final.fsh",
        "#version 330 compatibility\nuniform sampler2D colortex0;\nuniform sampler2D colortex1;\n\
         uniform sampler2D colortex2;\nuniform sampler2D colortex3;\nuniform usampler2D colortex4;\n\
         uniform sampler2D colortex5;\nuniform sampler2D colortex7;\n\
         void main() {\n    ivec2 p = ivec2(gl_FragCoord.xy);\n    vec4 c = vec4(0.0);\n\
             switch (p.x % 8) {\n\
             case 0: c = texelFetch(colortex0, p, 0); break;\n\
             case 1: c = texelFetch(colortex1, p, 0); break;\n\
             case 2: c = texelFetch(colortex2, p, 0); break;\n\
             case 3: c = texelFetch(colortex3, p, 0); break;\n\
             case 4: c = vec4(texelFetch(colortex4, p, 0)) / 12.0; c.a *= 12.0; break;\n\
             case 5: c = texelFetch(colortex5, p, 0); break;\n\
             case 7: c = texelFetch(colortex7, p, 0); break;\n\
             }\n    gl_FragColor = vec4(c.a, c.g, 0.0, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-rgb-alpha",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    assert_eq!(post.programs.len(), 3, "{:?}", post.notes);
    post
}

#[test]
fn three_channel_buffers_read_alpha_1() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    // Twice: the second frame reads buffers that a first frame already used.
    for frame in 0..2 {
        let pixels = gpu.render(&mut post, [8, 8]);
        // (alpha, green) per buffer: 0 is the world (black) through the blit; 1, 5 and 7 are
        // written with alpha 0, 2 is cleared with alpha 0, 3 is never cleared nor written, 4 is
        // RGB32UI written (5, 6, 7, 0). Only 5 (RGBA16F) keeps the alpha 0 it was given.
        let expected = [
            [255, 0],
            [255, 128],
            [255, 128],
            [255, 0],
            [255, 128],
            [0, 128],
            [0, 0],
            [255, 128],
        ];
        let row: Vec<[u8; 2]> = pixels[..8].iter().map(|p| [p[0], p[1]]).collect();
        for (b, (got, want)) in row.iter().zip(expected).enumerate() {
            if b == 6 {
                continue;
            }
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
                "frame {frame}, colortex{b}: (alpha, green) {got:?}, expected {want:?}; row {row:?}"
            );
        }
    }
}
