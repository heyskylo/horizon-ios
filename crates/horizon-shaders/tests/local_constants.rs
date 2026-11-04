//! Original controls for `const` locals: one initialized with a constant expression is a
//! constant expression itself (GLSL 4.60, 4.3.3, as #8462 quotes it), so it may be a texel
//! offset (8.9.2, as #8462 quotes it). glslangValidator takes every shader here.
use horizon_shaders::{Origin, Preprocessed, Target, translate_composite};

fn stage(code: &str) -> Preprocessed {
    Preprocessed {
        code: code.into(),
        files: vec!["original.fsh".into()],
        lines: code
            .lines()
            .enumerate()
            .map(|(i, _)| Origin {
                file: 0,
                line: i as u32 + 1,
            })
            .collect(),
        version: Some((440, Some("compatibility".into()))),
        ..Default::default()
    }
}

fn translates(source: &str) {
    let program = translate_composite(
        &stage("void main() { gl_Position = gl_Vertex; }"),
        &stage(source),
        Target::Window,
    )
    .unwrap_or_else(|e| panic!("{source}\n{e:?}"));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&program.fragment)
    .unwrap();
    naga::back::spv::write_vec(
        &program.fragment,
        &info,
        &naga::back::spv::Options::default(),
        None,
    )
    .unwrap();
}

const SAMPLER: &str = "uniform sampler2D colortex0;\nuniform float k;\n";

#[test]
fn a_const_local_is_a_texel_offset() {
    translates(&format!(
        "{SAMPLER}vec4 f(vec2 p) {{ const ivec2 offset = ivec2(0, -2); return textureLodOffset(colortex0, p, 0.0, offset); }}\n\
         void main() {{ gl_FragColor = f(gl_FragCoord.xy); }}\n"
    ));
}

#[test]
fn a_const_local_in_a_loop_is_a_texel_offset() {
    translates(&format!(
        "{SAMPLER}void main() {{\n    vec4 s = vec4(0.0);\n    for (int i = 0; i < 3; i++) {{\n        \
         const ivec2 step = ivec2(1, 0) * 2;\n        \
         s += textureOffset(colortex0, gl_FragCoord.xy, step) + texelFetchOffset(colortex0, ivec2(gl_FragCoord.xy), 0, step);\n    }}\n    \
         gl_FragColor = s;\n}}\n"
    ));
}

/// A `const` local made of other constants, global and local, and then converted.
#[test]
fn a_const_local_of_other_constants_is_a_texel_offset() {
    translates(&format!(
        "{SAMPLER}const int far = 2;\n\
         void main() {{\n    const int step = far;\n    const int twice = step;\n    \
         const ivec2 offset = ivec2(twice, -step) * 2;\n    \
         gl_FragColor = textureOffset(colortex0, gl_FragCoord.xy, offset) * float(twice);\n}}\n"
    ));
}

/// Controls that already translated: a constant array indexed by a loop counter, and a `const`
/// local whose initializer reads a uniform (not a constant expression; glslangValidator takes it).
#[test]
fn const_locals_that_are_not_offsets_still_translate() {
    translates(&format!(
        "{SAMPLER}void main() {{\n    const float w[3] = float[3](0.25, 0.5, 0.25);\n    const float scale = k * 2.0;\n    \
         float s = 0.0;\n    for (int i = 0; i < 3; i++) s += w[i];\n    gl_FragColor = vec4(s * scale);\n}}\n"
    ));
}
