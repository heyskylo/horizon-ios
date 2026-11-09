//! Custom colours (`assets/<namespace>/polytone/colors.json` or `color.properties`, wiki "Custom
//! Colors"): hardcoded colours and a few switches a pack replaces. JSON objects nest the keys the
//! properties form joins with dots (`{"text": {"xpbar": ...}}` is `text.xpbar`); a value is a
//! number or a hex string.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value as Json;

/// Every pack's custom colours, by dotted key.
#[derive(Clone, Debug, Default)]
pub struct PackColors {
    entries: BTreeMap<String, Json>,
}

impl PackColors {
    /// The colours of every namespace's file under a pack overlay root (`colors.json` over
    /// `color.properties` in the same namespace).
    pub fn read(root: &Path) -> Self {
        let mut colors = Self::default();
        let mut namespaces: Vec<_> = std::fs::read_dir(root.join("assets"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        namespaces.sort();
        for namespace in namespaces {
            let polytone = namespace.join("polytone");
            if let Ok(text) = std::fs::read_to_string(polytone.join("color.properties")) {
                colors.add_properties(&text);
            }
            if let Ok(text) = std::fs::read_to_string(polytone.join("colors.json")) {
                match serde_json::from_str::<Json>(&text) {
                    Ok(json) => colors.add_json("", &json),
                    Err(e) => log::warn!("{}: {e}", polytone.join("colors.json").display()),
                }
            }
        }
        colors
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn add_json(&mut self, prefix: &str, json: &Json) {
        match json {
            Json::Object(map) => {
                for (key, value) in map {
                    let key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    self.add_json(&key, value);
                }
            }
            value if !prefix.is_empty() => {
                self.entries.insert(prefix.to_owned(), value.clone());
            }
            _ => {}
        }
    }

    fn add_properties(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            if let Some((key, value)) = line.split_once(['=', ':']) {
                self.entries
                    .insert(key.trim().to_owned(), Json::String(value.trim().to_owned()));
            }
        }
    }

    /// A colour, ARGB as written (`0xff00ffff`, `#00ff22`, `ff00ff`, or a decimal number).
    pub fn color(&self, key: &str) -> Option<u32> {
        parse_color(self.entries.get(key)?)
    }

    /// `entity_shadows`: false hides every entity's round shadow, whatever the video option says.
    pub fn entity_shadows(&self) -> bool {
        match self.entries.get("entity_shadows") {
            Some(Json::Bool(on)) => *on,
            Some(Json::String(s)) => s.trim() != "false",
            _ => true,
        }
    }

    /// `text.xpbar`: the experience level number's colour, or its `foreground` and `background`
    /// (the outline); `None` where the pack leaves vanilla's.
    pub fn xp_bar(&self) -> [Option<u32>; 2] {
        match self.color("text.xpbar") {
            Some(color) => [Some(color), None],
            None => [
                self.color("text.xpbar.foreground"),
                self.color("text.xpbar.background"),
            ],
        }
    }
}

fn parse_color(value: &Json) -> Option<u32> {
    match value {
        Json::Number(n) => n.as_i64().map(|v| v as u32),
        Json::String(s) => {
            let s = s.trim();
            let hex = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .or_else(|| s.strip_prefix('#'));
            match hex {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => s
                    .parse::<i64>()
                    .ok()
                    .filter(|_| !s.chars().any(|c| c.is_ascii_alphabetic()))
                    .map(|v| v as u32)
                    .or_else(|| u32::from_str_radix(s, 16).ok()),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_and_properties_name_the_same_keys() {
        let mut c = PackColors::default();
        c.add_json(
            "",
            &json!({"entity_shadows": false, "text": {"xpbar": "ff00ff"}, "map": {"grass": 7}}),
        );
        assert!(!c.entity_shadows());
        assert_eq!(c.xp_bar(), [Some(0xff00ff), None]);
        assert_eq!(c.color("map.grass"), Some(7));
        let mut p = PackColors::default();
        p.add_properties(
            "# a comment\ntext.xpbar.foreground = 0x00ff22\ntext.xpbar.background=-1\n",
        );
        assert!(p.entity_shadows());
        assert_eq!(p.xp_bar(), [Some(0x00ff22), Some(u32::MAX)]);
    }

    #[test]
    fn colours_read_hex_and_decimal() {
        let color = |v: Json| parse_color(&v);
        assert_eq!(color(json!("0xff00ffff")), Some(0xff00ffff));
        assert_eq!(color(json!("#c0c0d0")), Some(0xc0c0d0));
        assert_eq!(color(json!("62803d")), Some(0x62803d));
        assert_eq!(color(json!("4259712")), Some(4_259_712));
        assert_eq!(color(json!(23223)), Some(23223));
        assert_eq!(color(json!("no")), None);
    }
}
