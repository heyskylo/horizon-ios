//! Step 7, the coverage report: per mod, ported, partial, not yet or not
//! needed, with the reason, and the totals by class, tier and licence
//! (`coverage.json`, `coverage.md`). It reads what the other steps wrote:
//! the inventory, each mod's `carry-report.json` and its harness result
//! (`tools/mod_port_harness.py`).
use crate::{
    inventory::Record,
    resourcepacks::{Outcome, Report, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Ported,
    Partial,
    NotYet,
    NotNeeded,
}
impl Status {
    pub const ALL: [Status; 4] = [
        Status::Ported,
        Status::Partial,
        Status::NotYet,
        Status::NotNeeded,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Status::Ported => "ported",
            Status::Partial => "partial",
            Status::NotYet => "not yet",
            Status::NotNeeded => "not needed",
        }
    }
    /// What the status leaves out, in plain words.
    pub fn meaning(self) -> &'static str {
        match self {
            Status::Ported => PLAIN_PORTED,
            Status::Partial => PLAIN_PARTIAL,
            Status::NotYet => "It does not come over yet.",
            Status::NotNeeded => "Nothing to port: Horizon has it built in or does not need it.",
        }
    }
}

const PLAIN_PORTED: &str =
    "Its blocks and items came over and passed Horizon's checks; the mod's own code is not run.";
const PLAIN_PARTIAL: &str = "Part of it came over, but something is left out, differs from the original or has not been checked yet; the technical detail says what.";

#[derive(Debug, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub name: String,
    pub class: String,
    pub tier: String,
    pub license: String,
    /// `permissive`, `copyleft`, `all-rights-reserved`, `restricted`
    /// (non-commercial or no-derivatives), `custom` or `unknown`.
    pub license_kind: String,
    pub status: Status,
    /// The technical reason, for modders (the `why` column of `coverage.md`).
    pub reason: String,
    /// The same in plain words for a player. Empty in a `coverage.json` an older build wrote.
    #[serde(default)]
    pub plain: String,
    /// Unported dependencies, kept apart so the launcher can translate the fixed sentence.
    #[serde(default)]
    pub waiting: usize,
}

impl Row {
    /// The standalone coverage report stays English, including the dependency count.
    pub fn plain_text(&self) -> String {
        let mut plain = self.plain.clone();
        match self.waiting {
            0 => {}
            1 => plain.push_str(" It also waits for 1 mod it depends on that is not ported yet."),
            n => {
                let _ = write!(
                    plain,
                    " It also waits for {n} mods it depends on that are not ported yet."
                );
            }
        }
        plain
    }
}

/// The licence family of an SPDX id or a `mods.toml` licence text.
pub fn license_kind(license: &str) -> &'static str {
    let l = license.to_ascii_lowercase();
    if l.is_empty() || l.starts_with("unknown") {
        "unknown"
    } else if l.contains("-nc") || l.contains("-nd") || l.contains("noncommercial") {
        "restricted"
    } else if l.contains("all-rights-reserved") || l.contains("all rights reserved") || l == "arr" {
        "all-rights-reserved"
    } else if ["gpl", "mpl", "eupl", "general public"]
        .iter()
        .any(|k| l.contains(k))
    {
        "copyleft"
    } else if [
        "mit",
        "apache",
        "bsd",
        "cc0",
        "cc-by",
        "unlicense",
        "isc",
        "zlib",
    ]
    .iter()
    .any(|k| l.contains(k))
    {
        "permissive"
    } else {
        "custom"
    }
}

/// What still blocks a mod of this tier that has not been carried over.
fn not_yet(tier: &str) -> &'static str {
    match tier {
        "data" => "data tier: carry-over only, no agent; its jar is not here yet (fetch gets it)",
        "logic" => {
            "needs mod API 0.3: behaviour callbacks (use, place, break, ticks) and server events"
        }
        "machines" => "needs mod API 0.4: block entities, menus, capabilities and mod networking",
        "deep" => {
            "needs mod API 0.5: entities, custom renderers and models, dimensions, new vanilla hooks"
        }
        "client" => "needs the client interfaces (keys, target, inventory-read, ui, render)",
        _ => "not classified",
    }
}

