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

/// What kind of control a config entry is, with its kind's own fields.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigKind {
    /// A slider from `min` to `max` in steps of `step`.
    Number { min: f64, max: f64, step: f64 },
    /// A cycle button over `allowed_values`.
    Text { allowed: Vec<String> },
    /// An ON/OFF toggle.
    Bool,
}

/// A config entry's file (wiki "Polytone Configs"): its control, default and how the options
/// screen lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigEntry {
    pub kind: ConfigKind,
    pub default: ConfigValue,
    /// The `config.<namespace>.section.<id>` group it is listed under, and that group's place.
    pub section: Option<String>,
    pub section_order: Option<i64>,
    pub display_order: i64,
    /// `low`, `medium`, `high` or `varies`.
    pub performance_impact: Option<String>,
    /// A full-width row instead of a half-width one.
    pub wide: bool,
    /// The translation key of the value's label (`%s blocks`).
    pub value_translation: Option<String>,
    /// Its value in each pack-wide preset, and in each preset of its section.
    pub presets: BTreeMap<String, ConfigValue>,
    pub section_presets: BTreeMap<String, ConfigValue>,
}

impl ConfigValue {
    fn parse(v: &Json) -> Option<Self> {
        Some(match v {
            Json::Number(n) => Self::Number(n.as_f64()?),
            Json::String(s) => Self::Text(s.clone()),
            Json::Bool(b) => Self::Bool(*b),
            _ => return None,
        })
    }
}

impl ConfigEntry {
    /// `None` without a `default_value` of a number, string or boolean.
    pub fn parse(json: &Json) -> Option<Self> {
        let default = ConfigValue::parse(&json["default_value"])?;
        let num = |key: &str, d: f64| json[key].as_f64().unwrap_or(d);
        let kind = match &default {
            ConfigValue::Number(_) => ConfigKind::Number {
                min: num("min", 0.0),
                max: num("max", 1.0),
                step: num("step", 0.1),
            },
            ConfigValue::Text(_) => ConfigKind::Text {
                allowed: json["allowed_values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect(),
            },
            ConfigValue::Bool(_) => ConfigKind::Bool,
        };
        let presets = |key: &str| -> BTreeMap<String, ConfigValue> {
            json[key]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.clone(), ConfigValue::parse(v)?)))
                .collect()
        };
        let text = |key: &str| json[key].as_str().map(str::to_owned);
        Some(Self {
            kind,
            default,
            section: text("section"),
            section_order: json["section_order"].as_i64(),
            display_order: json["display_order"].as_i64().unwrap_or(0),
            performance_impact: text("performance_impact"),
            wide: json["wide"].as_bool() == Some(true),
            value_translation: text("value_translation"),
            presets: presets("presets"),
            section_presets: presets("section_presets"),
        })
    }

    /// `value` kept within what the entry allows: a number clamped to its range and snapped
    /// to its step, a text one of its values; `None` for a value of another kind.
    pub fn accept(&self, value: &ConfigValue) -> Option<ConfigValue> {
        match (&self.kind, value) {
            (ConfigKind::Number { min, max, step }, ConfigValue::Number(n)) => {
                let (lo, hi) = (min.min(*max), max.max(*min));
                let mut v = n.clamp(lo, hi);
                if *step > 0.0 {
                    v = (lo + ((v - lo) / step).round() * step).clamp(lo, hi);
                }
                Some(ConfigValue::Number(v))
            }
            (ConfigKind::Text { allowed }, ConfigValue::Text(t)) => {
                (allowed.is_empty() || allowed.contains(t)).then(|| value.clone())
            }
            (ConfigKind::Bool, ConfigValue::Bool(_)) => Some(value.clone()),
            _ => None,
        }
    }
}

/// A pack's config entries and their values, by id (`namespace:name`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Configs {
    values: BTreeMap<String, ConfigValue>,
    entries: BTreeMap<String, ConfigEntry>,
}

impl Configs {
    /// A `config_entries/<name>.json` file: its value starts at its `default_value`.
    pub fn add_entry(&mut self, id: String, json: &Json) {
        let Some(entry) = ConfigEntry::parse(json) else {
            return;
        };
        self.values.insert(id.clone(), entry.default.clone());
        self.entries.insert(id, entry);
    }

