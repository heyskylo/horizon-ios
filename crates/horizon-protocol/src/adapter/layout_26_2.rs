//! The pinned (26.3) clientbound packets a 26.2 client reads in another layout, written in 26.2's: the
//! mirror of `translate`'s 26.2 readers for a [`super::ClientSession`] (roadmap `older-clients`). Registry ids
//! inside these packets are renumbered here too, since they never reach the id layer's generic rewrite.
//!
//! Off by default ([`set_enabled`]): until a client has played through every packet here, the id layer
//! alone keeps the pinned layout.
use std::sync::atomic::{AtomicBool, Ordering};

use super::ids::{self, Ids, Way, down_or_substitute};
use crate::{
    Result, State,
    buf::{Codec, Reader, Writer},
    ids as packet_ids,
    packets::{configuration, play},
};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether 26.2 clients get these packets in their own layout.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Release);
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// What becomes of a pinned clientbound payload rewritten here.
#[derive(Debug, PartialEq)]
pub enum Out {
    /// Not one of the packets this layer rewrites.
    Untouched,
    Rewritten(Vec<u8>),
    /// The client has nothing to read it as.
    Dropped,
}

/// The pinned `payload` of packet `id` in `state`, as a 26.2 client (`ids`) reads it.
pub fn clientbound(ids: &'static Ids, state: State, id: i32, payload: &[u8]) -> Result<Out> {
    use packet_ids::play::clientbound as cb;
    Ok(match (state, id) {
        (State::Configuration, packet_ids::configuration::clientbound::REGISTRY_DATA) => {
            registry_data(payload)?
        }
        (State::Play, cb::LOGIN) => Out::Rewritten(login(payload)?),
        (State::Play, cb::RESPAWN) => Out::Rewritten(respawn(payload)?),
        (State::Play, cb::UPDATE_ADVANCEMENTS) => Out::Rewritten(advancements(ids, payload)?),
        (State::Play, cb::COMMANDS) => Out::Rewritten(commands(ids, payload)?),
        (State::Play, cb::MAP_ITEM_DATA) => Out::Rewritten(map_item_data(ids, payload)?),
        (State::Play, cb::AWARD_STATS) => Out::Rewritten(award_stats(ids, payload)?),
        _ => Out::Untouched,
    })
}

/// Synchronized registries 26.3 added (RegistryDataLoader.SYNCHRONIZED_REGISTRIES of both jars compared):
/// 26.2 has no codec for their entries.
const NEW_REGISTRIES: [&str; 3] = [
    "minecraft:decorated_pot_pattern",
    "minecraft:block_transformer",
    "minecraft:worldgen/block_state_provider",
];

/// A registry 26.2 doesn't synchronize is not sent; the others pass as they are (unknown fields in an
/// entry's NBT are left to 26.2's record codecs, which read past them).
fn registry_data(payload: &[u8]) -> Result<Out> {
    let p = configuration::RegistryData::read(&mut Reader::new(payload))?;
    Ok(if NEW_REGISTRIES.contains(&p.registry.as_str()) {
        Out::Dropped
    } else {
        Out::Untouched
    })
}

/// 26.2's CommonPlayerSpawnInfo: the game types as bytes, -1 for no previous one (26.3: VarInts, the
/// optional one as id + 1).
fn spawn_info(s: &play::SpawnInfo, w: &mut Writer) {
    w.varint(s.dimension_type);
    w.string(&s.dimension);
    w.i64(s.seed);
    w.i8(s.game_type as i8);
    w.i8(s.previous_game_type.map_or(-1, |g| g as i8));
    w.bool(s.debug);
    w.bool(s.flat);
    <crate::buf::Opt<play::GlobalPos>>::write(&s.last_death, w);
    w.varint(s.portal_cooldown);
    w.varint(s.sea_level);
}

fn login(payload: &[u8]) -> Result<Vec<u8>> {
    let l = play::Login::read(&mut Reader::new(payload))?;
    let mut w = Writer::new();
    w.i32(l.player_id);
    w.bool(l.hardcore);
    w.varint(l.levels.len() as i32);
    l.levels.iter().for_each(|s| w.string(s));
    w.varint(l.max_players);
    w.varint(l.chunk_radius);
    w.varint(l.simulation_distance);
    w.bool(l.reduced_debug_info);
    w.bool(l.show_death_screen);
    w.bool(l.limited_crafting);
    spawn_info(&l.spawn, &mut w);
    w.bool(l.online_mode);
    w.bool(l.enforces_secure_chat);
    Ok(w.data)
}