fn not_needed(class: &str) -> &'static str {
    match class {
        "library" => "library: its API maps to Horizon's mod API and SDK",
        "performance" => "performance mod: Horizon's engine is the replacement",
        "other" => "pack tooling or an engine feature, folded into the pipeline or the engine",
        "client-ui" => "built into Horizon",
        _ => "nothing to port",
    }
}

/// What keeps a mod that has not been carried over from coming over, in plain words.
fn plain_not_yet(r: &Record) -> &'static str {
    if r.class == "unknown" {
        return "Horizon cannot tell yet what this mod does (it needs its jar or a reviewed entry), so it cannot say whether it will come over.";
    }
    if r.class == "integration" {
        return "It joins other mods together, so it waits until those are ported.";
    }
    match r.tier.as_str() {
        "data" if r.jar.is_some() => {
            "Content only (blocks, items, structures). Its jar is here, but it has not been carried over yet."
        }
        "data" => {
            "Content only (blocks, items, structures). It can come over once its jar is here."
        }
        "logic" => {
            "Needs what blocks and items do when used, placed, broken or ticked, which Horizon's mod API cannot run from a mod yet, so it does not come over."
        }
        "machines" => {
            "Needs machine-style features (block entities, menus, item, fluid and energy handling, networking) that Horizon's mod API does not have yet, so it does not come over."
        }
        "deep" => {
            "Needs mobs, custom rendering and models, dimensions or new game hooks that Horizon's mod API does not have yet, so it does not come over."
        }
        "client" => {
            "Needs the game's client interfaces (keys, targeting, inventory reading, interface, rendering), so it does not come over yet."
        }
        _ => "Horizon has not worked out what this mod needs yet.",
    }
}

fn plain_not_needed(class: &str) -> &'static str {
    match class {
        "library" => {
            "A library other mods build on. Horizon's mod API and SDK take its place, so there is nothing to port."
        }
        "performance" => {
            "It makes the game faster. Horizon's own engine does that, so there is nothing to port."
        }
        "other" => {
            "Pack tooling or a game feature that Horizon already has, so there is nothing to port."
        }
        "client-ui" => "Built into Horizon already, so there is nothing to port.",
        _ => "Nothing to port.",
    }
}

