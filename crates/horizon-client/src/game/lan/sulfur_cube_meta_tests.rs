// `lan.rs`'s tests (included: that file may not grow).

/// `SulfurCube`'s entity data after `AbstractCubeMob.ID_SIZE` (18), in the order
/// the class defines it: `MAX_FUSE` (19, an int, -1 until primed), then
/// `FROM_BUCKET` (20, a boolean). A 26.3 client reads each index with its own
/// serializer, and a boolean at 19 disconnected one (#13721).
#[test]
fn a_sulfur_cube_sends_its_fuse_at_19_and_from_bucket_at_20() {
    use horizon_world::nbt::Tag;
    let root = std::env::temp_dir().join(format!("horizon-sulfur-meta-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("entity-types.json"),
        r#"{"minecraft:sulfur_cube":{"width":0.52,"height":0.52,"eye_height":0.325,"category":"monster","fire_immune":false,"attributes":{"minecraft:max_health":4.0}}}"#,
    )
    .unwrap();
    let types = horizon_client::mobs::MobTypes::load_for_server(&root).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    let kind = std::sync::Arc::new(types.types["minecraft:sulfur_cube"].clone());
    let at = horizon_core::geometry::Vec3::new(0.5, 64.0, 0.5);
    let mut cube = Mob::new(1, kind, at, 0.0);
    let hub = meta_hub("SulfurCubeMetaTest");
    let meta_of = |m: &Mob| {
        let (mut meta, mut defaults) = (Vec::new(), Vec::new());
        super::mob_meta(m, hub.sync_data(), None, 0, &mut meta, &mut defaults);
        (meta, defaults)
    };

    // A fresh cube: both entries are at their defaults.
    let (meta, defaults) = meta_of(&cube);
    assert!(
        defaults.contains(&(19, serializer::INT, Value::Int(-1))),
        "{defaults:?}"
    );
    assert!(
        defaults.contains(&(20, serializer::BOOLEAN, Value::Bool(false))),
        "{defaults:?}"
    );
    assert!(meta.iter().all(|e| e.0 != 19 && e.0 != 20), "{meta:?}");

    // A saved cube, out of a bucket and primed with 40 ticks to go.
    let saved = Tag::compound([("from_bucket", Tag::Byte(1)), ("fuse", Tag::Int(40))]);
    horizon_client::mobs::sulfur_cube::from_nbt(&mut cube, &saved);
    let (meta, _) = meta_of(&cube);
    assert!(
        meta.contains(&(19, serializer::INT, Value::Int(40))),
        "{meta:?}"
    );
    assert!(
        meta.contains(&(20, serializer::BOOLEAN, Value::Bool(true))),
        "{meta:?}"
    );
}