fn respawn(payload: &[u8]) -> Result<Vec<u8>> {
    let p = play::Respawn::read(&mut Reader::new(payload))?;
    let mut w = Writer::new();
    spawn_info(&p.spawn, &mut w);
    w.i8(p.keep);
    Ok(w.data)
}

/// 26.3 sends an advancement's tab position after the whole advancement; 26.2 inside its display, after
/// the background (an advancement without a display has none). The icon takes the client's item ids.
fn advancements(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.bool(r.bool()?);
    let n = r.count(r.remaining())?;
    w.varint(n as i32);
    for _ in 0..n {
        w.string(&r.string(32767)?);
        let parent = r.bool()?;
        w.bool(parent);
        if parent {
            w.string(&r.string(32767)?);
        }
        // The display waits for the position, which comes last in 26.3.
        let mut display = None;
        if r.bool()? {
            let mut d = Writer::new();
            d.nbt(&r.nbt()?);
            d.nbt(&r.nbt()?);
            let icon = crate::item::Template::read(&mut r)?;
            {
                let _down = ids::renumbering(ids, Way::Down);
                crate::item::Template::write(&icon, &mut d);
            }
            d.varint(r.varint()?);
            let flags = r.i32()?;
            d.i32(flags);
            if flags & 1 != 0 {
                d.string(&r.string(32767)?);
            }
            display = Some(d.data);
        }
        let mut rest = Writer::new();
        let groups = r.count(r.remaining())?;
        rest.varint(groups as i32);
        for _ in 0..groups {
            let k = r.count(r.remaining())?;
            rest.varint(k as i32);
            for _ in 0..k {
                rest.string(&r.string(32767)?);
            }
        }
        rest.bool(r.bool()?);
        let (x, y) = (r.f32()?, r.f32()?);
        w.bool(display.is_some());
        if let Some(d) = display {
            w.bytes(&d);
            w.f32(x);
            w.f32(y);
        }
        w.bytes(&rest.data);
    }
    w.bytes(r.rest());
    Ok(w.data)
}

/// `brigadier:string`, the same id in 26.2 and 26.3.
const STRING_PARSER: i32 = 5;

/// Each argument node's parser in 26.2's numbering. A parser 26.2 lacks becomes a string (a greedy one on
/// a node nothing follows, a word otherwise) whose suggestions the client asks the server for, so the
/// command still parses on the client and the server checks the argument.
fn commands(ids: &Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut tree = play::Commands::read(&mut Reader::new(payload))?;
    for node in &mut tree.nodes {
        let Some((parser, _)) = &node.parser else {
            continue;
        };
        match ids.command_argument_type.down(*parser) {
            Some(old) => {
                if let Some(p) = &mut node.parser {
                    p.0 = old;
                }
            }
            None => {
                let kind = if node.children.is_empty() { 2 } else { 0 };
                node.parser = Some((STRING_PARSER, vec![kind]));
                node.suggestions = Some("minecraft:ask_server".into());
            }
        }
    }
    let mut w = Writer::new();
    play::Commands::write(&tree, &mut w);
    Ok(w.data)
}

/// Map decorations in 26.2's `map_decoration_type` numbering; one it lacks isn't drawn.
fn map_item_data(ids: &Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut p = play::MapItemData::read(&mut Reader::new(payload))?;
    if let Some(decorations) = &mut p.decorations {
        decorations.retain_mut(|d| {
            ids.map_decoration_type
                .down(d.kind)
                .map(|k| d.kind = k)
                .is_some()
        });
    }
    let mut w = Writer::new();
    play::MapItemData::write(&p, &mut w);
    Ok(w.data)
}

/// `minecraft:stat_type` in vanilla's registration order (Stats): what each type's value is an id of.
const STAT_MINED: i32 = 0;
const STAT_KILLED: i32 = 6;
const STAT_KILLED_BY: i32 = 7;
const STAT_CUSTOM: i32 = 8;

