//! `SulfurCube.mobInteract`'s last step, `Bucketable.bucketMobPickup`: an
//! empty bucket picks up an adult, unprimed sulfur cube, and the held stack
//! becomes `ItemUtils.createFilledResult(held, player, sulfur_cube_bucket)`
//! (tools/test_sulfur_bucket_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::{chunk::Chunk, mobs::Mob};
use horizon_world::nbt::Tag;
use std::sync::Arc;

fn game() -> Game<'static> {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
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
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    game.screen = Screen::Playing;
    game.selected = 0;
    game
}

/// A fresh mob of `kind` in front of the player, alone; returns its id.
fn mob(game: &mut Game<'_>, kind: &str, edit: impl FnOnce(&mut Mob)) -> u32 {
    game.mobs.clear();
    let kind = game.res.mob_types().get(kind).unwrap().clone();
    let mut m = Mob::new(42, kind, Vec3::new(0.5, 64.0, 2.5), 0.0);
    m.no_ai = true;
    edit(&mut m);
    game.mobs.push(m);
    42
}

/// Clicks mob `id` holding `count` of `item` in `mode`: whether the click
/// was used, whether the mob is still there, and the inventory after it.
fn click(
    game: &mut Game<'_>,
    id: u32,
    mode: i32,
    item: &str,
    count: u32,
) -> (bool, bool, Vec<(String, u32)>) {
    game.set_mode_id(mode);
    game.inventory.iter_mut().for_each(|s| *s = None);
    game.inventory[0] = Some(ItemStack::new(item, count));
    let used = game
        .interact_entity(Some(InteractTarget {
            id: id as i32,
            location: [0.0, 1.0, 0.0],
        }))
        .unwrap();
    let there = game.mobs.iter().any(|m| m.id == id && !m.removed);
    let held = game
        .inventory
        .iter()
        .flatten()
        .map(|s| (s.item.to_string(), s.count))
        .collect();
    (used, there, held)
}

fn owned(items: &[(&str, u32)]) -> Vec<(String, u32)> {
    items.iter().map(|(i, n)| (i.to_string(), *n)).collect()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_empty_bucket_picks_up_an_adult_sulfur_cube() {
    let mut game = game();
    // Survival, two buckets: one is filled, the other stays in the hand.
    let id = mob(&mut game, "minecraft:sulfur_cube", |_| {});
    assert_eq!(
        click(&mut game, id, 0, "minecraft:bucket", 2),
        (
            true,
            false,
            owned(&[("minecraft:bucket", 1), ("minecraft:sulfur_cube_bucket", 1)])
        )
    );
    // The last bucket becomes the filled one.
    let id = mob(&mut game, "minecraft:sulfur_cube", |_| {});
    assert_eq!(
        click(&mut game, id, 0, "minecraft:bucket", 1),
        (true, false, owned(&[("minecraft:sulfur_cube_bucket", 1)]))
    );
    // Creative keeps its bucket and gets the filled one.
    let id = mob(&mut game, "minecraft:sulfur_cube", |_| {});
    assert_eq!(
        click(&mut game, id, 1, "minecraft:bucket", 1),
        (
            true,
            false,
            owned(&[("minecraft:bucket", 1), ("minecraft:sulfur_cube_bucket", 1)])
        )
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn no_pickup_without_an_empty_bucket_or_of_a_baby_or_primed_cube() {
    let mut game = game();
    fn plain(_: &mut Mob) {}
    fn baby(m: &mut Mob) {
        m.extra.age = -24000;
    }
    fn primed(m: &mut Mob) {
        m.extra.body = Some("minecraft:tnt".into());
        assert!(horizon_client::mobs::sulfur_cube::prime(m, false));
    }
    let cases: [(&str, fn(&mut Mob), &str); 3] = [
        ("water bucket", plain, "minecraft:water_bucket"),
        ("baby", baby, "minecraft:bucket"),
        ("primed", primed, "minecraft:bucket"),
    ];
    for (case, edit, item) in cases {
        let id = mob(&mut game, "minecraft:sulfur_cube", edit);
        let (_, there, held) = click(&mut game, id, 0, item, 1);
        assert!(there, "{case}: the cube stays");
        assert_eq!(held, owned(&[(item, 1)]), "{case}: the hand is unchanged");
    }
    // A cod still takes a water bucket.
    let id = mob(&mut game, "minecraft:cod", |_| {});
    assert_eq!(
        click(&mut game, id, 0, "minecraft:water_bucket", 1),
        (true, false, owned(&[("minecraft:cod_bucket", 1)]))
    );
}

/// The filled bucket the click left in the inventory.
fn filled(game: &Game<'_>, item: &str) -> ItemStack {
    game.inventory
        .iter()
        .flatten()
        .find(|s| &*s.item == item)
        .cloned()
        .unwrap()
}
fn component(stack: &ItemStack, id: &str) -> Option<Tag> {
    stack.extra.as_ref().and_then(|d| d.other(id)).cloned()
}
fn field(tag: &Option<Tag>, key: &str) -> Option<Tag> {
    match tag {
        Some(Tag::Compound(c)) => c.get(&key.into()).cloned(),
        _ => None,
    }
}

/// Added controls (after the baseline): `saveToBucketTag` writes the live
/// cube's age, age lock and body, and nothing for a cube with no body; a
/// cod's bucket keeps its own data.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_sulfur_bucket_carries_the_live_cube_s_age_lock_and_body() {
    let mut game = game();
    let id = mob(&mut game, "minecraft:sulfur_cube", |m| {
        m.extra.age = 1200;
        m.extra.age_locked = true;
        m.extra.body = Some("minecraft:oak_planks".into());
        m.health = 3.0;
    });
    assert!(click(&mut game, id, 0, "minecraft:bucket", 1).0);
    let bucket = filled(&game, "minecraft:sulfur_cube_bucket");
    let data = component(&bucket, "minecraft:bucket_entity_data");
    assert_eq!(field(&data, "age"), Some(Tag::Int(1200)));
    assert_eq!(field(&data, "age_locked"), Some(Tag::Byte(1)));
    assert_eq!(field(&data, "Health"), Some(Tag::Float(3.0f32.to_bits())));
    assert_eq!(field(&data, "NoAI"), Some(Tag::Byte(1)));
    assert_eq!(
        field(&component(&bucket, "minecraft:sulfur_cube_content"), "id"),
        Some(Tag::String("minecraft:oak_planks".into()))
    );
    // No body, no content; an unlocked adult.
    let id = mob(&mut game, "minecraft:sulfur_cube", |_| {});
    assert!(click(&mut game, id, 0, "minecraft:bucket", 1).0);
    let bucket = filled(&game, "minecraft:sulfur_cube_bucket");
    let data = component(&bucket, "minecraft:bucket_entity_data");
    assert_eq!(
        (field(&data, "age"), field(&data, "age_locked")),
        (Some(Tag::Int(0)), Some(Tag::Byte(0)))
    );
    assert_eq!(component(&bucket, "minecraft:sulfur_cube_content"), None);
    // A cod's bucket has no sulfur fields.
    let id = mob(&mut game, "minecraft:cod", |_| {});
    assert!(click(&mut game, id, 0, "minecraft:water_bucket", 1).0);
    let bucket = filled(&game, "minecraft:cod_bucket");
    let data = component(&bucket, "minecraft:bucket_entity_data");
    assert_eq!(
        (field(&data, "age"), field(&data, "age_locked")),
        (None, None)
    );
    assert_eq!(component(&bucket, "minecraft:sulfur_cube_content"), None);
}
