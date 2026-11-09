//! Jars that are only a data pack: no classes, a `pack.mcmeta` and a `data/` folder. They register
//! nothing a content mod could carry, and a content mod may not hold what they often exist for
//! (`data/minecraft`: a void world's noise settings and biomes). So they stay data packs: upgraded
//! to 26.3 into the profile's folder of packs for new worlds, which the game copies into each
//! world it creates (`horizon_client::datapack::NEW_WORLD_PACKS`). A world made earlier is left
//! as it is: its terrain was generated without them.
use crate::{inventory::Record, pipeline, pipeline::Log, upgrade};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const REPORT: &str = "datapacks.json";

/// The profile's packs for new worlds, in the profile's game folder (`<work>/home`).
pub fn folder(work: &Path) -> PathBuf {
    work.join("home").join("new-world-datapacks")
}

/// What became of one data-only jar.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Installed {
    /// Why it could not be installed; empty when it was.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// What the upgrade could not keep faithfully, or does not know.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The jar also has `assets/`, which a data pack cannot load.
    #[serde(default)]
    pub assets: bool,
}

/// `<work>/datapacks.json`: what happened to each data-only jar, by record key.
pub fn load(work: &Path) -> BTreeMap<String, Installed> {
    pipeline::read_json(&work.join(REPORT)).unwrap_or_default()
}

/// How a pack of this Minecraft version becomes a 26.3 data pack, if one can.
fn upgrades(minecraft: &str) -> Option<bool> {
    let series = |s: &str| minecraft == s || minecraft.starts_with(&format!("{s}."));
    if series("26.2") {
        Some(true)
    } else if series("26.3") {
        Some(false)
    } else {
        None
    }
}

/// Whether this record's jar is a data pack this pack's version can bring over. Older versions keep
/// the content mod carry-over, which upgrades their recipes and loot tables from 1.21.1.
pub fn is_data_only(r: &Record, minecraft: &str) -> bool {
    upgrades(minecraft).is_some()
        && r.class == "content"
        && r.tier == "data"
        && r.jar.is_some()
        && r.info.as_ref().is_some_and(|i| {
            i.census.classes == 0 && i.census.pack_mcmeta && i.census.data_files > 0
        })
}