/// Stats with their type and value in 26.2's numbering; one naming what 26.2 lacks isn't sent.
fn award_stats(ids: &Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut p = play::AwardStats::read(&mut Reader::new(payload))?;
    p.stats.retain_mut(|(kind, value, _)| {
        let renumbered = match *kind {
            STAT_MINED => ids.block.down(*value),
            STAT_KILLED | STAT_KILLED_BY => ids.entity_type.down(*value),
            STAT_CUSTOM => ids.custom_stat.down(*value),
            // crafted, used, broken, picked_up, dropped
            _ => down_or_substitute(ids.item, ids.substitutes.item, *value),
        };
        match (ids.stat_type.down(*kind), renumbered) {
            (Some(k), Some(v)) => {
                (*kind, *value) = (k, v);
                true
            }
            _ => false,
        }
    });
    let mut w = Writer::new();
    play::AwardStats::write(&p, &mut w);
    Ok(w.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ServerVersion;

    fn ids() -> &'static Ids {
        ServerVersion::V26_2.ids().unwrap()
    }

    fn spawn(previous: Option<i32>) -> play::SpawnInfo {
        play::SpawnInfo {
            dimension_type: 0,
            dimension: "minecraft:overworld".into(),
            seed: 42,
            game_type: 1,
            previous_game_type: previous,
            debug: false,
            flat: true,
            last_death: None,
            portal_cooldown: 0,
            sea_level: 63,
        }
    }

    #[test]
    fn respawn_game_types_are_bytes_again() {
        for previous in [None, Some(3)] {
            let mut w = Writer::new();
            play::Respawn::write(
                &play::Respawn {
                    spawn: spawn(previous),
                    keep: 3,
                },
                &mut w,
            );
            let body = respawn(&w.data).unwrap();
            // 26.2's own reader (translate's spawn_info) brings it back.
            let back = super::super::translate::tests_support::respawn_26_2(&body).unwrap();
            let p = play::Respawn::read(&mut Reader::new(&back)).unwrap();
            assert_eq!(p.spawn, spawn(previous));
            assert_eq!(p.keep, 3);
        }
    }

    #[test]
    fn login_reads_back_with_26_2_s_reader() {
        let login_packet = play::Login {
            player_id: 7,
            hardcore: false,
            levels: vec!["minecraft:overworld".into()],
            max_players: 20,
            chunk_radius: 10,
            simulation_distance: 8,
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: spawn(None),
            online_mode: true,
            enforces_secure_chat: false,
        };
        let mut w = Writer::new();
        play::Login::write(&login_packet, &mut w);
        let body = login(&w.data).unwrap();
        let back = super::super::translate::tests_support::login_26_2(&body).unwrap();
        assert_eq!(
            play::Login::read(&mut Reader::new(&back)).unwrap(),
            login_packet
        );
    }

    #[test]
    fn an_advancement_s_position_goes_back_into_its_display() {
        // One advancement with a display (diamond icon, no background) at (2.5, -1), one without at (0, 0).
        let mut w = Writer::new();
        w.bool(true);
        w.varint(2);
        for (id, display) in [("minecraft:a", true), ("minecraft:b", false)] {
            w.string(id);
            w.bool(false);
            w.bool(display);
            if display {
                w.nbt(&horizon_world::nbt::Tag::String("T".into()));
                w.nbt(&horizon_world::nbt::Tag::String("D".into()));
                crate::item::Template::write(&crate::item::Stack::new(1012, 1), &mut w);
                w.varint(0);
                w.i32(2);
            }
            w.varint(1);
            w.varint(1);
            w.string("done");
            w.bool(false);
            w.f32(if display { 2.5 } else { 0.0 });
            w.f32(if display { -1.0 } else { 0.0 });
        }
        w.varint(0);
        w.varint(0);
        w.bool(true);
        let body = advancements(ids(), &w.data).unwrap();
        let back = super::super::translate::tests_support::advancements_26_2(ids(), &body).unwrap();
        // Read back with 26.2's reader the icon is the pinned diamond again (926 in 26.2).
        assert_eq!(back, w.data);
    }

    #[test]
    fn a_26_3_only_parser_becomes_a_string_and_others_shift_down() {
        let node =
            |kind: u8, children: Vec<i32>, parser: Option<(i32, Vec<u8>)>| play::CommandNode {
                kind,
                executable: true,
                restricted: false,
                children,
                redirect: None,
                name: if kind == 0 { String::new() } else { "x".into() },
                parser,
                suggestions: None,
            };
        let uuid = play::ARGUMENT_TYPES
            .iter()
            .position(|&n| n == "minecraft:uuid")
            .unwrap() as i32;
        let tree = play::Commands {
            nodes: vec![
                node(0, vec![1, 2], None),
                node(2, vec![], Some((uuid, vec![]))),
                node(2, vec![], Some((3, vec![0]))), // brigadier:integer, no bounds
            ],
            root: 0,
        };
        let mut w = Writer::new();
        play::Commands::write(&tree, &mut w);
        let body = commands(ids(), &w.data).unwrap();
        let out = play::Commands::read(&mut Reader::new(&body)).unwrap();
        let uuid_down = ids().command_argument_type.down(uuid);
        match uuid_down {
            // uuid is new in 26.3: a greedy string asking the server.
            None => {
                assert_eq!(out.nodes[1].parser, Some((STRING_PARSER, vec![2])));
                assert_eq!(
                    out.nodes[1].suggestions.as_deref(),
                    Some("minecraft:ask_server")
                );
            }
            Some(old) => assert_eq!(out.nodes[1].parser.as_ref().unwrap().0, old),
        }
        assert_eq!(out.nodes[2].parser, Some((3, vec![0])));
    }

    #[test]
    fn registries_26_2_lacks_are_not_sent() {
        let data = |registry: &str| {
            let mut w = Writer::new();
            configuration::RegistryData::write(
                &configuration::RegistryData {
                    registry: registry.into(),
                    entries: vec![],
                },
                &mut w,
            );
            registry_data(&w.data).unwrap()
        };
        assert_eq!(data("minecraft:block_transformer"), Out::Dropped);
        assert_eq!(data("minecraft:worldgen/biome"), Out::Untouched);
    }

    #[test]
    fn stats_and_map_decorations_take_26_2_s_ids() {
        let ids = ids();
        // The last pinned custom stat and map decoration are the newest; whatever 26.2 lacks goes.
        let mut w = Writer::new();
        play::AwardStats::write(
            &play::AwardStats {
                stats: vec![(STAT_CUSTOM, 0, 5), (STAT_CUSTOM, 77, 1), (1, 1012, 2)],
            },
            &mut w,
        );
        let out = award_stats(ids, &w.data).unwrap();
        let p = play::AwardStats::read(&mut Reader::new(&out)).unwrap();
        let expected: Vec<_> = [(STAT_CUSTOM, 0, 5), (STAT_CUSTOM, 77, 1), (1, 1012, 2)]
            .into_iter()
            .filter_map(|(k, v, n)| {
                let v = match k {
                    STAT_CUSTOM => ids.custom_stat.down(v),
                    _ => ids.item.down(v),
                }?;
                Some((ids.stat_type.down(k)?, v, n))
            })
            .collect();
        assert_eq!(p.stats, expected);
        // diamond: 1012 pinned, 926 in 26.2.
        assert!(p.stats.contains(&(1, 926, 2)));
    }

    #[test]
    fn pot_sides_and_container_slots_take_26_2_s_layout_and_ids() {
        use crate::item::{Component, Stack, Template, component};
        let v26_2 = ids();
        // A pot with a diamond (1012 pinned, 926 in 26.2) at the back, the rest undecorated, and a
        // container holding three diamonds and an empty slot: their pinned bytes.
        let mut pot = Writer::new();
        pot.bool(true);
        Template::write(&Stack::new(1012, 1), &mut pot);
        (0..3).for_each(|_| pot.bool(false));
        let mut container = Writer::new();
        container.varint(2);
        container.bool(true);
        Template::write(&Stack::new(1012, 3), &mut container);
        container.bool(false);
        let mut stack = Stack::new(1012, 1);
        for (kind, bytes) in [
            (component::POT_DECORATIONS, &pot.data),
            (component::CONTAINER, &container.data),
        ] {
            let c = Component::read(kind, &mut Reader::new(bytes)).unwrap();
            stack.added.push((kind, c));
        }
        let mut w = Writer::new();
        {
            let _down = ids::renumbering(v26_2, Way::Down);
            Template::write(&stack, &mut w);
        }
        // Read with 26.2's reader (its quirks, its ids) both come back as they were.
        let back = {
            let _up = ids::renumbering(v26_2, Way::Up);
            Template::read(&mut Reader::new(&w.data)).unwrap()
        };
        assert_eq!(back.item, 1012);
        for (kind, bytes) in [
            (component::POT_DECORATIONS, &pot.data),
            (component::CONTAINER, &container.data),
        ] {
            let mut b = Writer::new();
            back.component(kind).unwrap().write(kind, &mut b);
            assert_eq!(&b.data, bytes, "component {kind}");
        }
    }

    #[test]
    fn a_tag_slot_display_is_a_bare_tag_id_for_26_2() {
        use crate::packets::recipes::{HolderSet, SlotDisplay};
        let round = |d: &SlotDisplay| {
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ids(), Way::Down);
                d.write(&mut w);
            }
            let _up = ids::renumbering(ids(), Way::Up);
            SlotDisplay::read(&mut Reader::new(&w.data)).unwrap()
        };
        let tag = SlotDisplay::Tag(HolderSet::Tag("minecraft:planks".into()));
        assert_eq!(round(&tag), tag);
        // Items have no TagSlotDisplay form in 26.2: a composite of the items.
        let items = SlotDisplay::Tag(HolderSet::Items(vec![1012]));
        assert_eq!(
            round(&items),
            SlotDisplay::Composite(vec![SlotDisplay::Item(1012)])
        );
    }
}
