//! A pack's own options and conditions: config entries (wiki "Polytone Configs") and
//! `polytone_condition`, on pack overlays (wiki "Extra Features & Tweaks") and on files (wiki
//! "Shared Concepts").
use crate::expr::{Host, Program, Root, Value};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// The Minecraft version Horizon plays, for `modOn('minecraft', range)`.
pub const MINECRAFT_VERSION: &str = "26.3";

/// A config entry's value: a number (slider), text (cycle button) or boolean (toggle).
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigValue {
    Number(f64),
    Text(String),
    Bool(bool),
}

impl ConfigValue {
    /// The value an expression's `config(id)` returns.
    pub fn to_value(&self) -> Value {
        match self {
            Self::Number(n) => Value::Num(*n),
            Self::Text(s) => Value::Str(s.as_str().into()),
            Self::Bool(b) => Value::Bool(*b),
        }
    }
}

/// The values of a pack's config entries, by id (`namespace:name`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Configs(BTreeMap<String, ConfigValue>);

impl Configs {
    /// A `config_entries/<name>.json` file: its `default_value`, a number, string or boolean.
    pub fn add_entry(&mut self, id: String, json: &Json) {
        let value = match &json["default_value"] {
            Json::Number(n) => ConfigValue::Number(n.as_f64().unwrap_or_default()),
            Json::String(s) => ConfigValue::Text(s.clone()),
            Json::Bool(b) => ConfigValue::Bool(*b),
            _ => return,
        };
        self.0.insert(id, value);
    }

    /// Changes an entry's value (a player's choice).
    pub fn set(&mut self, id: &str, value: ConfigValue) {
        self.0.insert(id.to_owned(), value);
    }

    pub fn get(&self, id: &str) -> Option<&ConfigValue> {
        self.0.get(id)
    }
}

/// A Maven-style version range (`[26.2,)`, `(1.0,2.0]`) or one exact version.
fn version_in_range(version: &str, range: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim()
            .split(['.', '-'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let cmp = |a: &str, b: &str| {
        let (a, b) = (parse(a), parse(b));
        let len = a.len().max(b.len());
        (0..len)
            .map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)))
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    let range = range.trim();
    let (Some(open), Some(close)) = (range.chars().next(), range.chars().last()) else {
        return false;
    };
    if !matches!(open, '[' | '(') || !matches!(close, ']' | ')') {
        return cmp(version, range).is_eq();
    }
    let inner = &range[1..range.len() - 1];
    let (low, high) = inner.split_once(',').unwrap_or((inner, inner));
    let low_ok = low.trim().is_empty() || {
        let o = cmp(version, low);
        if open == '[' { o.is_ge() } else { o.is_gt() }
    };
    let high_ok = high.trim().is_empty() || {
        let o = cmp(version, high);
        if close == ']' { o.is_le() } else { o.is_lt() }
    };
    low_ok && high_ok
}

/// What a condition can ask: the pack's configs and which mods run. Horizon plays Minecraft
/// and reads Polytone packs, so those two are "loaded"; no other mod is.
struct ConditionHost<'a> {
    configs: &'a Configs,
}

impl ConditionHost<'_> {
    fn mod_on(id: &str, range: Option<&str>) -> bool {
        match id {
            "minecraft" => range.is_none_or(|r| version_in_range(MINECRAFT_VERSION, r)),
            "polytone" => true,
            _ => false,
        }
    }
}

impl Host for ConditionHost<'_> {
    fn root(&mut self, _: Root) -> Value {
        Value::Null
    }
    fn method(&mut self, _: &Value, name: &str, _: &[Value]) -> Result<Value, String> {
        Err(format!("{name} is not available in a pack condition"))
    }
    fn function(&mut self, name: &str, args: &[Value]) -> Result<Value, String> {
        let text = |i: usize| match args.get(i) {
            Some(Value::Str(s)) => Some(s.to_string()),
            _ => None,
        };
        match name {
            "config" => {
                let id = text(0).ok_or("config needs an id")?;
                self.configs
                    .get(&id)
                    .map(ConfigValue::to_value)
                    .ok_or_else(|| format!("no config {id}"))
            }
            "modOn" => {
                let id = text(0).ok_or("modOn needs a mod id")?;
                Ok(Value::Bool(Self::mod_on(&id, text(1).as_deref())))
            }
            "modLoader" => Ok(Value::Str("horizon".into())),
            _ => Err(format!("{name} is not available in a pack condition")),
        }
    }
}

/// A `polytone_condition`: a scripting expression, or `{override, require_config,
/// require_mods, polytone_ignore}`.
pub enum Condition {
    Expression(Program),
    Fields {
        overrides: bool,
        configs: Vec<String>,
        mods: Vec<(String, Option<String>)>,
        ignore: bool,
    },
}

