//! Automatic updates of what the browser installed from Horizon's registry (Settings: "Update
//! registry mods automatically", on by default): at the launcher's start and before Play, each
//! installed registry project moves to its newest version for this game ([`Index::installable`]),
//! checked as an install is. What it never touches:
//!
//! - a game folder inside a modpack's profile: the pack pins its mods by SHA-256 and updates as a
//!   whole, when the player asks;
//! - a native mod (`.hnmod`) when the instance's build is pinned to a Nightly, or before Play
//!   (only the start's run updates them: the launcher rebuilds the game with them before it
//!   plays, [`Report::rebuild`]);
//! - a data pack: it lives in a world, not in the browser's list;
//! - a version for another Minecraft than the one installed, a newer version that needs a newer
//!   Horizon, and anything older than what is installed.
//!
//! A content mod (`.hmod`) whose new version drops a block, item, entity, effect, menu or
//! creative tab the installed one has waits for the player (its blocks would vanish from their
//! worlds): the notice says which. The file an update replaces is kept in
//! `mod-browser/replaced/` until the next start: an update cut short (a crash between moving the
//! file and saving the list) is rolled back from it then.
use super::{install_version_guarded, needs_port, Index};
use crate::browser::{installed, Kind, SourceId, GAME};
use crate::i18n::trf;
use horizon_mods::index;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Where replaced files wait for the next start, and their list.
const KEPT_DIR: &str = "mod-browser/replaced";
const KEPT: &str = "mod-browser/replaced.json";

/// What happens to one installed project.
#[derive(Debug, PartialEq)]
pub enum Decision<'a> {
    Update(&'a index::Version),
    /// Not updated automatically, or nothing newer.
    Skip,
}

/// The game folder's place: inside a modpack's profile, the mod API level of the game, and
/// whether its native mods may update (no Nightly pin, and a build can follow).
pub struct Context {
    pub in_pack: bool,
    pub level: u32,
    pub natives: bool,
}

/// What an automatic update did: `Title version` updated, what waits for the player and why,
/// and what failed.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub updated: Vec<String>,
    pub held: Vec<String>,
    pub failed: Vec<String>,
    /// A native mod was updated: the game is built again before it plays.
    pub rebuild: bool,
}

impl Report {
    /// The lines the launcher shows: nothing when nothing happened.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.updated.is_empty() {
            out.push(trf(
                "Updated automatically: {list}.",
                &[("list", &self.updated.join(", "))],
            ));
        }
        out.extend(self.held.iter().cloned());
        out.extend(self.failed.iter().cloned());
        out
    }
}

/// Whether game folder `home` is a registry modpack's profile (`pack::home`).
pub fn in_pack(home: &Path) -> bool {
    home.ends_with("home") && home.parent().is_some_and(super::pack::is_pack)
}

/// Whether `entry` updates automatically, and to what.
pub fn decide<'a>(index: &'a Index, entry: &installed::Entry, ctx: &Context) -> Decision<'a> {
    let native = crate::native_status::is_native_file(&entry.file);
    let skip_native = native && !ctx.natives;
    if entry.source != SourceId::Horizon
        || ctx.in_pack
        || skip_native
        || entry.kind == Kind::DataPack
    {
        return Decision::Skip;
    }
    let Some(p) = index.project(&entry.project) else {
        return Decision::Skip;
    };
    let Some(target) = index.installable(p, GAME, ctx.level) else {
        return Decision::Skip;
    };
    // Versions are newest first: only forward, from a version the index still lists.
    let at = |version: &str| p.versions.iter().position(|v| v.version == version);
    let (Some(now), Some(next)) = (at(&entry.version), at(&target.version)) else {
        return Decision::Skip;
    };
    let same_game = p.versions[now].game == target.game;
    let native_next = p.kind == index::Kind::NativeMod;
    if next >= now || !same_game || needs_port(target) || (native_next && !ctx.natives) {
        return Decision::Skip;
    }
    Decision::Update(target)
}

