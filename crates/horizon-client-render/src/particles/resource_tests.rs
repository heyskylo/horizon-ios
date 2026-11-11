use super::*;

fn resource_test_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "horizon-particle-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn test_rect() -> Rect {
    Rect {
        u0: 0.25,
        v0: 0.5,
        u1: 0.75,
        v1: 0.875,
        w: 16,
        h: 16,
    }
}

#[test]
fn vanilla_particle_description_resolves_foreign_and_default_namespace_sprites() {
    let root = resource_test_dir("namespaces");
    let particles = root.join("assets/minecraft/particles");
    std::fs::create_dir_all(&particles).unwrap();
    for (reference, atlas_key) in [
        ("gems:spark", "gems:particle/spark"),
        (":spark", "particle/spark"),
        ("minecraft:spark", "particle/spark"),
        ("spark", "particle/spark"),
        ("gems:trails/bright", "gems:particle/trails/bright"),
    ] {
        std::fs::write(
            particles.join("smoke.json"),
            serde_json::to_vec(&serde_json::json!({"textures": [reference]})).unwrap(),
        )
        .unwrap();
        let atlas = HashMap::from([(atlas_key.to_owned(), test_rect())]);
        let mut engine = ParticleEngine::new(&root, &atlas, &HashSet::new());
        let particle = engine
            .make(
                &ParticleOptions::simple(ParticleType::Smoke),
                [0.0, 0.0, -3.0],
                [0.0; 3],
                7,
            )
            .unwrap();
        println!(
            "reference={reference}; selected={}",
            engine.snapshot(&particle).quad.unwrap().sprite
        );
        engine.particles.push(particle);
        let mut vertices = Vec::new();
        engine.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0xf0, &mut vertices);
        assert_eq!(vertices.len(), 4, "particle with {reference} must render");
        assert!(vertices.iter().all(|vertex| {
            (0.25..=0.75).contains(&vertex.uv[0]) && (0.5..=0.875).contains(&vertex.uv[1])
        }));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_native_particle_id_keeps_sprite_frames_and_declared_behaviour() {
    let root = resource_test_dir("nested");
    let particles = root.join("assets/gems/particles/trails");
    std::fs::create_dir_all(&particles).unwrap();
    std::fs::write(
        particles.join("spark.json"),
        br#"{"textures":["gems:trails/bright","spark"],"horizon:behaviour":{
            "velocity":[0,0.5,0],"lifetime":37,"animation":"loop","frame_ticks":2
        }}"#,
    )
    .unwrap();
    let atlas = HashMap::from([
        ("gems:particle/trails/bright".into(), test_rect()),
        ("particle/spark".into(), test_rect()),
    ]);
    let mut engine = ParticleEngine::new(&root, &atlas, &HashSet::new());
    let particle = engine.make(
        &ParticleOptions::named("gems:trails/spark"),
        [0.0, 0.0, -3.0],
        [0.0; 3],
        7,
    );
    println!("nested gems:trails/spark created={}", particle.is_some());
    let mut particle = particle.expect("nested resource paths are part of the particle id");
    assert_eq!(particle.velocity, [0.0, 0.5, 0.0]);
    assert_eq!(particle.lifetime, 37);
    assert_eq!(particle.frame_ticks, 2);
    engine.set_sprite_frame(&mut particle, 0);
    assert_eq!(
        engine.snapshot(&particle).quad.unwrap().sprite,
        "gems:particle/trails/bright"
    );
    engine.set_sprite_frame(&mut particle, 1);
    assert_eq!(engine.snapshot(&particle).quad.unwrap().sprite, "spark");
    engine.particles.push(particle);
    let mut vertices = Vec::new();
    engine.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0xf0, &mut vertices);
    assert_eq!(vertices.len(), 4);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resource_reload_clears_old_live_pending_emitters_and_firework_groups() {
    let root = resource_test_dir("live-set");
    let vanilla = root.join("assets/minecraft/particles");
    std::fs::create_dir_all(&vanilla).unwrap();
    std::fs::write(vanilla.join("smoke.json"), br#"{"textures":["smoke"]}"#).unwrap();
    let atlas = HashMap::from([
        ("particle/smoke".into(), test_rect()),
        ("gems:particle/new".into(), test_rect()),
    ]);
    let mut engine = ParticleEngine::new(&root, &atlas, &HashSet::new());
    let smoke = engine
        .make(
            &ParticleOptions::simple(ParticleType::Smoke),
            [0.0; 3],
            [0.0; 3],
            7,
        )
        .unwrap();
    engine.particles.push(smoke.clone());
    engine.pending.push(smoke);
    engine.tracking_emitter_for(
        ParticleOptions::simple(ParticleType::Smoke),
        TrackingTarget::LocalPlayer,
        TrackingBody {
            feet: [0.0; 3],
            width: 0.6,
            height: 1.8,
        },
        4,
    );
    engine.fireworks(
        [0.0; 3],
        [0.0; 3],
        vec![FireworkExplosion::default()],
        [0.0; 3],
        true,
    );
    assert!(!engine.particles.is_empty());
    assert!(!engine.pending.is_empty());
    assert!(!engine.emitters.is_empty());
    assert!(!engine.starters.is_empty());
    engine.reload(&root, &atlas, &HashSet::new());
    // Pinned 26.3 ParticleResources' apply callback clears live groups,
    // queued particles, tracking emitters and group counts before rebinding.
    println!(
        "after reload: live={}, pending={}, emitters={}, firework_starters={}",
        engine.len(),
        engine.pending.len(),
        engine.emitters.len(),
        engine.starters.len()
    );
    assert!(engine.is_empty());
    assert!(engine.pending.is_empty());
    assert!(engine.emitters.is_empty());
    assert!(engine.starters.is_empty());
    let mut vertices = Vec::new();
    engine.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0xf0, &mut vertices);
    assert!(vertices.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Clone, Debug)]
