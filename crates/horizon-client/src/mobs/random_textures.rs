//! OptiFine random entity textures on mobs: which variant of each of its textures a mob wears
//! (`horizon_optifine::random_textures` finds them in the packs).
//!
//! [`choose`] runs when the client first sees a mob and about once a second after that, never
//! per frame. The spawn-time facts (biome, height, blocks, moon, time, weather) are fixed when
//! the mob is first seen; the others (name, profession, colour, baby, health, size) are read
//! fresh each time. Drawing only looks the choice up ([`texture`]).
//!
//! Each texture with its own rules (or those of the name it extends) wears that rule file's
//! pick. A texture without rules follows the body's variant, so a sheep wearing `sheep3` has
//! `sheep_wool3` (ETF), or an even choice when the body has no variants; villagers' layers each
//! choose their own, as ETF does.

use super::render::{DYE_NAMES, PROFESSIONS};
use super::{Mob, Species};
use horizon_optifine::random::{self, Block, Facts, Weather};
use horizon_optifine::random_textures::RandomTextures;

/// Where the mob was when the client first saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    /// `minecraft:plains`; `None` in an unloaded chunk.
    pub biome: Option<String>,
    pub y: i32,
    /// The block it stood on and the block it stood in: ids and state properties.
    pub blocks: Vec<(String, Vec<(String, String)>)>,
    pub moon_phase: i32,
    pub day_time: i32,
    pub weather: Weather,
}

/// A mob's variants.
#[derive(Clone, Debug, Default)]
pub struct Choice {
    seen: Option<Seen>,
    seed: i32,
    /// The variant each rule file chose, by its index in [`RandomTextures::rules`].
    picks: Vec<(usize, u32)>,
    /// The body texture's variant.
    primary: Option<u32>,
    /// The packs changed since the picks.
    stale: bool,
}

impl Choice {
    /// Whether the mob's first sighting is recorded.
    pub fn has_seen(&self) -> bool {
        self.seen.is_some()
    }

    /// Whether [`choose`] should run now: on first sight, after the packs changed, and on
    /// one tick in 20 (staggered by `stagger`, the entity id) for the facts that change.
    pub fn due(&self, tick: i64, stagger: u32) -> bool {
        !self.has_seen() || self.stale || (tick + i64::from(stagger)).rem_euclid(20) == 0
    }

    /// The packs changed: choose again, from the same first sighting.
    pub fn forget_picks(&mut self) {
        self.stale = true;
    }
}

/// An entity's seed from the UUID ints the client keeps (`UUIDUtil.uuidToIntArray`, most
/// significant first); its entity id when the UUID is unknown.
pub fn seed(m: &Mob, uuid: Option<[i32; 4]>) -> i32 {
    let long = |hi: i32, lo: i32| (i64::from(hi) << 32) | i64::from(lo as u32);
    uuid.map_or(m.id as i32, |u| {
        random::seed(long(u[0], u[1]), long(u[2], u[3]))
    })
}