impl Condition {
    pub fn parse(json: &Json) -> Result<Self, String> {
        if let Some(src) = json.as_str() {
            return Program::compile(src).map(Self::Expression);
        }
        let object = json
            .as_object()
            .ok_or("polytone_condition is neither text nor an object")?;
        let list = |v: Option<&Json>| -> Vec<Json> {
            match v {
                Some(Json::Array(a)) => a.clone(),
                Some(other) => vec![other.clone()],
                None => Vec::new(),
            }
        };
        let configs = list(
            object
                .get("require_config")
                .or(object.get("required_configs")),
        )
        .iter()
        .filter_map(|c| c.as_str().map(str::to_owned))
        .collect();
        let mods = list(object.get("require_mods"))
            .iter()
            .filter_map(|m| match m {
                Json::String(id) => Some((id.clone(), None)),
                Json::Object(o) => Some((
                    o.get("mod")?.as_str()?.to_owned(),
                    o.get("version").and_then(Json::as_str).map(str::to_owned),
                )),
                _ => None,
            })
            .collect();
        Ok(Self::Fields {
            overrides: object.get("override").and_then(Json::as_bool) == Some(true),
            configs,
            mods,
            ignore: object.get("polytone_ignore").and_then(Json::as_bool) == Some(true),
        })
    }

    /// Whether it holds. A config that is `false`, `0` or missing fails `require_config`; an
    /// expression that fails to run doesn't hold.
    pub fn holds(&self, configs: &Configs) -> bool {
        let mut host = ConditionHost { configs };
        match self {
            Self::Expression(program) => program.eval(&mut host).is_ok_and(|v| v.truthy()),
            Self::Fields {
                configs: required,
                mods,
                ignore,
                ..
            } => {
                !ignore
                    && required
                        .iter()
                        .all(|id| configs.get(id).is_some_and(|c| c.to_value().truthy()))
                    && mods
                        .iter()
                        .all(|(id, range)| ConditionHost::mod_on(id, range.as_deref()))
            }
        }
    }

    /// Whether it decides an overlay alone, its version ranges ignored: an expression always
    /// does (packs pair one with `formats` no version matches), an object with `override`.
    pub fn overrides_formats(&self) -> bool {
        match self {
            Self::Expression(_) => true,
            Self::Fields { overrides, .. } => *overrides,
        }
    }
}

/// Whether an `overlays.entries[]` item is active: its `polytone_condition` alone when that
/// overrides the formats, else the formats (`formats_apply`) and the condition.
pub fn overlay_active(entry: &Json, configs: &Configs, formats_apply: bool) -> bool {
    match entry.get("polytone_condition").map(Condition::parse) {
        None => formats_apply,
        Some(Err(e)) => {
            log::warn!("overlay {}: {e}", entry["directory"]);
            false
        }
        Some(Ok(condition)) if condition.overrides_formats() => condition.holds(configs),
        Some(Ok(condition)) => formats_apply && condition.holds(configs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn configs() -> Configs {
        let mut c = Configs::default();
        c.add_entry("pack:leaves".into(), &json!({"default_value": "§aON"}));
        c.add_entry(
            "pack:fog".into(),
            &json!({"default_value": 60, "min": 0, "max": 100}),
        );
        c.add_entry("pack:glare".into(), &json!({"default_value": false}));
        c
    }

    #[test]
    fn version_ranges_follow_maven() {
        assert!(version_in_range("26.3", "[26.2,)"));
        assert!(!version_in_range("26.1", "[26.2,)"));
        assert!(version_in_range("26.3", "(26.2,26.3]"));
        assert!(!version_in_range("26.3", "[1.21,26.3)"));
        assert!(version_in_range("26.3", "26.3"));
        assert!(!version_in_range("26.3", "26.2"));
    }

    #[test]
    fn expression_conditions_read_configs_and_override_formats() {
        let c = configs();
        let on = json!({"directory": "opt_leaves", "formats": [0, 0],
            "polytone_condition": "config('pack:leaves') == '§aON' && modOn('minecraft', '[26.2,)')"});
        assert!(overlay_active(&on, &c, false));
        let off = json!({"directory": "x", "polytone_condition": "config('pack:fog') > 80"});
        assert!(!overlay_active(&off, &c, true));
        // An unknown config or a broken expression keeps the overlay off.
        let missing = json!({"directory": "x", "polytone_condition": "config('pack:nope') == 1"});
        assert!(!overlay_active(&missing, &c, true));
        let broken = json!({"directory": "x", "polytone_condition": "config("});
        assert!(!overlay_active(&broken, &c, true));
    }

    #[test]
    fn object_conditions_need_their_configs_and_mods() {
        let c = configs();
        let fields = |v: Json| json!({"directory": "x", "polytone_condition": v});
        assert!(overlay_active(
            &fields(json!({"require_config": "pack:leaves"})),
            &c,
            true
        ));
        assert!(!overlay_active(
            &fields(json!({"require_config": "pack:glare"})),
            &c,
            true
        ));
        // Without `override`, the formats must apply too.
        assert!(!overlay_active(
            &fields(json!({"require_config": "pack:leaves"})),
            &c,
            false
        ));
        let overriding = fields(
            json!({"override": true, "require_mods": [{"mod": "minecraft", "version": "[26,)"}]}),
        );
        assert!(overlay_active(&overriding, &c, false));
        assert!(!overlay_active(
            &fields(json!({"override": true, "require_mods": ["quark"]})),
            &c,
            false
        ));
        assert!(overlay_active(&json!({"directory": "plain"}), &c, true));
    }
}
