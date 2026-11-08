//! A mod that alone stops a build (it hooks code the build's Horizon doesn't have, or declares
//! the same support module as another mod) is left out, with the mods that need it, and the
//! build runs again without them: the player still gets a game with the others, and is told
//! which mods were left out and why.
use super::dependencies::Dependency;
use super::Bundle;
use crate::i18n::trf;
use std::collections::BTreeSet;
use std::fs;

/// Why a mod was left out of a build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// It doesn't apply to, or compile on, the Horizon the build uses.
    OtherVersion,
    /// It declares the same support module, in the same file, as this mod.
    Clash(String),
    /// It needs this mod, which was left out.
    Needs(String),
}

/// A mod left out of a build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeftOut {
    pub id: String,
    pub version: String,
    pub why: Why,
}

impl LeftOut {
    pub(super) fn new(bundle: &Bundle, why: Why) -> Self {
        Self {
            id: bundle.id.clone(),
            version: bundle.version.clone(),
            why,
        }
    }

    /// What the player reads.
    pub fn label(&self) -> String {
        match &self.why {
            Why::OtherVersion => trf(
                "{name} was left out: made for another Horizon version",
                &[("name", &self.id)],
            ),
            Why::Clash(other) => trf(
                "{name} was left out: clashes with {other}",
                &[("name", &self.id), ("other", other)],
            ),
            Why::Needs(other) => trf(
                "{name} was left out: it needs {other}, which was left out",
                &[("name", &self.id), ("other", other)],
            ),
        }
    }

    /// How a build's `native-mods.json` keeps it (`left_out`).
    pub(super) fn to_json(&self) -> serde_json::Value {
        let (why, with) = match &self.why {
            Why::OtherVersion => ("other-version", None),
            Why::Clash(other) => ("clash", Some(other)),
            Why::Needs(other) => ("needs", Some(other)),
        };
        serde_json::json!({"id": self.id, "version": self.version, "why": why, "with": with})
    }

    pub(super) fn from_json(value: &serde_json::Value) -> Option<Self> {
        let with = || value["with"].as_str().map(str::to_owned);
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            version: value["version"].as_str()?.to_owned(),
            why: match value["why"].as_str()? {
                "clash" => Why::Clash(with()?),
                "needs" => Why::Needs(with()?),
                _ => Why::OtherVersion,
            },
        })
    }
}

/// The support modules a bundle declares, as (destination file, module name).
fn modules(bundle: &Bundle) -> Vec<(String, String)> {
    let text = fs::read_to_string(bundle.dir.join(super::MANIFEST)).unwrap_or_default();
    let table: toml::Table = text.parse().unwrap_or_default();
    let entries = table.get("module").and_then(|m| m.as_array());
    entries
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                m.get("file")?.as_str()?.into(),
                m.get("name")?.as_str()?.into(),
            ))
        })
        .collect()
}

