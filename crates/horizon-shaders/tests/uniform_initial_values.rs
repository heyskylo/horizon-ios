//! Original synthetic inputs; no third-party shader code is included.
use horizon_shaders::{Origin, Preprocessed, Target, translate_composite, translate_compute};

fn source(code: &str) -> Preprocessed {
    Preprocessed {
        code: code.into(),
        files: vec!["original.glsl".into()],
        lines: code
            .lines()
            .enumerate()
            .map(|(i, _)| Origin {
                file: 0,
                line: i as u32 + 1,
            })
            .collect(),
        version: Some((430, None)),
        ..Default::default()
    }
}

const VERTEX: &str = "void main() { gl_Position = vec4(0.0); }";
fn program(declarations: &str) -> horizon_shaders::Program {
    translate_composite(
        &source(VERTEX),
        &source(&format!(
            "{declarations}\nout vec4 color; void main() {{ color = vec4(1.0); }}"
        )),
        Target::Buffers,
    )
    .unwrap()
}
fn words(p: &horizon_shaders::Program, name: &str, n: usize) -> Vec<u32> {
    let offset = p
        .interface
        .uniforms
        .iter()
        .find(|u| u.name == name)
        .unwrap()
        .offset as usize;
    p.interface.initial_values[offset..offset + n * 4]
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
#[test]
fn authored_scalar_and_vector_values_use_compiler_arithmetic_and_exact_integer_bits() {
    let p = program(
        "const float scale = 0.125; uniform float gain = scale * 3.0, bias = -0.5; uniform bool enabled = true; uniform uint mask = 4294967295u; uniform ivec4 shifts = ivec4(-7, 2, -3, 9); uniform vec3 tint = vec3(0.25, 0.5, 0.75); uniform float absent;",
    );
    assert_eq!(words(&p, "gain", 1), [0.375f32.to_bits()]);
    assert_eq!(words(&p, "bias", 1), [(-0.5f32).to_bits()]);
    assert_eq!(words(&p, "enabled", 1), [1]);
    assert_eq!(words(&p, "mask", 1), [u32::MAX]);
    assert_eq!(
        words(&p, "shifts", 4),
        [(-7i32) as u32, 2, (-3i32) as u32, 9]
    );
    assert_eq!(
        words(&p, "tint", 3),
        [0.25f32.to_bits(), 0.5f32.to_bits(), 0.75f32.to_bits()]
    );
    assert_eq!(words(&p, "absent", 1), [0]);
}
#[test]
fn matrix_columns_and_arrays_keep_uniform_padding_and_neighbor_values() {
    let p = program(
        "uniform mat3 basis = mat3(1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0); uniform mat4 textureMatrix = mat4(1.0); uniform float stops[3] = float[](0.125, 0.375, 0.875); uniform vec4 after = vec4(0.625);",
    );
    let f = |x: f32| x.to_bits();
    assert_eq!(
        words(&p, "basis", 12),
        [
            f(1.),
            f(2.),
            f(3.),
            0,
            f(4.),
            f(5.),
            f(6.),
            0,
            f(7.),
            f(8.),
            f(9.),
            0
        ]
    );
    assert_eq!(
        words(&p, "textureMatrix", 16),
        [
            f(1.),
            0,
            0,
            0,
            0,
            f(1.),
            0,
            0,
            0,
            0,
            f(1.),
            0,
            0,
            0,
            0,
            f(1.)
        ]
    );
    assert_eq!(
        words(&p, "stops", 12),
        [f(0.125), 0, 0, 0, f(0.375), 0, 0, 0, f(0.875), 0, 0, 0]
    );
    assert_eq!(words(&p, "after", 4), [f(0.625); 4]);
}
#[test]
fn shared_stage_defaults_merge_by_value_and_conflicts_report_original_location() {
    let vertex = source("uniform float gain = 0.25; void main() { gl_Position = vec4(gain); }");
    let fragment = |decl: &str| {
        source(&format!(
            "{decl}\nout vec4 color; void main() {{ color = vec4(gain); }}"
        ))
    };
    let p = translate_composite(
        &vertex,
        &fragment("uniform float gain = 1.0 / 4.0;"),
        Target::Buffers,
    )
    .unwrap();
    assert_eq!(words(&p, "gain", 1), [0.25f32.to_bits()]);
    let p =
        translate_composite(&vertex, &fragment("uniform float gain;"), Target::Buffers).unwrap();
    assert_eq!(words(&p, "gain", 1), [0.25f32.to_bits()]);
    let error = translate_composite(
        &vertex,
        &fragment("uniform float gain = 0.75;"),
        Target::Buffers,
    )
    .err()
    .unwrap();
    assert!(
        error
            .iter()
            .any(|e| e.file == "original.glsl" && e.line == 1 && e.message.contains("conflicting")),
        "{error:?}"
    );
}
#[test]
fn compute_programs_receive_initial_contents_and_nonconstant_uniform_initializers_fail() {
    let p = translate_compute(&source(
        "layout(local_size_x=1) in; uniform vec4 seed = vec4(0.125,0.25,0.5,1.0); void main() {}",
    ))
    .unwrap();
    let u = p
        .interface
        .uniforms
        .iter()
        .find(|u| u.name == "seed")
        .unwrap();
    let b = &p.interface.initial_values[u.offset as usize..u.offset as usize + 16];
    assert_eq!(
        b,
        [0.125f32, 0.25, 0.5, 1.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert!(translate_composite(&source(VERTEX), &source("uniform float runtime; uniform float value = runtime; out vec4 c; void main(){ c=vec4(value); }"), Target::Buffers).is_err());
}
