//! The SDK: every template and the example pack pass the check, and the
//! `horizon-shader` tool makes, checks and packages a pack.
use horizon_shader_api::{Pack, check};
use std::{path::Path, process::Command};

const TOOL: &str = env!("CARGO_BIN_EXE_horizon-shader");

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("horizon-shader-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(TOOL).args(args).output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

#[test]
fn the_example_pack_passes() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/packs/golden_hour"));
    let report = check(&Pack::read(dir).unwrap()).unwrap();
    let labels: Vec<_> = report.stages.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["Warm horizon", "Glow", "Colour grade"]);
}

#[test]
fn the_tool_checks_a_0_2_pack() {
    let dir = scratch("api-0-2");
    let files = [
        (
            "horizon-shaders.toml",
            "schema = 1\nid = \"linear\"\nname = \"Linear\"\nversion = \"1.0.0\"\napi = \"0.2\"\n\
             [[post]]\nfile = \"post.wgsl\"\n[tonemap]\nfile = \"tonemap.wgsl\"\nname = \"Curve\"\n",
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
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let (ok, text) = run(&["check", dir.to_str().unwrap()]);
    assert!(
        ok && text.contains("linear 1.0.0 (shader API 0.2): ok"),
        "{text}"
    );
    assert!(text.contains("  tonemap   Curve"), "{text}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_template_makes_a_pack_that_passes() {
    let root = scratch("templates");
    for template in ["post", "sky", "material"] {
        let dir = root.join(format!("My-{template}"));
        let dir_s = dir.to_str().unwrap();
        let (ok, text) = run(&["new", dir_s, "--template", template]);
        assert!(ok, "{text}");
        let pack = Pack::read(&dir).unwrap();
        assert_eq!(pack.manifest.id, format!("my_{template}"));
        assert_eq!(pack.manifest.name, format!("My {template}"));
        let (ok, text) = run(&["check", dir_s]);
        assert!(ok && text.contains(": ok"), "{text}");
    }
    let (ok, text) = run(&[
        "new",
        root.join("x").to_str().unwrap(),
        "--template",
        "water",
    ]);
    assert!(!ok && text.contains("no template \"water\""), "{text}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn packs_and_checks_the_package() {
    let root = scratch("pack");
    let dir = root.join("glow");
    let dir_s = dir.to_str().unwrap();
    assert!(run(&["new", dir_s, "--name", "Glow"]).0);
    let out = root.join("glow.hshader");
    let (ok, text) = run(&["pack", dir_s, "--out", out.to_str().unwrap()]);
    assert!(ok, "{text}");
    let sha = text
        .split("sha256 ")
        .nth(1)
        .unwrap()
        .trim_end_matches([')', '\n'])
        .to_owned();
    let (ok, json) = run(&["check", out.to_str().unwrap(), "--json"]);
    assert!(ok, "{json}");
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report[0]["ok"], true);
    assert_eq!(report[0]["id"], "glow");
    assert_eq!(report[0]["sha256"], sha.as_str());
    assert_eq!(report[0]["stages"][0]["stage"], "post");

    std::fs::write(
        dir.join("post.wgsl"),
        "fn post(input: PostInput) -> vec4<f32> { return input.color }\n",
    )
    .unwrap();
    let (ok, json) = run(&["check", dir_s, "--json"]);
    assert!(!ok);
    let report: serde_json::Value =
        serde_json::from_str(json.split("\nerror:").next().unwrap()).unwrap();
    assert_eq!(report[0]["ok"], false);
    assert_eq!(report[0]["problems"][0]["file"], "post.wgsl");
    assert_eq!(report[0]["problems"][0]["line"], 1);
    let (ok, text) = run(&["pack", dir_s]);
    assert!(!ok && text.contains("failed the check"), "{text}");
    std::fs::remove_dir_all(root).unwrap();
}
