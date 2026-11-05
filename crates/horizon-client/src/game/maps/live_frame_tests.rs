use super::*;
use crate::game::{Audio, Game, Options, Progress, Settings, commands::Target, load_resources};

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn live_frame_item_adoption_detaches_only_a_changed_old_stack() {
    frame_adoption(false);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn live_glow_frame_adoption_keeps_type_without_saved_id() {
    frame_adoption(true);
}

fn frame_adoption(glow_first: bool) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(
            game.motion,
            horizon_core::geometry::Vec3::new(4.5, 100.0, 4.5),
        )
        .unwrap(),
    );
    game.creative = false;
    let empty = ItemStack::new("minecraft:map", 1);
    *game.held_mut() = Some(empty.clone());
    assert!(game.use_empty_map(&empty));
    let stack = game.held().unwrap().clone();
    assert_eq!(map_id(&stack), Some(0));
    for (n, kind) in [
        (1, "minecraft:item_frame"),
        (2, "minecraft:glow_item_frame"),
    ] {
        let tag = Tag::compound([
            ("id", Tag::String(kind.into())),
            ("UUID", Tag::IntArray(vec![317, 1, 0, n])),
            ("block_pos", Tag::IntArray(vec![10 + n, 101, 5])),
            ("Facing", Tag::Byte(5)),
            ("ItemRotation", Tag::Byte(3)),
            ("Item", horizon_client::save::item_to_nbt(&stack)),
        ]);
        game.load_item_frame(tag, Some([317, 1, 0, n]));
    }
    let mut ids: Vec<_> = game.item_frames.iter().map(|f| f.id).collect();
    if glow_first {
        ids.reverse();
    }
    for &id in &ids {
        let frame = game
            .item_frames
            .iter()
            .find(|f| f.id == id)
            .unwrap()
            .clone();
        game.map(0).unwrap().tick_framed(
            1,
            0,
            false,
            false,
            Framed {
                pos: frame.pos,
                facing: frame.facing,
                id: id as i32,
            },
        );
    }
    assert_eq!(game.map(0).unwrap().frames.len(), 2);
    for &id in &ids {
        for change in ["rotation", "unchanged", "components", "count", "empty"] {
            let frame = game
                .item_frames
                .iter()
                .find(|f| f.id == id)
                .unwrap()
                .clone();
            let mut tag = horizon_client::entity_nbt::item_frame_to_nbt(
                &frame,
                *game.persist.uuids.get(&id).unwrap(),
                None,
            );
            let Tag::Compound(fields) = &mut tag else {
                unreachable!()
            };
            // Live command data omits the entity id before adoption.
            fields.remove(&"id".into());
            match change {
                "rotation" => {
                    fields.insert("ItemRotation".into(), Tag::Byte(4));
                }
                "components" | "count" => {
                    let mut next = stack.clone();
                    if change == "count" {
                        next.count = 2;
                    } else {
                        next.damage = 1;
                    }
                    fields.insert("Item".into(), horizon_client::save::item_to_nbt(&next));
                }
                "empty" => {
                    fields.remove(&"Item".into());
                }
                _ => {}
            }
            game.apply_entity_nbt(Target::Frame(id), tag).unwrap();
            assert_eq!(
                game.item_frames.iter().find(|f| f.id == id).unwrap().kind(),
                frame.kind(),
                "{change}: immutable frame type"
            );
            let retained = matches!(change, "unchanged" | "rotation");
            let map = game.map(0).unwrap();
            assert_eq!(
                map.frames.iter().any(|f| f.entity_id == id as i32),
                retained,
                "{change}: old saved frame association"
            );
            assert_eq!(
                map.decorations
                    .iter()
                    .any(|d| d.key == Key::Frame(id as i32)),
                retained,
                "{change}: old frame decoration"
            );
            let other = ids.iter().copied().find(|&other| other != id).unwrap();
            assert!(map.frames.iter().any(|f| f.entity_id == other as i32));
            // Re-establish the actual production map association for the next independent write.
            let frame = game.item_frames.iter_mut().find(|f| f.id == id).unwrap();
            frame.item = Some(stack.clone());
            let f = Framed {
                pos: frame.pos,
                facing: frame.facing,
                id: id as i32,
            };
            game.map(0).unwrap().tick_framed(1, 0, false, false, f);
        }
    }
}