    /// Changes an entry's value (a player's choice), kept within what the entry allows. An id
    /// without an entry takes any value.
    pub fn set(&mut self, id: &str, value: ConfigValue) {
        let value = match self.entries.get(id) {
            Some(entry) => match entry.accept(&value) {
                Some(v) => v,
                None => return,
            },
            None => value,
        };
        self.values.insert(id.to_owned(), value);
    }

    pub fn get(&self, id: &str) -> Option<&ConfigValue> {
        self.values.get(id)
    }

    /// The entries, by id.
    pub fn entries(&self) -> &BTreeMap<String, ConfigEntry> {
        &self.entries
    }

    /// The values that differ from their entries' defaults: what a player chose.
    pub fn chosen(&self) -> BTreeMap<String, ConfigValue> {
        self.values
            .iter()
            .filter(|(id, v)| self.entries.get(*id).is_none_or(|e| &e.default != *v))
            .map(|(id, v)| (id.clone(), v.clone()))
            .collect()
    }

    /// Every entry with a value in `preset` (a pack-wide preset) takes it.
    pub fn apply_preset(&mut self, preset: &str) {
        let picks: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(id, e)| Some((id.clone(), e.presets.get(preset)?.clone())))
            .collect();
        for (id, value) in picks {
            self.set(&id, value);
        }
    }

    /// Takes `other`'s entries and values (another pack's), over its own of the same id.
    pub fn merge(&mut self, other: &Configs) {
        self.entries
            .extend(other.entries.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.values
            .extend(other.values.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    /// The pack-wide presets' names, in order.
    pub fn presets(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .entries
            .values()
            .flat_map(|e| e.presets.keys().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
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

    #[test]
    fn config_entries_keep_their_control_and_values_within_it() {
        let mut configs = Configs::default();
        configs.add_entry(
            "p:fog".into(),
            &serde_json::json!({
                "default_value": 0.5, "min": 0, "max": 2, "step": 0.25,
                "section": "rendering", "section_order": 1, "performance_impact": "medium",
                "presets": {"performance": 0, "pretty": 2}
            }),
        );
        configs.add_entry(
            "p:rain".into(),
            &serde_json::json!({"default_value": "drops", "allowed_values": ["drops", "streaks"]}),
        );
        configs.add_entry(
            "p:birds".into(),
            &serde_json::json!({"default_value": true}),
        );
        // No default: no entry.
        configs.add_entry("p:broken".into(), &serde_json::json!({"min": 1}));
        assert_eq!(configs.entries().len(), 3);
        let fog = &configs.entries()["p:fog"];
        assert_eq!(
            fog.kind,
            ConfigKind::Number {
                min: 0.0,
                max: 2.0,
                step: 0.25
            }
        );
        assert_eq!(
            (fog.section.as_deref(), fog.section_order),
            (Some("rendering"), Some(1))
        );
        // Clamped and snapped to the step; another kind, or a value not allowed, is refused.
        configs.set("p:fog", ConfigValue::Number(1.6));
        assert_eq!(configs.get("p:fog"), Some(&ConfigValue::Number(1.5)));
        configs.set("p:fog", ConfigValue::Number(9.0));
        assert_eq!(configs.get("p:fog"), Some(&ConfigValue::Number(2.0)));
        configs.set("p:fog", ConfigValue::Bool(false));
        configs.set("p:rain", ConfigValue::Text("snow".into()));
        assert_eq!(
            configs.get("p:rain"),
            Some(&ConfigValue::Text("drops".into()))
        );
        configs.set("p:birds", ConfigValue::Bool(false));
        // What differs from the defaults.
        assert_eq!(
            configs.chosen().into_iter().collect::<Vec<_>>(),
            [
                ("p:birds".to_owned(), ConfigValue::Bool(false)),
                ("p:fog".to_owned(), ConfigValue::Number(2.0)),
            ]
        );
        assert_eq!(configs.presets(), ["performance", "pretty"]);
        configs.apply_preset("performance");
        assert_eq!(configs.get("p:fog"), Some(&ConfigValue::Number(0.0)));
    }
}
