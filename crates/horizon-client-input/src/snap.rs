//! Where the controller's menu cursor settles once the stick is let go, and
//! how it gets there. Every rule's number lives in [`Tuning`], so the feel can
//! be adjusted in one place.

/// The snapping engine's knobs, in GUI pixels and seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// A let-go cursor settles on a target this close to it.
    pub radius: f32,
    /// How long the glide onto a target takes.
    pub glide_seconds: f32,
    /// Movement shorter than this is a wobble: the cursor stays where it was.
    pub min_travel: f32,
    /// A flick that ends still nearest its starting target commits to the
    /// neighbour it was heading for once it covered this fraction of the way.
    pub commit: f32,
    /// How far off the flick's direction a neighbour may lie (cosine of the
    /// angle: 0.7 is about 45 degrees).
    pub cone: f32,
    /// The pointer's speed over a slot or control, as a fraction of its
    /// speed elsewhere (1 leaves it unchanged).
    pub over_target_speed: f32,
}

pub const TUNING: Tuning = Tuning {
    radius: 16.0,
    glide_seconds: 0.025,
    min_travel: 1.5,
    commit: 0.15,
    cone: 0.7,
    over_target_speed: 1.0,
};

type Point = (f32, f32);

fn distance(a: Point, b: Point) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// The target nearest `at`, if one lies within `radius`.
pub fn nearest(at: Point, targets: &[Point], radius: f32) -> Option<usize> {
    targets
        .iter()
        .enumerate()
        .map(|(i, t)| (i, distance(at, *t)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Where a stick movement from `start` to `end` settles among `targets`
/// (their centres), or `None` to leave the cursor where it is.
pub fn settle(start: Point, end: Point, targets: &[Point], tuning: &Tuning) -> Option<usize> {
    let near = nearest(end, targets, tuning.radius);
    let travel = (end.0 - start.0, end.1 - start.1);
    let length = distance(start, end);
    if length < tuning.min_travel {
        return near;
    }
    let origin = nearest(start, targets, tuning.radius);
    if near != origin {
        return near;
    }
    // Still nearest where it started: a quick flick toward a neighbour.
    let Some(o) = origin else {
        return near;
    };
    let from = targets[o];
    let heading = (travel.0 / length, travel.1 / length);
    let next = targets
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != o)
        .filter_map(|(i, t)| {
            let to = (t.0 - from.0, t.1 - from.1);
            let gap = distance(from, *t);
            let aligned = (to.0 * heading.0 + to.1 * heading.1) / gap;
            (gap > 0.0 && aligned >= tuning.cone).then_some((i, gap, to))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    match next {
        Some((i, gap, to)) => {
            let progress = ((end.0 - from.0) * to.0 + (end.1 - from.1) * to.1) / (gap * gap);
            if progress >= tuning.commit {
                Some(i)
            } else {
                near
            }
        }
        None => near,
    }
}

/// Where a cursor left on a target goes once the targets moved: with most of
/// them if they moved together (a recipe book opened), else to the nearest
/// one (a creative tab with other slots), so it never rests between targets.
/// `None` when it wasn't on a target, or one is still there.
pub fn follow(at: Point, before: &[Point], after: &[Point]) -> Option<usize> {
    const SAME: f32 = 0.5;
    let on = before.iter().find(|t| distance(at, **t) <= SAME)?;
    if after.iter().any(|t| distance(at, *t) <= SAME) {
        return None;
    }
    let carried = |by: Point| {
        before
            .iter()
            .filter(|b| {
                let moved = (b.0 + by.0, b.1 + by.1);
                after.iter().any(|a| distance(moved, *a) <= SAME)
            })
            .count()
    };
    let together = after
        .iter()
        .enumerate()
        .map(|(i, a)| (i, carried((a.0 - on.0, a.1 - on.1))))
        .max_by_key(|&(_, count)| count)
        .filter(|&(_, count)| count * 2 > before.len());
    together
        .map(|(i, _)| i)
        .or_else(|| nearest(at, after, f32::INFINITY))
}

/// The cursor `seconds` into a glide from `from` to `to`: eased out, so it
/// slows as it lands, and exactly `to` once the glide is over.
pub fn glide(from: Point, to: Point, seconds: f32, tuning: &Tuning) -> Point {
    let t = (seconds / tuning.glide_seconds).clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - t).powi(3);
    (
        from.0 + (to.0 - from.0) * eased,
        from.1 + (to.1 - from.1) * eased,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of inventory slots, 18 pixels apart, and the slot below the first.
    const SLOTS: [Point; 4] = [(8.0, 8.0), (26.0, 8.0), (44.0, 8.0), (8.0, 26.0)];

    #[test]
    fn follow_goes_with_targets_that_moved_together() {
        let moved = SLOTS.map(|s| (s.0 + 77.0, s.1));
        assert_eq!(follow(SLOTS[1], &SLOTS, &moved), Some(1));
    }

    #[test]
    fn follow_takes_the_nearest_of_other_targets() {
        let other = [(30.0, 50.0), (200.0, 200.0)];
        assert_eq!(follow(SLOTS[1], &SLOTS, &other), Some(0));
    }

    #[test]
    fn follow_leaves_a_free_cursor_and_a_target_still_there() {
        let other = [(30.0, 50.0)];
        assert_eq!(follow((100.0, 100.0), &SLOTS, &other), None);
        assert_eq!(follow(SLOTS[0], &SLOTS, &SLOTS), None);
        assert_eq!(follow(SLOTS[0], &SLOTS, &[]), None);
    }

    #[test]
    fn a_short_flick_moves_to_the_neighbour_it_heads_for() {
        // 4 pixels right of slot 0: still nearest slot 0, but heading for slot 1.
        assert_eq!(settle((8.0, 8.0), (12.0, 8.0), &SLOTS, &TUNING), Some(1));
        assert_eq!(settle((8.0, 8.0), (8.0, 12.0), &SLOTS, &TUNING), Some(3));
        assert_eq!(settle((26.0, 8.0), (22.0, 8.0), &SLOTS, &TUNING), Some(0));
    }

    #[test]
    fn a_wobble_stays_put() {
        assert_eq!(settle((8.0, 8.0), (8.5, 8.4), &SLOTS, &TUNING), Some(0));
        // Barely started toward the neighbour: not committed.
        assert_eq!(settle((8.0, 8.0), (9.6, 8.0), &SLOTS, &TUNING), Some(0));
    }

    #[test]
    fn a_long_move_settles_on_the_nearest_target() {
        assert_eq!(settle((8.0, 8.0), (42.0, 9.0), &SLOTS, &TUNING), Some(2));
        // Far from everything: left where it is.
        assert_eq!(settle((8.0, 8.0), (200.0, 200.0), &SLOTS, &TUNING), None);
    }

    #[test]
    fn a_flick_off_any_row_does_not_jump_sideways() {
        // Up from slot 0: nothing lies that way, so it stays on slot 0.
        assert_eq!(settle((8.0, 8.0), (8.0, 4.0), &SLOTS, &TUNING), Some(0));
    }

    #[test]
    fn the_glide_eases_onto_its_target_and_stops_there() {
        let (from, to) = ((0.0, 0.0), (20.0, -10.0));
        assert_eq!(glide(from, to, 0.0, &TUNING), from);
        let half = glide(from, to, TUNING.glide_seconds / 2.0, &TUNING);
        assert!(
            half.0 > 10.0 && half.0 < 20.0,
            "eased out: past halfway at half time"
        );
        assert_eq!(glide(from, to, TUNING.glide_seconds, &TUNING), to);
        assert_eq!(glide(from, to, 1.0, &TUNING), to);
    }
}
