use super::*;
use std::cell::Cell;

fn vertex(pos: [f32; 3]) -> crate::mesher::Vertex {
    crate::mesher::Vertex {
        pos,
        uv: [0.0; 2],
        layer: 0,
        color: u32::MAX,
        light: 0,
        normal: [0; 4],
        glint_uv: [0.0; 2],
    }
}

fn bits(bounds: ([f64; 3], [f64; 3])) -> ([u64; 3], [u64; 3]) {
    (bounds.0.map(f64::to_bits), bounds.1.map(f64::to_bits))
}

fn per_vertex_reference(
    camera: [f64; 3],
    vertices: &[crate::mesher::Vertex],
    start: usize,
) -> Option<([f64; 3], [f64; 3])> {
    let points = vertices.get(start..)?;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    if points.is_empty() {
        return None;
    }
    for vertex in points {
        for axis in 0..3 {
            let value = f64::from(vertex.pos[axis]) + camera[axis];
            if !value.is_finite() {
                return None;
            }
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    Some((min.map(|value| value - 0.05), max.map(|value| value + 0.05)))
}

#[test]
fn bounds_match_per_vertex_reference_bit_for_bit() {
    let camera = [12.25, -64.5, 31.75];
    let vertices = [
        vertex([500.0, 500.0, 500.0]),
        vertex([-1.25, 2.5, -0.125]),
        vertex([3.75, -4.5, 1.0]),
        vertex([0.5, 0.25, 8.0]),
    ];
    let actual = Culler::bounds(camera, &vertices, 1).unwrap();
    let expected = per_vertex_reference(camera, &vertices, 1).unwrap();
    assert_eq!(bits(actual), bits(expected));

    let signed_zero_vertices = [vertex([0.0; 3]), vertex([-0.0; 3])];
    let actual = Culler::bounds([0.0; 3], &signed_zero_vertices, 0).unwrap();
    let expected = per_vertex_reference([0.0; 3], &signed_zero_vertices, 0).unwrap();
    assert_eq!(bits(actual), bits(expected));
}

#[test]
fn bounds_fail_open_for_empty_and_non_finite_geometry() {
    assert_eq!(Culler::bounds([0.0; 3], &[], 0), None);
    assert_eq!(Culler::bounds([0.0; 3], &[vertex([1.0; 3])], 1), None);

    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            Culler::bounds([0.0; 3], &[vertex([value, 0.0, 0.0])], 0),
            None
        );
    }
    assert_eq!(
        Culler::bounds(
            [0.0; 3],
            &[vertex([1.0; 3]), vertex([0.0, f32::NAN, 0.0])],
            0
        ),
        None
    );

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            Culler::bounds([value, 0.0, 0.0], &[vertex([1.0; 3])], 0),
            None
        );
    }
}

#[test]
fn bounds_handle_a_single_vertex_and_nonzero_start() {
    let vertices = [vertex([-100.0; 3]), vertex([1.25, -2.5, 4.0])];
    let actual = Culler::bounds([10.0, 20.0, 30.0], &vertices, 1).unwrap();
    assert_eq!(
        bits(actual),
        bits(([11.2, 17.45, 33.95], [11.3, 17.55, 34.05]))
    );
}

#[test]
fn completed_mesh_culling_preserves_prior_geometry_and_protruding_parts() {
    let prior = vertex([0.0; 3]);
    let body = [vertex([8.0, -1.0, -1.0]), vertex([10.0, 1.0, 1.0])];
    let wall = |p: [i32; 3]| p[0] == 4 && (-1..=0).contains(&p[1]);
    let mut hidden = vec![prior];
    hidden.extend(body);
    assert!(Culler::new([0.0; 3], 1_000).cull_vertices(&mut hidden, 1, wall));
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].pos, prior.pos);

    // A held item or animated part beyond the wall keeps the whole model.
    let mut exposed = vec![prior];
    exposed.extend(body);
    exposed.push(vertex([8.0, 5.0, 0.0]));
    let before = exposed.clone();
    assert!(!Culler::new([0.0; 3], 1_000).cull_vertices(&mut exposed, 1, wall));
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&exposed),
        bytemuck::cast_slice::<_, u8>(&before)
    );
    assert!(!Culler::new([0.0; 3], 0).cull_vertices(&mut exposed, 1, |_| panic!()));
}

#[test]
fn opaque_walls_hide_bounds_in_all_directions() {
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut min = [-2.0; 3];
            let mut max = [2.0; 3];
            min[axis] = if sign > 0.0 { 8.0 } else { -10.0 };
            max[axis] = if sign > 0.0 { 10.0 } else { -8.0 };
            let wall = if sign > 0.0 { 4 } else { -5 };
            assert!(Culler::new([0.0; 3], 1_000).occluded(min, max, |p| p[axis] == wall));
        }
    }
}

#[test]
fn windows_slits_and_partial_walls_stay_visible() {
    let min = [8.0, -4.0, -4.0];
    let max = [10.0, 4.0, 4.0];
    for opening in 0..3 {
        assert!(!Culler::new([0.0; 3], 1_000).occluded(min, max, |p| {
            p[0] == 4
                && match opening {
                    0 => !(p[1] == 1 && p[2] == 1),
                    1 => p[1] != 1,
                    _ => p[1] < 0,
                }
        }));
    }
    assert!(!Culler::new([0.0; 3], 1_000).occluded(min, max, |_| false));
}

#[test]
fn budgets_are_shared_and_exhaustion_fails_open() {
    let culler = Culler::new([0.0; 3], 3);
    let calls = Cell::new(0);
    for _ in 0..2 {
        assert!(!culler.occluded([8.0; 3], [10.0; 3], |_| {
            calls.set(calls.get() + 1);
            false
        }));
    }
    assert_eq!(calls.get(), 3);
    assert!(!Culler::new([0.0; 3], 0).occluded([8.0; 3], [10.0; 3], |_| panic!()));
    assert!(!Culler::new([0.0; 3], 1).occluded([8.0; 3], [10.0; 3], |_| true));
}

#[test]
fn camera_inside_invalid_and_target_crossing_wall_stay_visible() {
    let culler = Culler::new([0.0; 3], 1_000);
    assert!(!culler.occluded([-1.0; 3], [1.0; 3], |_| panic!()));
    assert!(!culler.occluded([f64::NAN; 3], [1.0; 3], |_| panic!()));
    assert!(!culler.occluded([2.0; 3], [1.0; 3], |_| panic!()));
    assert!(!culler.occluded([4.0, -1.0, -1.0], [8.0, 1.0, 1.0], |p| p[0] == 4));
    assert!(!Culler::new([f64::INFINITY; 3], 1_000).occluded([8.0; 3], [10.0; 3], |_| panic!()));
    assert!(!Culler::new([f64::MAX; 3], 1_000).occluded([-f64::MAX; 3], [-1.0; 3], |_| panic!()));
}
