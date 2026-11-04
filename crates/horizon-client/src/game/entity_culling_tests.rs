use super::*;

#[allow(dead_code)]
fn vertex(pos: [f32; 3]) -> Vertex {
    Vertex {
        pos,
        uv: [0.0; 2],
        layer: 0,
        color: u32::MAX,
        light: 0,
        normal: [0; 4],
        glint_uv: [0.0; 2],
    }
}

/// `Entity.setViewScale`: the entity distance option times the render
/// distance's boost — 1 at eight chunks, 1.5 at twelve, 2.5 at twenty.
#[test]
fn view_scale_follows_the_render_distance() {
    assert_eq!(entity_view_scale(4, 1.0), 1.0);
    assert_eq!(entity_view_scale(8, 1.0), 1.0);
    assert_eq!(entity_view_scale(12, 1.0), 1.5);
    assert_eq!(entity_view_scale(20, 1.0), 2.5);
    assert_eq!(entity_view_scale(32, 1.0), 2.5);
    assert_eq!(entity_view_scale(12, 0.5), 0.75);
}

/// `Entity.shouldRenderAtSqrDistance`: a chicken's box (0.4 by 0.7 by
/// 0.4) is drawn up to 48 blocks away at the vanilla defaults.
#[test]
fn small_mobs_use_the_mean_edge_for_render_distance() {
    let range = default_range(glam::DVec3::new(0.4, 0.7, 0.4), entity_view_scale(12, 1.0));
    assert_eq!(range, 48.0);
}

/// Dropped items keep their 0.25-block box, so their default range scales
/// with the render distance but never exceeds the vanilla clamp.
#[test]
fn dropped_items_use_their_small_render_box() {
    let item = glam::DVec3::splat(0.25);
    assert_eq!(default_range(item, entity_view_scale(12, 1.0)), 24.0);
    assert_eq!(default_range(item, entity_view_scale(16, 1.0)), 32.0);
    assert_eq!(default_range(item, entity_view_scale(32, 1.0)), 40.0);
}

#[test]
fn invalid_render_sizes_fail_open_to_one_block() {
    let range = default_range(
        glam::DVec3::new(f64::NAN, 0.7, 0.4),
        entity_view_scale(12, 1.0),
    );
    assert_eq!(range, 96.0);
}

#[test]
fn culling_decisions_only_hide_after_a_confirmed_occlusion() {
    assert!(!EntityCullDecision::Bypass.hides_entity());
    assert!(!EntityCullDecision::CheckVertices.hides_entity());
    assert!(!EntityCullDecision::Pending.hides_entity());
    assert!(EntityCullDecision::Hidden.hides_entity());
}

#[test]
fn coarse_aabb_and_mesh_paths_agree_for_a_complete_wall() {
    let camera = [0.0; 3];
    let bounds = Aabb::new(Vec3::new(8.0, -1.0, -1.0), Vec3::new(10.0, 1.0, 1.0));
    let wall = |pos: [i32; 3]| pos[0] == 4;
    assert!(
        horizon_client::voxel_occlusion::Culler::new(camera, 1_000).hidden_bounds(bounds, wall)
    );

    let mesh = [vertex([8.0, -1.0, -1.0]), vertex([10.0, 1.0, 1.0])];
    assert!(horizon_client::voxel_occlusion::Culler::new(camera, 1_000).hidden(&mesh, 0, wall));
}

#[test]
fn partial_wall_and_budget_exhaustion_fail_open() {
    let bounds = [8.0, -4.0, -4.0];
    let max = [10.0, 4.0, 4.0];
    let culler = horizon_client::voxel_occlusion::Culler::new([0.0; 3], 1_000);
    assert!(!culler.occluded(bounds, max, |pos| {
        pos[0] == 4 && !(pos[1] == 1 && pos[2] == 1)
    }));

    let exhausted = horizon_client::voxel_occlusion::Culler::new([0.0; 3], 1);
    assert!(!exhausted.occluded(bounds, max, |_| true));
}

#[test]
fn mesh_culling_keeps_geometry_before_the_entity_prefix() {
    let prefix = vertex([0.0; 3]);
    let mesh = [vertex([8.0, -1.0, -1.0]), vertex([10.0, 1.0, 1.0])];
    let mut vertices = vec![prefix];
    vertices.extend(mesh);
    let hidden = horizon_client::voxel_occlusion::Culler::new([0.0; 3], 1_000).cull_vertices(
        &mut vertices,
        1,
        |pos| pos[0] == 4,
    );
    assert!(hidden);
    assert_eq!(vertices.len(), 1);
    assert_eq!(vertices[0].pos, prefix.pos);
}
