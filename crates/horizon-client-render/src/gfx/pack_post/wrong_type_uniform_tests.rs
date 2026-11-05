//! A built-in uniform declared with another GLSL type than the one Iris 1.11.7 gives is disabled
//! there ("Wrong uniform type ... Disabling that uniform") and reads 0; our probe packs (#9307)
//! read 0 for each such declaration. Declared with its own type, it keeps its value.
use super::destination_tests::Fixture;
use super::*;
use std::io::{Cursor, Write};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { uv = gl_MultiTexCoord0.xy; gl_Position = ftransform(); }\n";

/// composite declares two floats as `int` and `vec2` and an int as `float`, and writes what it
/// reads into colortex1; final declares the same names with their own types and adds them.
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
    add("final.vsh", VERTEX);
    add(
        "composite.fsh",
        "#version 130\nuniform int viewWidth;\nuniform vec2 viewHeight;\nuniform float fogShape;\n\
         void main() {\n/* DRAWBUFFERS:1 */\n\
         \x20   gl_FragData[0] = vec4(float(viewWidth), viewHeight.x, fogShape + 1.0, 255.0) / 255.0;\n}\n",
    );
    add(
        "final.fsh",
        "#version 130\nuniform sampler2D colortex1;\nuniform float viewWidth;\nuniform int fogShape;\n\
         varying vec2 uv;\nvoid main() {\n\
         \x20   vec4 c = texture(colortex1, uv);\n\
         \x20   gl_FragColor = vec4(c.r, c.g + viewWidth * 4.0 / 255.0, c.b + float(fogShape + 2) * 10.0 / 255.0, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-wrong-uniform-types",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default())
}

#[test]
fn built_ins_declared_with_another_type_read_zero() {
    let Some(gpu) = Fixture::new() else { return };
    let mut post = gpu.build(pack());
    // 8 pixels wide; post programs read `fogShape` -1. composite reads 0, 0 and 0 (+1 = 1);
    // final reads 8 (green 32) and -1 (+2 = 1, blue 10).
    for p in gpu.render(&mut post, [8, 8]) {
        assert_eq!(p, [0, 32, 11, 255]);
    }
}
