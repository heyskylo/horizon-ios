//! A partial check of the world's predicate graph before a dedicated server is Ready.
//!
//! Vanilla refuses to start a world whose packs' predicates reference a predicate no pack defines
//! (`MappedRegistry.freeze`: "Unbound values in registry"), or reference each other in a cycle
//! (`ReloadableServerRegistries.validateLootRegistries`: a reference "is recursively called").
//!
//! This is new, partial validation, not a port of `RegistryDataLoader` or `ValidationContext`. It
//! follows a predicate's holders only where the condition codecs hold one: `all_of` and `any_of`
//! `terms`, and `inverted` `term`. A holder is an identifier (a reference) or an inline condition,
//! which is followed in the same way. Every other condition type and field, an identifier that does
//! not parse, and the holders of loot tables, advancements and item modifiers are not checked.
use horizon_client_core::identifier::parse_identifier;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const REGISTRY: &str = "minecraft:predicate";

/// Why the graph refuses the world.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum GraphError {
    /// Referenced ids that no predicate binds.
    Unbound(BTreeSet<String>),
    /// References that re-enter a predicate while it is being validated.
    Recursive(Vec<Recursion>),
}

/// One recursive reference: the path from the validated predicate to it, and the re-entered id.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Recursion {
    pub path: String,
    pub id: String,
}

impl GraphError {
    /// Vanilla's diagnostic texts for the failure.
    pub(super) fn lines(&self) -> Vec<String> {
        match self {
            GraphError::Unbound(ids) => vec![format!(
                "Unbound values in registry ResourceKey[minecraft:root / {REGISTRY}]: [{}]",
                ids.iter().cloned().collect::<Vec<_>>().join(", ")
            )],
            GraphError::Recursive(found) => found
                .iter()
                .map(|r| {
                    format!(
                        "Found loot table element validation problem in {}: {} of type {REGISTRY} is recursively called",
                        r.path, r.id
                    )
                })
                .collect(),
        }
    }
}

