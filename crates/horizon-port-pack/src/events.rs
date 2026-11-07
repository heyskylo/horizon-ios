//! `events <jar or folder>`: the NeoForge events a mod listens to and the
//! capabilities it uses, looked up in the hook catalog. `native-hooks.json`
//! names each hook's NeoForge counterpart (docs/native-hooks-neoforge.md);
//! this reads a jar's class files (`classes`: `@SubscribeEvent` methods,
//! method references and lambdas that take one event, `Capabilities` fields)
//! and finds the hooks for each name. Over a folder of jars it ranks the
//! names by how many mods use them, so a pack shows which missing hooks
//! matter most. It reads names, never code, and translates nothing: it says
//! where a port starts, for a human to check.
use crate::{classes, jar};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufReader, Read, Seek},
    path::{Path, PathBuf},
};

/// The hook catalog this build was made from.
pub const CATALOG: &str = include_str!("../../../native-hooks.json");

/// Names NeoForge changed after 1.21.1, which packs such as ATM10 still use
/// (NeoForge's documentation for both versions): a name the catalog lacks is
/// looked up again under its new one. `RecipesUpdatedEvent` is the exception:
/// 1.21.1's documentation doesn't name it, only that the client has "a full
/// copy of the `RecipeManager`". 26.1's client gets just the recipe types a
/// mod asks for (`OnDatapackSyncEvent#sendRecipes`), in `RecipesReceivedEvent`.
const RENAMED: [(&str, &str); 5] = [
    ("Capabilities.ItemHandler.", "Capabilities.Item."),
    ("Capabilities.EnergyStorage.", "Capabilities.Energy."),
    ("AddReloadListenerEvent", "AddServerReloadListenersEvent"),
    (
        "ModelEvent.RegisterAdditional",
        "ModelEvent.RegisterStandalone",
    ),
    ("RecipesUpdatedEvent", "RecipesReceivedEvent"),
];

/// Events that are only a parent of the events NeoForge documents under
/// them (its config page: "the `ModConfigEvent.Loading`,
/// `ModConfigEvent.Reloading`, and `ModConfigEvent.Unloading` events"). A
/// mod that listens to the parent hears each of them, so the parent is
/// hooked when every one of them is.
const PARENTS: [(&str, &[&str]); 1] = [("ModConfigEvent", &["Loading", "Reloading", "Unloading"])];

/// The catalog's NeoForge names: the hooks that are each name's
/// counterpart, and the hooks whose `none (…)` label mentions it.
#[derive(Debug, Default)]
pub struct Catalog {
    exact: BTreeMap<String, Vec<String>>,
    noted: BTreeMap<String, Vec<String>>,
}

/// How the catalog answers one name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The hooks whose counterpart it is (for a parent in `PARENTS`, the
    /// hooks of all its events).
    Hooked(Vec<String>),
    /// The hooks of another phase (`.Post` for `.Pre`), or of the events
    /// nested in it (`ModConfigEvent.Loading` for `ModConfigEvent`).
    Phase(Vec<String>),
    /// The hooks whose label says why they aren't its counterpart.
    Noted(Vec<String>),
    Missing,
}

impl Catalog {
    pub fn parse(json: &str) -> Result<Catalog, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("hook catalog: {e}"))?;
        let mut catalog = Catalog::default();
        for hook in value["hooks"].as_array().ok_or("hook catalog: no hooks")? {
            let (Some(name), Some(label)) = (hook["name"].as_str(), hook["neoforge"].as_str())
            else {
                continue;
            };
            let (names, map) = if label.to_ascii_lowercase().starts_with("none") {
                (mentioned(label), &mut catalog.noted)
            } else {
                (counterparts(label), &mut catalog.exact)
            };
            for n in names {
                map.entry(n).or_default().push(name.to_owned());
            }
        }
        Ok(catalog)
    }

    /// The status of `name` (`ServerTickEvent.Post`), and the newer name it
    /// was found under.
    pub fn status(&self, name: &str) -> (Status, Option<String>) {
        let status = self.lookup(name);
        if status != Status::Missing {
            return (status, None);
        }
        for (old, new) in RENAMED {
            if let Some(rest) = name.strip_prefix(old) {
                let renamed = format!("{new}{rest}");
                let status = self.lookup(&renamed);
                if status != Status::Missing {
                    return (status, Some(renamed));
                }
            }
        }
        (Status::Missing, None)
    }

    fn lookup(&self, name: &str) -> Status {
        if let Some(hooks) = self.exact.get(name) {
            return Status::Hooked(hooks.clone());
        }
        if let Some((_, events)) = PARENTS.iter().find(|(parent, _)| *parent == name) {
            let hooks: Option<Vec<&Vec<String>>> = events
                .iter()
                .map(|event| self.exact.get(&format!("{name}.{event}")))
                .collect();
            if let Some(hooks) = hooks {
                let hooks: BTreeSet<&String> = hooks.into_iter().flatten().collect();
                return Status::Hooked(hooks.into_iter().cloned().collect());
            }
        }
        let phase = |s: &str| s == "Pre" || s == "Post";
        let related = |k: &str| {
            let nested = k
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('.'));
            let sibling = match (name.rsplit_once('.'), k.rsplit_once('.')) {
                (Some((p, a)), Some((q, b))) => p == q && phase(a) && phase(b),
                _ => false,
            };
            nested || sibling
        };
        let hooks: BTreeSet<&String> = self
            .exact
            .iter()
            .filter(|(k, _)| related(k))
            .flat_map(|(_, hooks)| hooks)
            .collect();
        if !hooks.is_empty() {
            return Status::Phase(hooks.into_iter().cloned().collect());
        }
        match self.noted.get(name) {
            Some(hooks) => Status::Noted(hooks.clone()),
            None => Status::Missing,
        }
    }
}