/// A carried-over mod: ported when its harness passes with nothing left
/// out and nothing the templates miss; else partial, with what is missing.
fn carried(work: &Path, key: &str) -> Option<(Status, String)> {
    let dir = work.join("mods").join(key);
    let carry: Value =
        serde_json::from_slice(&std::fs::read(dir.join("carry-report.json")).ok()?).ok()?;
    let count = |k: &str| carry[k].as_object().map_or(0, |o| o.len());
    let (blocks, items) = (count("blocks"), carry["items"].as_u64().unwrap_or(0));
    let (left_blocks, left_items) = (count("blocks_unknown"), count("items_unknown"));
    // A class of its own is code the facts cannot show; anything else the
    // template misses (shapes, sounds) the harness also reports.
    let mut differences = 0;
    let mut own_code = 0;
    for list in carry["differences"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, v)| v)
    {
        let notes: Vec<&str> = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if notes.iter().any(|n| n.starts_with("class ")) {
            own_code += 1;
        }
        if notes.iter().any(|n| !n.starts_with("class ")) {
            differences += 1;
        }
    }
    let mut gaps = Vec::new();
    if left_blocks > 0 {
        gaps.push(format!(
            "{left_blocks} blocks left out (no vanilla template)"
        ));
    }
    if left_items > 0 {
        gaps.push(format!("{left_items} items left out"));
    }
    if differences > 0 {
        gaps.push(format!(
            "{differences} blocks or items differ from their templates"
        ));
    }
    let harness: Option<Value> = std::fs::read(dir.join("harness/result.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let mut checked = 0;
    match &harness {
        Some(h) => {
            if h["passed"].as_bool() == Some(false) {
                gaps.push("harness failed".into());
            }
            for (name, c) in h["checks"].as_object().into_iter().flatten() {
                let (p, f) = (
                    c["passed"].as_u64().unwrap_or(0),
                    c["failed"].as_u64().unwrap_or(0),
                );
                checked += p + f;
                if f > 0 {
                    gaps.push(format!("{name} {p}/{}", p + f));
                }
            }
            if checked == 0 {
                gaps.push("harness has no checks".into());
            }
        }
        None => gaps.push("harness not run".into()),
    }
    let worldgen = carry["worldgen"].as_u64().unwrap_or(0);
    if blocks == 0 && items == 0 && checked == 0 {
        let what = if worldgen > 0 {
            format!("{worldgen} worldgen files carried, not compared yet")
        } else {
            "nothing registered to compare".into()
        };
        return Some((Status::Partial, what));
    }
    let mut summary = format!("{blocks} blocks, {items} items");
    if own_code > 0 {
        let _ = write!(
            summary,
            " ({own_code} with code of their own, not compared)"
        );
    }
    if gaps.is_empty() {
        Some((Status::Ported, format!("{summary}; harness green")))
    } else {
        Some((Status::Partial, format!("{summary}; {}", gaps.join("; "))))
    }
}

fn resource_row(path: &str, outcome: Option<&Outcome>, record: Option<&Record>) -> Row {
    let state = outcome.map_or(State::Missing, |o| o.state);
    let shader = path.starts_with("shaderpacks/");
    Row {
        key: path.into(),
        name: path.into(),
        class: if shader { "shaderpack" } else { "resourcepack" }.into(),
        tier: "data".into(),
        license: record.map_or("unknown", |r| r.license.as_str()).into(),
        license_kind: license_kind(record.map_or("unknown", |r| r.license.as_str())).into(),
        status: if !shader && state == State::Available {
            Status::Partial
        } else {
            Status::NotYet
        },
        waiting: 0,
        plain: match (shader, outcome.map(|o| o.state)) {
            (true, _) => "Shader packs are not converted yet.",
            (false, Some(State::Available)) => "Copied as a resource pack; Horizon has not checked how it renders. Turn it on in the game's Resource Packs screen.",
            (false, None) => "Not downloaded yet; Port gets it.",
            (false, Some(_)) => "Horizon could not use this resource pack yet; the technical detail says why.",
        }
        .into(),
        reason: outcome.map_or_else(
            || {
                if shader {
                    "shader-pack conversion is pending".into()
                } else {
                    "resource pack not obtained yet; run port".into()
                }
            },
            |o| format!("{:?}: {}", o.state, o.detail),
        ),
    }
}

pub fn build(records: &[Record], work: &Path) -> Vec<Row> {
    let resourcepacks = Report::load(work);
    let mut rows: Vec<Row> = records
        .iter()
        .map(|r| {
            if r.path.starts_with("resourcepacks/") || r.path.starts_with("shaderpacks/") {
                return resource_row(&r.path, resourcepacks.files.get(&r.path), Some(r));
            }
            let (status, reason) = if r.class == "unknown" {
                let why =
                    "unclassified: its jar (mods.toml, file census) or a reviewed row decides";
                (Status::NotYet, why.to_owned())
            } else if r.tier == "none" || matches!(r.class.as_str(), "library" | "performance") {
                (Status::NotNeeded, not_needed(&r.class).to_owned())
            } else if let Some(c) = carried(work, &r.key) {
                c
            } else {
                let mut why = not_yet(&r.tier).to_owned();
                if r.tier == "data" && r.jar.is_some() {
                    why = "data tier: carry-over not run yet".into();
                }
                if r.class == "integration" {
                    why = format!("after the mods it joins are ported; {why}");
                }
                (Status::NotYet, why)
            };
            Row {
                key: r.key.clone(),
                name: r.name.clone(),
                class: r.class.clone(),
                tier: r.tier.clone(),
                license: r.license.clone(),
                license_kind: license_kind(&r.license).to_owned(),
                status,
                reason,
                waiting: 0,
                plain: match status {
                    Status::NotYet => plain_not_yet(r),
                    Status::NotNeeded => plain_not_needed(&r.class),
                    carried => carried.meaning(),
                }
                .into(),
            }
        })
        .collect();
    // A mod waits on its dependencies that are not ported.
    let status: BTreeMap<String, Status> = rows.iter().map(|r| (r.key.clone(), r.status)).collect();
    for (row, r) in rows.iter_mut().zip(records) {
        if row.status == Status::NotNeeded {
            continue;
        }
        let waiting: Vec<&str> = r
            .depends
            .iter()
            .filter(|d| status.get(*d) == Some(&Status::NotYet))
            .map(String::as_str)
            .collect();
        if !waiting.is_empty() {
            let _ = write!(row.reason, "; waits on {}", waiting.join(", "));
            row.waiting = waiting.len();
        }
    }
    for (path, outcome) in &resourcepacks.files {
        if !records.iter().any(|r| &r.path == path) {
            rows.push(resource_row(path, Some(outcome), None));
        }
    }
    rows
}

/// What a pack's mods add up to, for the headline above the numbers. It counts the report's own
/// rows and tiers; resource packs and shader packs are not mods and are left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Mods that need a port: all but the "not needed" ones.
    pub needing: usize,
    /// Ported or partial: carried over, as content.
    pub carried: usize,
    /// Of those, partial.
    pub partial: usize,
    /// Not yet: content of the data tier, which port can carry over.
    pub ready: usize,
    /// Not yet: needs mod API Horizon does not have yet (every tier above data).
    pub blocked: usize,
    /// Not yet, for another reason: not identified, or waiting on other mods or other work.
    pub later: usize,
}

