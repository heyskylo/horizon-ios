//! A collapsed page strip: the selected page's name between `‹ ›` arrows, with a pip per page
//! under it. A page list with more pages than room (the settings shell's, `settings_shell`)
//! shows them this way instead of leaving some out.
//!
//! [`collapsed`] only lays out (pure, testable); [`draw`] paints a laid-out strip. It knows
//! nothing of clicks or bumpers: the screen using it decides what each part does.
//!
//! Sizes are GUI px. The name is cut to the room between the arrows, and when the pips don't all
//! fit across the area, the ones shown are a window around the selected page's.
//!
//! ```
//! use horizon_client_render::hud::HudAtlas;
//! use horizon_client_render::hud_kit::tabs;
//!
//! let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
//! let names = ["General", "Camera", "Build"].map(String::from);
//! let strip = tabs::collapsed(&atlas, &names, 0, [8.0, 100.0, 112.0, 16.0]);
//! assert_eq!(strip.label, "General");
//! assert!(!strip.has_prev && strip.has_next);
//! assert_eq!(strip.pips.len(), 3);
//! ```
use crate::hud::{HudAtlas, HudBatch};
use crate::hud_kit::colour;

/// A pip under the strip: its size and the step from one to the next, in GUI px.
pub const PIP: [f32; 2] = [3.0, 2.0];
pub const PIP_STEP: f32 = 5.0;
/// How far, in GUI px, the arrows sit from the name.
pub const ARROW_GAP: f32 = 4.0;
/// An arrow with a page that way, and one at an end, with none.
pub const ARROW: u32 = colour(0x9FA3AE, 255);
pub const ARROW_OFF: u32 = colour(0x9FA3AE, 77);
/// The pips (the selected one is white).
pub const PIP_OFF: u32 = colour(0xFFFFFF, 89);
pub const PIP_ON: u32 = colour(0xFFFFFF, 255);
/// The name's colour.
pub const NAME: u32 = colour(0xFFFFFF, 255);
/// The line of text's height in GUI px (vanilla's font, without its shadow).
const LINE: f32 = 8.0;

/// A laid-out strip.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    /// The area it was laid out in, `[x, y, w, h]` GUI px.
    pub area: [f32; 4],
    /// The selected page's index in the names, and its name, cut to fit.
    pub selected: usize,
    pub label: String,
    /// Where the name is drawn, `[x, y, w, h]`.
    pub label_area: [f32; 4],
    /// The arrows' text positions `[x, y]` (before, after).
    pub arrows: [[f32; 2]; 2],
    /// Whether there is a page before and after the selected one: an arrow without is dimmed.
    /// A screen may change these (a search skipping pages without matches).
    pub has_prev: bool,
    pub has_next: bool,
    /// The pips `[x, y, w, h]` under the area, and whether each is the selected page's. At most
    /// as many as fit across the area.
    pub pips: Vec<([f32; 4], bool)>,
}

/// The strip for `names` with `selected` in `area` (`[x, y, w, h]`, GUI px): the selected name
/// between `‹ ›` arrows, cut to fit, with pips under the area. Past the end, the last page.
pub fn collapsed(atlas: &HudAtlas, names: &[String], selected: usize, area: [f32; 4]) -> Strip {
    let [x, y, w, h] = area;
    let n = names.len();
    let selected = selected.min(n.saturating_sub(1));
    let text_y = y + ((h - LINE) / 2.0).floor();
    let centre = x + w / 2.0;
    let arrow_w = atlas.font.width("‹").max(atlas.font.width("›"));
    let room = (w - 2.0 * (arrow_w + ARROW_GAP)).max(0.0);
    let name = names.get(selected).map_or("", String::as_str);
    let label = crate::graphics_style::fit(atlas, name, room);
    let label_w = atlas.font.width(&label).ceil();
    let left = (centre - label_w / 2.0).floor();
    let arrows = [
        [left - ARROW_GAP - arrow_w, text_y],
        [left + label_w + ARROW_GAP, text_y],
    ];
    // As many pips as fit across the area, a window round the selected one.
    let fit = (((w - PIP[0]) / PIP_STEP).floor() as usize + 1).min(n);
    let first = selected.saturating_sub(fit / 2).min(n - fit.min(n));
    let pips_x = (centre - (fit as f32 * PIP_STEP - (PIP_STEP - PIP[0])) / 2.0).floor();
    let pips = (0..fit)
        .map(|i| {
            let px = pips_x + i as f32 * PIP_STEP;
            ([px, y + h + 1.0, PIP[0], PIP[1]], first + i == selected)
        })
        .collect();
    Strip {
        area,
        selected,
        label,
        label_area: [left, y, label_w, h],
        arrows,
        has_prev: selected > 0,
        has_next: selected + 1 < n,
        pips,
    }
}

