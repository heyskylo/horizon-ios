//! Original controls for the type of each comma-separated GLSL declarator.
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

fn program(source: &str) -> Result<horizon_shaders::Program, Vec<horizon_shaders::Error>> {
    translate_composite(
        &stage("void main() { gl_Position = gl_Vertex; }"),
        &stage(source),
        Target::Window,
    )
}

fn count(source: &str, expected: i32) -> horizon_shaders::Program {
    let program = program(source).unwrap_or_else(|e| panic!("{e:?}"));
    let constant = program
        .fragment
        .constants
        .iter()
        .find_map(|(_, c)| (c.name.as_deref() == Some("count")).then_some(c))
        .unwrap();
    assert!(
        matches!(program.fragment.global_expressions[constant.init], naga::Expression::Literal(naga::Literal::I32(value)) if value == expected)
    );
    for module in [&program.vertex, &program.fragment] {
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(module)
        .unwrap();
        naga::back::spv::write_vec(module, &info, &naga::back::spv::Options::default(), None)
            .unwrap();
    }
    program
}

#[test]
fn each_sized_array_starts_from_the_common_scalar_type() {
    count(
        "const int first[2] = {1, 2}, second[3] = {3, 4, 5}; const int count = first.length() + second.length(); void main() { gl_FragColor = vec4(float(count)); }",
        5,
    );
}

#[test]
fn constructor_inferred_sizes_are_independent() {
    count(
        "const int first[] = int[](1, 2), second[] = int[](3, 4, 5); const int count = first.length() + second.length(); void main() { gl_FragColor = vec4(float(count)); }",
        5,
    );
}

#[test]
fn scalar_after_an_array_keeps_its_scalar_type() {
    count(
        "const int first[2] = {1, 2}, tail = 4; const int count = first.length() + tail; void main() { gl_FragColor = vec4(float(count)); }",
        6,
    );
}

#[test]
fn a_middle_array_does_not_change_either_scalar() {
    count(
        "const int head = 1, values[2] = {3, 4}, tail = 5; const int count = head + values.length() + tail; void main() { gl_FragColor = vec4(float(count)); }",
        8,
    );
}

#[test]
fn an_array_on_the_common_type_is_retained_for_each_declarator() {
    count(
        "const int[2] first = {1, 2}, second[3] = {{3, 4}, {5, 6}, {7, 8}}; const int count = first.length() + second.length(); void main() { gl_FragColor = vec4(float(count)); }",
        5,
    );
}

#[test]
fn local_array_declarators_keep_independent_sizes() {
    let program = program("void main() { int first[2], second[3]; first[1] = 7; second[2] = 9; gl_FragColor = vec4(float(first[1] + second[2])); }").unwrap_or_else(|e| panic!("{e:?}"));
    let function = program
        .fragment
        .functions
        .iter()
        .find_map(|(_, f)| (f.name.as_deref() == Some("hz_main")).then_some(f))
        .unwrap();
    for (name, expected) in [("first", 2), ("second", 3)] {
        let var = function
            .local_variables
            .iter()
            .find_map(|(_, v)| (v.name.as_deref() == Some(name)).then_some(v))
            .unwrap();
        assert!(
            matches!(program.fragment.types[var.ty].inner, naga::TypeInner::Array { size: naga::ArraySize::Constant(n), .. } if n.get() == expected)
        );
    }
}

#[test]
fn separate_declarations_remain_a_positive_control() {
    count(
        "const int first[2] = {1, 2}; const int second[3] = {3, 4, 5}; const int count = first.length() + second.length(); void main() { gl_FragColor = vec4(float(count)); }",
        5,
    );
}

#[test]
fn invalid_later_initializers_remain_errors() {
    for source in [
        "const int first[2] = {1, 2}, second[3] = {3, 4}; void main() { gl_FragColor = vec4(float(second.length())); }",
        "const int first[2] = {1, 2}, second[3] = {vec2(3.0), 4, 5}; void main() { gl_FragColor = vec4(float(second.length())); }",
    ] {
        assert!(
            program(source).is_err(),
            "accepted invalid source: {source}"
        );
    }
}
