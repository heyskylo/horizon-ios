//! The rule for new names (docs/api/shaders-reference-0.2.md, "Room to grow"): what Horizon puts
//! before a 0.2 pack's files gains a name only where the pack asked for it, by a manifest key or
//! by a stage added later. A pack names its line (`api = "0.2"`), not the additions it was
//! written against, so a name added to a prelude it already has would break the pack that uses
//! that name for something of its own.
//!
//! `tests/names-0.2/<stage>.txt` lists every name each stage's prelude declares for a pack that
//! sets no key bringing more. To add a name to a stage a released pack may have, put it behind a
//! manifest key (and compose the pack here without it), or add its line to the list in a pull
//! request that says why no pack can hold the name. A stage added later starts its own list:
//! no pack had it, so its names collide with nothing.
use horizon_shader_api::{Api, MANIFEST, Pack, Problem, check, compose};
use std::collections::BTreeSet;

const STAGES: &str = "[sky]\nfile = \"sky.wgsl\"\n[material]\nfile = \"material.wgsl\"\n\
                      [vertex]\nfile = \"vertex.wgsl\"\n[weather]\nfile = \"weather.wgsl\"\n\
                      [[post]]\nfile = \"post.wgsl\"\n[tonemap]\nfile = \"tonemap.wgsl\"\n";

/// Each stage written as plainly as it can be, by its file.
const FILES: [(&str, &str); 6] = [
    (
        "sky.wgsl",
        "fn sky(input: SkyInput) -> vec4<f32> { return input.color; }\n",
    ),
    (
        "material.wgsl",
        "fn material(input: MaterialInput) -> Surface { return Surface(input.albedo); }\n",
    ),
    (
        "vertex.wgsl",
        "fn vertex(input: VertexInput) -> vec3<f32> { return input.position; }\n",
    ),
    (
        "weather.wgsl",
        "fn weather(input: WeatherInput) -> vec4<f32> { return input.color; }\n",
    ),
    (
        "post.wgsl",
        "fn post(input: PostInput) -> vec4<f32> { return input.color; }\n",
    ),
    (
        "tonemap.wgsl",
        "fn tonemap(input: TonemapInput) -> vec3<f32> { return input.color; }\n",
    ),
];

/// A 0.2 pack with every stage and no other key, `replaced` standing in for one of its files.
fn pack(replaced: Option<(&str, &str)>) -> Pack {
    let manifest =
        format!("schema = 1\nid = \"p\"\nname = \"P\"\nversion = \"1\"\napi = \"0.2\"\n{STAGES}");
    let mut files: std::collections::BTreeMap<String, Vec<u8>> = FILES
        .iter()
        .map(|(name, text)| ((*name).to_owned(), text.as_bytes().to_vec()))
        .collect();
    files.insert(MANIFEST.into(), manifest.into_bytes());
    if let Some((name, text)) = replaced {
        files.insert(name.into(), text.as_bytes().to_vec());
    }
    Pack::from_files(files).unwrap()
}

/// Every name `wgsl` declares at its top level, read from the text so that nothing a parser
/// folds away (an alias) is missed: the identifier after `fn`, `struct`, `alias`, `const`,
/// `override` or `var`, wherever the lines break.
fn declared(wgsl: &str) -> BTreeSet<String> {
    let text: String = wgsl
        .lines()
        .map(|line| line.split("//").next().unwrap())
        .flat_map(|line| line.chars().chain(['\n']))
        .collect();
    let text = text.replace('{', " { ").replace('}', " } ");
    let mut tokens = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || "_{}".contains(c)))
        .filter(|t| !t.is_empty());
    let mut names = BTreeSet::new();
    let mut depth = 0usize;
    while let Some(token) = tokens.next() {
        match token {
            "{" => depth += 1,
            "}" => depth -= 1,
            "fn" | "struct" | "alias" | "const" | "override" | "var" if depth == 0 => {
                // `var<uniform> frame`, `var<private> x`: the address space comes first.
                let name = tokens
                    .by_ref()
                    .find(|t| !["uniform", "private", "storage", "read", "read_write"].contains(t));
                names.extend(name.map(str::to_owned));
            }
            _ => {}
        }
    }
    names
}

/// The scan finds what naga folds away, however it is laid out.
#[test]
fn declared_reads_aliases_across_lines() {
    let found = declared(
        "alias\n  Wide = vec4<f32>; alias Thin = f32; // alias Not = u32;\nfn f() { const inner = 1; }\n",
    );
    assert_eq!(
        found,
        BTreeSet::from(["Wide", "Thin", "f"].map(str::to_owned))
    );
}