/// An event (`…Event`, nested or not) or a capability (`Capabilities.X.Y`).
fn is_name(s: &str) -> bool {
    let first = s.split('.').next().unwrap_or_default();
    (first.ends_with("Event") || first == "Capabilities")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// The names a label gives (`ServerTickEvent.Pre / .Post`): notes in
/// brackets and a leading package dropped, a name that starts with `.` taken
/// as a sibling of the one before it.
fn counterparts(label: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in label.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let mut out: Vec<String> = Vec::new();
    for part in plain.split(" / ").map(str::trim) {
        let full = match (part.strip_prefix('.'), out.last()) {
            (Some(rest), Some(prev)) => match prev.rsplit_once('.') {
                Some((parent, _)) => format!("{parent}.{rest}"),
                None => continue,
            },
            _ => part
                .split('.')
                .skip_while(|s| s.starts_with(|c: char| c.is_ascii_lowercase()))
                .collect::<Vec<_>>()
                .join("."),
        };
        if is_name(&full) {
            out.push(full);
        }
    }
    out
}

/// The names a `none (…)` label mentions.
fn mentioned(label: &str) -> Vec<String> {
    label
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        .map(|w| w.trim_end_matches('.'))
        .filter(|w| is_name(w))
        .map(str::to_owned)
        .collect()
}

/// The events one jar's classes listen to and the capabilities they use,
/// as the catalog names them (`ServerTickEvent.Post`,
/// `Capabilities.ItemHandler.BLOCK`). Jars inside the jar aren't opened.
pub fn scan<R: Read + Seek>(jar: R) -> Result<BTreeSet<String>, String> {
    let mut zip = zip::ZipArchive::new(jar).map_err(|e| e.to_string())?;
    let mut names = BTreeSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if !entry.name().is_ok_and(|n| n.ends_with(".class")) || entry.size() >= 1 << 24 {
            continue;
        }
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
        if let Some(class) = classes::parse(&bytes) {
            names.extend(
                class
                    .listens
                    .iter()
                    .map(|c| c.rsplit('/').next().unwrap_or(c).replace('$', ".")),
            );
            names.extend(class.capabilities);
        }
    }
    Ok(names)
}

/// The report for the jar at `path`, or for every jar in the folder at
/// `path`: each name, how many mods use it, its hooks and a few of the mods.
pub fn report(path: &Path, catalog: &str) -> Result<String, String> {
    let catalog = Catalog::parse(catalog)?;
    let jars: Vec<PathBuf> = if path.is_dir() {
        let mut jars: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "jar"))
            .collect();
        jars.sort();
        jars
    } else {
        vec![path.to_path_buf()]
    };
    let mut mods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut unread = Vec::new();
    for jar_path in &jars {
        let names = fs::File::open(jar_path)
            .map_err(|e| e.to_string())
            .and_then(|f| scan(BufReader::new(f)));
        let names = match names {
            Ok(names) => names,
            Err(e) if jars.len() == 1 => return Err(format!("{}: {e}", jar_path.display())),
            Err(_) => {
                unread.push(jar_path.display().to_string());
                continue;
            }
        };
        let id = jar::read(jar_path)
            .ok()
            .map(|info| info.mod_id)
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| {
                let stem = jar_path.file_stem().unwrap_or_default();
                stem.to_string_lossy().into_owned()
            });
        mods.entry(id).or_default().extend(names);
    }
    let mut out = table(&mods, &catalog);
    if !unread.is_empty() {
        out += &format!("\nNot read as jars: {}.\n", unread.join(", "));
    }
    Ok(out)
}

/// The ranked table over `mods` (mod id to the names it uses).
pub fn table(mods: &BTreeMap<String, BTreeSet<String>>, catalog: &Catalog) -> String {
    let mut users: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, names) in mods {
        for n in names {
            users.entry(n).or_default().push(id);
        }
    }
    let mut rows: Vec<(&str, Vec<&str>)> = users.into_iter().collect();
    rows.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    let code = |hooks: &[String]| {
        let hooks: Vec<String> = hooks.iter().map(|h| format!("`{h}`")).collect();
        hooks.join(", ")
    };
    let mut counts = [0usize; 4];
    let mut body = String::new();
    for (name, ids) in &rows {
        let (status, under) = catalog.status(name);
        let (i, hooks) = match &status {
            Status::Hooked(h) => (0, code(h)),
            Status::Phase(h) => (1, format!("other phase: {}", code(h))),
            Status::Noted(h) => (2, format!("not quite: {}", code(h))),
            Status::Missing => (3, "none".to_owned()),
        };
        counts[i] += 1;
        let under = under.map(|u| format!(" (as `{u}`)")).unwrap_or_default();
        let more = if ids.len() > 3 { ", …" } else { "" };
        let some = ids[..ids.len().min(3)].join(", ");
        body += &format!(
            "| {} | `{name}` | {hooks}{under} | {some}{more} |\n",
            ids.len()
        );
    }
    let listening = mods.values().filter(|names| !names.is_empty()).count();
    format!(
        "{} mods; {listening} listen to NeoForge events or use its capabilities.\n\
         {} names: {} with a hook, {} with another phase's, {} noted as not quite one, {} with none.\n\n\
         | Mods | NeoForge | Hooks | Used by |\n|---:|---|---|---|\n{body}",
        mods.len(),
        rows.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3],
    )
}
