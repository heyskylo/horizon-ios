//! Whether a native mod fits the Horizon the player plays, said before it is installed (#9860),
//! so a mod made for another Horizon is no surprise at build time. A mod naming a hook points'
//! version fits any Horizon with that `NATIVE_HOOKS_API` or a later one; a pinned mod fits the
//! commit it pins (docs/native-mods.md, "Pinned to a Nightly").
use crate::i18n::{tr, trf};
use crate::instances::Instance;
use crate::{backend, native_mods};
use horizon_mods::index::{self, BrokenOn};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The Horizon the selected instance plays: the commit its game was compiled from and that
/// commit's hook points' version (None when it couldn't be read).
#[derive(Clone, Debug, PartialEq)]
pub struct Plays {
    pub revision: String,
    pub hooks: Option<u32>,
}

/// How a native mod fits [`Plays`].
#[derive(Clone, Debug, PartialEq)]
pub enum Fit {
    /// It builds on the Horizon the player plays.
    Fits,
    /// It needs hook points the player's Horizon doesn't have yet.
    NeedsNewer { needs: u32, has: u32 },
    /// It pins another commit, and mods turned on pin yet another one: the build asks for
    /// Build anyway, and the mods made for the other commit may not compile.
    Clashes { pin: String, with: Vec<String> },
    /// It pins another commit, alone: the build compiles the game on that commit.
    MovesTo { pin: String },
    /// The registry found it doesn't apply on the Horizon the player plays (horizon-mixins'
    /// `code`): a build leaves it out.
    Broken { code: String },
}

/// How mod `id` fits: it names hook points' version `hooks`, else pins commit `pin`. `pins` are
/// the mods turned on that pin a commit (id, commit); the mod's own older version among them is
/// replaced by this one. `broken_on` are the commits the registry found it doesn't apply on: one
/// it would build on, the Horizon played, makes it [`Fit::Broken`]. None when the hook points of
/// the Horizon played aren't known.
pub fn fit(
    id: &str,
    hooks: Option<u32>,
    pin: &str,
    plays: &Plays,
    pins: &[(String, String)],
    broken_on: &[BrokenOn],
) -> Option<Fit> {
    let builds_on_yours = hooks.is_some() || pin == plays.revision;
    if let Some(broken) = broken_on
        .iter()
        .find(|b| builds_on_yours && b.horizon == plays.revision)
    {
        return Some(Fit::Broken {
            code: broken.code.clone(),
        });
    }
    if let Some(needs) = hooks {
        let has = plays.hooks?;
        return Some(match needs <= has {
            true => Fit::Fits,
            false => Fit::NeedsNewer { needs, has },
        });
    }
    if pin == plays.revision {
        return Some(Fit::Fits);
    }
    let with: Vec<String> = pins
        .iter()
        .filter(|(other, revision)| other != id && revision != pin)
        .map(|(other, _)| other.clone())
        .collect();
    Some(match with.is_empty() {
        true => Fit::MovesTo { pin: pin.into() },
        false => Fit::Clashes {
            pin: pin.into(),
            with,
        },
    })
}

fn short(revision: &str) -> &str {
    &revision[..revision.len().min(7)]
}

/// What the player reads about `fit`.
pub fn describe(fit: &Fit, plays: &Plays) -> String {
    match fit {
        Fit::Fits => tr("Builds on the Horizon you play").to_owned(),
        Fit::NeedsNewer { needs, has } => trf(
            "Needs native hook points v{needs}, the Horizon you play has v{has}: update Horizon first",
            &[("needs", &needs.to_string()), ("has", &has.to_string())],
        ),
        Fit::Clashes { pin, with } => trf(
            "Made for Horizon {pin}, but {mods} pin another commit: the build will ask you to Build anyway and may not compile",
            &[("pin", short(pin)), ("mods", &with.join(", "))],
        ),
        Fit::MovesTo { pin } => trf(
            "Made for Horizon {pin}, you play {yours}: the launcher builds your game on {pin}",
            &[("pin", short(pin)), ("yours", short(&plays.revision))],
        ),
        Fit::Broken { code } => trf(
            "Doesn't build on the Horizon you play ({code}): a build leaves it out until its author updates it",
            &[("code", code)],
        ),
    }
}

