//! A custom uniform of `shaders.properties` that the GLSL declares with another type: an original
//! probe (#9307). In Iris 1.11.7 our probe packs read 0 for a float custom uniform declared `int`,
//! an int declared `float`, a vec3 declared `vec2` and a vec2 declared `vec3`, and the uniform's
//! own value where the types agree. A float of 2.5 declared `bool` read true (Iris logged it as
//! disabled, but it read true in both games).
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
    add(
        "shaders.properties",
        "uniform.float.floatAsInt = 7.0\n\
         uniform.int.intAsFloat = 7\n\
         uniform.vec3.vec3AsVec2 = vec3(1.0, 2.0, 3.0)\n\
         uniform.vec2.vec2AsVec3 = vec2(4.0, 5.0)\n\
         uniform.float.floatAsFloat = 0.5\n\
         uniform.int.intAsInt = 3\n\
         uniform.float.floatAsBool = 2.5\n",
    );
    add("final.vsh", VERTEX);
    // Pixel column x shows case x % 7 in red.
    add(
        "final.fsh",
        "#version 330 compatibility\n\
         uniform int floatAsInt;\nuniform float intAsFloat;\nuniform vec2 vec3AsVec2;\n\
         uniform vec3 vec2AsVec3;\nuniform float floatAsFloat;\nuniform int intAsInt;\n\
         uniform bool floatAsBool;\n\
         void main() {\n    int x = int(gl_FragCoord.x) % 7;\n    float o = 0.0;\n\
             if (x == 0) o = float(floatAsInt) / 10.0;\n\
             else if (x == 1) o = intAsFloat / 10.0;\n\
             else if (x == 2) o = (vec3AsVec2.x + vec3AsVec2.y) / 10.0;\n\
             else if (x == 3) o = (vec2AsVec3.x + vec2AsVec3.y) / 10.0;\n\
             else if (x == 4) o = floatAsFloat;\n\
             else if (x == 5) o = float(intAsInt) / 10.0;\n\
             else o = floatAsBool ? 1.0 : 0.0;\n\
             gl_FragColor = vec4(o, 0.0, 0.0, 1.0);\n}\n",
    );
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original-custom-types",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let mut macros = horizon_shaders::Macros::standard();
    let (properties, _) = horizon_shaders::read_properties(&pack, &mut macros);
    let post = horizon_shaders::load_post(&pack, "", &properties, &macros, &Default::default());
    assert_eq!(post.programs.len(), 1, "{:?}", post.notes);
    post
}

#[test]
fn a_custom_uniform_declared_with_another_type_reads_zero() {
    let Some(gpu) = Fixture::new() else { return };
    let Ok(mut post) = gpu.try_build(pack()) else {
        panic!("the pack runs")
    };
    let pixels = gpu.render(&mut post, [7, 1]);
    let red: Vec<u8> = pixels[..7].iter().map(|p| p[0]).collect();
    // 0 for the four mismatches, 0.5 and 0.3 where the types agree, then true.
    let want = [0.0, 0.0, 0.0, 0.0, 0.5, 0.3, 1.0].map(|v: f32| (v * 255.0).round() as u8);
    assert!(
        red.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
        "{red:?}, expected {want:?}"
    );
}
