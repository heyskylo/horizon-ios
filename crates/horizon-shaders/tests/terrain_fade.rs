//! Original terrain-fade controls, authored from the allowed pack API names only.
use horizon_shaders::{
    GbuffersInputs, GeometryInputs, Macros, Preprocessed, Properties, Target, translate_composite,
    translate_gbuffers, translate_geometry,
};

#[test]
fn requested_fade_feature_loads() {
    let properties = Properties::parse(
        "iris.features.optional=FADE_VARIABLE\niris.features.required=FADE_VARIABLE\n",
        &Macros::standard(),
    );
    let notes = Macros::feature_notes(&properties);
    assert!(notes.is_empty(), "requested fade rejected: {notes:?}");
}

#[test]
fn terrain_vertex_reads_renderer_fade_without_a_pack_declaration() {
    let pp = |code: &str| Preprocessed {
        code: code.into(),
        ..Default::default()
    };
    let vertex =
        pp("varying float seen;\nvoid main() { seen = mc_chunkFade; gl_Position = gl_Vertex; }");
    let fragment =
        pp("varying float seen;\nvoid main() { gl_FragColor = vec4(seen, 0.0, 0.0, 1.0); }");
    let inputs = GbuffersInputs {
        attributes: vec![("mc_chunkFade".into(), "0.375".into())],
        ..Default::default()
    };
    let result = translate_gbuffers(&vertex, &fragment, Target::Buffers, &inputs);
    assert!(
        result.is_ok(),
        "terrain fade translation: {:?}",
        result.err()
    );
}

#[test]
fn missing_terrain_value_is_an_error_instead_of_an_invented_fade() {
    let vertex = Preprocessed {
        code: "void main() { gl_Position = vec4(mc_chunkFade); }".into(),
        ..Default::default()
    };
    let fragment = Preprocessed {
        code: "void main() { gl_FragColor = vec4(1.0); }".into(),
        ..Default::default()
    };
    assert!(
        translate_gbuffers(
            &vertex,
            &fragment,
            Target::Buffers,
            &GbuffersInputs::default()
        )
        .is_err()
    );
    assert!(translate_composite(&vertex, &fragment, Target::Buffers).is_err());
    let supplied = GbuffersInputs {
        attributes: vec![("mc_chunkFade".into(), "0.25".into())],
        ..Default::default()
    };
    let fragment = Preprocessed {
        code: "void main() { gl_FragColor = vec4(mc_chunkFade); }".into(),
        ..Default::default()
    };
    let vertex = Preprocessed {
        code: "void main() { gl_Position = gl_Vertex; }".into(),
        ..Default::default()
    };
    assert!(translate_gbuffers(&vertex, &fragment, Target::Buffers, &supplied).is_err());
}

#[test]
fn authored_fade_globals_keep_their_own_meaning() {
    let inputs = GbuffersInputs {
        attributes: vec![("mc_chunkFade".into(), "0.25".into())],
        ..Default::default()
    };
    let fragment = Preprocessed {
        code: "void main() { gl_FragColor = vec4(1.0); }".into(),
        ..Default::default()
    };
    for declaration in [
        "const float mc_chunkFade = 0.75;",
        "uniform float mc_chunkFade;",
    ] {
        let vertex = Preprocessed {
            code: format!("{declaration}\nvoid main() {{ gl_Position = vec4(mc_chunkFade); }}"),
            ..Default::default()
        };
        let program = translate_gbuffers(&vertex, &fragment, Target::Buffers, &inputs).unwrap();
        assert_eq!(
            program
                .interface
                .uniforms
                .iter()
                .any(|u| u.name == "mc_chunkFade"),
            declaration.starts_with("uniform")
        );
    }
}

#[test]
fn fade_feature_negotiation_ignores_case_and_defines_no_macro() {
    for (request, rejected) in [
        ("", false),
        ("iris.features.required=FADE_VARIABLE\n", false),
        ("iris.features.required=fade_variable\n", false),
        ("iris.features.optional=fade_variable\n", false),
        ("iris.features.required=UNKNOWN_FADE\n", true),
    ] {
        let macros = Macros::standard();
        let properties = Properties::parse(request, &macros);
        assert_eq!(!Macros::feature_notes(&properties).is_empty(), rejected);
        // No `IRIS_FEATURE_*` (spec OQ-2): the pack keeps its base path.
        let selected = Properties::parse(
            "#ifdef IRIS_FEATURE_fade_variable\nprobe=transition\n#else\nprobe=opaque\n#endif\n",
            &macros,
        );
        assert_eq!(selected.get("probe"), Some("opaque"));
    }
}

fn geometry_probe(vertex_declaration: &str) -> [Preprocessed; 3] {
    [
        format!("{vertex_declaration}\nout float terrain_weight; void main() {{ terrain_weight = mc_chunkFade; gl_Position = vec4(0.0, 0.0, 0.0, 1.0); }}"),
        "layout(triangles) in; layout(triangle_strip, max_vertices=3) out; in float terrain_weight[]; out float seen; const float mc_chunkFade = 0.8125; void main() { for (int k=0; k<3; ++k) { seen=terrain_weight[k] + mc_chunkFade; gl_Position=gl_in[k].gl_Position; EmitVertex(); } EndPrimitive(); }".into(),
        "in float seen; void main() { gl_FragColor = vec4(seen); }".into(),
    ].map(|code| Preprocessed { code, ..Default::default() })
}

#[test]
fn geometry_vertex_reads_the_provided_fade_and_requires_its_provider() {
    let [vertex, mut geometry, fragment] = geometry_probe("");
    let mut inputs = GeometryInputs {
        inputs: GbuffersInputs {
            body: "float hz_probe_transition = 0.375;".into(),
            attributes: vec![("mc_chunkFade".into(), "hz_probe_transition".into())],
            ..Default::default()
        },
        index: "@".into(),
        ..Default::default()
    };
    let (_, count) = translate_geometry(&vertex, &geometry, &fragment, &inputs).unwrap();
    assert_eq!(count, 1);
    inputs.inputs.attributes.clear();
    geometry.code = geometry.code.replace("mc_chunkFade", "stage_bias");
    assert!(translate_geometry(&vertex, &geometry, &fragment, &inputs).is_err());
}

#[test]
fn geometry_vertex_keeps_authored_fade_globals() {
    let inputs = GeometryInputs {
        inputs: GbuffersInputs {
            attributes: vec![("mc_chunkFade".into(), "0.25".into())],
            ..Default::default()
        },
        index: "@".into(),
        ..Default::default()
    };
    for declaration in [
        "const float mc_chunkFade = 0.75;",
        "uniform float mc_chunkFade;",
    ] {
        let [vertex, mut geometry, fragment] = geometry_probe(declaration);
        geometry.code = geometry.code.replace("mc_chunkFade", "stage_bias");
        let (program, _) = translate_geometry(&vertex, &geometry, &fragment, &inputs).unwrap();
        assert_eq!(
            program
                .interface
                .uniforms
                .iter()
                .any(|u| u.name == "mc_chunkFade"),
            declaration.starts_with("uniform")
        );
    }
}