/// The blocks, items, entities, effects, menus and creative tabs a content mod adds.
pub fn content_ids(path: &Path) -> Result<BTreeSet<String>, String> {
    let manifest = horizon_mods::Package::read(path)?.manifest;
    let c = horizon_mods::Content::read(path, &manifest)?;
    let ids = c.blocks.iter().map(|b| format!("block {}", b.id));
    let ids = ids.chain(c.items.iter().map(|i| format!("item {}", i.id)));
    let ids = ids.chain(c.entity_types.iter().map(|e| format!("entity {}", e.id)));
    let ids = ids.chain(c.effects.iter().map(|e| format!("effect {}", e.id)));
    let ids = ids.chain(c.menu_types.iter().map(|m| format!("menu {}", m.id)));
    Ok(ids
        .chain(
            c.creative_tabs
                .iter()
                .map(|t| format!("creative tab {}", t.id)),
        )
        .collect())
}

/// What the content mod at `old` has that the one at `new` lacks.
pub fn dropped(old: &Path, new: &Path) -> Result<Vec<String>, String> {
    let new = content_ids(new)?;
    Ok(content_ids(old)?.difference(&new).cloned().collect())
}

/// A replaced file kept until the next start, with the entry it had.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Kept {
    entry: installed::Entry,
    /// Its copy, relative to the game folder.
    copy: String,
    /// The launcher process that replaced it: its own updates aren't cleared before it ends.
    pid: u32,
}

