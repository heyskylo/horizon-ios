use super::*;
use crate::game::entity::RenderEntityBounds;
use horizon_client::mobs::{Mob, MobData, MobTypes, Species, render};
use horizon_core::geometry::Vec3;
use std::path::Path;

#[test]
fn vanilla_models_and_animations_stay_inside_the_precheck_envelope() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3");
    if !root.join("entity-models.json").exists() {
        eprintln!("no prepared resources: skipped model coverage");
        return;
    }
    let types = MobTypes::load(&root).unwrap();
    let data = MobData {
        states: Vec::new(),
        placements: Default::default(),
        heightmaps: Default::default(),
        animations: MobData::load_animations(&root),
        villager_hats: Default::default(),
        cem: Default::default(),
        random_textures: Default::default(),
        emissive: Default::default(),
        blinks: Default::default(),
        biome_tags: Default::default(),
        entity_tags: Default::default(),
        pois: Default::default(),
        species_placements: Default::default(),
    };
    let camera = glam::DVec3::new(3.0, 62.0, -4.0);
    let mut checked = 0;
    for kind in types.types.values() {
        for sample in 0..12 {
            let mut mob = Mob::new(1, Arc::new(kind.clone()), Vec3::new(1.5, 64.0, 3.25), 37.0);
            mob.previous = Vec3::new(1.25, 64.25, 3.0);
            mob.age = sample * 37;
            mob.limb_speed = 0.8;
            mob.limb_position = sample as f32 * 1.7;
            mob.head_pitch = if sample % 2 == 0 { 85.0 } else { -85.0 };
            mob.extra.age = if sample >= 6 { -100 } else { 0 };
            mob.death_time = if sample % 3 == 0 { 10 } else { 0 };
            mob.extra.flag = sample % 2 == 0;
            mob.extra.size = match mob.species() {
                Species::Slime | Species::MagmaCube | Species::SulfurCube | Species::Phantom => {
                    if sample % 2 == 0 {
                        1
                    } else {
                        4
                    }
                }
                _ => mob.extra.size,
            };
            if sample % 3 == 1 {
                mob.custom_name = Some("Dinnerbone".into());
            }
            for partial in [0.0, 0.4, 1.0] {
                let bounds = mob.render_bounds(f64::from(partial));
                let lo = bounds.min_position();
                let hi = bounds.max_position();
                let lo = [lo.x, lo.y, lo.z];
                let hi = [hi.x, hi.y, hi.z];
                let mut vertices = Vec::new();
                render::render_with(
                    &mob,
                    partial,
                    camera,
                    &|_| Some([0.0, 0.0, 1.0, 1.0]),
                    0xF0F0,
                    &data,
                    &[],
                    &mut vertices,
                );
                for vertex in &vertices {
                    for axis in 0..3 {
                        let world = f64::from(vertex.pos[axis]) + camera[axis];
                        assert!(
                            world >= lo[axis] - 1.0e-4 && world <= hi[axis] + 1.0e-4,
                            "{} sample {sample} partial {partial}: {world} outside {lo:?}..{hi:?}",
                            kind.id,
                        );
                    }
                }
                checked += usize::from(!vertices.is_empty());
            }
        }
    }
    assert!(
        checked > 1_000,
        "insufficient real model coverage: {checked}"
    );
}

#[test]
fn a_hole_inside_a_coarse_envelope_remains_visible() {
    let camera = [0.0; 3];
    let min = [7.95, -3.0, -3.0];
    let max = [10.05, 3.0, 3.0];
    assert!(OcclusionCuller::new(camera, 8192).occluded(min, max, |p| p[0] == 4));
    assert!(
        !OcclusionCuller::new(camera, 8192).occluded(min, max, |p| p[0] == 4 && p != [4, 1, 0])
    );
    assert!(!OcclusionCuller::new(camera, 0).occluded(min, max, |p| p[0] == 4));
}

#[test]
fn hidden_cache_entries_require_the_same_camera_and_bounds() {
    let mut culler = AsyncOcclusionCuller::new().unwrap();
    let bounds = Aabb::new(Vec3::new(8.0, -1.0, -1.0), Vec3::new(10.0, 1.0, 1.0));
    let original = culler.query_entity_bounds(bounds).unwrap();
    culler.query_results.push(CachedQueryResult {
        key: original,
        stamp: culler.stamp,
        camera_cell: Some([0; 3]),
        proof: Some(OcclusionProof {
            sections: Vec::new(),
            chunks: Vec::new(),
        }),
        last_used_frame: Cell::new(0),
    });
    assert!(matches!(
        culler.query_result(&original, QuerySource::EntityAabb),
        Some((true, _))
    ));
    let moved = culler
        .query_entity_bounds(bounds.move_by(Vec3::new(0.01, 0.0, 0.0)))
        .unwrap();
    assert!(
        culler
            .query_result(&moved, QuerySource::EntityAabb)
            .is_none()
    );
    culler.camera[0] = 0.01;
    let moved_camera = culler.query_entity_bounds(bounds).unwrap();
    assert!(
        culler
            .query_result(&moved_camera, QuerySource::EntityAabb)
            .is_none()
    );
}

#[test]
fn resetting_benchmark_counters_preserves_the_profiling_choice() {
    let culler = AsyncOcclusionCuller::new().unwrap();
    for enabled in [false, true] {
        culler.measure_bounds_timing.set(enabled);
        culler.reset_stats();
        assert_eq!(culler.stats_value()["profiling_enabled"], enabled);
    }
}

#[test]
fn inactive_answers_expire_while_frequently_used_proofs_stay_cached() {
    let mut culler = AsyncOcclusionCuller::new().unwrap();
    let bounds = |x| Aabb::new(Vec3::new(x, -1.0, -1.0), Vec3::new(x + 1.0, 1.0, 1.0));
    let hot = culler.query_entity_bounds(bounds(8.0)).unwrap();
    let cold = culler.query_entity_bounds(bounds(12.0)).unwrap();
    let fresh = culler.query_entity_bounds(bounds(16.0)).unwrap();
    for key in [hot, cold] {
        culler.query_results.push(CachedQueryResult {
            key,
            stamp: culler.stamp,
            camera_cell: Some([0; 3]),
            proof: Some(OcclusionProof {
                sections: Vec::new(),
                chunks: Vec::new(),
            }),
            last_used_frame: Cell::new(0),
        });
    }
    culler.frame = 10;
    assert!(matches!(
        culler.query_result(&hot, QuerySource::EntityAabb),
        Some((true, _))
    ));
    culler.cache_query_results(&mut CullResults {
        stamp: culler.stamp,
        camera: culler.camera.map(f64::to_bits),
        generation: 0,
        cancelled: false,
        hidden: Vec::new(),
        visible: vec![fresh],
        scratch_queries: Vec::new(),
        worker_ns: 0,
        hidden_entities: 0,
        visible_entities: 1,
        deferred_entities: 0,
    });
    assert!(matches!(
        culler.query_result(&hot, QuerySource::EntityAabb),
        Some((true, _))
    ));
    assert!(
        culler
            .query_result(&cold, QuerySource::EntityAabb)
            .is_none()
    );
    assert!(matches!(
        culler.query_result(&fresh, QuerySource::EntityAabb),
        Some((false, _))
    ));
}
