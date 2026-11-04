//! Original terrain-fade controls, authored from the allowed pack API names only.
use horizon_shaders::{
    GbuffersInputs, Macros, Preprocessed, Properties, Target, translate_composite,
    translate_gbuffers,
};

#[test]
fn optional_fade_feature_selects_the_pack_transition() {
    let mut macros = Macros::standard();
    let properties = Properties::parse(
        "iris.features.optional=FADE_VARIABLE\niris.features.required=FADE_VARIABLE\n",
        &macros,
    );
    let notes = macros.define_features(&properties);
    assert!(notes.is_empty(), "requested fade rejected: {notes:?}");
    let selected = Properties::parse(
        "#ifdef IRIS_FEATURE_FADE_VARIABLE\nprobe=transition\n#else\nprobe=opaque\n#endif\n",
        &macros,
    );
    assert_eq!(selected.get("probe"), Some("transition"));
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
fn fade_feature_preserves_negotiation_and_pack_case() {
    for (request, expected, rejected) in [
        ("", "opaque", false),
        ("iris.features.required=FADE_VARIABLE\n", "opaque", false),
        (
            "iris.features.optional=fade_variable\n",
            "transition",
            false,
        ),
        ("iris.features.required=UNKNOWN_FADE\n", "opaque", true),
    ] {
        let mut macros = Macros::standard();
        let properties = Properties::parse(request, &macros);
        assert_eq!(!macros.define_features(&properties).is_empty(), rejected);
        let selected = Properties::parse(
            "#ifdef IRIS_FEATURE_fade_variable\nprobe=transition\n#else\nprobe=opaque\n#endif\n",
            &macros,
        );
        assert_eq!(selected.get("probe"), Some(expected));
    }
}