fn kept_list(home: &Path) -> Vec<Kept> {
    fs::read_to_string(home.join(KEPT))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_kept(home: &Path, list: &[Kept]) -> Result<(), String> {
    let path = home.join(KEPT);
    if list.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the browser's list has `kept`'s project at a newer version whose file is in place.
fn finished(home: &Path, kept: &Kept) -> bool {
    let list = installed::Installed::load(home);
    let Some(now) = list.find(SourceId::Horizon, &kept.entry.project) else {
        return false;
    };
    let hash = now.checksum.strip_prefix("SHA-256:").unwrap_or_default();
    now.version != kept.entry.version
        && now
            .path(home)
            .and_then(|p| index::sha256_file(&p).ok())
            .is_some_and(|h| h.eq_ignore_ascii_case(hash))
}

/// Puts `kept` back: its file where it was and its entry in the browser's list.
fn restore(home: &Path, kept: &Kept) -> Result<(), String> {
    let (Some(target), copy) = (kept.entry.path(home), home.join(&kept.copy)) else {
        return Err(format!("{} is outside the game folder", kept.entry.file));
    };
    fs::copy(&copy, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut list = installed::Installed::load(home);
    let ours =
        |e: &installed::Entry| e.source == SourceId::Horizon && e.project == kept.entry.project;
    if let Some(now) = list.entries.iter().find(|e| ours(e)) {
        if now.file != kept.entry.file {
            if let Some(path) = now.path(home) {
                let _ = fs::remove_file(path);
            }
        }
    }
    list.entries.retain(|e| !ours(e));
    list.entries.push(kept.entry.clone());
    list.save(home)
}

/// At start: each file an earlier launcher replaced is dropped when its update finished, put back
/// when it didn't.
pub fn recover(home: &Path) {
    let me = std::process::id();
    let (mine, earlier): (Vec<Kept>, Vec<Kept>) =
        kept_list(home).into_iter().partition(|k| k.pid == me);
    for k in &earlier {
        if !finished(home, k) {
            if let Err(e) = restore(home, k) {
                eprintln!("Couldn't put {} back: {e}", k.entry.title);
                continue;
            }
        }
        let _ = fs::remove_file(home.join(&k.copy));
    }
    let _ = save_kept(home, &mine);
}

/// Keeps the file `entry` installed before it is replaced.
fn keep(home: &Path, entry: &installed::Entry) -> Result<Kept, String> {
    let from = entry
        .path(home)
        .ok_or_else(|| format!("{} is outside the game folder", entry.file))?;
    let name = from.file_name().unwrap_or_default().to_string_lossy();
    let copy = format!("{KEPT_DIR}/{}-{name}", entry.version);
    let to = home.join(&copy);
    fs::create_dir_all(home.join(KEPT_DIR)).map_err(|e| e.to_string())?;
    fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    let kept = Kept {
        entry: entry.clone(),
        copy,
        pid: std::process::id(),
    };
    let mut list = kept_list(home);
    list.retain(|k| k.entry.project != entry.project);
    list.push(kept.clone());
    save_kept(home, &list)?;
    Ok(kept)
}

/// Updates every installed registry project of game folder `home` that [`decide`] says, by the
/// index (fetched at most every ten minutes); None when it can't be read.
pub fn run(home: &Path, ctx: &Context, progress: &dyn Fn(&str, f32)) -> Option<Report> {
    // The start's run and Play's never touch a folder at once.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let index = super::index().ok()?;
    Some(run_with(&index, super::local(), home, ctx, progress))
}

pub(super) fn run_with(
    index: &Index,
    local: bool,
    home: &Path,
    ctx: &Context,
    progress: &dyn Fn(&str, f32),
) -> Report {
    recover(home);
    let mut report = Report::default();
    for entry in installed::Installed::load(home).entries {
        let Decision::Update(v) = decide(index, &entry, ctx) else {
            continue;
        };
        let p = index.project(&entry.project).expect("decide found it");
        match update(index, local, p, v, &entry, home, progress) {
            Ok(None) => {
                report.rebuild |= p.kind == index::Kind::NativeMod;
                report.updated.push(format!("{} {}", p.name, v.version));
            }
            Ok(Some(why)) => report.held.push(why),
            Err(e) => report.failed.push(trf(
                "{title} wasn't updated: {error}",
                &[("title", &p.name), ("error", &e)],
            )),
        }
    }
    report
}

/// Updates `entry` to `v`; Some(why) when it waits for the player instead.
fn update(
    index: &Index,
    local: bool,
    p: &index::Project,
    v: &index::Version,
    entry: &installed::Entry,
    home: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Option<String>, String> {
    let old: Option<PathBuf> = entry.path(home).filter(|p| p.is_file());
    let held = RefCell::new(None);
    // Content mods: nothing the worlds may hold goes missing.
    let guard = |new: &Path| -> Result<(), String> {
        let content = entry.file.ends_with(".hmod") && v.file.name.ends_with(".hmod");
        let (Some(old), true) = (&old, content) else {
            return Ok(());
        };
        let gone = dropped(old, new)?;
        if gone.is_empty() {
            return Ok(());
        }
        let why = trf(
            "{title} {version} waits for you: it no longer has {ids}, which your worlds may use. Update it on the Mods page when you're ready.",
            &[
                ("title", &p.name),
                ("version", &v.version),
                ("ids", &gone.join(", ")),
            ],
        );
        *held.borrow_mut() = Some(why.clone());
        Err(why)
    };
    let kept = match &old {
        Some(_) => Some(keep(home, entry)?),
        None => None,
    };
    match install_version_guarded(index, local, p, v, home, &guard, progress) {
        Ok(_) => Ok(None),
        Err(e) => {
            if let Some(kept) = &kept {
                // Nothing moved when the check refused it; else the old file goes back now.
                if !finished(home, kept) {
                    let _ = restore(home, kept);
                }
                let _ = fs::remove_file(home.join(&kept.copy));
                let mut list = kept_list(home);
                list.retain(|k| k.entry.project != kept.entry.project);
                let _ = save_kept(home, &list);
            }
            match held.into_inner() {
                Some(why) => Ok(Some(why)),
                None => Err(e),
            }
        }
    }
}

#[cfg(test)]
#[path = "auto/tests.rs"]
mod tests;
