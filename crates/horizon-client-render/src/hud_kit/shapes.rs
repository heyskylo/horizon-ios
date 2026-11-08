//! Shapes: triangles, fans, arcs and discs for `HudBatch`, and changes to what was drawn since a
//! point (`alpha_since`, `translate_since`). An overlay that isn't made of rectangles (the radial
//! dial's ring, its lit sector and the dark disc behind it) draws with these.
//!
//! Every shape is made of 6-vertex units, as the batch's rectangles are: a triangle is the
//! quad `a, b, c, a, c, c`. So `clip_since`, the blend runs (`blend_since`) and anything that
//! reads the batch a quad at a time keep working over them. A triangle is never mistaken for a
//! rectangle: its fourth corner repeats the third.
//!
//! Angles are radians from straight up, clockwise, as a stick and the dial's slots go: 0 is up,
//! `FRAC_PI_2` right. Positions and radii are GUI px. Radii are snapped to whole GUI px, so a
//! ring's edge lands on the same pixels at every GUI scale.
//!
//! ```
//! use horizon_client_render::hud::{HudAtlas, HudBatch, rgba};
//! use horizon_client_render::hud_kit::shapes::{self, Stop};
//! use std::f32::consts::{FRAC_PI_8, TAU};
//!
//! let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
//! let mut batch = HudBatch::new([854, 480]);
//! let centre = [213.0, 120.0];
//! // A dark disc fading out, a faint ring, then the top 45 degree sector lit.
//! let dark = rgba(13, 14, 19, 180);
//! batch.fill_disc_gradient(&atlas, centre, 90.0, &[Stop(0.0, dark), Stop(1.0, dark & 0x00FF_FFFF)]);
//! batch.fill_arc(&atlas, centre, [40.0, 70.0], [0.0, TAU], rgba(13, 14, 19, 115));
//! let start = batch.vertices.len();
//! batch.fill_arc(&atlas, centre, [40.0, 70.0], [-FRAC_PI_8, FRAC_PI_8], rgba(118, 86, 250, 140));
//! // Fade the sector in and slide it: only what was drawn since `start` changes.
//! batch.alpha_since(start, 0.5);
//! batch.translate_since(start, 0.0, 2.0);
//! assert_eq!(batch.vertices.len() % 6, 0);
//! assert_eq!(shapes::arc_segments(TAU, 70.0), 110);
//! ```
use crate::hud::{HudAtlas, HudBatch, HudVertex};

/// How long, in GUI px along its outer edge, one segment of an arc is at most: short enough that
/// a circle of the dial's size reads as round at GUI scale 4.
pub const ARC_STEP: f32 = 4.0;
/// The most segments one arc is cut into, however large.
pub const MAX_SEGMENTS: usize = 256;

/// A colour stop of a radial gradient: the fraction of the radius (0 the centre, 1 the rim) and
/// the colour there (the batch's `rgba`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop(pub f32, pub u32);

/// How many segments an arc spanning `span` radians at `radius` GUI px is cut into: one per
/// `ARC_STEP` of its outer edge, at least 1 and at most `MAX_SEGMENTS`.
pub fn arc_segments(span: f32, radius: f32) -> usize {
    let length = span.abs() * radius.max(0.0);
    ((length / ARC_STEP).ceil() as usize).clamp(1, MAX_SEGMENTS)
}

/// A radius snapped to a whole GUI px (never below 0).
pub fn snap(radius: f32) -> f32 {
    radius.round().max(0.0)
}

/// The point `radius` GUI px from `centre` at `angle` (radians from up, clockwise).
pub fn on_circle([cx, cy]: [f32; 2], radius: f32, angle: f32) -> [f32; 2] {
    [cx + radius * angle.sin(), cy - radius * angle.cos()]
}

/// `colour` (the batch's `rgba`: alpha in the top byte) with its alpha times `factor` (0 to 1).
pub fn faded(colour: u32, factor: f32) -> u32 {
    let alpha = (colour >> 24) as f32 * factor.clamp(0.0, 1.0);
    (colour & 0x00FF_FFFF) | ((alpha.round() as u32) << 24)
}

/// The UV of the atlas's white texel, which every flat shape samples.
fn white(atlas: &HudAtlas) -> [f32; 2] {
    let w = atlas.white;
    [(w.u0 + w.u1) / 2.0, (w.v0 + w.v1) / 2.0]
}

impl HudBatch {
    /// One vertex of a flat shape.
    fn flat(&mut self, pos: [f32; 2], uv: [f32; 2], color: u32) {
        self.vertices.push(HudVertex {
            pos,
            uv,
            color,
            depth: 0.0,
            glint_uv: [0.0, 0.0],
            text: 0,
        });
    }