/// The mod of `mods` (in build order) a horizon-mixins diagnostic at the end of its `output`
/// (`{"code", "message", "mod_id", "file"}`) blames, and why; None when it names none of them,
/// so the error is the build's. A support module's diagnostic names no mod (horizon-mixins
/// e8cd0aa) but the module (`support module 'x' collides …`) or its destination (`file`, which
/// this Horizon may not have); between two mods, neither: the later one is left out.
pub(super) fn from_mixins(output: &str, mods: &[Bundle]) -> Option<LeftOut> {
    let value: serde_json::Value = serde_json::from_str(&output[output.find('{')?..]).ok()?;
    let message = value["message"].as_str().unwrap_or_default();
    let declared: Vec<(&Bundle, Vec<(String, String)>)> =
        mods.iter().map(|b| (b, modules(b))).collect();
    // The first mod declaring the same module as `bundle`, other than itself.
    let clash = |bundle: &Bundle| {
        let own = modules(bundle);
        declared
            .iter()
            .find(|(other, theirs)| other.id != bundle.id && theirs.iter().any(|m| own.contains(m)))
            .map(|(other, _)| other.id.clone())
    };
    let named = value["mod_id"]
        .as_str()
        .or_else(|| value["file"].as_str().and_then(super::mod_id_from_file))
        .and_then(|id| mods.iter().find(|b| b.id == id))
        .or_else(|| {
            let name = message
                .split("support module '")
                .nth(1)?
                .split('\'')
                .next()?;
            declared
                .iter()
                .find(|(_, theirs)| theirs.iter().any(|(_, n)| n == name))
                .map(|(b, _)| *b)
        })
        .or_else(|| {
            let file = value["file"].as_str()?;
            let mut declaring = declared
                .iter()
                .filter(|(_, theirs)| theirs.iter().any(|(f, _)| f == file));
            match (declaring.next(), declaring.next()) {
                (Some((bundle, _)), None) => Some(*bundle),
                _ => None,
            }
        });
    if let Some(bundle) = named {
        let why = match value["code"].as_str() {
            Some("E_MODULE_CONFLICT") => clash(bundle).map_or(Why::OtherVersion, Why::Clash),
            _ => Why::OtherVersion,
        };
        return Some(LeftOut::new(bundle, why));
    }
    if value["code"].as_str() != Some("E_MODULE_CONFLICT") {
        return None;
    }
    // Two mods' modules: the later of the first pair declaring the same one.
    let later = declared.iter().enumerate().find_map(|(i, (bundle, _))| {
        clash(bundle)
            .filter(|first| declared[..i].iter().any(|(b, _)| b.id == *first))
            .map(|first| LeftOut::new(bundle, Why::Clash(first)))
    });
    later
}

/// The mods the compiler's errors were traced to (`owners`, as `id version`), among `mods`.
pub(super) fn from_compiler(owners: &[String], mods: &[Bundle]) -> Vec<LeftOut> {
    mods.iter()
        .filter(|b| owners.iter().any(|o| o.split(' ').next() == Some(&b.id)))
        .map(|b| LeftOut::new(b, Why::OtherVersion))
        .collect()
}