/// Chooses the variant of each texture `m` draws now. `seen` describes its surroundings the
/// first time (when [`Choice::has_seen`] is false); later calls keep that.
pub fn choose(m: &mut Mob, textures: &RandomTextures, seen: Option<Seen>, seed: i32) {
    let mut choice = std::mem::take(&mut m.random_textures);
    if choice.seen.is_none() {
        choice.seen = seen;
    }
    let Some(seen) = choice.seen.take() else {
        m.random_textures = choice;
        return;
    };
    choice.seed = seed;
    choice.picks.clear();
    choice.primary = None;
    choice.stale = false;
    let states: Vec<Vec<(&str, &str)>> = seen
        .blocks
        .iter()
        .map(|(_, s)| s.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect())
        .collect();
    let blocks: Vec<Block> = seen
        .blocks
        .iter()
        .zip(&states)
        .map(|((id, _), states)| Block { id, states })
        .collect();
    let profession =
        matches!(m.species(), Species::Villager | Species::ZombieVillager).then(|| {
            let name = PROFESSIONS[m.extra.variant2 as usize % PROFESSIONS.len()];
            (format!("minecraft:{name}"), i32::from(m.extra.variant3))
        });
    let facts = Facts {
        biome: seen.biome.as_deref(),
        y: Some(seen.y),
        blocks: &blocks,
        name: m.custom_name.as_deref(),
        profession: profession.as_ref().map(|(p, level)| (p.as_str(), *level)),
        color: color(m),
        baby: Some(m.is_baby()),
        health: Some((m.health, super::finalize::max_health(m))),
        moon_phase: Some(seen.moon_phase),
        day_time: Some(seen.day_time),
        weather: Some(seen.weather),
        size: size(m),
    };
    let (_, body) = super::render::body(m);
    let layers = super::render::layers(m, m.age as f32);
    let names = std::iter::once(body.as_str()).chain(layers.iter().map(|l| l.texture.as_str()));
    for (i, name) in names.enumerate() {
        let Some(entry) = textures.get(name) else {
            continue;
        };
        let index = match entry.rules {
            Some(id) => match choice.picks.iter().find(|p| p.0 == id) {
                Some(&(_, index)) => index,
                None => {
                    // ETF's 0 is the default texture too.
                    let index = textures.rules(id).select(&facts, seed).max(1);
                    choice.picks.push((id, index));
                    index
                }
            },
            None => entry.even(seed),
        };
        if i == 0 {
            choice.primary = Some(index);
        }
    }
    choice.seen = Some(seen);
    m.random_textures = choice;
}

/// The atlas name of the variant `m` wears for `texture`, or `None` for the texture itself.
pub fn texture<'a>(m: &Mob, textures: &'a RandomTextures, texture: &str) -> Option<&'a str> {
    let entry = textures.get(texture)?;
    let choice = &m.random_textures;
    if !choice.has_seen() || choice.stale {
        return None;
    }
    let index = match entry.rules {
        Some(id) => choice.picks.iter().find(|p| p.0 == id)?.1,
        None => match choice.primary {
            Some(primary)
                if !matches!(m.species(), Species::Villager | Species::ZombieVillager) =>
            {
                primary
            }
            _ => entry.even(choice.seed),
        },
    };
    entry.texture(index)
}

/// `colors`: a sheep's wool, a wolf's or cat's collar, a llama's carpet, a shulker's shell.
fn color(m: &Mob) -> Option<&'static str> {
    match m.species() {
        Species::Sheep | Species::Wolf | Species::Cat => {
            Some(DYE_NAMES[usize::from(m.extra.color & 15)])
        }
        Species::Shulker => DYE_NAMES.get(usize::from(m.extra.color)).copied(),
        Species::Llama | Species::TraderLlama => {
            let carpet = super::llama::carpet(m)?;
            DYE_NAMES.iter().find(|&&d| d == carpet).copied()
        }
        _ => None,
    }
}

