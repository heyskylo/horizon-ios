//! A buffer gets its B texture only when a composite-style program draws into it or flips it:
//! the others' B is a second view of A, and they read the same as before. The programs here
//! are original and cut down.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

/// `composite` writes colortex1 from colortex2, which no program writes; colortex4 is only
/// flipped by a directive. `shadowcomp` writes shadowcolor1; shadowcolor0 is only read.
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
        "#version 120\n/* RENDERTARGETS: 1 */\nvoid main() { gl_FragData[0]=vec4(0,0.5,0,1); }\n",
    );
    add("composite.vsh", VERTEX);
    add(
        "composite.fsh",
        "#version 120\nuniform sampler2D colortex2;\nvarying vec2 uv;\n/* RENDERTARGETS: 1 */\n\
         void main() { gl_FragData[0]=vec4(texture2D(colortex2,uv).rgb*0.5,1); }\n",
    );
    add("final.vsh", VERTEX);
    // Pixel column x shows colortex1, colortex4, shadowcolor0 and shadowcolor1 in turn.
    add(
        "final.fsh",
        "#version 330 compatibility\nconst int shadowMapResolution=16;\n\
         const vec4 colortex2ClearColor=vec4(1,1,1,1);\n\
         const vec4 colortex4ClearColor=vec4(0,0,0.75,1);\n\
         const vec4 shadowcolor0ClearColor=vec4(0.25,0,0,1);\n\
         uniform sampler2D colortex1;\nuniform sampler2D colortex4;\n\
         uniform sampler2D shadowcolor0;\nuniform sampler2D shadowcolor1;\nin vec2 uv;\n\
         void main() {\n    vec4 c[4] = vec4[4](texture(colortex1,uv), texture(colortex4,uv),\n\
         texture(shadowcolor0,uv), texture(shadowcolor1,uv));\n\
             gl_FragColor = c[int(gl_FragCoord.x) % 4];\n}\n",
    );
    add(
        "shaders.properties",
        "shadow.enabled=true\nflip.composite.colortex4=true\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-single-side",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

#[test]
fn only_buffers_drawn_into_or_flipped_get_a_second_texture() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    // Two frames: the second starts from the first one's buffers.
    for frame in 0..2 {
        let pixels = gpu.render(&mut post, [8, 8]);
        let expected = [
            [128, 128, 128, 255],
            [0, 0, 191, 255],
            [64, 0, 0, 255],
            [0, 128, 0, 255],
        ];
        for (x, want) in expected.iter().enumerate() {
            let got = pixels[x];
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(*w) <= 1),
                "frame {frame}, column {x}: {got:?}, expected {want:?}"
            );
        }
    }
    let shared = |sides: &[wgpu::TextureView; 2]| sides[0].texture() == sides[1].texture();
    let targets = post.targets.as_ref().expect("rendered");
    for (b, single) in [(1, false), (2, true), (4, false)] {
        let sides = targets.color[b].as_ref().expect("used buffer");
        assert_eq!(shared(sides), single, "colortex{b}");
    }
    let shadow = post.shadow_targets.as_ref().expect("shadow pass");
    for (n, single) in [(0, true), (1, false)] {
        let sides = shadow.color[n].as_ref().expect("used buffer");
        assert_eq!(shared(sides), single, "shadowcolor{n}");
    }
}
