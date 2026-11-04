//! Zombie villagers (`ZombieVillager`): the cure started by a golden apple on
//! a weakened one, the conversion clock that iron bars and beds speed up, and
//! the saved fields. Turning villagers into zombie villagers and back is done
//! by the game (`game/zombie_villagers.rs`), which owns entity creation.
use super::{Mob, MobEvent, MobWorld, Species, goals::Cx, villager};
use horizon_core::random::RandomSource;
use horizon_world::nbt::{Compound, Tag};

/// `VILLAGER_CONVERSION_WAIT_MIN` and the width of the random range.
pub const WAIT_MIN: i32 = 3600;
pub const WAIT_SPREAD: i32 = 2401;
/// `MAX_SPECIAL_BLOCKS_COUNT` and `SPECIAL_BLOCK_RADIUS`.
const MAX_SPECIAL_BLOCKS: i32 = 14;
const RADIUS: i32 = 4;
const SPEEDS_UP_CURING: &str = "minecraft:speeds_up_zombie_villager_curing";

/// A cure under way: ticks left and the player who started it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cure {
    pub time: i32,
    pub starter: Option<[i32; 4]>,
}

pub fn cure(m: &Mob) -> Option<&Cure> {
    villager::ext(m).and_then(|e| e.cure.as_ref())
}
/// `ZombieVillager.handleEntityEvent(16)`: the client's cure sound.
pub fn cure_sound(m: &mut Mob) -> Option<MobEvent> {
    if m.species() != Species::ZombieVillager || m.silent {
        return None;
    }
    let volume = 1.0 + m.random.next_float();
    let pitch = m.random.next_float() * 0.7 + 0.3;
    Some(MobEvent::Sound(
        "entity.zombie_villager.cure",
        [m.position.x, m.eye_y(), m.position.z],
        volume,
        pitch,
    ))
}
/// `ZombieVillager.DATA_CONVERTING_ID` on a client: only `is_converting`
/// (the cure's shaking) reads the cure, so its time does not matter.
pub fn set_converting(m: &mut Mob, on: bool) {
    villager::ext_mut(m).cure = on.then_some(Cure {
        time: 0,
        starter: None,
    });
}
pub fn is_converting(m: &Mob) -> bool {
    m.species() == Species::ZombieVillager && cure(m).is_some()
}
/// `removeWhenFarAway`: curing ones and those with experience stay.
pub fn keeps_alive(m: &Mob) -> bool {
    m.species() == Species::ZombieVillager
        && (is_converting(m) || m.extra.trade.as_ref().is_some_and(|t| t.xp != 0))
}

/// `ZombieVillager.startConverting`: weakness gives way to strength for the
/// length of the cure and the cure sound plays (entity event 16).
pub fn start_converting(
    m: &mut Mob,
    starter: Option<[i32; 4]>,
    time: i32,
    events: &mut Vec<MobEvent>,
) {
    villager::ext_mut(m).cure = Some(Cure { time, starter });
    m.extra.effects.retain(|e| e.0 != "minecraft:weakness");
    // Strength amplifier: min(difficulty id - 1, 0), always 0 with monsters.
    m.add_effect("minecraft:strength", time, 0);
    events.push(MobEvent::EntityEvent {
        mob: m.id,
        event: 16,
        at: [m.position.x, m.position.y, m.position.z],
    });
}

/// `getConversionProgress`: 1 per tick, plus a chance per nearby iron bars or
/// bed (up to 14 of them) found on one tick in a hundred.
fn progress(m: &mut Mob, w: &MobWorld) -> i32 {
    let mut amount = 1;
    if m.random.next_float() < 0.01 {
        let mut special = 0;
        // `(int) getX()` truncates toward zero.
        let (bx, by, bz) = (
            m.position.x as i32,
            m.position.y as i32,
            m.position.z as i32,
        );
        'scan: for x in bx - RADIUS..bx + RADIUS {
            for y in by - RADIUS..by + RADIUS {
                for z in bz - RADIUS..bz + RADIUS {
                    if special >= MAX_SPECIAL_BLOCKS {
                        break 'scan;
                    }
                    if w.has_tag(SPEEDS_UP_CURING, w.block_name(x, y, z)) {
                        if m.random.next_float() < 0.3 {
                            amount += 1;
                        }
                        special += 1;
                    }
                }
            }
        }
    }
    amount
}

