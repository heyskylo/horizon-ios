//! Page strips. [`collapsed`]: the selected page's name between `‹ ›` arrows, with a pip per
//! page under it; a page list with more pages than room (the settings shell's, `settings_shell`)
//! shows them this way instead of leaving some out. [`tab_strip`]: a row of page names across an
//! area, the selected one white and centred, its neighbours peeking in on each side and fading
//! with distance (the radial dial's page header); when three tabs don't fit it falls back to the
//! collapsed strip.
//!
//! [`collapsed`] and [`tab_strip`] only lay out (pure, testable); [`draw`] and [`draw_tabs`]
//! paint what they laid out. They know nothing of clicks or bumpers: the screen using them
//! decides what each part does.
//!
//! Sizes are GUI px. Collapsed, the name is cut to the room between the arrows, and when the
//! pips don't all fit across the area, the ones shown are a window around the selected page's.
//! A tab is `clamp(text + 10, 34, 64)` wide (`PAD`, `MIN`, `MAX`), tabs are `GAP` apart, and the
//! row collapses when its width is less than the selected tab plus two narrowest tabs and their
//! gaps. A page turn scrolls the row: pass the offset (from a `motion::slide`) as `scroll` and
//! the tabs move together, clipped to the area.
//!
//! ```
//! use horizon_client_render::hud::HudAtlas;
//! use horizon_client_render::hud_kit::tabs::{self, TabStrip};
//!
//! let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
//! let names = ["General", "Camera", "Build"].map(String::from);
//! let strip = tabs::collapsed(&atlas, &names, 0, [8.0, 100.0, 112.0, 16.0]);
//! assert_eq!(strip.label, "General");
//! assert!(!strip.has_prev && strip.has_next);
//! assert_eq!(strip.pips.len(), 3);
//! let TabStrip::Tabs(row) = tabs::tab_strip(&atlas, &names, 1, [20.0, 2.0, 380.0, 12.0], 0.0)
//! else {
//!     panic!("room for a row of tabs");
//! };
//! // The selected tab sits in the middle of the area.
//! let camera = row.tabs.iter().find(|t| t.selected).unwrap();
//! assert!((camera.area[0] + camera.area[2] / 2.0 - 210.0).abs() <= 1.0);
//! let narrow = tabs::tab_strip(&atlas, &names, 1, [20.0, 2.0, 90.0, 12.0], 0.0);
//! assert!(matches!(narrow, TabStrip::Collapsed(_)));
//! ```
use crate::hud::{HudAtlas, HudBatch};
use crate::hud_kit::{colour, fit};
use crate::menu_style::{self, OverlayColours};

/// The overlays' colours (`menu_style`, given at startup).
fn overlay() -> &'static OverlayColours {
    &menu_style::get().overlay
}

/// A pip under the strip: its size and the step from one to the next, in GUI px.
pub const PIP: [f32; 2] = [3.0, 2.0];
pub const PIP_STEP: f32 = 5.0;
/// How far, in GUI px, the arrows sit from the name.
pub const ARROW_GAP: f32 = 4.0;
/// An arrow with a page that way, and one at an end, with none.
pub fn arrow_colour() -> u32 {
    colour(overlay().backdrop_muted, 255)
}
pub fn arrow_off_colour() -> u32 {
    colour(overlay().backdrop_muted, 77)
}
/// The pips (the selected one is white).
pub const PIP_OFF: u32 = colour(0xFFFFFF, 89);
pub const PIP_ON: u32 = colour(0xFFFFFF, 255);
/// The name's colour.
pub const NAME: u32 = colour(0xFFFFFF, 255);
/// Space beside a tab's text, in GUI px (both sides together).
pub const PAD: f32 = 10.0;
/// A tab's width at least and at most, in GUI px.
pub const MIN: f32 = 34.0;
pub const MAX: f32 = 64.0;
/// The gap between two tabs, in GUI px.
pub const GAP: f32 = 2.0;
/// How far, in GUI px, a tab crossing the row's edge fades out before it.
pub const EDGE_FADE: f32 = 12.0;
/// A tab's opacity by its distance from the selected one; the last holds beyond.
pub const FADE: [f32; 5] = [1.0, 0.7, 0.35, 0.15, 0.1];
/// The selected tab's fill and text, and the others'.
pub const SELECTED_FILL: u32 = colour(0xFFFFFF, 242);
pub fn selected_text() -> u32 {
    colour(overlay().page, 255)
}
pub fn tab_fill() -> u32 {
    colour(overlay().page, 191)
}
pub fn tab_text() -> u32 {
    colour(overlay().backdrop_muted, 255)
}
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
    let label = fit::ellipsize(atlas, name, room).into_owned();
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
            if on { arrow_colour() } else { arrow_off_colour() },
            true,
        );
    }
    for (pip, on) in &strip.pips {
        let [px, py, pw, ph] = *pip;
        batch.fill(atlas, px, py, pw, ph, if *on { PIP_ON } else { PIP_OFF });
    }
}

