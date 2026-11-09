//! Fitting text: a line cut to the room it has, a paragraph wrapped to a width, and a line that
//! fits across a circle. Every width is in GUI px, measured in the HUD atlas's font
//! (`Font::width`), so what fits is what draws.
//!
//! ```
//! use horizon_client_render::hud::HudAtlas;
//! use horizon_client_render::hud_kit::fit;
//!
//! let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
//! // Room enough: the same text, not a copy.
//! assert_eq!(fit::ellipsize(&atlas, "Jump", 100.0), "Jump");
//! // Too long: cut, with an ellipsis, inside the width.
//! let cut = fit::ellipsize(&atlas, "Release to Shoot", 40.0);
//! assert!(cut.ends_with(fit::ELLIPSIS) && atlas.font.width(&cut) <= 40.0);
//! // Across a circle of radius 20, 16 px above or below its centre, there are 24 px.
//! assert_eq!(fit::chord(20.0, 16.0), 24.0);
//! ```
//!
//! Vanilla's screens cut text their own way (`screen_draw::clip`, `Font.plainSubstrByWidth`) and
//! slide what doesn't fit (`screen_draw::scrolling_text`); those stay vanilla's. This is for
//! Horizon's own overlays and settings.

use crate::hud::HudAtlas;
use std::borrow::Cow;

/// What a cut line ends with.
pub const ELLIPSIS: &str = "...";

/// `text` if it fits in `width` GUI px, else its longest start that fits with [`ELLIPSIS`]
/// after it, trailing spaces dropped. Too narrow for even the ellipsis, it is the ellipsis alone.
///
/// One walk over the characters, adding their advances as `Font::fit` does, with the ellipsis's
/// advance on top. Text with `§` codes or right-to-left characters is measured whole at each
/// length instead, since codes take no room and right-to-left text is shaped as it draws.
pub fn ellipsize<'a>(atlas: &HudAtlas, text: &'a str, width: f32) -> Cow<'a, str> {
    let font = &atlas.font;
    if font.width(text) <= width {
        return Cow::Borrowed(text);
    }
    // Plain left-to-right text, the common case: its width is the sum of its advances, rounded
    // up (`Font::width`), and so is that of every start of it with the ellipsis after.
    let plain = !text.contains('§') && matches!(crate::bidi::visual_str(text), Cow::Borrowed(_));
    if !plain {
        return Cow::Owned(ellipsize_shaped(atlas, text, width));
    }
    let tail: Vec<f32> = ELLIPSIS.chars().map(|c| font.glyph(c).advance).collect();
    let mut start = 0.0_f32;
    let mut end = 0;
    for (i, c) in text.char_indices() {
        let longer = start + font.glyph(c).advance;
        // Summed in the order `Font::width` sums the longer start and the ellipsis.
        if tail.iter().fold(longer, |w, a| w + a).ceil() > width {
            break;
        }
        start = longer;
        end = i + c.len_utf8();
    }
    Cow::Owned(format!("{}{ELLIPSIS}", text[..end].trim_end()))
}

/// [`ellipsize`] for text whose width isn't the sum of its characters': each longer start is
/// measured whole, as it would draw.
fn ellipsize_shaped(atlas: &HudAtlas, text: &str, width: f32) -> String {
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        out.push_str(ELLIPSIS);
        let wide = atlas.font.width(&out) > width;
        out.truncate(out.len() - ELLIPSIS.len());
        if wide {
            out.pop();
            break;
        }
    }
    format!("{}{ELLIPSIS}", out.trim_end())
}