/// The holders a condition holds, with their field segment (`.terms[0]`, `.term`).
fn holders(condition: &Value) -> Vec<(String, &Value)> {
    let Some(ty) = condition
        .get("type")
        .and_then(Value::as_str)
        .and_then(parse_identifier)
    else {
        return Vec::new();
    };
    match ty.as_str() {
        "minecraft:all_of" | "minecraft:any_of" => condition
            .get("terms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, term)| (format!(".terms[{i}]"), term))
            .collect(),
        "minecraft:inverted" => condition
            .get("term")
            .map(|term| vec![(".term".to_owned(), term)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Every reference a condition holds, inline conditions included.
fn references(condition: &Value, out: &mut Vec<String>) {
    for (_, holder) in holders(condition) {
        match holder {
            Value::String(name) => out.extend(parse_identifier(name)),
            Value::Object(_) => references(holder, out),
            _ => {}
        }
    }
}

/// Follows `condition`'s holders from `path`; `stack` holds the predicates being validated.
fn walk<'v>(
    predicates: &'v BTreeMap<String, Value>,
    condition: &'v Value,
    path: &str,
    stack: &mut Vec<&'v str>,
    found: &mut Vec<Recursion>,
) {
    for (segment, holder) in holders(condition) {
        let here = format!("{path}{segment}");
        match holder {
            Value::String(name) => {
                let Some(id) = parse_identifier(name) else {
                    continue;
                };
                // Each path holds a predicate once, so the walk ends on a cycle.
                if stack.contains(&id.as_str()) {
                    found.push(Recursion { path: here, id });
                } else if let Some((key, next)) = predicates.get_key_value(&id) {
                    stack.push(key);
                    walk(
                        predicates,
                        next,
                        &format!("{here}->{{{key}@{REGISTRY}}}"),
                        stack,
                        found,
                    );
                    stack.pop();
                }
            }
            Value::Object(_) => walk(predicates, holder, &here, stack, found),
            _ => {}
        }
    }
}

/// The world's predicates (by normalized id, after pack precedence): missing references first, as
/// the registry freezes before anything is validated, then recursive ones.
pub(super) fn check(predicates: &BTreeMap<String, Value>) -> Result<(), GraphError> {
    let mut referenced = Vec::new();
    for condition in predicates.values() {
        references(condition, &mut referenced);
    }
    let unbound: BTreeSet<String> = referenced
        .into_iter()
        .filter(|id| !predicates.contains_key(id))
        .collect();
    if !unbound.is_empty() {
        return Err(GraphError::Unbound(unbound));
    }
    let mut found = Vec::new();
    for (id, condition) in predicates {
        let mut stack = vec![id.as_str()];
        walk(
            predicates,
            condition,
            &format!("{{{id}@{REGISTRY}}}"),
            &mut stack,
            &mut found,
        );
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err(GraphError::Recursive(found))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
        entries
            .iter()
            .map(|(id, value)| ((*id).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn valid_references_and_unchecked_forms_pass() {
        let truth = json!({"type": "minecraft:all_of", "terms": []});
        let predicates = graph(&[
            ("a:t", truth.clone()),
            // A diamond is not a cycle; an id without a namespace is a minecraft one.
            (
                "a:x",
                json!({"type": "any_of", "terms": ["a:t", {"type": "inverted", "term": "a:t"}]}),
            ),
            (
                "a:y",
                json!({"type": "minecraft:all_of", "terms": ["a:x", "a:t"]}),
            ),
            ("minecraft:m", truth),
            ("a:z", json!({"type": "inverted", "term": "m"})),
            // Fields other than the holder positions are not followed.
            (
                "a:u",
                json!({"type": "minecraft:entity_properties", "term": "a:none", "terms": ["a:none"]}),
            ),
            // An identifier that does not parse is not checked here.
            ("a:v", json!({"type": "inverted", "term": "Not An Id"})),
        ]);
        assert_eq!(check(&predicates), Ok(()));
    }

    #[test]
    fn missing_references_are_unbound_even_inline_and_before_cycles() {
        let predicates = graph(&[
            (
                "a:r",
                json!({"type": "all_of", "terms": [{"type": "inverted", "term": "a:gone"}, "b:none"]}),
            ),
            ("a:c", json!({"type": "inverted", "term": "a:c"})),
        ]);
        let error = check(&predicates).unwrap_err();
        assert_eq!(
            error,
            GraphError::Unbound(["a:gone".to_owned(), "b:none".to_owned()].into())
        );
        assert_eq!(
            error.lines(),
            [
                "Unbound values in registry ResourceKey[minecraft:root / minecraft:predicate]: [a:gone, b:none]"
            ]
        );
    }

    #[test]
    fn a_cycle_is_reported_from_each_predicate_on_it() {
        let predicates = graph(&[
            (
                "prg:ref",
                json!({"type": "minecraft:all_of", "terms": ["prg:loop"]}),
            ),
            (
                "prg:loop",
                json!({"type": "minecraft:inverted", "term": "prg:ref"}),
            ),
            (
                "prg:self",
                json!({"type": "any_of", "terms": [{"type": "inverted", "term": "prg:self"}]}),
            ),
        ]);
        let mut lines = check(&predicates).unwrap_err().lines();
        lines.sort();
        // The two cycle lines are the ones vanilla 26.3 logged for this pack.
        assert_eq!(
            lines,
            [
                "Found loot table element validation problem in {prg:loop@minecraft:predicate}.term->{prg:ref@minecraft:predicate}.terms[0]: prg:loop of type minecraft:predicate is recursively called",
                "Found loot table element validation problem in {prg:ref@minecraft:predicate}.terms[0]->{prg:loop@minecraft:predicate}.term: prg:ref of type minecraft:predicate is recursively called",
                "Found loot table element validation problem in {prg:self@minecraft:predicate}.terms[0].term: prg:self of type minecraft:predicate is recursively called",
            ]
        );
    }
}
