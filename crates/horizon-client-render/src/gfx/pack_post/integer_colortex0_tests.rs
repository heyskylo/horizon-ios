//! A pack may give colortex0 an integer format (OptiFine's `shaders.txt`, "Texture Formats",
//! lists `RGBA32UI`, as #8459 quotes it): the pack still loads, and reads back what it wrote
//! there. #8459's probe; #8459 reports Iris 1.11.7 drawing (0, 200, 0) at every point
//! sampled from it. Without a `final` that draws it, the pack is refused (#9165).
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv=gl_MultiTexCoord0.xy; gl_Position=ftransform(); }\n";

/// A `final` that shows the integer colortex0.
const FINAL: &str = "#version 330 compatibility\nuniform usampler2D colortex0;\n\
     void main() { gl_FragColor = vec4(texelFetch(colortex0, ivec2(gl_FragCoord.xy), 0)) / 255.0; }\n";

/// A `final` that doesn't compile (it calls a function nobody declares), so it is skipped.
const BROKEN_FINAL: &str =
    "#version 330 compatibility\nvoid main() { gl_FragColor = undeclared_function(); }\n";

/// `depth`: the composite reads depthtex0, so the blit copies the world's depth; `last`: the
/// pack's `final`, if it has one.
fn pack(depth: bool, last: Option<&str>) -> Post {
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
    // With depthtex0: green where it holds the empty world's far plane (1), red elsewhere.
    let value = if depth {
        "texelFetch(depthtex0, ivec2(gl_FragCoord.xy), 0).r == 1.0 ? uvec4(0u, 200u, 0u, 255u) : uvec4(200u, 0u, 0u, 255u)"
    } else {
        "uvec4(0u, 200u, 0u, 255u)"
    };
    add(
        "composite.fsh",
        &format!(
            "#version 330 compatibility\n/* RENDERTARGETS: 0 */\n/*\nconst int colortex0Format = RGBA32UI;\n*/\n\
             uniform sampler2D depthtex0;\nlayout(location = 0) out uvec4 o;\nvoid main() {{ o = {value}; }}\n"
        ),
    );
    if let Some(source) = last {
        add("final.vsh", VERTEX);
        add("final.fsh", source);
    }
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-integer-colortex0",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

#[test]
fn an_integer_colortex0_keeps_what_the_pack_wrote() {
    let Some(gpu) = Fixture::new() else { return };
    for depth in [false, true] {
        let mut post = gpu.build(pack(depth, Some(FINAL)));
        // Twice: the second frame's world blit runs over a colortex0 the first frame wrote.
        for frame in 0..2 {
            let pixels = gpu.render(&mut post, [8, 8]);
            let wrong: Vec<_> = pixels.iter().filter(|p| **p != [0, 200, 0, 255]).collect();
            assert!(wrong.is_empty(), "depth {depth}, frame {frame}: {wrong:?}");
        }
    }
}

/// Without a `final` left to draw it (none, or one that is skipped), an integer colortex0 has
/// no colours the screen can show: the pack is refused, so the game turns it off and draws
/// without it instead of a black screen (#9165). The refusal keeps why `final` was skipped.
#[test]
fn an_integer_colortex0_without_a_working_final_is_refused() {
    let Some(gpu) = Fixture::new() else { return };
    for last in [None, Some(BROKEN_FINAL)] {
        let Err(refused) = gpu.try_build(pack(false, last)) else {
            panic!("{last:?}: a pack that can't show colortex0 runs");
        };
        assert_eq!(
            refused.notes.iter().any(|n| n.starts_with("final")),
            last.is_some(),
            "{refused:?}"
        );
    }
}