pub fn summary(rows: &[Row]) -> Summary {
    let mut s = Summary::default();
    for r in rows
        .iter()
        .filter(|r| !matches!(r.class.as_str(), "resourcepack" | "shaderpack"))
    {
        match r.status {
            Status::NotNeeded => continue,
            Status::Ported => s.carried += 1,
            Status::Partial => (s.carried, s.partial) = (s.carried + 1, s.partial + 1),
            Status::NotYet if r.class == "content" && r.tier == "data" => s.ready += 1,
            Status::NotYet
                if r.class != "unknown"
                    && matches!(r.tier.as_str(), "logic" | "machines" | "deep" | "client") =>
            {
                s.blocked += 1
            }
            Status::NotYet => s.later += 1,
        }
        s.needing += 1;
    }
    s
}

impl Summary {
    /// The headline in plain words, one paragraph per line; none for a pack with no mod to port.
    /// The launcher words the same lines through its translations (a test keeps them equal).
    pub fn lines(&self) -> Vec<String> {
        if self.needing == 0 {
            return Vec::new();
        }
        let mut lines = vec![format!(
            "{} of {} mods that need porting come over as content (blocks, items and structures); their own code does not run.",
            self.carried, self.needing
        )];
        if self.ready + self.blocked + self.later > 0 {
            lines.push(format!(
                "Not yet: {} are content mods Port can carry over, {} need mod support Horizon does not have yet (machines, menus, behaviour, mobs, dimensions, client features), {} are not sorted yet (not identified, or waiting on other mods or other work).",
                self.ready, self.blocked, self.later
            ));
        }
        if self.partial > 0 {
            lines.push(
                "Partial means part of a mod came over, but something is left out, differs from the original or has not been checked yet."
                    .into(),
            );
        }
        lines
    }
}

/// Rows counted by `field` and status.
fn table(rows: &[Row], title: &str, field: fn(&Row) -> &str) -> String {
    let mut counts: BTreeMap<&str, [usize; 4]> = BTreeMap::new();
    for r in rows {
        let i = Status::ALL.iter().position(|s| *s == r.status).unwrap_or(0);
        counts.entry(field(r)).or_default()[i] += 1;
    }
    let mut out = format!(
        "| {title} | ported | partial | not yet | not needed | all |\n|---|---:|---:|---:|---:|---:|\n"
    );
    let mut totals = [0usize; 4];
    for (k, c) in &counts {
        let all: usize = c.iter().sum();
        let _ = writeln!(
            out,
            "| {k} | {} | {} | {} | {} | {all} |",
            c[0], c[1], c[2], c[3]
        );
        for (t, n) in totals.iter_mut().zip(c) {
            *t += n;
        }
    }
    let all: usize = totals.iter().sum();
    let _ = writeln!(
        out,
        "| **all** | **{}** | **{}** | **{}** | **{}** | **{all}** |",
        totals[0], totals[1], totals[2], totals[3]
    );
    out
}