    /// A triangle as the 6-vertex unit `a, b, c, a, c, c`, each corner its own colour.
    fn triangle_shaded(&mut self, uv: [f32; 2], [a, b, c]: [[f32; 2]; 3], [ca, cb, cc]: [u32; 3]) {
        for (pos, color) in [(a, ca), (b, cb), (c, cc), (a, ca), (c, cc), (c, cc)] {
            self.flat(pos, uv, color);
        }
    }

    /// A four-cornered piece, corners in order round it, each its own colour.
    fn quad_shaded(&mut self, uv: [f32; 2], p: [[f32; 2]; 4], c: [u32; 4]) {
        for i in [0, 1, 2, 0, 2, 3] {
            self.flat(p[i], uv, c[i]);
        }
    }

    /// A triangle in one colour (GUI px corners, any winding).
    pub fn fill_triangle(&mut self, atlas: &HudAtlas, corners: [[f32; 2]; 3], color: u32) {
        self.triangle_shaded(white(atlas), corners, [color; 3]);
    }

    /// A fan in one colour: a triangle from `centre` to each two neighbouring points of `rim`
    /// (GUI px). Repeat the first point at the end to close it.
    pub fn fill_fan(&mut self, atlas: &HudAtlas, centre: [f32; 2], rim: &[[f32; 2]], color: u32) {
        let uv = white(atlas);
        for pair in rim.windows(2) {
            self.triangle_shaded(uv, [centre, pair[0], pair[1]], [color; 3]);
        }
    }

    /// An annular sector in one colour: between radii `r0` and `r1` (GUI px, snapped to whole
    /// px) from `angles[0]` to `angles[1]` (radians from up, clockwise), cut into
    /// [`arc_segments`]. With `r0` 0 it is a pie slice; a span of `TAU` is a whole ring.
    pub fn fill_arc(
        &mut self,
        atlas: &HudAtlas,
        centre: [f32; 2],
        [r0, r1]: [f32; 2],
        [a0, a1]: [f32; 2],
        color: u32,
    ) {
        self.arc_shaded(
            white(atlas),
            centre,
            [snap(r0), snap(r1)],
            [a0, a1],
            [color; 2],
        );
    }

    /// An annular sector from `colours[0]` at its inner radius to `colours[1]` at its outer one;
    /// the radii are already snapped.
    fn arc_shaded(
        &mut self,
        uv: [f32; 2],
        centre: [f32; 2],
        [r0, r1]: [f32; 2],
        [a0, a1]: [f32; 2],
        [inner, outer]: [u32; 2],
    ) {
        if r1 <= r0 || a1 == a0 {
            return;
        }
        let n = arc_segments(a1 - a0, r1);
        let at = |i: usize| a0 + (a1 - a0) * i as f32 / n as f32;
        for i in 0..n {
            let (b0, b1) = (at(i), at(i + 1));
            let (o0, o1) = (on_circle(centre, r1, b0), on_circle(centre, r1, b1));
            if r0 <= 0.0 {
                self.triangle_shaded(uv, [centre, o0, o1], [inner, outer, outer]);
            } else {
                let (i0, i1) = (on_circle(centre, r0, b0), on_circle(centre, r0, b1));
                self.quad_shaded(uv, [i0, o0, o1, i1], [inner, outer, outer, inner]);
            }
        }
    }

    /// A disc of radius `radius` GUI px (snapped) in one colour.
    pub fn fill_disc(&mut self, atlas: &HudAtlas, centre: [f32; 2], radius: f32, color: u32) {
        self.fill_arc(
            atlas,
            centre,
            [0.0, radius],
            [0.0, std::f32::consts::TAU],
            color,
        );
    }

