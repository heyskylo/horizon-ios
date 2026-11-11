//! Original GLSL for issue #8406, exercised through the public translation API.
use std::io::{Cursor, Write};

use horizon_shaders::{
    GameInputs, GbuffersInputs, Macros, Pack, Program, Target, load_post, preprocess,
    read_properties, translate_composite, translate_gbuffers, translate_shadow,
};

const VERTEX: &str = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";

fn pack(files: &[(&str, &str)]) -> Pack {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, text) in files {
        archive
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(text.as_bytes()).unwrap();
    }
    Pack::from_zip_bytes(
        "original-reachability.zip",
        archive.finish().unwrap().into_inner(),
    )
    .unwrap()
}

fn fragment(source: &str) -> Result<Program, Vec<horizon_shaders::Error>> {
    let pack = pack(&[
        ("shaders/composite.vsh", VERTEX),
        ("shaders/composite.fsh", source),
    ]);
    let macros = Macros::standard();
    translate_composite(
        &preprocess(&pack, "composite.vsh", &macros),
        &preprocess(&pack, "composite.fsh", &macros),
        Target::Window,
    )
}

#[test]
fn unused_vertex_helper_in_a_shared_fragment_header_translates() {
    fragment("#version 120\nvec2 texel1() { return gl_MultiTexCoord1.xy; }\nvoid main() { gl_FragColor = vec4(0.5); }\n")
        .unwrap_or_else(|errors| panic!("unused vertex helper rejected: {errors:?}"));
}

#[test]
fn a_shared_helper_is_live_only_in_the_vertex_stage() {
    let pack = pack(&[
        (
            "shaders/lib/shared.glsl",
            "#define LIGHTMAP texel1\nvec2 texel1() { return gl_MultiTexCoord1.xy; }\n",
        ),
        (
            "shaders/composite.vsh",
            "#version 120\n#include \"/lib/shared.glsl\"\nvoid main() { gl_Position = ftransform(); gl_Position.xy += LIGHTMAP(); }\n",
        ),
        (
            "shaders/composite.fsh",
            "#version 120\n#include \"/lib/shared.glsl\"\nvoid main() { gl_FragColor = vec4(0.5); }\n",
        ),
    ]);
    let mut macros = Macros::standard();
    let (properties, notes) = read_properties(&pack, &mut macros);
    assert!(notes.is_empty(), "{notes:?}");
    let v = preprocess(&pack, "composite.vsh", &macros);
    let f = preprocess(&pack, "composite.fsh", &macros);
    translate_composite(&v, &f, Target::Window).unwrap();
    translate_gbuffers(&v, &f, Target::Buffers, &GbuffersInputs::default()).unwrap();
    translate_shadow(&v, &f, &GbuffersInputs::default()).unwrap();
    let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
    assert!(post.notes.is_empty(), "{:?}", post.notes);
    assert_eq!(post.programs.len(), 1);
    assert!(post.programs[0].program.is_some());
}

#[test]
fn required_overloads_prototypes_and_global_initializers_survive() {
    let live = r#"#version 120
float initial() { return 0.25; }
float factor = initial();
float scale(float value);
vec2 scale(vec2 value);
float scale(float value) { return value * factor; }
vec2 scale(vec2 value) { return value * factor; }
void main() { gl_FragColor = vec4(scale(1.0), scale(vec2(2.0)), 1.0); }
"#;
    fragment(live).unwrap();
    fragment(&format!(
        "{live}\nvec2 unused() {{ return gl_MultiTexCoord1.xy; }}\n"
    ))
    .unwrap();
}

#[test]
fn only_unreachable_call_chains_are_removed() {
    let helpers = "#version 120\nvec2 texel1() { return gl_MultiTexCoord1.xy; }\nvec4 relay() { return vec4(texel1(), 0.0, 1.0); }\n";
    fragment(&format!(
        "{helpers}void main() {{ gl_FragColor = vec4(0.5); }}\n"
    ))
    .unwrap();
    let errors = fragment(&format!(
        "{helpers}void main() {{ gl_FragColor = relay(); }}\n"
    ))
    .err()
    .expect("reachable vertex input must remain an error");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("gl_MultiTexCoord1")),
        "{errors:?}"
    );
}

#[test]
fn retained_global_initializers_are_reachability_roots() {
    let errors = fragment("#version 120\nvec2 texel1() { return gl_MultiTexCoord1.xy; }\nvec2 saved = texel1();\nvoid main() { gl_FragColor = vec4(0.5); }\n")
        .err().expect("an initializer must not lose its required function");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("gl_MultiTexCoord1")),
        "{errors:?}"
    );
}

#[test]
fn errors_after_removed_helpers_keep_the_include_file_and_line() {
    let pack = pack(&[
        ("shaders/composite.vsh", VERTEX),
        (
            "shaders/composite.fsh",
            "#version 120\n#include \"/lib/fault.glsl\"\nvoid main() { gl_FragColor = used(); }\n",
        ),
        (
            "shaders/lib/fault.glsl",
            "vec2 unused() {\n    return gl_MultiTexCoord1.xy;\n}\nvec4 used() {\n    return vec4(missing_colour);\n}\n",
        ),
    ]);
    let macros = Macros::standard();
    let errors = translate_composite(
        &preprocess(&pack, "composite.vsh", &macros),
        &preprocess(&pack, "composite.fsh", &macros),
        Target::Window,
    )
    .err()
    .expect("the live error must remain");
    assert!(
        errors.iter().any(|e| e.file == "lib/fault.glsl"
            && e.line == 5
            && e.message.contains("missing_colour")),
        "{errors:?}"
    );
}