/// `ZombieVillager.tick`: count the cure down, finishing it at zero.
pub fn step(m: &mut Mob, cx: &mut Cx) {
    if !m.alive() || !is_converting(m) {
        return;
    }
    let amount = progress(m, cx.w);
    let Some(c) = villager::ext_mut(m).cure.as_mut() else {
        return;
    };
    c.time -= amount;
    if c.time <= 0 {
        cx.events.push(MobEvent::CureZombieVillager { mob: m.id });
    }
}

/// `ConversionTime` (-1: not converting) and `ConversionPlayer`.
fn put_cure(map: &mut Compound, cure: Option<Cure>) {
    map.insert(
        "ConversionTime".into(),
        Tag::Int(cure.map_or(-1, |c| c.time)),
    );
    if let Some(starter) = cure.and_then(|c| c.starter) {
        map.insert("ConversionPlayer".into(), Tag::IntArray(starter.to_vec()));
    }
}
fn parse_cure(tag: &Tag) -> Option<Cure> {
    let time = match tag.get("ConversionTime") {
        Some(Tag::Int(v)) => *v,
        _ => -1,
    };
    (time != -1).then(|| Cure {
        time,
        starter: crate::save::uuid_from_tag(tag.get("ConversionPlayer")),
    })
}
/// The saved fields: the cure and the gossip.
pub fn write_nbt(m: &Mob, map: &mut Compound) {
    put_cure(map, cure(m).copied());
    if let Some(e) = villager::ext(m)
        && !e.gossips.is_empty()
    {
        map.insert("Gossips".into(), e.gossips.to_nbt());
    }
}
pub fn read_nbt(m: &mut Mob, tag: &Tag) {
    let cure = parse_cure(tag);
    let e = villager::ext_mut(m);
    e.cure = cure;
    e.gossips = tag
        .get("Gossips")
        .map(crate::gossip::Gossips::from_nbt)
        .unwrap_or_default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::{geometry::Vec3, random::LegacyRandom};

    fn zombie_villager() -> Mob {
        let mut kind = super::super::sensor_scan_tests::kind();
        let data = std::sync::Arc::make_mut(&mut kind);
        data.id = "minecraft:zombie_villager".into();
        data.species = Species::ZombieVillager;
        data.eye_height = 1.5;
        let mut m = Mob::new(42, kind, Vec3::new(1.25, 2.0, 3.75), 0.0);
        m.random = LegacyRandom::new(0);
        m
    }

    #[test]
    fn a_cure_round_trips_through_nbt() {
        let cure = Cure {
            time: 4200,
            starter: Some([1, 2, 3, 4]),
        };
        let mut map = Compound::new();
        put_cure(&mut map, Some(cure));
        assert_eq!(parse_cure(&Tag::Compound(map)), Some(cure));
    }
    /// `ZombieVillager.tick` counts the cure down before `Zombie.tick`,
    /// whatever the AI: a `NoAI` zombie villager finishes curing too (#3429).
    #[test]
    fn a_no_ai_zombie_villager_still_cures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-types.json").exists()
            || !root.join("movement-properties.nbt").exists()
        {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let registry = assets.registry;
        let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
        let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
        let data = super::super::MobData::load(&root, &facts);
        let types = super::super::MobTypes::load(&root).unwrap();
        let biomes = crate::biome::Biomes::load(&root).unwrap();
        let items = crate::items::Items::load(
            &root,
            registry,
            &facts,
            &horizon_world::loot::PackOverlay::NONE,
        )
        .unwrap();
        let collision = crate::collisions::load(&root, registry).unwrap();
        let air = registry.default_state("minecraft:air").unwrap();
        let world = crate::world::World::new(air, 0);
        let lid_openers = std::collections::HashMap::new();
        let w = MobWorld {
            teams: Default::default(),
            world: &world,
            registry,
            facts: &facts,
            fluid_tables: &motion.fluids,
            movement: &motion.blocks,
            collision: &collision,
            data: &data,
            player: None,
            controlling_player_movement: None,
            nearest_player: None,
            others: &[],
            dragon: None,
            leash_boats: &[],
            others_grid: None,
            others_index: None,
            others_villagers: None,
            daylight: true,
            monsters_burn: false,
            game_time: 1000,
            day_time: 18000,
            raining: false,
            thundering: false,
            nether: false,
            block_tags: &items.block_tags,
            landing: &motion.landing,
            biomes: &biomes,
            mob_griefing: true,
            universal_anger: false,
            mob_drops: true,
            max_entity_cramming: 24,
            difficulty: super::super::spawning::LocalDifficulty::new(2, 18000, 0, 0.0),
            end: false,
            world_seed: 0,
            sea_level: 63,
            items: Some(&items),
            lid_openers: &lid_openers,
            mod_effects: None,
            border: None,
        };
        for no_ai in [false, true] {
            let kind = std::sync::Arc::new(types.types["minecraft:zombie_villager"].clone());
            let mut m = Mob::new(
                1,
                kind,
                horizon_core::geometry::Vec3::new(0.5, 0.0, 0.5),
                0.0,
            );
            m.persistent = true;
            m.no_ai = no_ai;
            start_converting(&mut m, None, 3, &mut Vec::new());
            let mut level_random = horizon_core::random::LegacyRandom::new(0);
            let mut ticks = 0;
            while ticks < 10 {
                let mut events = Vec::new();
                m.tick(&w, &mut level_random, &mut events);
                ticks += 1;
                if events
                    .iter()
                    .any(|e| matches!(e, MobEvent::CureZombieVillager { mob: 1 }))
                {
                    break;
                }
            }
            assert!(
                ticks <= 3,
                "no_ai={no_ai}: the cure must finish within its 3 ticks, took {ticks}"
            );
        }
    }
    #[test]
    fn no_cure_saves_minus_one() {
        let mut map = Compound::new();
        put_cure(&mut map, None);
        assert_eq!(map.get(&"ConversionTime".into()), Some(&Tag::Int(-1)));
        assert_eq!(parse_cure(&Tag::Compound(map)), None);
    }

    #[test]
    fn starting_a_cure_broadcasts_even_when_silent_without_drawing_sound_randomness() {
        for silent in [false, true] {
            let mut m = zombie_villager();
            m.silent = silent;
            m.add_effect("minecraft:weakness", 100, 0);
            let mut random = m.random.clone();
            let starter = Some([1, 2, 3, 4]);
            let mut events = Vec::new();

            start_converting(&mut m, starter, 4000, &mut events);

            assert_eq!(
                cure(&m),
                Some(&Cure {
                    time: 4000,
                    starter
                })
            );
            assert!(!m.has_effect("minecraft:weakness"));
            assert!(m.extra.effects.contains(&("minecraft:strength", 4000, 0)));
            assert_eq!(events.len(), 1);
            let MobEvent::EntityEvent { mob, event, at } = events[0] else {
                panic!("Starting a cure must broadcast an entity event");
            };
            assert_eq!((mob, event, at), (42, 16, [1.25, 2.0, 3.75]));
            assert_eq!(m.random.next_long(), random.next_long());
        }
    }

    #[test]
    fn cure_sound_draws_volume_before_pitch_at_eye_height() {
        let mut m = zombie_villager();
        let Some(MobEvent::Sound(name, at, volume, pitch)) = cure_sound(&mut m) else {
            panic!("A non-silent zombie villager must play the cure sound");
        };
        assert_eq!(name, "entity.zombie_villager.cure");
        assert_eq!(at, [1.25, 3.5, 3.75]);
        // The first two java.util.Random(0) floats, volume before pitch.
        assert_eq!(volume.to_bits(), 0x3fdd_905a);
        assert_eq!(pitch.to_bits(), 0x3f61_cb52);
        assert_eq!(m.random.next_float().to_bits(), 0x3e76_4f2c);
    }

    #[test]
    fn silent_or_other_mobs_do_not_play_the_cure_sound_or_draw_randomness() {
        let mut silent = zombie_villager();
        silent.silent = true;
        let other = Mob::new(43, super::super::sensor_scan_tests::kind(), Vec3::ZERO, 0.0);
        for mut m in [silent, other] {
            let mut random = m.random.clone();
            assert!(cure_sound(&mut m).is_none());
            assert_eq!(m.random.next_long(), random.next_long());
        }
    }
}
