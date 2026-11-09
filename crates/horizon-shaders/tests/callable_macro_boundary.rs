//! Original token controls, independently checked with GCC and Clang preprocessing.
//! The shader controls also pass glslang and Horizon's translation/backends.
use horizon_shaders::{Macros, Pack, Target, preprocess, translate_composite};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn macros() -> Macros {
    let mut macros = Macros::default();
    macros.define_function("OFFSET", vec!["x".into()], "((x) + 3)");
    macros.define("ROUTE", "OFFSET");
    macros.define_function("SELECT", vec![], "ROUTE");
    macros.define_function("PASS", vec!["x".into()], "x");
    macros.define_function("CAT", vec!["a".into(), "b".into()], "a ## b");
    macros
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect()
}

#[test]
fn an_object_alias_can_take_arguments_from_the_source() {
    assert_eq!(macros().expand("ROUTE(9)"), "((9) + 3)");
}

#[test]
fn produced_names_and_nested_arguments_are_rescanned_with_the_source() {
    let macros = macros();
    for (source, expected) in [
        ("SELECT()(4)", "((4) + 3)"),
        ("PASS(ROUTE)(OFFSET(2))", "((((2) + 3)) + 3)"),
        ("CAT(OFF, SET)(6)", "((6) + 3)"),
        ("PASS(PASS(ROUTE))(1)", "((1) + 3)"),
        ("PASS(ROUTE(2))", "((2) + 3)"),
        ("CAT(, ROUTE)(3)", "((3) + 3)"),
        ("CAT(ROUTE, )(3)", "((3) + 3)"),
        ("PASS(SELECT())(5)", "((5) + 3)"),
    ] {
        assert_eq!(
            compact(&macros.expand(source)),
            compact(expected),
            "{source}"
        );
    }
}

#[test]
fn recursive_tokens_stay_hidden_without_disabling_independent_source_calls() {
    let mut macros = macros();
    macros.define("NAME", "FUNC");
    macros.define_function("FUNC", vec!["x".into()], "NAME");
    macros.define_function("SELF", vec!["x".into()], "x SELF");
    macros.define("LEFT", "RIGHT");
    macros.define("RIGHT", "LEFT");
    for (source, expected) in [
        ("NAME(1)(2)", "FUNC(2)"),
        ("PASS(NAME)(1)(2)", "FUNC(2)"),
        ("CAT(FU, NC)(1)(2)", "FUNC(2)"),
        ("SELF(SELF)(3)", "SELF SELF(3)"),
        ("PASS(SELF(2))(3)", "2 SELF(3)"),
        ("LEFT(8)", "LEFT(8)"),
        ("PASS(PASS)(6)", "PASS(6)"),
        ("ROUTE(1) ROUTE(2)", "((1) + 3) ((2) + 3)"),
    ] {
        assert_eq!(macros.expand(source), expected, "{source}");
    }
}

#[test]
fn empty_paste_operands_preserve_an_arguments_recursive_hiding() {
    let mut macros = macros();
    macros.define_function("SPIN", vec![], "SPIN");
    macros.define_function("KEEP_LEFT", vec!["x".into()], "CAT(x, )");
    macros.define_function("KEEP_RIGHT", vec!["x".into()], "CAT(, x)");
    for source in ["KEEP_LEFT(SPIN())()", "KEEP_RIGHT(SPIN())()"] {
        assert_eq!(macros.expand(source), "SPIN()", "{source}");
    }
}

#[test]
fn only_a_following_source_parenthesis_forms_a_call() {
    let mut macros = macros();
    macros.define("OPEN", "(");
    for (source, expected) in [
        ("OFFSET ROUTE suffix(5)", "OFFSET OFFSET suffix(5)"),
        ("ROUTE + (7)", "OFFSET + (7)"),
        ("ROUTE(1", "OFFSET(1"),
        ("OFFSET OPEN 7)", "OFFSET ( 7)"),
        ("ROUTE_suffix(7)", "ROUTE_suffix(7)"),
    ] {
        assert_eq!(macros.expand(source), expected, "{source}");
    }
}