/// One tab, laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// Its page's index in the names.
    pub index: usize,
    /// `[x, y, w, h]` in GUI px; may reach past the strip's area (it is clipped when drawn).
    pub area: [f32; 4],
    /// Its text, cut to fit.
    pub label: String,
    /// Its opacity, 0 to 1, from its distance to the selected tab.
    pub alpha: f32,
    pub selected: bool,
}

/// A laid-out row of tabs, or the collapsed strip it fell back to.
#[derive(Clone, Debug, PartialEq)]
pub enum TabStrip {
    Tabs(Row),
    Collapsed(Strip),
}

/// A row of tabs.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The area it was laid out in, `[x, y, w, h]` GUI px: drawing clips to it.
    pub area: [f32; 4],
    /// The tabs in page order that reach into the area.
    pub tabs: Vec<Tab>,
}

/// A tab's width for `text`: `clamp(text + PAD, MIN, MAX)` GUI px.
pub fn tab_width(atlas: &HudAtlas, text: &str) -> f32 {
    (atlas.font.width(text).ceil() + PAD).clamp(MIN, MAX)
}

/// Where tab `index`'s middle sits in the row, in GUI px from the first tab's left edge.
fn middle(widths: &[f32], index: usize) -> f32 {
    widths[..index].iter().map(|w| w + GAP).sum::<f32>() + widths[index] / 2.0
}

/// How far, in GUI px, tab `to`'s middle is from tab `from`'s in the row: a page turn scrolls
/// the strip by it (`motion::slide` from it to 0).
pub fn distance(atlas: &HudAtlas, names: &[String], from: usize, to: usize) -> f32 {
    let widths: Vec<f32> = names.iter().map(|n| tab_width(atlas, n)).collect();
    match (widths.get(from), widths.get(to)) {
        (Some(_), Some(_)) => middle(&widths, to) - middle(&widths, from),
        _ => 0.0,
    }
}

/// The width, in GUI px, the row takes with any tab centred: twice the most it reaches to either
/// side of a centred tab. An area this wide holds every tab whichever is selected, so things
/// placed at its ends (a strip's bumpers) stay put as the pages turn.
pub fn span(atlas: &HudAtlas, names: &[String]) -> f32 {
    let widths: Vec<f32> = names.iter().map(|n| tab_width(atlas, n)).collect();
    let total = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
    (0..widths.len())
        .map(|i| {
            let mid = middle(&widths, i);
            2.0 * mid.max(total - mid)
        })
        .fold(0.0, f32::max)
        .ceil()
}

