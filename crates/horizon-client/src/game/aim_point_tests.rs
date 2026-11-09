use super::visible_aim_point;

const EYE: [f64; 3] = [-760.7, 70.27, -450.4];
// A zombie one block behind a top slab: its centre is above the 0.5 gap under the slab.
const LO: [f64; 3] = [-763.0, 70.0, -450.9];
const HI: [f64; 3] = [-762.4, 71.95, -450.3];

/// A top slab filling x in [-762, -761] with its underside at y 70.5: a point is hidden when
/// the line from the eyes is above 70.5 anywhere inside that column.
fn behind_top_slab(p: [f64; 3]) -> bool {
    let y_at = |x: f64| EYE[1] + (p[1] - EYE[1]) * (x - EYE[0]) / (p[0] - EYE[0]);
    p[0] < -762.0 && (y_at(-761.0) > 70.5 || y_at(-762.0) > 70.5)
}

#[test]
fn a_visible_centre_is_kept() {
    let centre = [
        (LO[0] + HI[0]) / 2.0,
        (LO[1] + HI[1]) / 2.0,
        (LO[2] + HI[2]) / 2.0,
    ];
    assert_eq!(visible_aim_point(EYE, LO, HI, |_| false), centre);
}

/// A bottom slab hides everything under y 70.9: the aim goes above it, through the gap over the slab.
#[test]
fn a_mob_behind_a_bottom_slab_is_aimed_at_above_the_slab() {
    let centre = [
        (LO[0] + HI[0]) / 2.0,
        (LO[1] + HI[1]) / 2.0,
        (LO[2] + HI[2]) / 2.0,
    ];
    let p = visible_aim_point(EYE, LO, HI, |pt| pt[1] < 70.9);
    assert!(p[1] >= 70.9, "{p:?} is still under the slab");
    assert!(p[1] > centre[1] - 0.5, "{p:?} is far from the centre");
}

/// With only the centre hidden, two equally near candidates go to the one nearer the eyes.
#[test]
fn an_equally_near_point_on_the_side_of_the_eyes_wins() {
    let eye = [5.0, 70.5, 0.0];
    let (lo, hi) = ([-0.3, 70.0, -0.3], [0.3, 71.9, 0.3]);
    let centre = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let p = visible_aim_point(eye, lo, hi, |pt| pt == centre);
    assert!(p[0] > centre[0], "{p:?} is on the far side from the eyes");
}

#[test]
fn a_mob_seen_only_through_a_gap_is_aimed_at_through_the_gap() {
    let centre = [
        (LO[0] + HI[0]) / 2.0,
        (LO[1] + HI[1]) / 2.0,
        (LO[2] + HI[2]) / 2.0,
    ];
    assert!(behind_top_slab(centre), "the slab hides the centre");
    let p = visible_aim_point(EYE, LO, HI, behind_top_slab);
    assert!(!behind_top_slab(p), "{p:?} is still hidden");
    for k in 0..3 {
        assert!(LO[k] <= p[k] && p[k] <= HI[k], "{p:?} is outside the mob");
    }
    assert!(p[1] < centre[1], "{p:?} is not lower than the centre");
}

#[test]
fn a_fully_hidden_mob_keeps_the_centre() {
    let centre = [
        (LO[0] + HI[0]) / 2.0,
        (LO[1] + HI[1]) / 2.0,
        (LO[2] + HI[2]) / 2.0,
    ];
    assert_eq!(visible_aim_point(EYE, LO, HI, |_| true), centre);
}

#[test]
fn the_choice_is_deterministic() {
    assert_eq!(
        visible_aim_point(EYE, LO, HI, behind_top_slab),
        visible_aim_point(EYE, LO, HI, behind_top_slab)
    );
}
