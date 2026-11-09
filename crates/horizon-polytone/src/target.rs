//! Targeting (wiki "Shared Concepts"): a modifier aims at the id its file path names, or at the
//! ids, `#tags` and regular expressions of its `targets` field.
use regex::Regex;
use serde_json::Value;

/// One entry of `targets`.
#[derive(Clone, Debug)]
pub enum Target {
    /// A literal id, `minecraft` when the namespace is left out.
    Id(String),
    /// `#namespace:path`, stored without the `#`.
    Tag(String),
    /// Anything else: a regular expression the whole id must match.
    Pattern(Regex),
}

/// What a modifier applies to. Each entry carries `required` (default `true`): whether a missing
/// target is an error.
#[derive(Clone, Debug, Default)]
pub struct Targets(pub Vec<(Target, bool)>);

/// `minecraft:` for an id without a namespace; `None` when it holds characters ids can't
/// (`a-z 0-9 . _ -`, and `/` in the path).
pub(crate) fn parse_id(s: &str) -> Option<String> {
    let (namespace, path) = s.split_once(':').unwrap_or(("minecraft", s));
    let valid = |part: &str, slash: bool| {
        !part.is_empty()
            && part.chars().all(|c| {
                matches!(c, 'a'..='z' | '0'..='9' | '.' | '_' | '-') || (slash && c == '/')
            })
    };
    (valid(namespace, false) && valid(path, true)).then(|| format!("{namespace}:{path}"))
}

/// The id a file's path targets when it has no `targets` field:
/// `assets/byg/polytone/biome_modifiers/redwood_forest.json` is `byg:redwood_forest` for the kind
/// `biome_modifiers`.
pub fn implicit_id(path: &str, kind: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let after = &path[path.find("assets/")? + "assets/".len()..];
    let (namespace, rest) = after.split_once('/')?;
    let name = rest.strip_prefix("polytone/")?.strip_prefix(kind)?;
    let name = name.strip_prefix('/')?.strip_suffix(".json")?;
    parse_id(&format!("{namespace}:{name}"))
}

impl Target {
    /// A string is an id first, then a `#tag`, then a regular expression.
    fn parse(s: &str) -> Result<Self, String> {
        if let Some(id) = parse_id(s) {
            return Ok(Self::Id(id));
        }
        if let Some(tag) = s.strip_prefix('#').and_then(parse_id) {
            return Ok(Self::Tag(tag));
        }
        Regex::new(&format!("^(?:{s})$"))
            .map(Self::Pattern)
            .map_err(|e| format!("target {s:?} is no id, tag or pattern: {e}"))
    }
}

impl Targets {
    /// The `targets` field (a string, an object or a list of them), else the file's implicit id.
    pub fn parse(field: Option<&Value>, implicit: Option<&str>) -> Result<Self, String> {
        let entry = |v: &Value| match v {
            Value::String(s) => Ok((Target::parse(s)?, true)),
            Value::Object(o) => {
                let id = o
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("target without an id")?;
                let required = o.get("required").and_then(Value::as_bool).unwrap_or(true);
                Ok((Target::parse(id)?, required))
            }
            _ => Err(format!("target {v} is neither a string nor an object")),
        };
        match field {
            Some(Value::Array(list)) => list.iter().map(entry).collect::<Result<_, _>>().map(Self),
            Some(v) => Ok(Self(vec![entry(v)?])),
            None => {
                let id = implicit.ok_or("no targets field and no implicit target")?;
                Ok(Self(vec![(Target::parse(id)?, true)]))
            }
        }
    }

    /// Whether `id` (`namespace:path`) is a target; `in_tag` answers tag membership.
    pub fn matches(&self, id: &str, in_tag: impl Fn(&str) -> bool) -> bool {
        self.0.iter().any(|(target, _)| match target {
            Target::Id(t) => t == id,
            Target::Tag(tag) => in_tag(tag),
            Target::Pattern(re) => re.is_match(id),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn targets(v: Value) -> Targets {
        Targets::parse(Some(&v), None).unwrap()
    }

    #[test]
    fn the_file_path_names_the_target_without_a_targets_field() {
        let path = "pack/assets/byg/polytone/biome_modifiers/redwood_forest.json";
        assert_eq!(
            implicit_id(path, "biome_modifiers").as_deref(),
            Some("byg:redwood_forest")
        );
        assert_eq!(implicit_id(path, "dimension_modifiers"), None);
        let t = Targets::parse(None, Some("byg:redwood_forest")).unwrap();
        assert!(t.matches("byg:redwood_forest", |_| false));
        assert!(!t.matches("minecraft:forest", |_| false));
    }

    #[test]
    fn ids_default_to_minecraft_and_match_literally() {
        let t = targets(json!(["stone", "create:brass_block"]));
        assert!(t.matches("minecraft:stone", |_| false));
        assert!(t.matches("create:brass_block", |_| false));
        // `.` is an id character: no pattern, so it doesn't match other ids.
        assert!(!targets(json!("minecraft:a.b")).matches("minecraft:axb", |_| false));
    }

    #[test]
    fn strings_that_are_no_id_are_tags_then_patterns() {
        let t = targets(json!(["#minecraft:is_overworld", "create:.*"]));
        assert!(t.matches("create:anything", |_| false));
        assert!(!t.matches("minecraft:plains", |_| false));
        assert!(t.matches("minecraft:plains", |tag| tag == "minecraft:is_overworld"));
    }

    #[test]
    fn the_long_form_carries_required() {
        let t =
            targets(json!([{"id": "minecraft:pale_oak_leaves", "required": false}, "oak_leaves"]));
        let required: Vec<bool> = t.0.iter().map(|e| e.1).collect();
        assert_eq!(required, [false, true]);
        assert!(t.matches("minecraft:pale_oak_leaves", |_| false));
    }

    #[test]
    fn a_bad_pattern_is_an_error() {
        assert!(Targets::parse(Some(&json!("broken(")), None).is_err());
        assert!(Targets::parse(None, None).is_err());
    }
}
