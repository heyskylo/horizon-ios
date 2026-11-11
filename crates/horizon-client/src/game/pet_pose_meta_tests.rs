//! A cat's chest/bed pose is `TamableAnimal.DATA_FLAGS_ID` bit 1, the same
//! bit `SitWhenOrderedToGoal` writes: what the host's cat goal sets is what
//! the other players' clients draw, and the order they never learn stays
//! behind on the host.
use super::super::lan;
use super::apply;
use horizon_client::mobs::{Mob, MobTypes, tame};
use horizon_core::geometry::Vec3;
use horizon_protocol::entity_data::Value;
use horizon_protocol::registry::Builtin;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION)
}

/// A cat as a client spawns it (None without the prepared resources).
fn cat() -> Option<Mob> {
    let types = MobTypes::load(&root()).ok()?;
    let kind = std::sync::Arc::new(types.types.get("minecraft:cat")?.clone());
    let mut m = Mob::new(1, kind, Vec3::ZERO, 0.0);
    super::client_defaults(&mut m);
    Some(m)
}

/// A hub with no players and empty registries, for `mob_meta`.
fn meta_hub() -> horizon_client::lan::hub::Hub {
    use horizon_client::lan::{Lan, NetConfig, chunks::ChunkEncoder, data, hub};
    let sync = std::sync::Arc::new(data::SyncData::empty());
    let config = NetConfig {
        announce: false,
        ..NetConfig::loopback(0, "CatPoseMetaTest".into(), 2)
    };
    let lan = Lan::start_with(sync.clone(), config).unwrap();
    let encoder = ChunkEncoder::new(
        [] as [(std::sync::Arc<str>, bool); 0],
        &Builtin::default(),
        &[],
        &[],
    );
    let hub_config = hub::HubConfig {
        default_game_mode: 0,
        guest_commands: false,
        force_game_mode: false,
        hardcore: false,
        view_distance: 10,
        simulation_distance: 10,
        dimension: "minecraft:overworld".into(),
        allow_flight: false,
        seed: 0,
        max_players: 20,
        integrated: false,
        levels: Default::default(),
    };
    hub::Hub::new(lan, sync, encoder, hub_config)
}

/// What the host sends for `m`, and the flags byte among it.
fn sent(m: &Mob, hub: &horizon_client::lan::hub::Hub) -> (Vec<(u8, i32, Value)>, Option<i8>) {
    let (mut meta, mut defaults) = (Vec::new(), Vec::new());
    lan::mob_meta(m, hub.sync_data(), None, 0, &mut meta, &mut defaults);
    let flags = meta
        .iter()
        .find(|(index, _, _)| *index == 18)
        .and_then(|(_, _, v)| match v {
            Value::Byte(b) => Some(*b),
            _ => None,
        });
    (meta, flags)
}

#[test]
fn a_cat_on_a_chest_sits_for_the_other_players_too() {
    let Some(mut host) = cat() else {
        return;
    };
    let hub = meta_hub();
    // Tamed and ordered to sit, but in water: the pose is still off, so the
    // other players see it standing (bit 1 clear, tame in bit 2).
    host.extra.tame = true;
    host.extra.sitting = true;
    let (ordered, ordered_flags) = sent(&host, &hub);
    assert_eq!(ordered_flags, Some(4), "only the order, no pose");
    // The chest goal writes the pose; now the bit goes out.
    host.extra.sitting_pose = true;
    let (on_chest, on_chest_flags) = sent(&host, &hub);
    assert_eq!(on_chest_flags, Some(5), "the pose and the tame flag");

    for (sent, sits) in [(ordered, false), (on_chest, true)] {
        let mut client = cat().unwrap();
        apply(&mut client, &sent, &Builtin::default(), 99);
        assert!(client.extra.tame, "the tame flag reached the client");
        assert_eq!(
            tame::cat_sitting(&client),
            sits,
            "the client draws the host's pose"
        );
        assert!(
            !client.extra.sitting,
            "the order stays on the host: bit 1 is the pose"
        );
    }
}
