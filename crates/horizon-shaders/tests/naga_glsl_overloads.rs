//! GLSL overload resolution (GLSL 4.60 §6.1): an exact match on an argument beats any implicit
//! conversion there, so `greaterThanEqual(ivec3, uvec3)` takes the uvec3 overload (int to uint on
//! the first argument, exact on the second) over the vec3 one (both converted to float).

fn parse(body: &str) -> Result<naga::Module, naga::front::glsl::ParseErrors> {
    let source =
        format!("#version 450\nlayout(location = 0) out vec4 color;\nvoid main() {{\n{body}\n}}\n");
    naga::front::glsl::Frontend::default().parse(
        &naga::front::glsl::Options::from(naga::ShaderStage::Fragment),
        &source,
    )
}

/// The scalar kinds the arguments of the one `>=` in `main` were converted to.
fn compared_as(body: &str) -> naga::ScalarKind {
    let module = parse(body).unwrap_or_else(|e| panic!("{body}\n{e:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{body}\n{e:?}"));
    // `main` itself is a function the entry point calls.
    let f = &module.functions.iter().next().expect("main").1;
    let (left, right) = f
        .expressions
        .iter()
        .find_map(|(_, e)| match *e {
            naga::Expression::Binary {
                op: naga::BinaryOperator::GreaterEqual,
                left,
                right,
            } => Some((left, right)),
            _ => None,
        })
        .expect("a >= comparison");
    let kind = |h: naga::Handle<naga::Expression>| match f.expressions[h] {
        naga::Expression::As { kind, .. } => kind,
        _ => naga::ScalarKind::Uint, // the uvec3 argument itself, unconverted
    };
    assert_eq!(kind(left), kind(right), "{body}");
    kind(left)
}

#[test]
fn ivec_against_uvec_compares_as_uint() {
    let body = "ivec3 a = ivec3(gl_FragCoord.xyz); uvec3 b = uvec3(gl_FragCoord.yzx);\n\
                color = vec4(vec3(greaterThanEqual(a, b)), 1.0);";
    assert_eq!(compared_as(body), naga::ScalarKind::Uint);
}

#[test]
fn equally_ranked_conversions_stay_ambiguous() {
    // int to float and int to uint rank the same, so neither overload is better.
    let source = "#version 450\nlayout(location = 0) out vec4 color;\n\
                  float f(float a) { return a; }\nfloat f(uint a) { return 1.0; }\n\
                  void main() { color = vec4(f(1)); }\n";
    let errors = naga::front::glsl::Frontend::default()
        .parse(
            &naga::front::glsl::Options::from(naga::ShaderStage::Fragment),
            source,
        )
        .unwrap_err()
        .errors;
    assert!(
        errors[0]
            .kind
            .to_string()
            .contains("Ambiguous best function"),
        "{errors:?}"
    );
}
