//! Creative tab modifiers (`assets/<namespace>/polytone/creative_tab_modifiers/**.json`, wiki
//! "Creative Tab Modifiers"): a tab's icon and name, and the items it loses or gains.

use std::path::Path;

use serde_json::Value as Json;

use crate::Targets;

/// An `Item Predicate`: which items a removal or an addition's anchor matches.
#[derive(Clone, Debug)]
pub enum ItemPredicate {
    Or(Vec<ItemPredicate>),
    And(Vec<ItemPredicate>),
    Not(Box<ItemPredicate>),
    /// `tag_match`: an item tag id.
    Tag(String),
    /// `items_match`, and `itemstack_match` by its item (its components aren't compared).
    Items(Vec<String>),
    /// `id_match`: regular expressions on the id's namespace and path, searched in them.
    Id {
        namespace: Option<regex::Regex>,
        path: Option<regex::Regex>,
    },
    True,
}

impl ItemPredicate {
    fn parse(json: &Json) -> Result<Self, String> {
        let list = |key: &str| -> Result<Vec<Self>, String> {
            json[key]
                .as_array()
                .ok_or(format!("{key} missing"))?
                .iter()
                .map(Self::parse)
                .collect()
        };
        let regex = |key: &str| -> Result<Option<regex::Regex>, String> {
            json[key]
                .as_str()
                .map(regex::Regex::new)
                .transpose()
                .map_err(|e| e.to_string())
        };
        Ok(match json["type"].as_str().unwrap_or("") {
            "or" => Self::Or(list("predicates")?),
            "and" => Self::And(list("predicates")?),
            "not" => Self::Not(Box::new(Self::parse(&json["predicate"])?)),
            "tag_match" => Self::Tag(namespaced(json["tag"].as_str().ok_or("tag missing")?)),
            "items_match" => Self::Items(items(&json["items"])),
            "itemstack_match" => Self::Items(items(&json["itemstack"])),
            "id_match" => Self::Id {
                namespace: regex("namespace")?,
                path: regex("path")?,
            },
            "true" => Self::True,
            other => return Err(format!("unknown item predicate {other:?}")),
        })
    }

    /// Whether an item (namespaced id) matches, with its tags by `in_tag`.
    pub fn matches(&self, item: &str, in_tag: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Self::Or(all) => all.iter().any(|p| p.matches(item, in_tag)),
            Self::And(all) => all.iter().all(|p| p.matches(item, in_tag)),
            Self::Not(p) => !p.matches(item, in_tag),
            Self::Tag(tag) => in_tag(tag),
            Self::Items(ids) => ids.iter().any(|id| id == item),
            Self::Id { namespace, path } => {
                let (ns, p) = item.split_once(':').unwrap_or(("minecraft", item));
                namespace.as_ref().is_none_or(|r| r.is_match(ns))
                    && path.as_ref().is_none_or(|r| r.is_match(p))
            }
            Self::True => true,
        }
    }
}

/// An `Item Addition`: items added at the tab's end, or before or after the first item
/// `predicate` matches.
#[derive(Clone, Debug)]
pub struct Addition {
    pub items: Vec<String>,
    pub before: bool,
    pub predicate: Option<ItemPredicate>,
}

/// One modifier, with the tabs it targets.
#[derive(Clone, Debug)]
pub struct TabModifier {
    pub targets: Targets,
    /// `icon`: an item id (an item stack's `id`).
    pub icon: Option<String>,
    /// `name`: the tab's name, a text component.
    pub name: Option<Json>,
    pub removals: Vec<ItemPredicate>,
    pub additions: Vec<Addition>,
}

fn namespaced(id: &str) -> String {
    let id = id.trim().to_ascii_lowercase();
    if id.contains(':') {
        id
    } else {
        format!("minecraft:{id}")
    }
}

/// An `Item` or `Item Stack` field, or a list of them, as item ids.
fn items(json: &Json) -> Vec<String> {
    match json {
        Json::String(id) => vec![namespaced(id)],
        Json::Object(stack) => stack
            .get("id")
            .and_then(Json::as_str)
            .map(namespaced)
            .into_iter()
            .collect(),
        Json::Array(all) => all.iter().flat_map(items).collect(),
        _ => Vec::new(),
    }
}