/// `left` and, after them, every mod of `mods` that needs one of them (not optionally),
/// transitively, each once.
pub(super) fn with_dependents(left: &[LeftOut], mods: &[(&Bundle, &[Dependency])]) -> Vec<LeftOut> {
    let mut out = left.to_vec();
    let mut gone: BTreeSet<String> = left.iter().map(|l| l.id.clone()).collect();
    loop {
        let next = mods.iter().find_map(|(bundle, deps)| {
            let needed = deps.iter().find(|d| !d.optional && gone.contains(&d.id))?;
            (!gone.contains(&bundle.id))
                .then(|| LeftOut::new(bundle, Why::Needs(needed.id.clone())))
        });
        let Some(next) = next else {
            return out;
        };
        gone.insert(next.id.clone());
        out.push(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn bundle(root: &Path, id: &str, modules: &[(&str, &str)]) -> Bundle {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut text = format!("[mod]\nid = \"{id}\"\nversion = \"1.0.0\"\n");
        for (file, name) in modules {
            text += &format!(
                "[[module]]\nid = \"m\"\nfile = \"{file}\"\nname = \"{name}\"\nsource = \"m.rs\"\n"
            );
        }
        fs::write(dir.join(super::super::MANIFEST), text).unwrap();
        Bundle {
            id: id.into(),
            version: "1.0.0".into(),
            dir,
            horizon: None,
            mixins: None,
            hooks: None,
            api: None,
            mixins_content: true,
            injections: Vec::new(),
        }
    }

    #[test]
    fn a_target_this_horizon_lacks_leaves_its_mod_out() {
        let root = super::super::tests::dir("isolate-target");
        let mods = [
            bundle(&root, "zoom", &[]),
            bundle(&root, "eatinganimation", &[]),
        ];
        // horizon-mixins e8cd0aa on Horizon 6475f62, a player's build of 2026-11-05.
        let output = r#"error: plan failed
{
  "code": "E_TARGET_NOT_FOUND",
  "message": "no source function 'horizon_client::item_render::ItemRenderer::resolve'",
  "mod_id": "eatinganimation",
  "injection_id": "held-frame"
}"#;
        let left = from_mixins(output, &mods).unwrap();
        assert_eq!(
            (left.id.as_str(), &left.why),
            ("eatinganimation", &Why::OtherVersion)
        );
        assert_eq!(
            left.label(),
            "eatinganimation was left out: made for another Horizon version"
        );
        // A diagnostic naming no mod of the build is the build's own error.
        assert_eq!(
            from_mixins(r#"{"code": "E_METADATA", "message": "cargo"}"#, &mods),
            None
        );
        assert_eq!(from_mixins("no diagnostic", &mods), None);
    }

    #[test]
    fn a_module_clash_names_its_mod_by_the_module() {
        let root = super::super::tests::dir("isolate-module");
        let lib = "crates/horizon-client/src/lib.rs";
        let mods = [
            bundle(&root, "gyro", &[(lib, "gyro")]),
            bundle(&root, "hitbox_plus", &[(lib, "official_hitbox_plus")]),
        ];
        // horizon-mixins e8cd0aa: no mod_id on a module's diagnostic.
        let output = r#"{"code": "E_MODULE_CONFLICT", "message": "support module 'official_hitbox_plus' collides with an existing type/module/import or unresolved glob import"}"#;
        let left = from_mixins(output, &mods).unwrap();
        assert_eq!(
            (left.id.as_str(), left.why),
            ("hitbox_plus", Why::OtherVersion)
        );
        // Or by its destination, a file this Horizon doesn't have (worldedit 1.2.1 on 6475f62).
        let chat = "crates/horizon-client/src/chat.rs";
        let mods = [
            bundle(&root, "zoom", &[]),
            bundle(&root, "worldedit", &[(chat, "worldedit_chat")]),
        ];
        let output = r#"{"code": "E_MODULE", "message": "module destination must be an indexed Rust source file", "file": "crates/horizon-client/src/chat.rs"}"#;
        assert_eq!(from_mixins(output, &mods).unwrap().id, "worldedit");
    }

    #[test]
    fn two_mods_declaring_one_module_leave_the_later_out() {
        let root = super::super::tests::dir("isolate-clash");
        let game = "crates/horizon-client/src/game.rs";
        let mods = [
            bundle(&root, "a_zoom", &[(game, "zoom")]),
            bundle(&root, "fps", &[(game, "fps")]),
            bundle(&root, "b_zoom", &[(game, "zoom")]),
        ];
        let output = r#"{"code": "E_MODULE_CONFLICT", "message": "competing support modules", "file": "crates/horizon-client/src/game.rs"}"#;
        let left = from_mixins(output, &mods).unwrap();
        assert_eq!(left.id, "b_zoom");
        assert_eq!(left.label(), "b_zoom was left out: clashes with a_zoom");
        // Named, it says which mod it clashes with too.
        let output = r#"{"code": "E_MODULE_CONFLICT", "message": "competing support modules", "mod_id": "a_zoom"}"#;
        assert_eq!(
            from_mixins(output, &mods).unwrap().why,
            Why::Clash("b_zoom".into())
        );
    }

    #[test]
    fn the_mods_needing_a_left_out_mod_go_too_but_not_optional_ones() {
        let root = super::super::tests::dir("isolate-deps");
        let [lib, addon, extra, maybe, other] =
            ["lib", "addon", "extra", "maybe", "other"].map(|id| bundle(&root, id, &[]));
        let dep = |id: &str, optional| Dependency {
            id: id.into(),
            requirement: semver::VersionReq::STAR,
            optional,
        };
        let (on_lib, on_addon, maybe_lib) = (
            [dep("lib", false)],
            [dep("addon", false)],
            [dep("lib", true)],
        );
        let mods: Vec<(&Bundle, &[Dependency])> = vec![
            (&extra, &on_addon),
            (&lib, &[]),
            (&addon, &on_lib),
            (&maybe, &maybe_lib),
            (&other, &[]),
        ];
        let left = with_dependents(&[LeftOut::new(&lib, Why::OtherVersion)], &mods);
        let ids: Vec<&str> = left.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["lib", "addon", "extra"]);
        assert_eq!(
            left[2].label(),
            "extra was left out: it needs addon, which was left out"
        );
        // What a build's record keeps reads back the same.
        for l in &left {
            assert_eq!(LeftOut::from_json(&l.to_json()).as_ref(), Some(l));
        }
    }
}
