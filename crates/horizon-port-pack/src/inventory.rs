//! Step 2, inventory: one record per pack file, from the jar (when the
//! user's instance has it) and Modrinth's metadata, classified and ordered.
use crate::{
    classify::{self, Facts, Known},
    import::Pack,
    jar::{self, JarInfo},
    modrinth::Cache,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Record {
    /// The mod id from the jar, else the Modrinth slug, else the file name.
    pub key: String,
    pub name: String,
    pub version: String,
    pub path: String,
    pub class: String,
    pub tier: String,
    /// `known` (reviewed table), `sides`, `categories`, `jar`, `loaders`,
    /// `kind`, `guess` or `unknown`.
    pub confidence: String,
    /// `modrinth`, `curseforge` or `file`.
    pub source: String,
    pub license: String,
    pub client: String,
    pub server: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// Required dependencies inside the pack, by key.
    pub depends: Vec<String>,
    /// Required dependencies the pack does not have.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    pub layer: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<JarInfo>,
}

/// The jar of a pack file: the instance's own, or one the user put in
/// `<work>/jars/`.
fn jar_of(pack_jar: Option<&PathBuf>, file_name: &str, work: &Path) -> Option<PathBuf> {
    pack_jar
        .cloned()
        .or_else(|| Some(work.join("jars").join(file_name)))
        .filter(|p| p.is_file())
}

pub fn build(pack: &Pack, cache: &Cache, work: &Path) -> Vec<Record> {
    let known = Known::atm10();
    let mut records = Vec::new();
    // Modrinth project id -> record key, for dependencies.
    let mut by_project: HashMap<String, String> = HashMap::new();
    let mut project_deps: Vec<Vec<String>> = Vec::new();
    for f in &pack.files {
        let jar = jar_of(f.jar.as_ref(), f.file_name(), work);
        let info = jar.as_deref().and_then(|j| jar::read(j).ok());
        let sha1 = f.sha1.as_deref().unwrap_or("");
        let project = cache.project_of(sha1);
        let p = |k: &str| project.and_then(|p| p[k].as_str()).unwrap_or("").to_owned();
        let categories: Vec<String> = project
            .into_iter()
            .flat_map(|p| {
                ["categories", "additional_categories"]
                    .iter()
                    .flat_map(|k| p[*k].as_array().cloned().unwrap_or_default())
            })
            .filter_map(|c| c.as_str().map(str::to_owned))
            .collect();
        let project_loaders: Vec<String> = project
            .and_then(|p| p["loaders"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str().map(str::to_owned))
            .collect();
        let stem = f.file_name().trim_end_matches(".jar").to_owned();
        let key = info
            .as_ref()
            .map(|i| i.mod_id.clone())
            .filter(|k| !k.is_empty())
            .or_else(|| f.mod_id.clone())
            .or_else(|| Some(p("slug")).filter(|s| !s.is_empty()))
            .unwrap_or(stem);
        let (client, server) = match project {
            Some(_) if f.client.is_empty() => (p("client_side"), p("server_side")),
            _ => (f.client.clone(), f.server.clone()),
        };
        let census = info.as_ref().map(|i| &i.census);
        let facts = Facts {
            kind: f.kind(),
            client: &client,
            server: &server,
            categories: &categories,
            project_loaders: &project_loaders,
            classes: census.map(|c| c.classes),
            has_content: census
                .is_some_and(|c| c.blockstates + c.item_models + c.recipes + c.data_files > 0),
        };
        let name = info
            .as_ref()
            .map(|i| i.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| Some(p("title")).filter(|t| !t.is_empty()))
            .or_else(|| f.name.clone())
            .unwrap_or_default();
        let (class, tier, confidence) =
            match known.get(&[key.as_str(), p("slug").as_str(), name.as_str()]) {
                Some((c, t)) => (c.clone(), t.clone(), "known".to_owned()),
                None => {
                    let (c, t, conf) = classify::rules(&facts);
                    (c.into(), t.into(), conf.into())
                }
            };
        let license = info
            .as_ref()
            .map(|i| i.license.clone())
            .filter(|l| !l.is_empty())
            .or_else(|| project.and_then(|p| p["license"]["id"].as_str().map(str::to_owned)))
            .unwrap_or_else(|| "unknown".into());
        if let Some(id) = project.and_then(|p| p["id"].as_str()) {
            by_project.insert(id.to_owned(), key.clone());
        }
        project_deps.push(cache.required(sha1));
        records.push(Record {
            name,
            version: info.as_ref().map(|i| i.version.clone()).unwrap_or_else(|| {
                cache
                    .versions
                    .get(sha1)
                    .and_then(|v| v["version_number"].as_str())
                    .map(str::to_owned)
                    .or_else(|| f.version.clone())
                    .unwrap_or_default()
            }),
            path: f.path.clone(),
            class,
            tier,
            confidence,
            source: if project.is_some() {
                "modrinth".into()
            } else if f.curseforge.is_some() {
                "curseforge".into()
            } else {
                "file".into()
            },
            license,
            client,
            server,
            categories,
            depends: info.as_ref().map(|i| i.depends.clone()).unwrap_or_default(),
            jar,
            info,
            key,
            ..Record::default()
        });
    }
    // Dependencies: the jar's mod ids, else Modrinth's required projects.
    let keys: std::collections::HashSet<String> = records.iter().map(|r| r.key.clone()).collect();
    for (r, deps) in records.iter_mut().zip(project_deps) {
        if r.depends.is_empty() {
            r.depends = deps
                .iter()
                .map(|p| {
                    by_project
                        .get(p)
                        .cloned()
                        .unwrap_or_else(|| format!("modrinth:{p}"))
                })
                .collect();
        }
        let (inside, missing): (Vec<String>, Vec<String>) =
            r.depends.drain(..).partition(|d| keys.contains(d));
        r.depends = inside;
        r.missing = missing;
    }
    let graph: BTreeMap<String, Vec<String>> = records
        .iter()
        .map(|r| (r.key.clone(), r.depends.clone()))
        .collect();
    let layers = classify::layers(&graph);
    for r in &mut records {
        r.layer = layers.get(&r.key).copied().unwrap_or(0);
    }
    records.sort_by(|a, b| (a.layer, &a.key).cmp(&(b.layer, &b.key)));
    records
}

/// Counts by a field, largest first.
pub fn counts<'a>(
    records: &'a [Record],
    field: impl Fn(&'a Record) -> &'a str,
) -> Vec<(&'a str, usize)> {
    let mut m: BTreeMap<&str, usize> = BTreeMap::new();
    for r in records {
        *m.entry(field(r)).or_default() += 1;
    }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v
}

/// `inventory.csv`: the columns of `docs/modpacks/atm10-mods.csv`, plus the layer.
pub fn csv(records: &[Record]) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    };
    let mut out = String::from(
        "mod_id,name,version,class,tier,confidence,source,license,client,server,layer\n",
    );
    for r in records {
        let row = [
            &r.key,
            &r.name,
            &r.version,
            &r.class,
            &r.tier,
            &r.confidence,
            &r.source,
            &r.license,
            &r.client,
            &r.server,
        ];
        let cells: Vec<String> = row.iter().map(|s| field(s)).collect();
        out.push_str(&format!("{},{}\n", cells.join(","), r.layer));
    }
    out
}
