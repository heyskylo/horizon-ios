//! The canonical-JSON rules of docs/perf-platform.md §3.3, enforced over the compiled descriptor set.

use std::collections::HashMap;
use std::path::PathBuf;

use prost_types::field_descriptor_proto::Type;
use prost_types::{DescriptorProto, EnumDescriptorProto, FileDescriptorSet};

fn lint(set: &FileDescriptorSet) -> Vec<String> {
    let mut problems = Vec::new();
    for file in &set.file {
        let name = file.name();
        if file.syntax() != "proto3" {
            problems.push(format!("{name}: syntax {:?}, not proto3", file.syntax()));
        }
        for message in &file.message_type {
            lint_message(
                &format!("{name}:{}", message.name()),
                message,
                &mut problems,
            );
        }
        for e in &file.enum_type {
            lint_enum(&format!("{name}:{}", e.name()), e, &mut problems);
        }
    }
    problems
}

fn lint_message(path: &str, message: &DescriptorProto, problems: &mut Vec<String>) {
    let mut json_names: HashMap<String, &str> = HashMap::new();
    for field in &message.field {
        let name = field.name();
        if !is_lower_snake_case(name) {
            problems.push(format!("{path}.{name}: not lower_snake_case"));
        }
        if field.r#type() == Type::Group {
            problems.push(format!("{path}.{name}: a group"));
        }
        if field.type_name() == ".google.protobuf.Any" {
            problems.push(format!("{path}.{name}: google.protobuf.Any"));
        }
        if is_time_name(name) && !is_number(field.r#type()) {
            problems.push(format!("{path}.{name}: a time that is not a number"));
        }
        let json = field
            .json_name
            .clone()
            .unwrap_or_else(|| to_json_name(name));
        if let Some(other) = json_names.insert(json.clone(), name) {
            problems.push(format!(
                "{path}.{name}: JSON name {json:?} collides with {other}"
            ));
        }
    }
    for nested in &message.nested_type {
        if nested.options.as_ref().is_some_and(|o| o.map_entry()) {
            let key = nested
                .field
                .iter()
                .find(|f| f.number() == 1)
                .map(|f| f.r#type());
            if !key.is_some_and(|k| k == Type::String || is_integer(k)) {
                problems.push(format!(
                    "{path}.{}: a map key that is not a string or an integer",
                    nested.name()
                ));
            }
        }
        lint_message(&format!("{path}.{}", nested.name()), nested, problems);
    }
    for e in &message.enum_type {
        lint_enum(&format!("{path}.{}", e.name()), e, problems);
    }
}

fn lint_enum(path: &str, e: &EnumDescriptorProto, problems: &mut Vec<String>) {
    let zero = e.value.iter().find(|v| v.number() == 0);
    if !zero.is_some_and(|v| v.name().ends_with("_UNSPECIFIED")) {
        problems.push(format!("{path}: no *_UNSPECIFIED = 0 value"));
    }
}

fn is_lower_snake_case(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.split('_').all(|part| !part.is_empty())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_time_name(name: &str) -> bool {
    matches!(name, "t" | "t0") || ["_t", "_s", "_ms", "_us"].iter().any(|s| name.ends_with(s))
}

fn is_integer(t: Type) -> bool {
    use Type::*;
    matches!(
        t,
        Int32 | Int64 | Uint32 | Uint64 | Sint32 | Sint64 | Fixed32 | Fixed64 | Sfixed32 | Sfixed64
    )
}

fn is_number(t: Type) -> bool {
    is_integer(t) || matches!(t, Type::Double | Type::Float)
}

/// protobuf's default JSON name: drop each `_` and upper-case the character after it.
fn to_json_name(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn compile(dir: &str, file: &str) -> FileDescriptorSet {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir);
    protox::compile([file], [root]).unwrap_or_else(|e| panic!("{file} compiles: {e:?}"))
}

/// The whole check a `.proto` gets: protox's own refusals, then the lint.
fn problems_of(file: &str) -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/lint");
    match protox::compile([file], [root]) {
        Ok(set) => lint(&set),
        Err(e) => vec![format!("{file}: refused by protox: {e}")],
    }
}

#[test]
fn capture_proto_passes() {
    assert_eq!(
        lint(&compile("proto", "horizon/perf/v1/capture.proto")),
        Vec::<String>::new()
    );
}

#[test]
fn an_enum_without_unspecified_zero_is_rejected() {
    assert_eq!(
        problems_of("enum_without_unspecified.proto"),
        ["enum_without_unspecified.proto:Color: no *_UNSPECIFIED = 0 value"]
    );
}

#[test]
fn a_json_name_collision_is_rejected() {
    let problems = problems_of("json_name_collision.proto");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("conflicts with field 'field1'"),
        "{problems:?}"
    );
}

#[test]
fn a_group_is_rejected() {
    let problems = problems_of("group.proto");
    assert!(
        problems.contains(&"group.proto:Outer.inner: a group".to_string()),
        "{problems:?}"
    );
}
