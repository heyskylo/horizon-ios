//! `HIGHER_SHADOWCOLOR`: shadowcolor2 to 7 are written by shadow programs' directives, read by
//! them and by the other programs, and cleared to their own colours, as in Iris 1.11.7 (probe
//! packs p7951a to c; the PR has the readings). The programs here are original and cut down.
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
    add("shadowcomp.vsh", VERTEX);
    add(
        "shadowcomp.fsh",
        "#version 120\n/* RENDERTARGETS: 2,7 */\n\
         void main() { gl_FragData[0]=vec4(0.25,0,0,1); gl_FragData[1]=vec4(0,0.5,0,1); }\n",
    );
    // A shadow program reads a higher buffer it didn't write.
    add("shadowcomp1.vsh", VERTEX);
    add(
        "shadowcomp1.fsh",
        "#version 120\n/* RENDERTARGETS: 3 */\nuniform sampler2D shadowcolor7;\nvarying vec2 uv;\n\
         void main() { gl_FragData[0]=texture2D(shadowcolor7,uv); }\n",
    );
    add("final.vsh", VERTEX);
    // Pixel column x shows shadowcolor 2, 3, 5 and 6 in turn.
    add(
        "final.fsh",
        "#version 330 compatibility\nconst int shadowMapResolution=16;\n\
         const vec4 shadowcolor5ClearColor=vec4(0,0,0.75,1);\n\
         uniform sampler2D shadowcolor2;\nuniform sampler2D shadowcolor3;\n\
         uniform sampler2D shadowcolor5;\nuniform sampler2D shadowcolor6;\nin vec2 uv;\n\
         void main() {\n    vec4 c[4] = vec4[4](texture(shadowcolor2,uv), texture(shadowcolor3,uv),\n\
         texture(shadowcolor5,uv), texture(shadowcolor6,uv));\n\
             gl_FragColor = c[int(gl_FragCoord.x) % 4];\n}\n",
    );
    add(
        "shaders.properties",
        "shadow.enabled=true\niris.features.required = HIGHER_SHADOWCOLOR\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-higher-shadowcolor",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

#[test]
fn higher_shadowcolor_buffers_are_written_read_and_cleared() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    // Two frames: the second starts from the first one's buffers.
    for frame in 0..2 {
        let pixels = gpu.render(&mut post, [8, 8]);
        // shadowcolor2 written red, 3 a copy of 7 (green), 5 its clear colour, 6 white.
        let expected = [
            [64, 0, 0, 255],
            [0, 128, 0, 255],
            [0, 0, 191, 255],
            [255; 4],
        ];
        for (x, want) in expected.iter().enumerate() {
            let got = pixels[x];
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(*w) <= 1),
                "frame {frame}, column {x}: {got:?}, expected {want:?}; row {:?}",
                &pixels[..4]
            );
        }
    }
}