/// The tabs for `names` with `selected` in `area` (`[x, y, w, h]`, GUI px), moved `scroll` GUI
/// px sideways (a page turn's slide); [`collapsed`] when three tabs don't fit.
pub fn tab_strip(
    atlas: &HudAtlas,
    names: &[String],
    selected: usize,
    area: [f32; 4],
    scroll: f32,
) -> TabStrip {
    let [x, y, w, h] = area;
    let selected = selected.min(names.len().saturating_sub(1));
    let widths: Vec<f32> = names.iter().map(|n| tab_width(atlas, n)).collect();
    let Some(&own) = widths.get(selected) else {
        return TabStrip::Tabs(Row {
            area,
            tabs: Vec::new(),
        });
    };
    if names.len() > 1 && w < own + 2.0 * (MIN + GAP) {
        return TabStrip::Collapsed(collapsed(atlas, names, selected, area));
    }
    // From the selected tab, centred, outwards each way.
    let centre = x + w / 2.0;
    let mut lefts = vec![0.0; names.len()];
    lefts[selected] = (centre - own / 2.0).floor() + scroll;
    for i in (0..selected).rev() {
        lefts[i] = lefts[i + 1] - GAP - widths[i];
    }
    for i in selected + 1..names.len() {
        lefts[i] = lefts[i - 1] + widths[i - 1] + GAP;
    }
    let tabs = names
        .iter()
        .enumerate()
        .filter(|(i, _)| lefts[*i] < x + w && lefts[*i] + widths[*i] > x)
        .map(|(i, name)| Tab {
            index: i,
            area: [lefts[i], y, widths[i], h],
            label: fit::ellipsize(atlas, name, widths[i] - PAD).into_owned(),
            alpha: FADE[i.abs_diff(selected).min(FADE.len() - 1)],
            selected: i == selected,
        })
        .collect();
    TabStrip::Tabs(Row { area, tabs })
}

