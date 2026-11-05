//! The stages' WGSL against the Rust definitions: the preludes of both lines compile, the
//! frame's WGSL layout is the uploaded struct's, 0.1's WGSL is what it always was, and
//! docs/api/shaders-reference.md and shaders-reference-0.2.md are up to date. Rewrite the
//! references with `HORIZON_WRITE_DOCS=1 cargo test -p horizon-shader-api --test reference`.
use horizon_shader_api::{Api, Frame, Pack, StageKind, compose, package::sha256_hex, reference};
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn validate(text: &str) -> naga::Module {
    let module = naga::front::wgsl::parse_str(text)
        .unwrap_or_else(|e| panic!("{}\n{text}", e.emit_to_string(text)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{}\n{text}", e.emit_to_string(text)));
    module
}

/// Vanilla's look, written as each stage.
fn vanilla(kind: StageKind) -> &'static str {
    match kind {
        StageKind::Sky => "fn sky(input: SkyInput) -> vec4<f32> { return input.color; }\n",
        StageKind::Material => {
            "fn material(input: MaterialInput) -> Surface {\n    \
             return Surface(input.albedo * input.tint * vec4<f32>(input.lightmap, 1.0));\n}\n"
        }
        StageKind::Vertex => {
            "fn vertex(input: VertexInput) -> vec3<f32> { return input.position; }\n"
        }
        StageKind::Weather => {
            "fn weather(input: WeatherInput) -> vec4<f32> { return input.color; }\n"
        }
        StageKind::Post => "fn post(input: PostInput) -> vec4<f32> { return input.color; }\n",
        StageKind::Tonemap => {
            "fn tonemap(input: TonemapInput) -> vec3<f32> { return input.color; }\n"
        }
    }
}

#[test]
fn every_stage_compiles_with_vanilla_s_function() {
    for api in Api::SERVED {
        for &kind in api.stages() {
            let text = format!(
                "{}{}{}",
                kind.prelude(api),
                vanilla(kind),
                kind.entries(api)
            );
            let module = validate(&text);
            assert!(
                module
                    .entry_points
                    .iter()
                    .all(|e| e.name.starts_with("horizon_")),
                "{api} {kind:?}"
            );
        }
    }
    assert_eq!(Api::V0_1.stages().len(), 5);
    assert!(!Api::V0_1.stages().contains(&StageKind::Tonemap));
}

/// The SHA-256 of 0.1's prelude and entries of each stage, as they were before 0.2: the line
/// packs run on today doesn't move.
const PINNED_0_1: [(StageKind, &str, &str); 5] = [
    (
        StageKind::Sky,
        "5a12b39ea8b56ad8deead04aa848671ceebde05cdd35af029d1bca5fe1e4cb51",
        "8fa92b1f28b48f70f7704cd1db183d5720484c1c92f779542a5805cd8d540fbc",
    ),
    (
        StageKind::Vertex,
        "1ba03e6acd07fc26e267f58fe65aaa607976faab1834d16f55c694e15eacf885",
        "0c512329404218b074bd5140c2dcb4268721ad2b280d5d516a8066ffc2a78f97",
    ),
    (
        StageKind::Material,
        "1f165861a0f74fa58bab41d9770798fd80ccbc12b61fa8ede7bb4804bcbea24e",
        "9dfaf3b191390a268a43bb3ecb8cbc1739e083b4e7c1783b58c76b332afe88cc",
    ),
    (
        StageKind::Weather,
        "174ad066203226f4b28848d6cc2037df75c9ea68481bbff8b73eca469d9b5bb7",
        "6fd42fe9ab23d5aca1ca0bb61d9021cad149e795574d892fcd8178a3dae15032",
    ),
    (
        StageKind::Post,
        "4b1b7b6c7b1149657965a178a879535975ed76d09524c294d4350c6c06db83ab",
        "a1d88de49274e1d9aed65d2a7fd500e9e385a7a7c9fbb6f9064d4bdbd3a8d6da",
    ),
];

#[test]
fn the_0_1_wgsl_is_unchanged() {
    for (kind, prelude, entries) in PINNED_0_1 {
        let have = (
            sha256_hex(kind.prelude(Api::V0_1).as_bytes()),
            sha256_hex(kind.entries(Api::V0_1).as_bytes()),
        );
        assert_eq!(have, (prelude.to_owned(), entries.to_owned()), "{kind:?}");
    }
}

/// The `Frame` struct the line's sky prelude declares, as (name, offset) and its span.
fn frame_layout(api: Api) -> (Vec<(String, usize)>, usize) {
    let text = format!("{}{}", StageKind::Sky.prelude(api), vanilla(StageKind::Sky));
    let module = validate(&text);
    let (_, ty) = module
        .types
        .iter()
        .find(|(_, t)| t.name.as_deref() == Some("Frame"))
        .expect("Frame in the prelude");
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
        panic!("Frame is a struct")
    };
    let members = members
        .iter()
        .map(|m| (m.name.clone().unwrap(), m.offset as usize))
        .collect();
    (members, *span as usize)
}

#[test]
fn the_frame_s_wgsl_layout_is_the_uploaded_struct_s() {
    let rust: Vec<_> = Frame::WGSL
        .fields
        .iter()
        .zip(Frame::OFFSETS)
        .map(|(f, &o)| (f.name.to_owned(), o))
        .collect();
    let (wgsl, span) = frame_layout(Api::V0_2);
    assert_eq!(span, std::mem::size_of::<Frame>());
    assert_eq!(wgsl, rust);
    // 0.1 declares the start of the same buffer.
    let (wgsl, span) = frame_layout(Api::V0_1);
    assert_eq!(wgsl, rust[..Frame::FIELDS_0_1]);
    assert_eq!(span, rust[Frame::FIELDS_0_1].1);
}

#[test]
fn the_sample_pack_s_stages_compile() {
    let dir = root().join("crates/horizon-shader-api/tests/samples/schema-1");
    let pack = Pack::read(&dir).unwrap();
    let m = &pack.manifest;
    let stages = [
        (StageKind::Sky, m.sky.iter().collect::<Vec<_>>()),
        (StageKind::Material, m.material.iter().collect()),
        (StageKind::Post, m.post.iter().collect()),
    ];
    for (kind, list) in stages {
        for stage in list {
            let composed = compose(&pack, kind, &stage.file);
            validate(&composed.text);
            // Errors point into the pack's own file.
            let part = composed.parts.iter().find(|(f, _)| *f == stage.file);
            let part = part.unwrap().1.clone();
            let needle = format!("fn {}(", kind.function());
            let at = part.start + composed.text[part].find(&needle).unwrap();
            let source = pack.source(&stage.file).unwrap();
            let line = source.lines().position(|l| l.starts_with(&needle)).unwrap() + 1;
            assert_eq!(composed.locate(at), (stage.file.as_str(), line, 1));
        }
    }
}

#[test]
fn the_references_are_up_to_date() {
    for api in Api::SERVED {
        let doc = reference::path(api);
        let path = root().join(doc);
        let want = reference::markdown(api);
        if std::env::var_os("HORIZON_WRITE_DOCS").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            have == want,
            "{doc} is out of date: HORIZON_WRITE_DOCS=1 cargo test -p horizon-shader-api --test reference"
        );
    }
}