/// A record key is a folder name here, and it comes from the jar's own metadata: one plain name
/// only, never a path.
fn plain_name(key: &str) -> bool {
    !key.is_empty()
        && key != "."
        && key != ".."
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// The content mod an older build carried from the same jar, which would load beside the pack.
fn drop_carry(work: &Path, key: &str) -> Result<(), String> {
    let carried = work.join("mods").join(key);
    if carried.exists() {
        fs::remove_dir_all(&carried).map_err(|e| format!("{}: {e}", carried.display()))?;
    }
    Ok(())
}

/// Drops the packs of jars that are no longer data-only in this pack (gone from it, or read again
/// with classes), so new worlds stop getting them.
fn prune(work: &Path, keys: &[&str]) -> Result<(), String> {
    for e in fs::read_dir(folder(work)).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !keys.contains(&name.as_str()) {
            let path = e.path();
            fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// One jar into `out`. A pack installed before is replaced: worlds keep their own copies.
fn install_one(jar: &Path, out: &Path, upgrade_it: bool) -> Result<Vec<String>, String> {
    if out.exists() {
        fs::remove_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let written = if upgrade_it {
        upgrade::upgrade_into(jar, out).map(Some)
    } else {
        upgrade::copy_into(jar, out).map(|()| None)
    };
    // Half a pack must not reach new worlds.
    let report = match written {
        Ok(Some(report)) => report,
        Ok(None) => return Ok(Vec::new()),
        Err(e) => {
            let _ = fs::remove_dir_all(out);
            return Err(e);
        }
    };
    let lossy = report
        .lossy
        .iter()
        .map(|(what, n)| format!("{what} ({n}x)"));
    let unknown = report.unknown.keys().map(|t| format!("left as it is: {t}"));
    Ok(lossy.chain(unknown).collect())
}

/// Every data-only jar of `records` into [`folder`]. A jar that fails is reported and the others go
/// on. A content mod an older build carried from the same jar goes, or both would load. Returns
/// how many are installed.
pub fn install(
    work: &Path,
    records: &[Record],
    minecraft: &str,
    log: Log,
) -> Result<usize, String> {
    let mut report = BTreeMap::new();
    let keys: Vec<&str> = records
        .iter()
        .filter(|r| is_data_only(r, minecraft))
        .map(|r| r.key.as_str())
        .collect();
    prune(work, &keys)?;
    for r in records.iter().filter(|r| is_data_only(r, minecraft)) {
        let (Some(jar), Some(info)) = (&r.jar, &r.info) else {
            continue;
        };
        let mut installed = Installed {
            assets: info.census.textures + info.census.languages > 0,
            ..Installed::default()
        };
        let upgrade_it = upgrades(minecraft) == Some(true);
        let result = if plain_name(&r.key) {
            drop_carry(work, &r.key)
                .and_then(|()| install_one(jar, &folder(work).join(&r.key), upgrade_it))
        } else {
            Err(format!("{:?} is not a plain folder name", r.key))
        };
        match result {
            Ok(notes) => {
                log(format!(
                    "{}: data pack for new worlds ({} notes)",
                    r.key,
                    notes.len()
                ));
                installed.notes = notes;
            }
            Err(e) => {
                log(format!("{}: {e}", r.key));
                installed.error = e;
            }
        }
        report.insert(r.key.clone(), installed);
    }
    let done = report.values().filter(|i| i.error.is_empty()).count();
    pipeline::write_json(&work.join(REPORT), &report)?;
    Ok(done)
}

/// The packs in [`folder`], the ones new worlds in this profile start with.
pub fn installed(work: &Path) -> usize {
    fs::read_dir(folder(work)).map_or(0, |dir| {
        dir.flatten()
            .filter(|e| e.path().join("pack.mcmeta").is_file())
            .count()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jar::{Census, JarInfo};
    use std::io::Write;

    /// A jar holding `files`, named by the test so parallel tests stay apart.
    fn jar(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        let path = root.join(format!("{name}.jar"));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (file, text) in files {
            zip.start_file(*file, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn record(key: &str, jar: PathBuf) -> Record {
        let info = crate::jar::read(&jar).unwrap();
        Record {
            key: key.into(),
            class: "content".into(),
            tier: "data".into(),
            jar: Some(jar),
            info: Some(info),
            ..Record::default()
        }
    }

    const META: &str = r#"{"pack": {"description": "Void", "pack_format": 107}}"#;
    const NOISE: &str = r#"{"firstOctave": -7, "amplitudes": [1.0]}"#;

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("port-pack-datapacks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn only_a_jar_with_no_classes_and_a_data_pack_of_a_new_enough_pack_counts() {
        let info = |classes, pack_mcmeta, data_files| JarInfo {
            census: Census {
                classes,
                pack_mcmeta,
                data_files,
                ..Census::default()
            },
            ..JarInfo::default()
        };
        let r = |info| Record {
            class: "content".into(),
            tier: "data".into(),
            jar: Some(PathBuf::from("x.jar")),
            info: Some(info),
            ..Record::default()
        };
        assert!(is_data_only(&r(info(0, true, 3)), "26.2"));
        assert!(is_data_only(&r(info(0, true, 3)), "26.2.1"));
        assert!(is_data_only(&r(info(0, true, 3)), "26.3"));
        assert!(!is_data_only(&r(info(0, true, 3)), "1.21.1"));
        assert!(!is_data_only(&r(info(0, true, 3)), "26.21"));
        assert!(!is_data_only(&r(info(1, true, 3)), "26.2"));
        assert!(!is_data_only(&r(info(0, false, 3)), "26.2"));
        assert!(!is_data_only(&r(info(0, true, 0)), "26.2"));
    }

    #[test]
    fn a_26_2_jar_keeps_its_vanilla_overrides_in_26_3_form_and_replaces_its_old_carry() {
        let root = root("upgrade");
        let work = root.join("work");
        let carried = work.join("mods/void/carry");
        fs::create_dir_all(&carried).unwrap();
        let jar = jar(
            &root,
            "void",
            &[
                ("pack.mcmeta", META),
                ("fabric.mod.json", "{}"),
                ("data/minecraft/worldgen/noise/ice.json", NOISE),
                ("assets/void/lang/en_us.json", "{}"),
            ],
        );
        let records = [record("void", jar)];
        assert_eq!(install(&work, &records, "26.2", &|_| {}), Ok(1));
        let pack = folder(&work).join("void");
        let noise: serde_json::Value =
            pipeline::read_json(&pack.join("data/minecraft/worldgen/noise/ice.json")).unwrap();
        assert_eq!(noise["base_octave"], -7);
        let meta: serde_json::Value = pipeline::read_json(&pack.join("pack.mcmeta")).unwrap();
        assert_eq!(meta["pack"]["pack_format"], upgrade::PACK_FORMAT);
        assert!(!carried.exists());
        assert_eq!(installed(&work), 1);
        assert!(load(&work)["void"].assets);
        // Run again: the same result, not "already exists".
        assert_eq!(install(&work, &records, "26.2", &|_| {}), Ok(1));
        // A jar no longer in the pack stops coming into new worlds.
        assert_eq!(install(&work, &[], "26.2", &|_| {}), Ok(0));
        assert!(!pack.exists());
        assert_eq!(installed(&work), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_26_3_jar_is_already_in_the_game_s_form() {
        let root = root("as-is");
        let work = root.join("work");
        let files = [
            ("pack.mcmeta", META),
            ("data/m/worldgen/noise/ice.json", NOISE),
        ];
        let records = [record("void", jar(&root, "void", &files))];
        assert_eq!(install(&work, &records, "26.3", &|_| {}), Ok(1));
        let pack = folder(&work).join("void");
        assert_eq!(
            fs::read_to_string(pack.join("data/m/worldgen/noise/ice.json")).unwrap(),
            NOISE
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_jar_that_cannot_be_read_is_reported_and_the_others_still_come_over() {
        let root = root("bad");
        let work = root.join("work");
        let files = [("pack.mcmeta", META), ("data/m/function/a.mcfunction", "")];
        let good = record("good", jar(&root, "good", &files));
        let mut bad = record("bad", jar(&root, "bad", &files));
        bad.jar = Some(root.join("gone.jar"));
        // A mod id that is a path must not reach outside the work folder.
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let escape = record("../../outside", jar(&root, "escape", &files));
        assert_eq!(install(&work, &[bad, good, escape], "26.2", &|_| {}), Ok(1));
        assert!(outside.is_dir());
        let report = load(&work);
        assert!(!report["../../outside"].error.is_empty());
        assert!(!report["bad"].error.is_empty());
        assert!(report["good"].error.is_empty());
        assert_eq!(installed(&work), 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