/// `text`'s words in lines no wider than `max` by `width` (GUI px), broken between words. A word
/// wider than `max` keeps a line of its own; runs of spaces count as one.
pub fn wrap(text: &str, max: f32, width: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let longer = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if width(&longer) > max && !line.is_empty() {
            lines.push(std::mem::replace(&mut line, word.to_owned()));
        } else {
            line = longer;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// How wide a circle of radius `r` is `dy` above or below its centre (GUI px): 0 at or past its
/// edge.
pub fn chord(r: f32, dy: f32) -> f32 {
    2.0 * (r * r - dy * dy).max(0.0).sqrt()
}

/// How wide a line can be inside a circle of radius `r` when it spans `top` to `top + height`
/// from the circle's centre (negative above it): the chord where the line is farthest from the
/// centre, so both its corners on each side stay inside.
pub fn room_in_circle(r: f32, top: f32, height: f32) -> f32 {
    chord(r, top.abs().max((top + height).abs()))
}

/// `text` [ellipsized](ellipsize) to fit across a circle of radius `r`, on a line from `top` to
/// `top + height` (GUI px from the circle's centre, negative above it), with `inset` kept clear
/// at each end. Centre it on the circle: `x = cx - width / 2`.
pub fn fit_in_circle<'a>(
    atlas: &HudAtlas,
    text: &'a str,
    r: f32,
    top: f32,
    height: f32,
    inset: f32,
) -> Cow<'a, str> {
    let room = (room_in_circle(r, top, height) - 2.0 * inset).max(0.0);
    ellipsize(atlas, text, room)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    /// `graphics_style::fit` as it was before the kit, the oracle `ellipsize` must match: a
    /// whole `Font::width` for every longer start.
    fn old_fit(atlas: &HudAtlas, text: &str, width: f32) -> String {
        if atlas.font.width(text) <= width {
            return text.to_owned();
        }
        let mut out = String::new();
        for c in text.chars() {
            if atlas.font.width(&format!("{out}{c}...")) > width {
                break;
            }
            out.push(c);
        }
        format!("{}...", out.trim_end())
    }

    /// Every Horizon string in a few languages, long and short, with `§` codes and right-to-left
    /// text among them, and a few made-up edge cases.
    fn corpus() -> Vec<String> {
        let lang = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../horizon-client/assets/horizon/lang");
        let mut texts: Vec<String> = [
            "en_us", "de_de", "fr_fr", "ru_ru", "ja_jp", "ta_in", "ar_sa",
        ]
        .iter()
        .flat_map(|code| {
            let file = std::fs::read_to_string(lang.join(format!("{code}.json"))).unwrap();
            let map: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&file).unwrap();
            map.into_values()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect();
        texts.extend(
            [
                "",
                " ",
                "a",
                "trailing space before the cut          x",
                "§cRed §lbold§r text that runs on and on",
                "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWW",
                "iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii",
                "...already ends with dots...",
            ]
            .map(str::to_owned),
        );
        texts
    }

    #[test]
    fn ellipsize_cuts_every_string_as_the_old_fit_did() {
        let atlas = atlas();
        let mut cut = 0;
        for text in corpus() {
            for width in (-4..=160).map(|w| w as f32 * 1.5) {
                let new = ellipsize(&atlas, &text, width);
                assert_eq!(new, old_fit(&atlas, &text, width), "{text:?} at {width}");
                cut += usize::from(matches!(new, Cow::Owned(_)));
            }
        }
        // The corpus does reach the cut, not only text that fits.
        assert!(cut > 10_000, "only {cut} cuts");
    }

    #[test]
    fn a_cut_line_fits_its_room_and_one_that_fits_is_not_copied() {
        let atlas = atlas();
        let text = "Release to Shoot";
        let whole = atlas.font.width(text);
        assert!(matches!(ellipsize(&atlas, text, whole), Cow::Borrowed(t) if t == text));
        for width in [whole - 1.0, whole / 2.0, 20.0] {
            let cut = ellipsize(&atlas, text, width);
            assert!(cut.ends_with(ELLIPSIS), "{cut:?}");
            assert!(
                atlas.font.width(&cut) <= width,
                "{cut:?} wider than {width}"
            );
            assert!(text.starts_with(cut.trim_end_matches(ELLIPSIS)));
        }
        // No room even for the dots: the dots, so the cut still shows.
        assert_eq!(ellipsize(&atlas, text, 0.0), ELLIPSIS);
    }

    #[test]
    fn wrap_breaks_between_words_within_the_width() {
        // Each character 6 wide: no line over 60.
        let width = |s: &str| 6.0 * s.len() as f32;
        let lines = wrap("This server needs each player's own account", 60.0, width);
        assert!(lines.iter().all(|l| width(l) <= 60.0));
        assert_eq!(
            lines.join(" "),
            "This server needs each player's own account"
        );
        // A word longer than the line keeps a line of its own.
        assert_eq!(
            wrap("a verylongwordindeed b", 30.0, width),
            ["a", "verylongwordindeed", "b"]
        );
        assert!(wrap("", 60.0, width).is_empty());
    }

    #[test]
    fn a_chord_is_the_circle_s_width_at_that_height() {
        // A 3-4-5 triangle: 4 either side of the centre line, 3 off the centre.
        assert_eq!(chord(5.0, 3.0), 8.0);
        assert_eq!(chord(5.0, -3.0), 8.0);
        assert_eq!(chord(5.0, 0.0), 10.0);
        assert_eq!(chord(5.0, 5.0), 0.0);
        assert_eq!(chord(5.0, 7.0), 0.0);
        // A line across the centre is as narrow as its farther edge; one below it, its lower.
        assert_eq!(room_in_circle(5.0, -3.0, 4.0), 8.0);
        assert_eq!(room_in_circle(5.0, -1.0, 4.0), 8.0);
        assert_eq!(room_in_circle(5.0, 6.0, 2.0), 0.0);
    }

    #[test]
    fn a_line_in_a_circle_gets_shorter_away_from_its_centre() {
        let atlas = atlas();
        let text = "Courtly Bow and a long tail";
        let (r, line) = (40.0, 9.0);
        let mut last = f32::INFINITY;
        for top in [-line / 2.0, 10.0, 20.0, 30.0] {
            let fitted = fit_in_circle(&atlas, text, r, top, line, 2.0);
            let wide = atlas.font.width(&fitted);
            assert!(
                wide <= room_in_circle(r, top, line) - 4.0 || fitted == ELLIPSIS,
                "{fitted:?} at {top}"
            );
            assert!(wide <= last, "{fitted:?} wider than the line above it");
            last = wide;
        }
        // At the centre a short label is whole.
        assert_eq!(
            fit_in_circle(&atlas, "Bow", r, -line / 2.0, line, 2.0),
            "Bow"
        );
    }
}
