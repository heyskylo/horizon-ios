//! In deferred, composite and final programs the atlas's sampler names (`gtexture`, `tex`,
//! `texture`) and names the game gives no texture read texture unit 0, which OptiFine's
//! shaders.txt gives to `colortex0` there. Our probe packs (#9307) read the scene in `colortex0`
//! through each in Iris 1.11.7, also in programs that don't declare `colortex0`.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv = gl_MultiTexCoord0.xy; gl_Position = ftransform(); }\n";

/// deferred writes a colour into colortex0; composite reads it through `gtexture` into
/// colortex1, composite1 through `tex` into colortex2, composite2 through a name of its own into
/// colortex3; final shows them in that order, left to right, then what its own `texture` reads.
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
    for program in ["deferred", "composite", "composite1", "composite2", "final"] {
        add(&format!("{program}.vsh"), VERTEX);
    }
    add(
        "deferred.fsh",
        "#version 120\nvoid main() {\n/* DRAWBUFFERS:0 */\n    gl_FragData[0] = vec4(0.0, 200.0 / 255.0, 40.0 / 255.0, 1.0);\n}\n",
    );
    add(
        "composite.fsh",
        "#version 130\nuniform sampler2D gtexture;\nvarying vec2 uv;\nvoid main() {\n/* DRAWBUFFERS:1 */\n    gl_FragData[0] = texture(gtexture, uv);\n}\n",
    );
    add(
        "composite1.fsh",
        "#version 130\nuniform sampler2D tex;\nvarying vec2 uv;\nvoid main() {\n/* DRAWBUFFERS:2 */\n    gl_FragData[0] = texture(tex, uv);\n}\n",
    );
    add(
        "composite2.fsh",
        "#version 130\nuniform sampler2D probeOwnName;\nvarying vec2 uv;\nvoid main() {\n/* DRAWBUFFERS:3 */\n    gl_FragData[0] = texture(probeOwnName, uv);\n}\n",
    );
    add(
        "final.fsh",
        "#version 120\nuniform sampler2D texture;\nuniform sampler2D colortex1;\nuniform sampler2D colortex2;\n\
         uniform sampler2D colortex3;\nvarying vec2 uv;\nvoid main() {\n\
         \x20   gl_FragColor = uv.x < 0.25 ? texture2D(colortex1, uv) : uv.x < 0.5 ? texture2D(colortex2, uv)\n\
         \x20       : uv.x < 0.75 ? texture2D(colortex3, uv) : texture2D(texture, uv);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-atlas-names-in-post",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

#[test]
fn atlas_names_read_colortex0_in_deferred_composite_and_final() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    let pixels = gpu.render(&mut post, [8, 8]);
    for (i, p) in pixels.iter().enumerate() {
        let name = match i % 8 {
            0..2 => "gtexture in composite",
            2..4 => "tex in composite1",
            4..6 => "a name of the pack's in composite2",
            _ => "texture in final",
        };
        assert_eq!(*p, [0, 200, 40, 255], "{name}, pixel {i}");
    }
}