/// [`Plays`] by the version an instance plays (kept for good once its hook points are known, a
/// failure for [`RETRY`]), and hook points' versions by commit: reading them asks GitHub, and a
/// search asks for every native mod it finds.
type CacheKey = (String, Option<String>);
type Known = HashMap<CacheKey, (Instant, Option<Plays>)>;
static PLAYS: Mutex<Option<Known>> = Mutex::new(None);
static HOOKS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);
const RETRY: Duration = Duration::from_secs(60);

fn cached(cache: &Known, key: &CacheKey) -> Option<Option<Plays>> {
    cache
        .get(key)
        .filter(|(at, plays)| {
            plays.as_ref().is_some_and(|p| p.hooks.is_some()) || at.elapsed() < RETRY
        })
        .map(|(_, plays)| plays.clone())
}

/// The Horizon `instance` plays; None for a Java instance, when nothing is installed, or when
/// GitHub can't say its commit.
pub fn plays(instance: &Instance) -> Option<Plays> {
    if instance.java.is_some() {
        return None;
    }
    let version = instance.resolve_version(None).ok()?;
    // Native builds can be rebuilt under the same version name on a different commit.
    // Read the build record before consulting the cache so the cached answer follows it.
    let native_revision = native_mods::about(&version).map(|about| about.horizon);
    let key = (version.clone(), native_revision.clone());
    let known = cached(PLAYS.lock().unwrap().get_or_insert_default(), &key);
    if let Some(plays) = known {
        return plays;
    }
    // A native build is the commit it compiled; anything else, the commit of the release or
    // Nightly a build would start from.
    let revision = match native_revision {
        Some(revision) => Some(revision),
        None => native_mods::base_for(Some(&version), &backend::installed(), false)
            .and_then(|base| native_mods::revision_of(&base).ok()),
    };
    let plays = revision.map(|revision| Plays {
        hooks: hooks_at(&revision),
        revision,
    });
    PLAYS
        .lock()
        .unwrap()
        .get_or_insert_default()
        .insert(key, (Instant::now(), plays.clone()));
    plays
}

fn hooks_at(revision: &str) -> Option<u32> {
    if let Some(&hooks) = HOOKS.lock().unwrap().get_or_insert_default().get(revision) {
        return Some(hooks);
    }
    let hooks = native_mods::hooks_at(revision)?;
    HOOKS
        .lock()
        .unwrap()
        .get_or_insert_default()
        .insert(revision.into(), hooks);
    Some(hooks)
}

/// The commit a native mod's version pins (`native-1/<horizon>/<mixins>`), empty when it names none.
pub fn pin_of(v: &index::Version) -> &str {
    v.api.split('/').nth(1).unwrap_or_default()
}

/// The version of native mod `p` to use on the Horizon played: the newest of the `usable` ones
/// that builds on it, else the newest that moves the build to its own commit without clashing
/// with `pins`, else none. An older version made for the Horizon played beats a newer one that
/// would move the game to another commit.
pub fn best<'a>(
    p: &'a index::Project,
    plays: &Plays,
    pins: &[(String, String)],
    usable: impl Fn(&index::Version) -> bool,
) -> Option<&'a index::Version> {
    let fits = |v: &index::Version| fit(&p.id, v.hooks, pin_of(v), plays, pins, &v.broken_on);
    let usable: Vec<&index::Version> = p.versions.iter().filter(|v| usable(v)).collect();
    usable
        .iter()
        .find(|v| fits(v) == Some(Fit::Fits))
        .or_else(|| {
            usable
                .iter()
                .find(|v| matches!(fits(v), Some(Fit::MovesTo { .. })))
        })
        .copied()
}

/// The native mods turned on in game folder `home` that pin a commit, as (id, commit).
pub fn pins(home: &Path) -> Vec<(String, String)> {
    crate::native_status::bundles_on(home)
        .into_iter()
        .filter(|b| b.manifest.hooks.is_none())
        .map(|b| (b.manifest.id, b.manifest.horizon))
        .collect()
}