/// Paints `strip`, everything's alpha times `alpha` (0 to 1). A row is clipped to its area: a
/// neighbour's plate and text fade together by its [`Tab::alpha`], and a tab crossing the
/// area's edge fades out over the last [`EDGE_FADE`]. A collapsed strip is drawn by [`draw`].
pub fn draw_tabs(batch: &mut HudBatch, atlas: &HudAtlas, strip: &TabStrip, alpha: f32) {
    let start = batch.vertices.len();
    let row = match strip {
        TabStrip::Collapsed(strip) => {
            draw(batch, atlas, strip);
            batch.alpha_since(start, alpha);
            return;
        }
        TabStrip::Tabs(row) => row,
    };
    let [x, y, w, h] = row.area;
    for tab in &row.tabs {
        let [tx, ty, tw, th] = tab.area;
        let (fill, text) = if tab.selected {
            (SELECTED_FILL, selected_text())
        } else {
            (tab_fill(), tab_text())
        };
        let tab_start = batch.vertices.len();
        // Cut where the edges' fades start, so each fade runs over its own piece.
        let mut cuts = vec![tx, tx + tw];
        cuts.extend(
            [x + EDGE_FADE, x + w - EDGE_FADE]
                .into_iter()
                .filter(|c| *c > tx && *c < tx + tw),
        );
        cuts.sort_by(f32::total_cmp);
        for pair in cuts.windows(2) {
            batch.fill(atlas, pair[0], ty, pair[1] - pair[0], th, fill);
        }
        let label_w = atlas.font.width(&tab.label).ceil();
        let lx = (tx + (tw - label_w) / 2.0).floor();
        let ly = ty + ((th - LINE) / 2.0).floor();
        // The selected tab's dark text has no shadow; light text does, as vanilla's.
        batch.text(atlas, &tab.label, lx, ly, text, !tab.selected);
        // A neighbour fades as a whole, its plate and its text alike.
        if !tab.selected {
            batch.alpha_since(tab_start, tab.alpha);
        }
    }
    batch.clip_since(start, x, y, x + w, y + h);
    // A tab crossing an edge fades out over its last `EDGE_FADE`, rather than being cut mid-word.
    for v in &mut batch.vertices[start..] {
        let reach = (v.pos[0] - x).min(x + w - v.pos[0]);
        v.color = crate::hud_kit::shapes::faded(v.color, (reach / EDGE_FADE).clamp(0.0, 1.0));
    }
    batch.alpha_since(start, alpha);
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
                    (v.pos[0] - ax).abs() < 4.0 && v.color & 0x00FF_FFFF == arrow_colour() & 0x00FF_FFFF
                })
                .map(|v| v.color >> 24)
                .max()
                .unwrap_or(0)
        };
        let first = collapsed(&atlas, &pages, 0, AREA);
        assert_eq!(alpha(&first, 0), arrow_off_colour() >> 24);
        assert_eq!(alpha(&first, 1), arrow_colour() >> 24);
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

    const ROW_AREA: [f32; 4] = [20.0, 2.0, 387.0, 12.0];

    fn row(strip: TabStrip) -> Row {
        match strip {
            TabStrip::Tabs(row) => row,
            TabStrip::Collapsed(_) => panic!("collapsed"),
        }
    }
    fn folded(strip: TabStrip) -> Strip {
        match strip {
            TabStrip::Collapsed(strip) => strip,
            TabStrip::Tabs(_) => panic!("a row of tabs"),
        }
    }

    #[test]
    fn tabs_are_their_text_plus_padding_within_the_limits() {
        let atlas = atlas();
        assert_eq!(tab_width(&atlas, "A"), MIN);
        let long = "A very long page name indeed";
        assert_eq!(tab_width(&atlas, long), MAX);
        let mid = "Redstone";
        let w = atlas.font.width(mid).ceil() + PAD;
        assert!(
            (MIN..=MAX).contains(&w),
            "pick a name between the limits: {w}"
        );
        assert_eq!(tab_width(&atlas, mid), w);
    }

    #[test]
    fn the_selected_tab_is_centred_and_the_rest_follow_in_order_fading() {
        let atlas = atlas();
        let pages = names(&["General", "Camera", "Build", "Redstone", "Hotbar"]);
        for selected in 0..pages.len() {
            let strip = row(tab_strip(&atlas, &pages, selected, ROW_AREA, 0.0));
            let own = strip.tabs.iter().find(|t| t.selected).unwrap();
            assert_eq!(own.index, selected);
            let mid = own.area[0] + own.area[2] / 2.0;
            assert!(
                (mid - (ROW_AREA[0] + ROW_AREA[2] / 2.0)).abs() <= 1.0,
                "{mid}"
            );
            // In order, a gap apart, none overlapping.
            for pair in strip.tabs.windows(2) {
                assert_eq!(pair[0].index + 1, pair[1].index);
                let end = pair[0].area[0] + pair[0].area[2];
                assert_eq!(pair[1].area[0] - end, GAP);
            }
            for tab in &strip.tabs {
                let d = tab.index.abs_diff(selected);
                assert_eq!(tab.alpha, FADE[d.min(FADE.len() - 1)]);
            }
        }
    }

    #[test]
    fn tabs_wholly_outside_the_area_are_left_out_and_scroll_moves_them_all() {
        let atlas = atlas();
        let pages: Vec<String> = (0..12).map(|i| format!("Page {i}")).collect();
        let strip = row(tab_strip(&atlas, &pages, 0, ROW_AREA, 0.0));
        assert!(strip.tabs.len() < pages.len());
        let right = ROW_AREA[0] + ROW_AREA[2];
        assert!(
            strip
                .tabs
                .iter()
                .all(|t| t.area[0] < right && t.area[0] + t.area[2] > ROW_AREA[0])
        );
        let moved = row(tab_strip(&atlas, &pages, 3, ROW_AREA, 30.0));
        let still = row(tab_strip(&atlas, &pages, 3, ROW_AREA, 0.0));
        let at = |s: &Row, i: usize| s.tabs.iter().find(|t| t.index == i).unwrap().area[0];
        assert_eq!(at(&moved, 3) - at(&still, 3), 30.0);
        assert_eq!(at(&moved, 4) - at(&still, 4), 30.0);
    }

    #[test]
    fn a_narrow_row_falls_back_to_the_collapsed_strip() {
        let atlas = atlas();
        let pages = names(&["General", "Camera", "Build"]);
        let own = tab_width(&atlas, "Camera");
        // Just room for the selected tab and two of the narrowest beside it.
        let fits = own + 2.0 * (MIN + GAP);
        row(tab_strip(&atlas, &pages, 1, [0.0, 0.0, fits, 12.0], 0.0));
        let narrow = [0.0, 0.0, fits - 1.0, 12.0];
        let strip = folded(tab_strip(&atlas, &pages, 1, narrow, 0.0));
        assert_eq!(strip, collapsed(&atlas, &pages, 1, narrow));
        assert_eq!(strip.label, "Camera");
    }

    #[test]
    fn a_span_wide_area_holds_every_tab_whichever_is_selected() {
        let atlas = atlas();
        let pages = names(&["General", "Camera", "Build", "Redstone", "Hotbar"]);
        let span = span(&atlas, &pages);
        for selected in 0..pages.len() {
            let area = [100.0, 0.0, span, 12.0];
            let strip = row(tab_strip(&atlas, &pages, selected, area, 0.0));
            assert_eq!(
                strip.tabs.len(),
                pages.len(),
                "a tab left out with {selected}"
            );
            for tab in &strip.tabs {
                let [x, _, w, _] = tab.area;
                assert!(
                    x >= 99.0 && x + w <= 101.0 + span,
                    "{selected}: {:?}",
                    tab.area
                );
            }
        }
    }

    #[test]
    fn a_turn_scrolls_by_the_distance_between_the_tabs() {
        let atlas = atlas();
        let pages = names(&["General", "Camera", "Build"]);
        let area = [0.0, 0.0, 400.0, 12.0];
        let at = |s: &Row, i: usize| {
            let t = s.tabs.iter().find(|t| t.index == i).unwrap();
            t.area[0] + t.area[2] / 2.0
        };
        // Turned from General to Build: scrolled by the distance, Build is where it was.
        let before = row(tab_strip(&atlas, &pages, 0, area, 0.0));
        let d = distance(&atlas, &pages, 0, 2);
        let turned = row(tab_strip(&atlas, &pages, 2, area, d));
        assert!((at(&turned, 2) - at(&before, 2)).abs() <= 1.0);
        assert_eq!(distance(&atlas, &pages, 2, 0), -d);
    }

    #[test]
    fn a_tab_crossing_the_edge_fades_out_towards_it() {
        let atlas = atlas();
        let pages: Vec<String> = (0..9).map(|i| format!("Page {i}")).collect();
        let area = [40.0, 10.0, 200.0, 12.0];
        let mut batch = HudBatch::new([854, 480]);
        let strip = tab_strip(&atlas, &pages, 4, area, 0.0);
        draw_tabs(&mut batch, &atlas, &strip, 1.0);
        for v in &batch.vertices {
            let reach = (v.pos[0] - 40.0).min(240.0 - v.pos[0]);
            if reach <= 0.01 {
                assert_eq!(v.color >> 24, 0, "opaque at the edge, {:?}", v.pos);
            }
        }
        assert!(
            batch.vertices.iter().any(|v| v.color >> 24 > 200),
            "nothing opaque inside"
        );
    }

    #[test]
    fn one_page_is_one_tab_however_narrow() {
        let atlas = atlas();
        let one = names(&["General"]);
        let strip = row(tab_strip(&atlas, &one, 0, [0.0, 0.0, 40.0, 10.0], 0.0));
        assert_eq!(strip.tabs.len(), 1);
    }

    #[test]
    fn a_row_draws_inside_its_area_and_fades_as_a_whole() {
        let atlas = atlas();
        let pages: Vec<String> = (0..9).map(|i| format!("Page {i}")).collect();
        let area = [40.0, 10.0, 200.0, 12.0];
        let strip = tab_strip(&atlas, &pages, 4, area, 13.0);
        let mut batch = HudBatch::new([854, 480]);
        draw_tabs(&mut batch, &atlas, &strip, 1.0);
        assert!(!batch.vertices.is_empty());
        for v in &batch.vertices {
            let [x, y] = v.pos;
            assert!(
                (40.0..=240.0).contains(&x) && (10.0..=22.0).contains(&y),
                "{x} {y}"
            );
        }
        let mut faded = HudBatch::new([854, 480]);
        draw_tabs(&mut faded, &atlas, &strip, 0.0);
        assert!(faded.vertices.iter().all(|v| v.color >> 24 == 0));
        // A collapsed strip fades as a whole too.
        let narrow = tab_strip(&atlas, &pages, 4, [0.0, 0.0, 80.0, 12.0], 0.0);
        let mut faded = HudBatch::new([854, 480]);
        draw_tabs(&mut faded, &atlas, &narrow, 0.0);
        assert!(!faded.vertices.is_empty());
        assert!(faded.vertices.iter().all(|v| v.color >> 24 == 0));
    }
}