struct NoopController;
impl ParticleController for NoopController {
    fn tick(&mut self, particle: &mut Particle, _: &ParticleLevel<'_>) -> ParticleUpdate {
        particle.advance_age();
        ParticleUpdate::default()
    }
}

#[test]
fn prepared_particle_reload_needs_no_files_and_discards_old_controlled_handles() {
    let root = resource_test_dir("snapshot");
    let mods = root.join("assets/gems/particles/trails");
    let vanilla = root.join("assets/minecraft/particles");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::create_dir_all(&vanilla).unwrap();
    std::fs::write(mods.join("spark.json"), br#"{"textures":["gems:spark"]}"#).unwrap();
    std::fs::write(vanilla.join("smoke.json"), br#"{"textures":["smoke"]}"#).unwrap();
    let mut atlas = HashMap::from([
        ("particle/smoke".into(), test_rect()),
        ("gems:particle/spark".into(), test_rect()),
    ]);
    let mut engine = ParticleEngine::new(&root, &atlas, &HashSet::new());
    let controlled = engine
        .controlled("gems:trails/spark", [0.0; 3], false, NoopController)
        .unwrap();
    engine.particles.push(controlled.clone());
    engine.pending.push(controlled);
    let smoke = engine
        .make(
            &ParticleOptions::simple(ParticleType::Smoke),
            [0.0; 3],
            [0.0; 3],
            7,
        )
        .unwrap();
    engine.particles.push(smoke);
    let prepared = PreparedParticles::load(&root);
    std::fs::remove_dir_all(&root).unwrap();
    let changed_rect = Rect {
        u0: 0.0,
        u1: 0.125,
        ..test_rect()
    };
    atlas.insert("particle/smoke".into(), changed_rect);
    engine.reload_prepared(&prepared, &atlas, &HashSet::new());
    assert!(engine.particles.is_empty());
    assert!(engine.pending.is_empty());
    let fresh = engine
        .make(
            &ParticleOptions::simple(ParticleType::Smoke),
            [0.0; 3],
            [0.0; 3],
            7,
        )
        .unwrap();
    engine.particles.push(fresh);
    let mut vertices = Vec::new();
    engine.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0xf0, &mut vertices);
    assert_eq!(vertices.len(), 4);
    assert!(
        vertices
            .iter()
            .all(|vertex| (0.0..=0.125).contains(&vertex.uv[0]))
    );
    // Prepared native behavior and nested sprite sets remain usable after
    // deleting the entire root that was read by the preparation worker.
    assert!(
        engine
            .controlled("gems:trails/spark", [0.0; 3], false, NoopController)
            .is_some()
    );
}