/// What naga finds in the same text, to hold `declared` to: it must miss nothing naga sees.
fn parsed(wgsl: &str) -> BTreeSet<String> {
    let text = format!("{wgsl}{}", horizon_shader_api::stages::PBR_STUB);
    let m = naga::front::wgsl::parse_str(&text).unwrap();
    let types = m.types.iter().filter_map(|(_, t)| t.name.clone());
    let functions = m.functions.iter().filter_map(|(_, f)| f.name.clone());
    let globals = m
        .global_variables
        .iter()
        .filter_map(|(_, g)| g.name.clone());
    let constants = m.constants.iter().filter_map(|(_, c)| c.name.clone());
    let overrides = m.overrides.iter().filter_map(|(_, o)| o.name.clone());
    let all = types.chain(functions).chain(globals).chain(constants);
    all.chain(overrides).collect()
}

#[test]
fn a_0_2_prelude_declares_only_the_names_on_its_list() {
    let pack = pack(None);
    for &kind in Api::V0_2.stages() {
        let file = format!("{}.wgsl", kind.name());
        let composed = compose(&pack, kind, &file);
        let (part, range) = &composed.parts[0];
        assert_eq!(part, "<prelude>");
        let prelude = &composed.text[range.clone()];
        // Names starting with `horizon_` are Horizon's by the API's rules: a pack can't hold one.
        let have: BTreeSet<String> = declared(prelude)
            .into_iter()
            .filter(|n| !n.starts_with("horizon_"))
            .collect();
        let seen = parsed(prelude);
        let missed: Vec<_> = seen
            .iter()
            .filter(|n| !n.starts_with("horizon_") && !have.contains(*n))
            .collect();
        assert!(missed.is_empty(), "{kind:?}: `declared` misses {missed:?}");

        let path = format!(
            "{}/tests/names-0.2/{}.txt",
            env!("CARGO_MANIFEST_DIR"),
            kind.name()
        );
        let list = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let listed: BTreeSet<String> = list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_owned)
            .collect();
        let new: Vec<_> = have.difference(&listed).collect();
        assert!(
            new.is_empty(),
            "the {} stage's 0.2 prelude now declares {new:?}: a pack written before may use that name for \
             something of its own and would stop loading. Bring it with a manifest key or a new stage, or add \
             it to {path} in a pull request that says why no pack can hold it (see this file's top).",
            kind.name()
        );
        let gone: Vec<_> = listed.difference(&have).collect();
        assert!(
            gone.is_empty(),
            "the {} stage's 0.2 prelude no longer declares {gone:?}, which packs may use: {path}",
            kind.name()
        );
    }
}

fn messages(result: Result<horizon_shader_api::Report, Vec<Problem>>) -> Vec<String> {
    result
        .expect_err("should fail")
        .iter()
        .map(Problem::to_string)
        .collect()
}

/// Why the rule is needed: a pack's own function by a name its stage's prelude has is refused,
/// so a name added to that prelude later would refuse a pack that passed before.
#[test]
fn a_pack_s_own_name_collides_only_with_its_own_stage_s_prelude() {
    let own = "fn depth_at(uv: vec2<f32>) -> f32 { return uv.x; }\n";
    let post = format!("{own}fn post(input: PostInput) -> vec4<f32> {{ return input.color; }}\n");
    let problems = messages(check(&pack(Some(("post.wgsl", &post)))));
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("post.wgsl:1:") && p.contains("redefinition of `depth_at`")),
        "{problems:#?}"
    );
    // The sky prelude has no `depth_at`: there the name is the pack's.
    let sky = format!("{own}fn sky(input: SkyInput) -> vec4<f32> {{ return input.color; }}\n");
    check(&pack(Some(("sky.wgsl", &sky)))).unwrap();
    // No prelude has `to_bt709` for a pack that doesn't ask for it: the helpers `[frame]
    // primaries = "bt2020"` will bring reach only the packs that set the key.
    let post = "fn to_bt709(c: vec3<f32>) -> vec3<f32> { return c; }\n\
                fn post(input: PostInput) -> vec4<f32> { return vec4<f32>(to_bt709(input.color.rgb), 1.0); }\n";
    check(&pack(Some(("post.wgsl", post)))).unwrap();
}