/// The line the registry browser shows for a native mod's version naming `hooks`, or pinning
/// `pin`, in `instance`; None when it can't be told.
pub fn line(
    instance: &Instance,
    id: &str,
    hooks: Option<u32>,
    pin: &str,
    broken_on: &[BrokenOn],
) -> Option<String> {
    let plays = plays(instance)?;
    fit(id, hooks, pin, &plays, &pins(&instance.home), broken_on).map(|fit| describe(&fit, &plays))
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOURS: &str = "1111111111111111111111111111111111111111";
    const OTHER: &str = "2222222222222222222222222222222222222222";
    const THIRD: &str = "3333333333333333333333333333333333333333";

    fn plays(hooks: Option<u32>) -> Plays {
        Plays {
            revision: YOURS.into(),
            hooks,
        }
    }

    fn pin(id: &str, revision: &str) -> (String, String) {
        (id.into(), revision.into())
    }

    #[test]
    fn rebuilding_a_native_version_does_not_reuse_its_previous_compatibility() {
        let version = "native-example".to_string();
        let old_key = (version.clone(), Some(YOURS.to_string()));
        let rebuilt_key = (version.clone(), Some(OTHER.to_string()));
        let unbuilt_key = (version, None);
        let old = plays(Some(28));
        let mut cache = Known::new();
        cache.insert(old_key.clone(), (Instant::now(), Some(old.clone())));
        assert_eq!(cached(&cache, &old_key), Some(Some(old)));
        assert_eq!(cached(&cache, &rebuilt_key), None);
        assert_eq!(cached(&cache, &unbuilt_key), None);
    }

    #[test]
    fn a_mod_the_registry_found_broken_on_the_horizon_played_says_so() {
        let broken = [BrokenOn {
            horizon: YOURS.into(),
            code: "E_TARGET_NOT_FOUND".into(),
        }];
        let at_28 = plays(Some(28));
        let found = fit("eatinganimation", Some(1), OTHER, &at_28, &[], &broken);
        assert_eq!(
            found,
            Some(Fit::Broken {
                code: "E_TARGET_NOT_FOUND".into()
            })
        );
        assert_eq!(
            describe(&found.unwrap(), &at_28),
            "Doesn't build on the Horizon you play (E_TARGET_NOT_FOUND): a build leaves it out until its author updates it"
        );
        // Broken on another commit than the one played: as before.
        let elsewhere = [BrokenOn {
            horizon: THIRD.into(),
            code: "E_TARGET_NOT_FOUND".into(),
        }];
        assert_eq!(
            fit("eatinganimation", Some(1), OTHER, &at_28, &[], &elsewhere),
            Some(Fit::Fits)
        );
        // A mod pinning another commit builds there, not on the Horizon played.
        assert_eq!(
            fit("jei", None, OTHER, &at_28, &[], &broken),
            Some(Fit::MovesTo { pin: OTHER.into() })
        );
    }

    #[test]
    fn a_hook_points_mod_fits_that_version_or_a_later_one() {
        let at_28 = plays(Some(28));
        assert_eq!(
            fit("zoom", Some(28), OTHER, &at_28, &[], &[]),
            Some(Fit::Fits)
        );
        assert_eq!(
            fit("zoom", Some(3), OTHER, &at_28, &[], &[]),
            Some(Fit::Fits)
        );
        assert_eq!(
            fit("zoom", Some(30), YOURS, &at_28, &[], &[]),
            Some(Fit::NeedsNewer { needs: 30, has: 28 })
        );
        // Unknown hook points: nothing is said rather than a guess.
        assert_eq!(fit("zoom", Some(1), OTHER, &plays(None), &[], &[]), None);
    }

    #[test]
    fn a_hook_points_mod_ignores_the_others_pins() {
        let pins = [pin("veinminer", THIRD)];
        assert_eq!(
            fit("zoom", Some(5), OTHER, &plays(Some(9)), &pins, &[]),
            Some(Fit::Fits)
        );
    }

    #[test]
    fn a_pinned_mod_fits_only_its_commit() {
        let p = plays(None);
        assert_eq!(fit("jei", None, YOURS, &p, &[], &[]), Some(Fit::Fits));
        assert_eq!(
            fit("jei", None, OTHER, &p, &[], &[]),
            Some(Fit::MovesTo { pin: OTHER.into() })
        );
    }

    fn version(number: &str, hooks: Option<u32>, pin: &str, broken: &[&str]) -> index::Version {
        serde_json::from_value(serde_json::json!({
            "version": number,
            "api": format!("native-1/{pin}/{THIRD}"),
            "hooks": hooks,
            "game": "26.3",
            "broken_on": broken.iter().map(|h| serde_json::json!({"horizon": h, "code": "E0061"})).collect::<Vec<_>>(),
            "file": {"name": "m.hnmod", "url": "https://example.invalid/m.hnmod", "size": 1, "sha256": "00"},
        }))
        .unwrap()
    }

    fn project(versions: Vec<index::Version>) -> index::Project {
        let mut p: index::Project = serde_json::from_value(serde_json::json!({
            "id": "flair", "kind": "native-mod", "name": "Flair", "summary": "", "description": "",
            "authors": [], "owners": [], "license": "MIT", "source": "https://example.invalid",
            "versions": [],
        }))
        .unwrap();
        p.versions = versions;
        p
    }

    fn picked(p: &index::Project, plays: &Plays, pins: &[(String, String)]) -> Option<String> {
        best(p, plays, pins, |_| true).map(|v| v.version.clone())
    }

    #[test]
    fn the_newest_version_that_builds_on_the_horizon_played_is_picked() {
        let p = project(vec![
            version("1.0.2", None, OTHER, &[]),
            version("1.0.1", None, YOURS, &[]),
            version("1.0.0", None, THIRD, &[]),
        ]);
        assert_eq!(picked(&p, &plays(None), &[]).as_deref(), Some("1.0.1"));
    }

    #[test]
    fn a_version_broken_on_the_horizon_played_is_passed_over() {
        let p = project(vec![
            version("2.0.0", Some(5), OTHER, &[YOURS]),
            version("1.9.0", Some(5), OTHER, &[]),
        ]);
        assert_eq!(picked(&p, &plays(Some(9)), &[]).as_deref(), Some("1.9.0"));
    }

    #[test]
    fn without_a_fitting_version_the_newest_that_moves_the_build_alone_is_picked() {
        let p = project(vec![
            version("1.2.0", Some(40), OTHER, &[]),
            version("1.1.0", None, OTHER, &[]),
            version("1.0.0", None, THIRD, &[]),
        ]);
        assert_eq!(picked(&p, &plays(Some(9)), &[]).as_deref(), Some("1.1.0"));
        let clash = [pin("veinminer", THIRD)];
        assert_eq!(
            picked(&p, &plays(Some(9)), &clash).as_deref(),
            Some("1.0.0")
        );
        let none_fit = project(vec![version("1.2.0", Some(40), OTHER, &[])]);
        assert_eq!(picked(&none_fit, &plays(Some(9)), &[]), None);
    }

    #[test]
    fn unusable_versions_are_never_picked() {
        let p = project(vec![
            version("1.0.1", None, YOURS, &[]),
            version("1.0.0", None, YOURS, &[]),
        ]);
        let found = best(&p, &plays(None), &[], |v| v.version != "1.0.1");
        assert_eq!(found.map(|v| v.version.as_str()), Some("1.0.0"));
    }

    #[test]
    fn mods_pinning_another_commit_clash() {
        let pins = [
            pin("veinminer", THIRD),
            pin("chop", OTHER),
            pin("jei", THIRD),
        ];
        // `chop` agrees with the pin and `jei` is the mod being replaced: only veinminer clashes.
        assert_eq!(
            fit("jei", None, OTHER, &plays(None), &pins, &[]),
            Some(Fit::Clashes {
                pin: OTHER.into(),
                with: vec!["veinminer".into()],
            })
        );
        let agreeing = [pin("chop", OTHER)];
        assert_eq!(
            fit("jei", None, OTHER, &plays(None), &agreeing, &[]),
            Some(Fit::MovesTo { pin: OTHER.into() })
        );
    }
}