/// `sizes`, as the docs number them: a slime's or magma cube's size less one (0 small, 1 medium,
/// 3 big), a phantom's size.
fn size(m: &Mob) -> Option<i32> {
    match m.species() {
        Species::Slime | Species::MagmaCube => Some((m.extra.size - 1).max(0)),
        Species::Phantom => Some(m.extra.size),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    fn mob(species: Species) -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        Arc::make_mut(&mut kind).species = species;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    fn seen(biome: &str) -> Seen {
        Seen {
            biome: Some(biome.into()),
            y: 64,
            blocks: vec![(
                "minecraft:grass_block".into(),
                vec![("snowy".into(), "false".into())],
            )],
            moon_phase: 0,
            day_time: 1000,
            weather: Weather::Clear,
        }
    }

    /// A temp pack of `files` (path, contents), scanned.
    fn pack(name: &str, files: &[(&str, &str)]) -> RandomTextures {
        let root =
            std::env::temp_dir().join(format!("horizon-mob-random-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root
                .join("assets/minecraft/optifine/random/entity")
                .join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let textures = RandomTextures::scan(&root);
        let _ = std::fs::remove_dir_all(&root);
        textures
    }

    const SHEEP: &str = "entity/sheep/sheep";
    const WOOL: &str = "entity/sheep/sheep_wool";
    fn variant(name: &str) -> String {
        format!("minecraft:optifine/random/{name}")
    }

    #[test]
    fn a_rule_picks_the_body_and_its_layers_and_follows_the_colour() {
        let t = pack(
            "colour",
            &[
                (
                    "sheep/sheep.properties",
                    "textures.1=3\ncolors.1=red\ntextures.2=2",
                ),
                ("sheep/sheep2.png", ""),
                ("sheep/sheep3.png", ""),
                ("sheep/sheep_wool2.png", ""),
                ("sheep/sheep_wool3.png", ""),
            ],
        );
        let mut m = mob(Species::Sheep);
        assert_eq!(texture(&m, &t, SHEEP), None, "not chosen yet");
        assert!(m.random_textures.due(5, m.id));
        m.extra.color = 14; // red
        choose(&mut m, &t, Some(seen("minecraft:plains")), 0);
        assert_eq!(
            texture(&m, &t, SHEEP),
            Some(variant("entity/sheep/sheep3").as_str())
        );
        assert_eq!(
            texture(&m, &t, WOOL),
            Some(variant("entity/sheep/sheep_wool3").as_str())
        );
        // Checked again on one tick in 20 (tick + id), the dyed sheep changes.
        assert!(!m.random_textures.due(5, m.id));
        assert!(m.random_textures.due(19, m.id));
        m.extra.color = 0;
        choose(&mut m, &t, None, 0);
        assert_eq!(
            texture(&m, &t, SHEEP),
            Some(variant("entity/sheep/sheep2").as_str())
        );
        // A pack reload: nothing until chosen again, at once.
        m.random_textures.forget_picks();
        assert_eq!(texture(&m, &t, SHEEP), None);
        assert!(m.random_textures.due(5, m.id));
    }

    #[test]
    fn the_first_sighting_is_kept() {
        let t = pack(
            "biome",
            &[
                ("sheep/sheep.properties", "textures.1=2\nbiomes.1=desert"),
                ("sheep/sheep2.png", ""),
            ],
        );
        let mut m = mob(Species::Sheep);
        choose(&mut m, &t, Some(seen("minecraft:desert")), 0);
        assert_eq!(
            texture(&m, &t, SHEEP),
            Some(variant("entity/sheep/sheep2").as_str())
        );
        // Walking into plains doesn't change it: biomes are read when first seen.
        choose(&mut m, &t, Some(seen("minecraft:plains")), 0);
        assert_eq!(
            texture(&m, &t, SHEEP),
            Some(variant("entity/sheep/sheep2").as_str())
        );
        // Without a first sighting nothing is chosen.
        let mut unseen = mob(Species::Sheep);
        choose(&mut unseen, &t, None, 0);
        assert_eq!(texture(&unseen, &t, SHEEP), None);
    }

    #[test]
    fn layers_without_rules_follow_the_body() {
        // No rules: an even choice of sheep 1-3 by seed, and the wool wears the same number.
        let t = pack(
            "even",
            &[
                ("sheep/sheep2.png", ""),
                ("sheep/sheep3.png", ""),
                ("sheep/sheep_wool2.png", ""),
            ],
        );
        let mut m = mob(Species::Sheep);
        choose(&mut m, &t, Some(seen("minecraft:plains")), 4);
        // 4 % 3 + 1 = 2.
        assert_eq!(
            texture(&m, &t, SHEEP),
            Some(variant("entity/sheep/sheep2").as_str())
        );
        assert_eq!(
            texture(&m, &t, WOOL),
            Some(variant("entity/sheep/sheep_wool2").as_str())
        );
        choose(&mut m, &t, None, 5);
        // 5 % 3 + 1 = 3: there is no sheep_wool3, so the wool is vanilla's.
        assert_eq!(
            texture(&m, &t, SHEEP),
            Some(variant("entity/sheep/sheep3").as_str())
        );
        assert_eq!(texture(&m, &t, WOOL), None);
    }

    #[test]
    fn seeds_from_uuid_ints() {
        let m = mob(Species::Sheep);
        // new UUID(1, 2).hashCode() = 3.
        assert_eq!(seed(&m, Some([0, 1, 0, 2])), 3);
        // The low int is unsigned: UUID(0x00000000ffffffff, 0).hashCode() = 0 ^ 0xffffffff.
        assert_eq!(seed(&m, Some([0, -1, 0, 0])), -1);
        assert_eq!(seed(&m, None), 1, "the entity id");
    }
}
