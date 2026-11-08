//! Configured and placed features.
use super::{Report, kind, states, vanilla};
use serde_json::{Map, Value, json};

/// Feature types 26.3 dropped for compositions of other features; a pack
/// using them needs vanilla's composition, which the upgrade does not
/// write.
const REPLACED_FEATURES: [&str; 10] = [
    "basalt_columns",
    "basalt_pillar",
    "coral_mushroom",
    "desert_well",
    "glowstone_blob",
    "kelp",
    "sea_pickle",
    "seagrass",
    "twisting_vines",
    "weeping_vines",
];

/// 1.21.1's patch features (`RandomPatchConfiguration`): `flower` and
/// `no_bonemeal_flower` place exactly like `random_patch`.
const PATCHES: [&str; 3] = ["random_patch", "flower", "no_bonemeal_flower"];

/// A patch in 26.3's form, as vanilla's own patches were rewritten: its
/// placed feature tried `tries` times at a trapezoid offset of the spreads
/// (`nextInt(s + 1) - nextInt(s + 1)`), inside a one-entry selector so the
/// configured feature stays one file.
fn patch(config: &Value, report: &mut Report) -> Value {
    let int = |k: &str, d: i64| config.get(k).and_then(Value::as_i64).unwrap_or(d);
    let (tries, xz, y) = (int("tries", 128), int("xz_spread", 7), int("y_spread", 3));
    let trap = |n: i64| json!({"type": "minecraft:trapezoid", "min": -n, "max": n, "plateau": 0});
    let mut placement = vec![
        json!({"type": "minecraft:count", "count": tries}),
        json!({"type": "minecraft:offset", "x": trap(xz), "y": trap(y), "z": trap(xz)}),
    ];
    let inner = config.get("feature").cloned().unwrap_or_default();
    let feature = if inner.is_object() {
        let p = placed(&inner, report);
        placement.extend(p["placement"].as_array().into_iter().flatten().cloned());
        p["feature"].clone()
    } else {
        json!({"type": "minecraft:simple_random_selector", "features": [inner]})
    };
    json!({"type": "minecraft:simple_random_selector",
        "features": [{"feature": feature, "placement": placement}]})
}

/// 1.21.1's `nether_forest_vegetation` as 26.3 wrote vanilla's crimson and
/// warped forest vegetation: on nylium, its block tried `spread_width²` times
/// at `nextInt(w) - nextInt(w)` (y from `spread_height`) where there is air.
fn nether_forest_vegetation(config: &Value, report: &mut Report) -> Value {
    let int = |k: &str| config.get(k).and_then(Value::as_i64).unwrap_or(1).max(1);
    let (w, h) = (int("spread_width"), int("spread_height"));
    let trap = |n: i64| json!({"type": "minecraft:trapezoid", "min": -n, "max": n, "plateau": 0});
    let on = |tag: &str| {
        json!({"type": "minecraft:block_predicate_filter",
        "predicate": {"type": "minecraft:matching_block_tag", "tag": tag}})
    };
    let mut nylium = on("minecraft:nylium");
    nylium["predicate"]["offset"] = json!([0, -1, 0]);
    let block = feature(
        &json!({"type": "minecraft:simple_block",
            "config": {"to_place": config.get("state_provider").cloned().unwrap_or_default()}}),
        report,
    );
    json!({"type": "minecraft:simple_random_selector", "features": [{"feature": block,
        "placement": [nylium, {"type": "minecraft:count", "count": w * w},
            {"type": "minecraft:offset", "x": trap(w - 1), "y": trap(h - 1), "z": trap(w - 1)},
            on("minecraft:air")]}]})
}

/// 26.2 block state provider types and their 26.3 names.
fn provider(t: &str) -> Option<&'static str> {
    Some(match t {
        "simple_state_provider" => "simple_state_provider",
        "weighted_state_provider" => "weighted",
        "rotated_block_provider" => "rotated",
        "randomized_int_state_provider" => "randomized_int",
        "rule_based_state_provider" => "rule_based",
        "noise_provider" => "noise",
        "dual_noise_provider" => "dual_noise",
        "noise_threshold_provider" => "noise_threshold",
        _ => return None,
    })
}

/// One placement modifier: `random_offset` became `offset` with one
/// provider per axis (x and z both from `xz_spread`, drawn in that order).
fn placement(m: &mut Value) {
    if kind(m) == Some("random_offset") {
        let xz = m.get("xz_spread").cloned().unwrap_or(json!(0));
        let y = m.get("y_spread").cloned().unwrap_or(json!(0));
        *m = json!({"type": "minecraft:offset", "x": xz.clone(), "y": y, "z": xz});
    }
}

/// A configured feature in 26.3's form: `config` unwrapped, block state
/// providers renamed, nested features and placements upgraded.
pub(super) fn feature(v: &Value, report: &mut Report) -> Value {
    let Value::Object(o) = v else {
        return v.clone();
    };
    let Some(t) = kind(v) else {
        return v.clone();
    };
    if PATCHES.contains(&t) {
        return patch(o.get("config").unwrap_or(&Value::Null), report);
    }
    if t == "nether_forest_vegetation" {
        return nether_forest_vegetation(o.get("config").unwrap_or(&Value::Null), report);
    }
    if REPLACED_FEATURES.contains(&t) {
        report.unknown(format!("feature {t} (written as data in 26.3)"));
    }
    let mut n = Map::new();
    n.insert("type".into(), o["type"].clone());
    if let Some(Value::Object(config)) = o.get("config") {
        for (k, x) in config {
            n.insert(k.clone(), x.clone());
        }
    }
    let mut v = Value::Object(n);
    states(&mut v);
    nested(&mut v, report);
    v
}

/// Inside a feature: block state providers, placed features and
/// configured features held inline.
fn nested(v: &mut Value, report: &mut Report) {
    match v {
        Value::Object(o) => {
            if let Some(t) = kind(&Value::Object(o.clone()))
                && let Some(to) = provider(t)
            {
                if t == "simple_state_provider" {
                    // A direct provider is a state object: a bare string
                    // would name a provider in its registry instead.
                    *v = match o.get("state").cloned().unwrap_or_default() {
                        id @ Value::String(_) => json!({"id": id}),
                        state => state,
                    };
                    return;
                }
                o.insert("type".into(), vanilla(to));
            }
            // An inline placed feature: {feature, placement}.
            if o.contains_key("placement") && o.contains_key("feature") {
                let placed = placed(&Value::Object(o.clone()), report);
                *v = placed;
                return;
            }
            for x in o.values_mut() {
                if x.get("config").is_some() && x.get("type").is_some() {
                    *x = feature(x, report);
                } else {
                    nested(x, report);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| nested(x, report)),
        _ => {}
    }
}

/// A placed feature: its feature (an id or inline) and its placement
/// modifiers.
pub(super) fn placed(v: &Value, report: &mut Report) -> Value {
    let mut v = v.clone();
    states(&mut v);
    if let Some(f) = v.get("feature").cloned()
        && f.is_object()
    {
        v["feature"] = feature(&f, report);
    }
    if let Some(Value::Array(ms)) = v.get_mut("placement") {
        for m in ms.iter_mut() {
            placement(m);
        }
    }
    v
}
