//! Switching what an instance plays (Edit's kinds, the mockup's `InstanceModal` for an existing
//! instance): Horizon, or Minecraft: Java Edition of one version with or without a loader. Only
//! its entry in `instances.json` changes. Its folder, with its worlds, mods and options, is never
//! touched, and a Java instance's own settings (memory, Java, arguments, window) are put aside
//! while it plays Horizon, to come back with Java.
use super::*;
use crate::loaders::Loader;

/// What an instance plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plays {
    Horizon,
    Java {
        minecraft: String,
        loader: Option<Loader>,
    },
}

impl Plays {
    /// What `instance` plays now.
    pub fn of(instance: &Instance) -> Plays {
        match &instance.java {
            None => Plays::Horizon,
            Some(j) => Plays::Java {
                minecraft: j.minecraft.clone(),
                loader: j.loader.clone(),
            },
        }
    }

    /// The Minecraft version its worlds are saved by; `horizon` is the one Horizon plays.
    fn minecraft<'a>(&'a self, horizon: &'a str) -> &'a str {
        match self {
            Plays::Horizon => horizon,
            Plays::Java { minecraft, .. } => minecraft,
        }
    }

    /// Whose mods it loads: Horizon's, a loader's, or none (Vanilla).
    fn mods_for(&self) -> Option<(&'static str, &'static str)> {
        match self {
            Plays::Horizon => Some(("horizon", "Horizon")),
            Plays::Java { loader, .. } => loader.as_ref().map(|l| (l.kind.id(), l.kind.name())),
        }
    }

    /// Whether it loads mods made for `id` (a Quilt instance takes Fabric's too).
    fn loads(&self, id: &str) -> bool {
        match (self, self.mods_for()) {
            (Plays::Horizon, _) => id == "horizon",
            (_, Some((own, _))) => own == id || (own == "quilt" && id == "fabric"),
            (_, None) => false,
        }
    }
}

/// Something in an instance's folder that a switch keeps on disk but can't keep working, for the
/// player to read before it applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unkept {
    /// `count` mods made for `made_for` (Horizon, Fabric…), which the new game doesn't load.
    Mods { count: usize, made_for: String },
    /// `count` mods made for Minecraft `from`: some may not load in `to`.
    ModsVersion {
        count: usize,
        from: String,
        to: String,
    },
    /// `count` worlds saved by Minecraft `from`: `to` upgrades them for good, or, older, can't
    /// open them.
    Worlds {
        count: usize,
        from: String,
        to: String,
    },
}

/// What switching `instance` to `to` can't keep. `horizon`: the Minecraft version Horizon plays.
pub fn unkept(instance: &Instance, to: &Plays, horizon: &str) -> Vec<Unkept> {
    let from = Plays::of(instance);
    let mut out = Vec::new();
    let (was, now) = (from.minecraft(horizon), to.minecraft(horizon));
    if let Some((id, name)) = from.mods_for() {
        let count = mod_files(&instance.home, id == "horizon");
        if count > 0 && !to.loads(id) {
            out.push(Unkept::Mods {
                count,
                made_for: name.into(),
            });
        } else if count > 0 && was != now {
            out.push(Unkept::ModsVersion {
                count,
                from: was.into(),
                to: now.into(),
            });
        }
    }
    let worlds = worlds_in(&instance.home);
    if worlds > 0 && was != now {
        out.push(Unkept::Worlds {
            count: worlds,
            from: was.into(),
            to: now.into(),
        });
    }
    out
}

/// The mods in `home`: a Horizon instance's content mods (`mods/*.hmod`) and native mods, a Java
/// one's jars, on or off.
fn mod_files(home: &Path, horizon: bool) -> usize {
    let count = |dir: &str, ext: &dyn Fn(&str) -> bool| {
        fs::read_dir(home.join(dir)).map_or(0, |d| {
            d.flatten()
                .filter(|e| e.path().is_file())
                .filter(|e| ext(&e.file_name().to_string_lossy()))
                .count()
        })
    };
    if horizon {
        count("mods", &|n| n.ends_with(".hmod"))
            + count(crate::native_mods::INSTALLED, &|n| {
                n.ends_with(".hnmod") || n.ends_with(".hmod")
            })
    } else {
        count("mods", &|n| {
            n.ends_with(".jar") || n.ends_with(".jar.disabled")
        })
    }
}

/// The worlds in `home`'s `saves`.
fn worlds_in(home: &Path) -> usize {
    fs::read_dir(home.join("saves")).map_or(0, |d| {
        d.flatten()
            .filter(|e| e.path().join("level.dat").is_file())
            .count()
    })
}

/// Switches what the instance `query` plays. Only an instance made in the launcher can: the
/// game's home and modpacks play what they were made with.
pub fn switch(query: &str, to: Plays) -> Result<Instance, String> {
    switch_in(&backend::game_home(), &backend::data_dir(), query, to)
}

pub(super) fn switch_in(
    game_home: &Path,
    data: &Path,
    query: &str,
    to: Plays,
) -> Result<Instance, String> {
    let instance = find_in(&list_in(game_home, data), query)?;
    if instance.kind != Kind::Own {
        return Err(format!("{} plays what it was made with.", instance.name));
    }
    edit_in(game_home, data, query, |m| match to {
        Plays::Horizon => {
            if let Some(java) = m.java.take() {
                m.java_kept = Some(java);
            }
        }
        Plays::Java { minecraft, loader } => {
            let mut java = m
                .java
                .take()
                .or_else(|| m.java_kept.take())
                .unwrap_or_default();
            java.minecraft = minecraft;
            java.loader = loader;
            m.java = Some(java);
        }
    })
}
