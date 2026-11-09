//! Original controls for the signed GLSL IDs, independent of any shader pack source.
use horizon_shaders::{
    GbuffersInputs, GeometryInputs, Origin, Preprocessed, Program, Target, translate_composite,
    translate_gbuffers, translate_geometry, translate_shadow,
};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{BinaryOperator, Expression, Scalar, TypeInner};

fn source(code: &str) -> Preprocessed {
    Preprocessed {
        code: code.into(),
        lines: code
            .lines()
            .enumerate()
            .map(|(line, _)| Origin {
                file: 0,
                line: line as u32 + 1,
            })
            .collect(),
        files: vec!["original.glsl".into()],
        version: Some((450, None)),
        ..Default::default()
    }
}

fn translate(code: &str, path: &str) -> Program {
    let vertex = source(code);
    let fragment = source("void main() { gl_FragColor = vec4(1.0); }");
    let inputs = GbuffersInputs::default();
    let result = match path {
        "composite" => translate_composite(&vertex, &fragment, Target::Buffers),
        "gbuffers" => translate_gbuffers(&vertex, &fragment, Target::Buffers, &inputs),
        "shadow" => translate_shadow(&vertex, &fragment, &inputs),
        _ => unreachable!(),
    };
    result.unwrap_or_else(|errors| panic!("{path}: {errors:?}\n{code}"))
}

#[test]
fn signed_ids_accept_signed_expressions_in_each_draw_path() {
    for path in ["composite", "gbuffers", "shadow"] {
        for id in ["gl_VertexID", "gl_InstanceID"] {
            for expression in [
                id.to_owned(),
                format!("{id} - 9"),
                format!("({id} - 9) / 2"),
                format!("({id} - 9) % 3"),
                format!("{id} >> 1"),
                format!("({id} & 3) - 4"),
                format!("{id} == 0 ? -1 : {id}"),
                format!("choose_type({id})"),
            ] {
                translate(
                    &format!(
                        "int signed_only(int x) {{ return x; }}\n\
                         int choose_type(int x) {{ return x; }}\n\
                         uint choose_type(uint x) {{ return x; }}\n\
                         void main() {{ gl_Position = vec4(float(signed_only({expression}))); }}"
                    ),
                    path,
                );
            }
        }
    }
}

fn check_arithmetic(program: &Program, name: &str, expected: Scalar) {
    let module = &program.vertex;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(module)
        .unwrap();
    let (handle, function) = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some(name))
        .unwrap();
    let mut operations = Vec::new();
    for (_, expression) in function.expressions.iter() {
        if let Expression::Binary { op, left, .. } = *expression
            && matches!(
                op,
                BinaryOperator::Subtract
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo
                    | BinaryOperator::ShiftRight
            )
        {
            assert_eq!(
                info[handle][left].ty.inner_with(&module.types),
                &TypeInner::Scalar(expected),
                "{name}: {op:?} must use the authored integer type before the operation"
            );
            operations.push(op);
        }
    }
    for op in [
        BinaryOperator::Subtract,
        BinaryOperator::Divide,
        BinaryOperator::Modulo,
        BinaryOperator::ShiftRight,
    ] {
        assert!(
            operations.contains(&op),
            "missing exercised operation {op:?}"
        );
    }
}

#[test]
fn id_arithmetic_is_signed_before_operations_and_explicit_uint_stays_unsigned() {
    let code = "ivec3 signed_math() { return ivec3((gl_VertexID - 7) / 2, (gl_InstanceID - 5) % 3, gl_VertexID >> 1); }\n\
                uvec3 unsigned_math() { return uvec3((uint(gl_VertexID) - 7u) / 2u, (uint(gl_InstanceID) - 5u) % 3u, uint(gl_VertexID) >> 1); }\n\
                void main() { gl_Position = vec4(vec3(signed_math()) + vec3(unsigned_math()), 1.0); }";
    for path in ["composite", "gbuffers", "shadow"] {
        let program = translate(code, path);
        check_arithmetic(&program, "signed_math", Scalar::I32);
        check_arithmetic(&program, "unsigned_math", Scalar::U32);
    }
}

#[test]
fn folded_geometry_keeps_source_vertex_ids_signed() {
    let vertex = source(
        "out float value;\n\
         int signed_only(int x) { return x; }\n\
         void main() { value = float(signed_only(gl_VertexID - 7) + signed_only(gl_InstanceID - 5)); gl_Position = vec4(0, 0, 0, 1); }",
    );
    let geometry = source(
        "layout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\n\
         in float value[]; out float result;\n\
         void main() { for (int i = 0; i < 3; i++) { result = value[i]; gl_Position = gl_in[i].gl_Position; EmitVertex(); } EndPrimitive(); }",
    );
    let fragment = source("in float result; void main() { gl_FragColor = vec4(result); }");
    let (_, triangles) = translate_geometry(
        &vertex,
        &geometry,
        &fragment,
        &GeometryInputs {
            index: "uint(@)".into(),
            ..Default::default()
        },
    )
    .unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(triangles, 1);
}