pub fn markdown(pack: &str, rows: &[Row]) -> String {
    let mut out = format!("# Coverage: {pack}\n\n");
    let lines = summary(rows).lines();
    if !lines.is_empty() {
        out += &format!("**{}**\n\n", lines[0]);
        for line in &lines[1..] {
            out += &format!("{line}\n\n");
        }
        out += "The tables below use the technical terms; the last column of the list says the same in plain words.\n\n";
    }
    out += &table(rows, "class", |r| &r.class);
    out += "\n";
    out += &table(rows, "tier", |r| &r.tier);
    out += "\n";
    out += &table(rows, "licence", |r| &r.license_kind);
    out += "\nPorts stay local. Only permissive or copyleft ports could ever be published, and only after Ludovic asks the authors.\n\n";
    out += "| mod | class | tier | licence | status | why | in plain words |\n|---|---|---|---|---|---|---|\n";
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| (a.status, &a.key).cmp(&(b.status, &b.key)));
    for r in sorted {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            r.key,
            r.class,
            r.tier,
            r.license.replace('|', "/"),
            r.status.label(),
            r.reason.replace('|', "/"),
            r.plain_text().replace('|', "/")
        );
    }
    out
}

pub fn counts(rows: &[Row]) -> Vec<(&'static str, usize)> {
    Status::ALL
        .iter()
        .map(|s| (s.label(), rows.iter().filter(|r| r.status == *s).count()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(key: &str, class: &str, tier: &str, depends: &[&str]) -> Record {
        Record {
            key: key.into(),
            class: class.into(),
            tier: tier.into(),
            license: "MIT".into(),
            depends: depends.iter().map(|d| (*d).to_owned()).collect(),
            ..Record::default()
        }
    }

    #[test]
    fn statuses_and_reasons() {
        let work = std::env::temp_dir().join(format!("horizon-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let write = |key: &str, carry: Value, harness: Option<Value>| {
            let dir = work.join("mods").join(key);
            std::fs::create_dir_all(dir.join("harness")).unwrap();
            std::fs::write(dir.join("carry-report.json"), carry.to_string()).unwrap();
            if let Some(h) = harness {
                std::fs::write(dir.join("harness/result.json"), h.to_string()).unwrap();
            }
        };
        let green = serde_json::json!({"checks": {"blocks": {"passed": 3, "failed": 0}}});
        write(
            "doors",
            serde_json::json!({"blocks": {"a": "minecraft:oak_door"}, "items": 0,
                "differences": {"a": ["class m.Door (behaviour beyond DoorBlock)"]}}),
            Some(green),
        );
        write(
            "paths",
            serde_json::json!({"blocks": {"a": "minecraft:stone"}, "items": 1,
                "blocks_unknown": {"b": "no vanilla Block"}}),
            Some(serde_json::json!({"checks": {"shapes": {"passed": 1, "failed": 2}}})),
        );
        let records = [
            record("doors", "content", "data", &[]),
            record("paths", "content", "data", &[]),
            record("lib", "library", "none", &[]),
            record("machine", "content", "machines", &["lib"]),
            record("addon", "integration", "logic", &["machine"]),
            record("mystery", "unknown", "none", &[]),
        ];
        let rows = build(&records, &work);
        let by: BTreeMap<&str, &Row> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
        assert_eq!(by["doors"].status, Status::Ported);
        assert!(
            by["doors"].reason.contains("1 with code of their own"),
            "{}",
            by["doors"].reason
        );
        assert_eq!(by["paths"].status, Status::Partial);
        assert!(
            by["paths"].reason.contains("1 blocks left out"),
            "{}",
            by["paths"].reason
        );
        assert!(
            by["paths"].reason.contains("shapes 1/3"),
            "{}",
            by["paths"].reason
        );
        assert_eq!(by["lib"].status, Status::NotNeeded);
        assert_eq!(by["machine"].status, Status::NotYet);
        assert!(by["machine"].reason.contains("0.4"));
        assert!(by["addon"].reason.starts_with("after the mods it joins"));
        assert!(
            by["addon"].reason.ends_with("waits on machine"),
            "{}",
            by["addon"].reason
        );
        assert_eq!(by["mystery"].status, Status::NotYet);
        assert_eq!(
            counts(&rows),
            [
                ("ported", 1),
                ("partial", 1),
                ("not yet", 3),
                ("not needed", 1)
            ]
        );
        let md = markdown("test", &rows);
        assert!(
            md.contains("| **all** | **1** | **1** | **3** | **1** | **6** |"),
            "{md}"
        );
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn carried_harness_needs_executed_checks() {
        for harness in [
            serde_json::json!({"passed": false, "checks": {}}),
            serde_json::json!({"passed": true, "checks": {}}),
            serde_json::json!({"checks": {"blocks": {"passed": 0, "failed": 0}}}),
            serde_json::json!({}),
        ] {
            let row = carried_harness_row(Some(&harness.to_string()));
            assert_eq!(row.status, Status::Partial, "{harness}: {}", row.reason);
            assert!(row.reason.contains("no checks"), "{}", row.reason);
            assert!(!row.reason.contains("harness green"), "{}", row.reason);
        }
    }

    #[test]
    fn carried_harness_rejects_failed_verdicts() {
        for harness in [
            serde_json::json!({"passed": false,
                "checks": {"blocks": {"passed": 3, "failed": 0}}}),
            serde_json::json!({"passed": true,
                "checks": {"blocks": {"passed": 1, "failed": 2}}}),
            serde_json::json!({"checks": {"blocks": {"passed": 1, "failed": 2}}}),
        ] {
            let row = carried_harness_row(Some(&harness.to_string()));
            assert_eq!(row.status, Status::Partial, "{harness}: {}", row.reason);
            assert!(!row.reason.contains("harness green"), "{}", row.reason);
            if harness["passed"] == false {
                assert!(row.reason.contains("harness failed"), "{}", row.reason);
            } else {
                assert!(row.reason.contains("blocks 1/3"), "{}", row.reason);
            }
        }
    }

    #[test]
    fn carried_harness_keeps_current_and_legacy_successes() {
        for harness in [
            serde_json::json!({"passed": true,
                "checks": {"blocks": {"passed": 3, "failed": 0}}}),
            serde_json::json!({"checks": {"blocks": {"passed": 3, "failed": 0}}}),
        ] {
            let row = carried_harness_row(Some(&harness.to_string()));
            assert_eq!(row.status, Status::Ported, "{harness}: {}", row.reason);
            assert!(row.reason.contains("harness green"), "{}", row.reason);
        }
    }

    #[test]
    fn carried_harness_keeps_missing_or_unreadable_results_partial() {
        for harness in [None, Some("not JSON")] {
            let row = carried_harness_row(harness);
            assert_eq!(row.status, Status::Partial, "{}", row.reason);
            assert!(row.reason.contains("harness not run"), "{}", row.reason);
        }
    }

    fn carried_harness_row(harness: Option<&str>) -> Row {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let work = std::env::temp_dir().join(format!(
            "horizon-report-verdict-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&work).unwrap();
        let dir = work.join("mods/example");
        std::fs::create_dir_all(dir.join("harness")).unwrap();
        std::fs::write(
            dir.join("carry-report.json"),
            r#"{"blocks":{"example:block":"minecraft:stone"},"items":0}"#,
        )
        .unwrap();
        if let Some(harness) = harness {
            std::fs::write(dir.join("harness/result.json"), harness).unwrap();
        }
        let row = build(&[record("example", "content", "data", &[])], &work)
            .pop()
            .unwrap();
        std::fs::remove_dir_all(work).unwrap();
        row
    }

    #[test]
    fn licence_families() {
        assert_eq!(license_kind("MIT"), "permissive");
        assert_eq!(license_kind("CC0-1.0"), "permissive");
        assert_eq!(license_kind("LGPL-3.0-only"), "copyleft");
        assert_eq!(
            license_kind("GNU Lesser General Public License v3.0"),
            "copyleft"
        );
        assert_eq!(
            license_kind("LicenseRef-All-Rights-Reserved"),
            "all-rights-reserved"
        );
        assert_eq!(license_kind("CC-BY-NC-SA-4.0"), "restricted");
        assert_eq!(license_kind("LicenseRef-Stardust-Labs-License"), "custom");
        assert_eq!(
            license_kind("unknown (read mods.toml at import)"),
            "unknown"
        );
    }
}