    /// A disc of radius `radius` GUI px (snapped) whose colour runs through `stops` from the
    /// centre out, blending linearly between them: a dark disc fading to nothing at its rim is
    /// `[Stop(0.0, dark), Stop(1.0, dark with alpha 0)]`. Stops are in order of their fraction;
    /// before the first and after the last the colour holds.
    pub fn fill_disc_gradient(
        &mut self,
        atlas: &HudAtlas,
        centre: [f32; 2],
        radius: f32,
        stops: &[Stop],
    ) {
        let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
            return;
        };
        debug_assert!(
            stops.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "gradient stops out of order: {stops:?}"
        );
        let radius = snap(radius);
        let uv = white(atlas);
        let full = [0.0, std::f32::consts::TAU];
        let mut bands: Vec<Stop> = Vec::with_capacity(stops.len() + 2);
        if first.0 > 0.0 {
            bands.push(Stop(0.0, first.1));
        }
        bands.extend(stops.iter().map(|s| Stop(s.0.clamp(0.0, 1.0), s.1)));
        if last.0 < 1.0 {
            bands.push(Stop(1.0, last.1));
        }
        for pair in bands.windows(2) {
            let (r0, r1) = (snap(pair[0].0 * radius), snap(pair[1].0 * radius));
            self.arc_shaded(uv, centre, [r0, r1], full, [pair[0].1, pair[1].1]);
        }
    }

    /// Multiplies the alpha of every vertex added since `start` by `factor` (0 to 1): a fade over
    /// what one piece drew, its text included.
    pub fn alpha_since(&mut self, start: usize, factor: f32) {
        if factor >= 1.0 {
            return;
        }
        for v in &mut self.vertices[start..] {
            v.color = faded(v.color, factor);
        }
    }

    /// Moves every vertex added since `start` by `(dx, dy)` GUI px.
    pub fn translate_since(&mut self, start: usize, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for v in &mut self.vertices[start..] {
            v.pos[0] += dx;
            v.pos[1] += dy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{Blend, rgba};
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn batch() -> HudBatch {
        HudBatch::new([854, 480])
    }

    /// The area the batch's vertices since `start` cover, adding each triangle's.
    fn area(batch: &HudBatch, start: usize) -> f32 {
        let tri = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
        };
        batch.vertices[start..]
            .as_chunks::<6>()
            .0
            .iter()
            .map(|q| tri(q[0].pos, q[1].pos, q[2].pos) + tri(q[3].pos, q[4].pos, q[5].pos))
            .sum()
    }

    fn distance(p: [f32; 2], c: [f32; 2]) -> f32 {
        ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt()
    }

    #[test]
    fn a_triangle_is_one_six_vertex_unit_that_isnt_a_rectangle() {
        let atlas = atlas();
        let mut b = batch();
        let corners = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        b.fill_triangle(&atlas, corners, rgba(255, 0, 0, 255));
        let pos: Vec<[f32; 2]> = b.vertices.iter().map(|v| v.pos).collect();
        assert_eq!(
            pos,
            [
                corners[0], corners[1], corners[2], corners[0], corners[2], corners[2]
            ]
        );
        assert_eq!(area(&b, 0), 50.0);
        // A clip keeps the part inside, as a triangle, not the box around it.
        b.clip_since(0, 0.0, 0.0, 5.0, 10.0);
        assert!((area(&b, 0) - 37.5).abs() < 1e-3, "{}", area(&b, 0));
    }

    #[test]
    fn a_disc_covers_its_circle_and_its_radius_is_snapped() {
        let atlas = atlas();
        let mut b = batch();
        let centre = [100.0, 100.0];
        b.fill_disc(&atlas, centre, 30.4, rgba(0, 0, 0, 255));
        let far = b
            .vertices
            .iter()
            .map(|v| distance(v.pos, centre))
            .fold(0.0, f32::max);
        assert!((far - 30.0).abs() < 1e-3, "the rim at {far}");
        // Within a percent of a true circle's area (a polygon inside it).
        let circle = PI * 30.0 * 30.0;
        let got = area(&b, 0);
        assert!(got <= circle && got > circle * 0.99, "{got} of {circle}");
        assert_eq!(b.vertices.len() % 6, 0);
    }

    #[test]
    fn an_arc_stays_in_its_band_and_its_span() {
        let atlas = atlas();
        let mut b = batch();
        let centre = [200.0, 120.0];
        // The right-hand quarter, from up-right to down-right.
        b.fill_arc(
            &atlas,
            centre,
            [40.0, 70.0],
            [FRAC_PI_4, 3.0 * FRAC_PI_4],
            u32::MAX,
        );
        for v in &b.vertices {
            let r = distance(v.pos, centre);
            assert!((39.99..=70.01).contains(&r), "{r}");
            let angle = (v.pos[0] - centre[0]).atan2(centre[1] - v.pos[1]);
            assert!(
                (FRAC_PI_4 - 1e-4..=3.0 * FRAC_PI_4 + 1e-4).contains(&angle),
                "{angle}"
            );
        }
        let ring = PI * (70.0f32.powi(2) - 40.0f32.powi(2)) / 4.0;
        let got = area(&b, 0);
        assert!((got - ring).abs() < ring * 0.01, "{got} of {ring}");
    }

    #[test]
    fn longer_arcs_get_more_segments_up_to_a_cap() {
        assert_eq!(arc_segments(0.0, 50.0), 1);
        assert_eq!(arc_segments(FRAC_PI_4, 8.0), 2);
        assert!(arc_segments(FRAC_PI_4, 64.0) > arc_segments(FRAC_PI_4, 32.0));
        assert!(arc_segments(TAU, 64.0) > arc_segments(FRAC_PI_4, 64.0));
        assert_eq!(arc_segments(TAU, 1.0e6), MAX_SEGMENTS);
        // No segment's outer edge is longer than the step.
        for (span, r) in [(TAU, 70.0), (FRAC_PI_4, 90.0), (FRAC_PI_2, 13.0)] {
            assert!(span * r / arc_segments(span, r) as f32 <= ARC_STEP);
        }
    }

    #[test]
    fn a_clip_keeps_half_a_disc_and_a_blend_run_keeps_its_vertices() {
        let atlas = atlas();
        let mut b = batch();
        b.fill(&atlas, 0.0, 0.0, 4.0, 4.0, u32::MAX);
        let start = b.vertices.len();
        let centre = [100.0, 100.0];
        b.fill_disc(&atlas, centre, 20.0, u32::MAX);
        b.blend_since(start, Blend::Additive);
        b.clip_since(start, 100.0, 0.0, 400.0, 400.0);
        assert!(b.vertices[start..].iter().all(|v| v.pos[0] >= 100.0 - 1e-3));
        let half = area(&b, start);
        let expected = PI * 400.0 / 2.0;
        assert!((half - expected).abs() < expected * 0.02, "{half}");
        // The additive run moved with the clipped vertices and is still whole units.
        let runs = b.runs();
        let (run, blend) = runs.last().unwrap();
        assert_eq!(*blend, Blend::Additive);
        assert_eq!(run.start as usize, start);
        assert_eq!(run.end as usize, b.vertices.len());
        assert_eq!((run.end - run.start) % 6, 0);
    }

    #[test]
    fn a_gradient_runs_from_its_centre_to_its_rim() {
        let atlas = atlas();
        let mut b = batch();
        let centre = [50.0, 50.0];
        let dark = rgba(13, 14, 19, 180);
        let clear = dark & 0x00FF_FFFF;
        b.fill_disc_gradient(
            &atlas,
            centre,
            40.0,
            &[
                Stop(0.0, dark),
                Stop(0.6, faded(dark, 0.75)),
                Stop(1.0, clear),
            ],
        );
        for v in &b.vertices {
            let r = distance(v.pos, centre);
            let alpha = v.color >> 24;
            match r {
                r if r < 1e-3 => assert_eq!(alpha, 180),
                r if (r - 24.0).abs() < 1e-3 => assert_eq!(alpha, 135),
                r if (r - 40.0).abs() < 1e-3 => assert_eq!(alpha, 0),
                r => panic!("a vertex between the stops, at {r}"),
            }
        }
        // A stop that starts past the centre holds its colour in to it.
        let mut held = batch();
        held.fill_disc_gradient(&atlas, centre, 10.0, &[Stop(0.5, dark), Stop(1.0, clear)]);
        let at_centre = held.vertices.iter().find(|v| v.pos == centre).unwrap();
        assert_eq!(at_centre.color, dark);
    }

    #[test]
    fn a_clipped_gradient_keeps_its_colours_across_the_cut() {
        let atlas = atlas();
        let mut b = batch();
        let centre = [100.0, 100.0];
        let dark = rgba(0, 0, 0, 200);
        b.fill_disc_gradient(
            &atlas,
            centre,
            40.0,
            &[Stop(0.0, dark), Stop(1.0, dark & 0x00FF_FFFF)],
        );
        // Cut through the middle of the radius: each new vertex on the cut has the alpha the
        // gradient has that far out, not the colour of a corner it was cut from.
        b.clip_since(0, 0.0, 0.0, 120.0, 400.0);
        let on_cut: Vec<_> = b
            .vertices
            .iter()
            .filter(|v| (v.pos[0] - 120.0).abs() < 1e-3)
            .collect();
        assert!(on_cut.len() > 4, "nothing on the cut");
        for v in on_cut {
            let expected = 200.0 * (1.0 - distance(v.pos, centre) / 40.0);
            let alpha = (v.color >> 24) as f32;
            assert!(
                (alpha - expected).abs() <= 1.5,
                "{alpha} at {:?}, not {expected}",
                v.pos
            );
        }
    }

    #[test]
    fn alpha_and_translate_change_only_what_came_after_the_mark() {
        let atlas = atlas();
        let mut b = batch();
        b.fill(&atlas, 0.0, 0.0, 4.0, 4.0, rgba(255, 255, 255, 200));
        let start = b.vertices.len();
        b.fill_triangle(
            &atlas,
            [[10.0, 10.0], [20.0, 10.0], [10.0, 20.0]],
            rgba(255, 255, 255, 200),
        );
        b.alpha_since(start, 0.5);
        b.translate_since(start, 3.0, -2.0);
        assert!(b.vertices[..start].iter().all(|v| v.color >> 24 == 200));
        assert!(b.vertices[start..].iter().all(|v| v.color >> 24 == 100));
        assert_eq!(b.vertices[0].pos, [0.0, 0.0]);
        assert_eq!(b.vertices[start].pos, [13.0, 8.0]);
        // The colour itself stays.
        assert_eq!(b.vertices[start].color & 0x00FF_FFFF, 0x00FF_FFFF);
    }
}