struct OriginalPack(PathBuf);

impl OriginalPack {
    fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "horizon-call-boundary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for (path, text) in files {
            let path = root.join("shaders").join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        Self(root)
    }

    fn open(&self) -> Pack {
        Pack::open(&self.0).unwrap()
    }
}

impl Drop for OriginalPack {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

const DEFINITIONS: &str = "#define OFFSET(x) ((x) + 3)\n\
#define ROUTE \\\nOFFSET\n\
#define SELECT() ROUTE\n\
#define PASS(x) x\n";

const VERTEX: &str = "void main() { gl_Position = gl_Vertex; }";
const FRAGMENT: &str = "#version 450 compatibility\n\
#include \"/lib/dispatch.glsl\"\n\
void main() {\n\
    float value = SELECT()(\n\
        PASS(ROUTE)(2.0)\n\
    );\n\
    gl_FragColor = vec4(value / 8.0, 0.0, 0.0, 1.0);\n\
}\n";

#[test]
fn expanded_calls_continue_across_lines_and_keep_original_file_origins() {
    let original = OriginalPack::new(&[
        ("lib/dispatch.glsl", DEFINITIONS),
        ("final.vsh", VERTEX),
        ("final.fsh", FRAGMENT),
    ]);
    let pack = original.open();
    let fragment = preprocess(&pack, "final.fsh", &Macros::default());
    assert!(
        fragment.diagnostics.is_empty(),
        "{:?}",
        fragment.diagnostics
    );
    assert!(fragment.warnings.is_empty(), "{:?}", fragment.warnings);
    let lines: Vec<_> = fragment.code.lines().collect();
    let value = lines
        .iter()
        .position(|l| l.contains("float value"))
        .unwrap();
    assert_eq!(compact(lines[value]), "floatvalue=((((2.0)+3))+3);");
    assert_eq!(fragment.locate(value), "final.fsh:4");
    assert_eq!(fragment.locate(value + 1), "final.fsh:5");
    assert_eq!(fragment.locate(value + 2), "final.fsh:6");
    let color = lines
        .iter()
        .position(|l| l.contains("gl_FragColor"))
        .unwrap();
    assert_eq!(fragment.locate(color), "final.fsh:7");
    assert_eq!(fragment.files, ["final.fsh", "lib/dispatch.glsl"]);

    let vertex = preprocess(&pack, "final.vsh", &Macros::default());
    let program = translate_composite(&vertex, &fragment, Target::Window).unwrap();
    for module in [&program.vertex, &program.fragment] {
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(module)
        .unwrap();
        naga::back::spv::write_vec(module, &info, &Default::default(), None).unwrap();
    }
}

#[test]
fn continuation_detection_does_not_consume_preprocessor_directives() {
    let original = OriginalPack::new(&[(
        "final.fsh",
        "#define ROUTE OFFSET\n#define OFFSET(x) x\nROUTE(\n#define VALUE 9\nVALUE\n)",
    )]);
    let output = preprocess(&original.open(), "final.fsh", &Macros::default());
    assert!(output.diagnostics.is_empty());
    assert_eq!(
        output.code.lines().collect::<Vec<_>>(),
        ["", "", "OFFSET(", "", "9", ")"]
    );
    assert_eq!(output.locate(4), "final.fsh:5");
}

#[test]
fn an_expanded_callable_can_take_an_opening_parenthesis_from_the_next_line() {
    let original = OriginalPack::new(&[(
        "final.fsh",
        "#define ROUTE OFFSET\n#define OFFSET(x) x\nROUTE\n\n(9)\nfloat after;",
    )]);
    let output = preprocess(&original.open(), "final.fsh", &Macros::default());
    assert!(output.diagnostics.is_empty());
    assert_eq!(
        output.code.lines().collect::<Vec<_>>(),
        ["", "", "9", "", "", "float after;"]
    );
    assert_eq!(output.locate(2), "final.fsh:3");
    assert_eq!(output.locate(5), "final.fsh:6");
}
