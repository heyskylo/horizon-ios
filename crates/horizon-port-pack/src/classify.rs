//! Step 2 and 3: classify each mod (class and API tier, `docs/modpacks.md`)
//! and order them by required dependencies.
use std::collections::{BTreeMap, HashMap};

/// The reviewed table: ATM10's 495 rows (`docs/modpacks/atm10-mods.csv`).
/// It wins over the heuristics.
const KNOWN_CSV: &str = include_str!("../../../docs/modpacks/atm10-mods.csv");

pub struct Known {
    by_key: HashMap<String, (String, String)>,
}

/// Lowercase letters and digits only: `Get It Together, Drops!` ->
/// `getittogetherdrops`.
pub fn normalise(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// One CSV line, with `"quoted, fields"`.
fn csv_fields(line: &str) -> Vec<String> {
    let (mut out, mut field, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    out.push(field);
    out
}

impl Known {
    pub fn atm10() -> Self {
        let mut by_key = HashMap::new();
        for line in KNOWN_CSV.lines().skip(1) {
            let f = csv_fields(line);
            if f.len() < 6 || f[5] == "guess" {
                continue;
            }
            let value = (f[3].clone(), f[4].clone());
            by_key.insert(normalise(&f[0]), value.clone());
            by_key.entry(normalise(&f[1])).or_insert(value);
        }
        Known { by_key }
    }

    /// By mod id, Modrinth slug or name, in that order.
    pub fn get(&self, keys: &[&str]) -> Option<&(String, String)> {
        keys.iter()
            .filter(|k| !k.is_empty())
            .find_map(|k| self.by_key.get(&normalise(k)))
    }
}

/// What the rules look at.
#[derive(Default)]
pub struct Facts<'a> {
    /// `mods`, `resourcepacks`, `shaderpacks`, `datapacks`.
    pub kind: &'a str,
    pub client: &'a str,
    pub server: &'a str,
    /// Modrinth categories.
    pub categories: &'a [String],
    /// Modrinth's loaders for the whole project, over all its versions.
    pub project_loaders: &'a [String],
    /// From the jar: classes, and whether it has blocks, items or data.
    pub classes: Option<usize>,
    pub has_content: bool,
}

/// Class, tier and confidence (`known` is set by the caller).
pub fn rules(f: &Facts) -> (&'static str, &'static str, &'static str) {
    let has = |c: &str| f.categories.iter().any(|x| x == c);
    match f.kind {
        "resourcepacks" => return ("other", "data", "kind"),
        "shaderpacks" => return ("other", "none", "kind"),
        "datapacks" => return ("content", "data", "kind"),
        _ => {}
    }
    if has("library") {
        return ("library", "none", "categories");
    }
    if has("optimization") {
        return ("performance", "none", "categories");
    }
    if f.client != "unsupported" && f.server == "unsupported" {
        return ("client-ui", "client", "sides");
    }
    if f.client == "unsupported" && f.server != "unsupported" && !f.server.is_empty() {
        return ("server-only", "logic", "sides");
    }
    if f.classes == Some(0) && f.has_content {
        return ("content", "data", "jar");
    }
    // The project also ships as a plain data pack, so its jar is probably one too. Only the jar
    // can tell, so this gets the jar fetched; once it is read, the rule above or those below decide.
    if f.classes.is_none() && f.project_loaders.iter().any(|l| l == "datapack") {
        return ("content", "data", "loaders");
    }
    let content = [
        "adventure",
        "decoration",
        "equipment",
        "food",
        "game-mechanics",
        "magic",
        "mobs",
        "storage",
        "technology",
        "transportation",
        "worldgen",
    ];
    if f.categories.iter().any(|c| content.contains(&c.as_str())) {
        let tier = if has("mobs") || has("magic") || has("adventure") {
            "deep"
        } else if has("technology") || has("storage") || has("transportation") {
            "machines"
        } else if f.categories.iter().all(|c| {
            ["worldgen", "decoration", "food"].contains(&c.as_str())
                || !content.contains(&c.as_str())
        }) {
            "data"
        } else {
            "logic"
        };
        return ("content", tier, "categories");
    }
    if has("utility") || has("management") || has("social") {
        return ("other", "logic", "guess");
    }
    ("unknown", "none", "unknown")
}

/// Layer of each mod: 0 needs nothing in the pack, n needs layer n-1.
/// Mods in a cycle get one layer past the rest; dependencies outside the
/// pack are left out (the caller lists them).
pub fn layers(depends: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    loop {
        let mut progressed = false;
        for (id, deps) in depends {
            if out.contains_key(id) {
                continue;
            }
            let inside = deps.iter().filter(|d| depends.contains_key(*d) && *d != id);
            let mut layer = 0;
            let mut ready = true;
            for d in inside {
                match out.get(d) {
                    Some(l) => layer = layer.max(l + 1),
                    None => ready = false,
                }
            }
            if ready {
                out.insert(id.clone(), layer);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    let past = out.values().max().map_or(0, |m| m + 1);
    for id in depends.keys() {
        out.entry(id.clone()).or_insert(past);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_table_matches_ids_and_names() {
        let k = Known::atm10();
        assert_eq!(k.get(&["create"]).map(|v| v.0.as_str()), Some("content"));
        assert_eq!(
            k.get(&["", "Get It Together, Drops!"])
                .map(|v| v.0.as_str()),
            Some("performance")
        );
        assert_eq!(k.get(&["sodium"]).map(|v| v.1.as_str()), Some("none"));
        assert!(k.get(&["not-a-mod"]).is_none());
    }

    #[test]
    fn rules_follow_sides_then_categories() {
        let cats = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let f = |client, server, c: &[String]| {
            rules(&Facts {
                kind: "mods",
                client,
                server,
                categories: c,
                ..Facts::default()
            })
        };
        assert_eq!(f("required", "unsupported", &[]).0, "client-ui");
        assert_eq!(f("unsupported", "required", &[]).0, "server-only");
        assert_eq!(f("required", "required", &cats(&["library"])).0, "library");
        assert_eq!(
            f("required", "unsupported", &cats(&["optimization"])).0,
            "performance"
        );
        assert_eq!(
            f("required", "required", &cats(&["technology", "utility"])),
            ("content", "machines", "categories")
        );
        assert_eq!(
            f("required", "required", &cats(&["worldgen", "neoforge"])),
            ("content", "data", "categories")
        );
        assert_eq!(f("", "", &[]).0, "unknown");
    }

    #[test]
    fn a_project_that_ships_as_a_data_pack_is_data_tier_until_its_jar_says_otherwise() {
        let adventure = ["adventure".to_owned()];
        let loaders = ["datapack".to_owned(), "fabric".to_owned()];
        let f = |classes, has_content| {
            rules(&Facts {
                kind: "mods",
                client: "required",
                server: "required",
                categories: &adventure,
                project_loaders: &loaders,
                classes,
                has_content,
            })
        };
        assert_eq!(f(None, false), ("content", "data", "loaders"));
        assert_eq!(f(Some(0), true), ("content", "data", "jar"));
        assert_eq!(f(Some(12), true), ("content", "deep", "categories"));
    }

    #[test]
    fn layers_put_dependencies_first() {
        let d = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let deps = BTreeMap::from([
            ("lib".to_string(), d(&[])),
            ("create".to_string(), d(&["lib", "outside"])),
            ("addon".to_string(), d(&["create", "lib"])),
            ("a".to_string(), d(&["b"])),
            ("b".to_string(), d(&["a"])),
        ]);
        let l = layers(&deps);
        assert_eq!((l["lib"], l["create"], l["addon"]), (0, 1, 2));
        assert_eq!((l["a"], l["b"]), (3, 3));
    }
}