/// Paints `strip`: the name, its arrows (dimmed at an end) and the pips.
pub fn draw(batch: &mut HudBatch, atlas: &HudAtlas, strip: &Strip) {
    let [lx, ly, ..] = strip.label_area;
    let text_y = ly + ((strip.area[3] - LINE) / 2.0).floor();
    batch.text(atlas, &strip.label, lx, text_y, NAME, true);
    let lit = [strip.has_prev, strip.has_next];
    for (([ax, ay], arrow), on) in strip.arrows.into_iter().zip(["‹", "›"]).zip(lit) {
        batch.text(
            atlas,
            arrow,
            ax,
            ay,
            if on { ARROW } else { ARROW_OFF },
            true,
        );
    }
    for (pip, on) in &strip.pips {
        let [px, py, pw, ph] = *pip;
        batch.fill(atlas, px, py, pw, ph, if *on { PIP_ON } else { PIP_OFF });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    const AREA: [f32; 4] = [8.0, 100.0, 112.0, 16.0];

    #[test]
    fn the_name_sits_centred_between_its_arrows_with_a_pip_per_page() {
        let atlas = atlas();
        let pages = names(&["General", "Sky", "Water", "Fog"]);
        let strip = collapsed(&atlas, &pages, 1, AREA);
        assert_eq!(strip.label, "Sky");
        let [nx, _, nw, _] = strip.label_area;
        assert!((nx + nw / 2.0 - 64.0).abs() <= 1.0, "{nx} {nw}");
        let [[before, _], [after, _]] = strip.arrows;
        assert!(before < nx && after >= nx + nw);
        let on: Vec<bool> = strip.pips.iter().map(|(_, on)| *on).collect();
        assert_eq!(on, [false, true, false, false]);
        // The pips sit under the row, centred.
        let (first, last) = (strip.pips[0].0, strip.pips[3].0);
        assert!(first[1] >= AREA[1] + AREA[3]);
        assert!(((first[0] + last[0] + last[2]) / 2.0 - 64.0).abs() <= 1.0);
        // Past the end, the last page; no pages, nothing to the arrows' sides.
        assert_eq!(collapsed(&atlas, &pages, 9, AREA).selected, 3);
        let none = collapsed(&atlas, &[], 0, AREA);
        assert!(none.label.is_empty() && none.pips.is_empty());
        assert!(!none.has_prev && !none.has_next);
    }

    #[test]
    fn a_long_name_is_cut_to_stay_between_its_arrows() {
        let atlas = atlas();
        let long = names(&["A page with a very long name", "B"]);
        let strip = collapsed(&atlas, &long, 0, [0.0, 0.0, 80.0, 12.0]);
        let [_, [after, _]] = strip.arrows;
        assert!(after + atlas.font.width("›") <= 80.0, "{after}");
        assert!(strip.label.ends_with("..."));
    }

    #[test]
    fn the_ends_have_no_page_beyond_and_draw_their_arrow_dimmed() {
        let atlas = atlas();
        let pages = names(&["A", "B", "C"]);
        let ends = [(0, false, true), (1, true, true), (2, true, false)];
        for (selected, prev, next) in ends {
            let strip = collapsed(&atlas, &pages, selected, AREA);
            assert_eq!((strip.has_prev, strip.has_next), (prev, next));
        }
        // The first page: its `‹` is drawn in the dimmed colour, its `›` in the arrow's.
        let alpha = |strip: &Strip, i: usize| {
            let mut batch = HudBatch::new([854, 480]);
            draw(&mut batch, &atlas, strip);
            let [ax, _] = strip.arrows[i];
            (batch.vertices.iter())
                .filter(|v| {
                    (v.pos[0] - ax).abs() < 4.0 && v.color & 0x00FF_FFFF == ARROW & 0x00FF_FFFF
                })
                .map(|v| v.color >> 24)
                .max()
                .unwrap_or(0)
        };
        let first = collapsed(&atlas, &pages, 0, AREA);
        assert_eq!(alpha(&first, 0), ARROW_OFF >> 24);
        assert_eq!(alpha(&first, 1), ARROW >> 24);
    }

    #[test]
    fn many_pages_show_as_many_pips_as_fit_round_the_selected_one() {
        let atlas = atlas();
        let pages: Vec<String> = (0..60).map(|i| format!("Page {i}")).collect();
        let [x, _, w, _] = AREA;
        for selected in [0, 1, 30, 58, 59] {
            let strip = collapsed(&atlas, &pages, selected, AREA);
            assert!(strip.pips.len() < pages.len());
            for ([px, _, pw, _], _) in &strip.pips {
                assert!(*px >= x && px + pw <= x + w, "{selected}: {px}");
            }
            assert_eq!(strip.pips.iter().filter(|p| p.1).count(), 1, "{selected}");
        }
        // At the ends, the lit pip is at the window's end too.
        let first = collapsed(&atlas, &pages, 0, AREA);
        assert!(first.pips[0].1);
        let last = collapsed(&atlas, &pages, 59, AREA);
        assert!(last.pips.last().unwrap().1);
    }
}