impl TabModifier {
    fn parse(json: &Json, implicit: Option<&str>) -> Result<Self, String> {
        let removals = json["removals"]
            .as_array()
            .into_iter()
            .flatten()
            .map(ItemPredicate::parse);
        let additions = json["additions"].as_array().into_iter().flatten().map(|a| {
            Ok(Addition {
                items: items(&a["items"]),
                before: a["before"].as_bool().unwrap_or(false),
                predicate: a.get("predicate").map(ItemPredicate::parse).transpose()?,
            })
        });
        Ok(Self {
            targets: Targets::parse(json.get("targets"), implicit)?,
            icon: items(&json["icon"]).into_iter().next(),
            name: json.get("name").cloned(),
            removals: removals.collect::<Result<_, _>>()?,
            additions: additions.collect::<Result<_, String>>()?,
        })
    }

    /// Every modifier under a pack overlay root (a file's implicit target is its path).
    pub fn read_all(root: &Path) -> Vec<Self> {
        let mut out = Vec::new();
        for namespace in std::fs::read_dir(root.join("assets"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let mut stack = vec![namespace.path().join("polytone/creative_tab_modifiers")];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
                    let Some(implicit) = crate::implicit_id(&relative, "creative_tab_modifiers")
                    else {
                        continue;
                    };
                    let parsed = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|t| serde_json::from_str::<Json>(&t).map_err(|e| e.to_string()))
                        .and_then(|json| Self::parse(&json, Some(&implicit)));
                    match parsed {
                        Ok(m) => out.push(m),
                        Err(e) => log::warn!("{relative}: {e}"),
                    }
                }
            }
        }
        out
    }

    /// The tab's items after this modifier's removals and additions.
    pub fn apply(&self, items: &mut Vec<String>, in_tag: &dyn Fn(&str, &str) -> bool) {
        items.retain(|item| {
            !self
                .removals
                .iter()
                .any(|p| p.matches(item, &|t| in_tag(item, t)))
        });
        for addition in &self.additions {
            let at = match &addition.predicate {
                None => items.len(),
                Some(p) => match items
                    .iter()
                    .position(|item| p.matches(item, &|t| in_tag(item, t)))
                {
                    Some(i) if addition.before => i,
                    Some(i) => i + 1,
                    None => items.len(),
                },
            };
            items.splice(at..at, addition.items.iter().cloned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn removals_and_additions_edit_a_tab_in_order() {
        let m = TabModifier::parse(
            &json!({
                "icon": {"id": "minecraft:diamond_block", "count": 1},
                "removals": [{"type": "tag_match", "tag": "swords"}, {"type": "id_match", "path": "^stone$"}],
                "additions": [
                    {"items": ["iron_shovel", "minecraft:iron_sword"], "predicate": {"type": "items_match", "items": ["minecraft:iron_axe"]}},
                    {"items": "minecraft:dirt"},
                    {"items": "minecraft:apple", "before": true, "predicate": {"type": "true"}}
                ]
            }),
            Some("minecraft:combat"),
        )
        .unwrap();
        assert_eq!(m.icon.as_deref(), Some("minecraft:diamond_block"));
        assert!(m.targets.matches("minecraft:combat", |_| false));
        let mut items: Vec<String> = [
            "minecraft:wooden_sword",
            "minecraft:iron_axe",
            "minecraft:stone",
            "minecraft:stone_axe",
        ]
        .map(String::from)
        .into();
        let in_tag = |item: &str, tag: &str| tag == "minecraft:swords" && item.ends_with("_sword");
        m.apply(&mut items, &in_tag);
        assert_eq!(
            items,
            [
                "minecraft:apple",
                "minecraft:iron_axe",
                "minecraft:iron_shovel",
                "minecraft:iron_sword",
                "minecraft:stone_axe",
                "minecraft:dirt"
            ]
        );
    }
}
