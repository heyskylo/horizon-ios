//! A controller button as the player sees it: Horizon's own art where the pad's family has one
//! (`assets/horizon/textures/gui/controller/`, which a resource pack can replace), else the label
//! printed on the pad, in its colour, on a dark key. Whatever shows a button draws and measures it
//! here; `Button::label` stays for plain text and narration.
use crate::gamepad::{Button, GlyphStyle};
use crate::hud::{HudAtlas, HudBatch, rgba};
use crate::pad_cursor;

/// The art's height as drawn, in GUI pixels. Its texture is twice that, so it maps pixel for pixel
/// at GUI scale 2 and to whole pixels at 4.
const DRAWN_HEIGHT: f32 = 16.0;
/// The clear edge the art has all round.
const MARGIN: f32 = 2.0;
/// How tall a glyph shows, art or key: the text line and 2 above and below it.
pub const HEIGHT: f32 = DRAWN_HEIGHT - 2.0 * MARGIN;
/// How wide a face button's art shows; a declared button's label leaves it room.
pub const FACE_WIDTH: f32 = 12.0;

/// A picture of a button in a family (Auto reads as Xbox, as `Button::label` has it). Every family
/// has its own file for every button, some of them a copy of another's until theirs is drawn.
struct Art {
    family: GlyphStyle,
    button: Button,
    /// The atlas sprite: where a pack's own file lands.
    sprite: &'static str,
    png: &'static [u8],
}

macro_rules! art {
    ($($family:ident $button:ident $file:literal,)*) => {
        &[$(Art {
            family: GlyphStyle::$family,
            button: Button::$button,
            sprite: concat!("horizon:gui/controller/", $file),
            png: include_bytes!(concat!(
                "../../horizon-client/assets/horizon/textures/gui/controller/",
                $file,
                ".png"
            )),
        },)*]
    };
}

/// All the art. A new picture is one line here and its file: a button not listed is text, and a
/// picture is measured from its texture, so it can be as wide as it needs.
static ART: &[Art] = art![
    Xbox South "xb_a",
    Xbox East "xb_b",
    Xbox West "xb_x",
    Xbox North "xb_y",
    Xbox LeftBumper "xb_lb",
    Xbox RightBumper "xb_rb",
    Xbox LeftTrigger "xb_lt",
    Xbox RightTrigger "xb_rt",
    Xbox Select "xb_view",
    Xbox Start "xb_menu",
    Xbox LeftStick "xb_l3",
    Xbox RightStick "xb_r3",
    Xbox DPadUp "xb_up",
    Xbox DPadDown "xb_down",
    Xbox DPadLeft "xb_left",
    Xbox DPadRight "xb_right",
    PlayStation South "psx_cross",
    PlayStation East "psx_circle",
    PlayStation West "psx_square",
    PlayStation North "psx_triangle",
    PlayStation LeftBumper "psx_l1",
    PlayStation RightBumper "psx_r1",
    PlayStation LeftTrigger "psx_l2",
    PlayStation RightTrigger "psx_r2",
    PlayStation Select "psx_create",
    PlayStation Start "psx_options",
    PlayStation LeftStick "psx_l3",
    PlayStation RightStick "psx_r3",
    PlayStation DPadUp "psx_up",
    PlayStation DPadDown "psx_down",
    PlayStation DPadLeft "psx_left",
    PlayStation DPadRight "psx_right",
    Nintendo South "nx_b",
    Nintendo East "nx_a",
    Nintendo West "nx_y",
    Nintendo North "nx_x",
    Nintendo LeftBumper "nx_l",
    Nintendo RightBumper "nx_r",
    Nintendo LeftTrigger "nx_zl",
    Nintendo RightTrigger "nx_zr",
    Nintendo Select "nx_minus",
    Nintendo Start "nx_plus",
    Nintendo LeftStick "nx_l3",
    Nintendo RightStick "nx_r3",
    Nintendo DPadUp "nx_up",
    Nintendo DPadDown "nx_down",
    Nintendo DPadLeft "nx_left",
    Nintendo DPadRight "nx_right",
];

fn find(button: Button, style: GlyphStyle) -> Option<&'static Art> {
    let family = match style {
        GlyphStyle::Auto => GlyphStyle::Xbox,
        style => style,
    };
    ART.iter()
        .find(|art| art.family == family && art.button == button)
}

/// The sprite of a button in a family, if it has one.
pub fn sprite(button: Button, style: GlyphStyle) -> Option<&'static str> {
    find(button, style).map(|art| art.sprite)
}

/// The sprite this atlas can draw for the button; none (the boot screen's atlas) means text.
pub fn art(atlas: &HudAtlas, button: Button, style: GlyphStyle) -> Option<&'static str> {
    sprite(button, style).filter(|name| atlas.sprites.contains_key(*name))
}

/// How wide the art shows: half its texture (IHDR's width, in a PNG's first bytes) less its edge.
/// A pack's own texture of another size is drawn in the same place.
fn art_width(art: &Art) -> f32 {
    let texels = u32::from_be_bytes([art.png[16], art.png[17], art.png[18], art.png[19]]);
    texels as f32 / 2.0 - 2.0 * MARGIN
}

/// How wide the glyph shows.
pub fn width(atlas: &HudAtlas, button: Button, style: GlyphStyle) -> f32 {
    match find(button, style).filter(|a| atlas.sprites.contains_key(a.sprite)) {
        Some(art) => art_width(art),
        None => HudBatch::text_width(atlas, button.label(style)).ceil() + 4.0,
    }
}

/// What the glyph covers with its label's text at `(x, y)`: `[x, y, w, h]`.
pub fn rect(atlas: &HudAtlas, button: Button, style: GlyphStyle, x: f32, y: f32) -> [f32; 4] {
    [x - 1.0, y - 2.0, width(atlas, button, style), HEIGHT]
}

/// Draw the glyph over the 8 pixel text line at `(x, y)`, filling [`rect`]. A dark key backs text
/// only: the art brings its own.
pub fn draw(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    button: Button,
    style: GlyphStyle,
    x: f32,
    y: f32,
) {
    let [left, top, wide, tall] = rect(atlas, button, style, x, y);
    if let Some(name) = art(atlas, button, style) {
        batch.sprite_scaled(
            atlas,
            name,
            left - MARGIN,
            top - MARGIN,
            wide + 2.0 * MARGIN,
            tall + 2.0 * MARGIN,
            u32::MAX,
        );
    } else {
        let [r, g, b] = button.color(style);
        batch.fill(atlas, left, top, wide, tall, rgba(40, 40, 40, 255));
        batch.text(
            atlas,
            button.label(style),
            x + 1.0,
            y,
            rgba(r, g, b, 255),
            false,
        );
    }
}

/// Horizon's art for the atlas: each `(sprite, width, height, RGBA texels)`.
pub fn images() -> impl Iterator<Item = (String, u32, u32, Vec<u8>)> {
    ART.iter()
        .map(|art| (art.sprite, art.png))
        .chain([(pad_cursor::SPRITE, pad_cursor::PNG)])
        .map(|(name, png)| {
            let (w, h, pixels) = decode(png).expect("Horizon's controller art is PNG");
            (name.to_owned(), w, h, pixels)
        })
}

fn decode(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let (w, h) = (reader.info().width, reader.info().height);
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let channels = frame.color_type.samples();
    let pixels = buf
        .chunks(channels)
        .take((w * h) as usize)
        .flat_map(|p| match channels {
            4 => [p[0], p[1], p[2], p[3]],
            3 => [p[0], p[1], p[2], 255],
            2 => [p[0], p[0], p[0], p[1]],
            _ => [p[0], p[0], p[0], 255],
        })
        .collect();
    Some((w, h, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn file(button: Button, style: GlyphStyle) -> Option<&'static str> {
        sprite(button, style).and_then(|name| name.strip_prefix("horizon:gui/controller/"))
    }

    /// The batch's quads as `[left, top, right, bottom]`.
    fn quads(batch: &HudBatch) -> Vec<[f32; 4]> {
        batch
            .vertices
            .as_chunks::<6>()
            .0
            .iter()
            .map(|quad| {
                let xs = quad.iter().map(|v| v.pos[0]);
                let ys = quad.iter().map(|v| v.pos[1]);
                [
                    xs.clone().fold(f32::MAX, f32::min),
                    ys.clone().fold(f32::MAX, f32::min),
                    xs.fold(f32::MIN, f32::max),
                    ys.fold(f32::MIN, f32::max),
                ]
            })
            .collect()
    }

    #[test]
    fn xbox_and_auto_show_the_letters_and_shoulders() {
        for style in [GlyphStyle::Xbox, GlyphStyle::Auto] {
            for (button, expected) in [
                (Button::South, "xb_a"),
                (Button::East, "xb_b"),
                (Button::West, "xb_x"),
                (Button::North, "xb_y"),
                (Button::LeftBumper, "xb_lb"),
                (Button::RightBumper, "xb_rb"),
                (Button::LeftTrigger, "xb_lt"),
                (Button::RightTrigger, "xb_rt"),
                (Button::Select, "xb_view"),
                (Button::Start, "xb_menu"),
            ] {
                assert_eq!(file(button, style), Some(expected));
            }
        }
    }

    #[test]
    fn playstation_shows_its_symbols_by_position_and_its_own_shoulders() {
        let style = GlyphStyle::PlayStation;
        assert_eq!(file(Button::South, style), Some("psx_cross"));
        assert_eq!(file(Button::East, style), Some("psx_circle"));
        assert_eq!(file(Button::West, style), Some("psx_square"));
        assert_eq!(file(Button::North, style), Some("psx_triangle"));
        assert_eq!(file(Button::LeftBumper, style), Some("psx_l1"));
        assert_eq!(file(Button::RightTrigger, style), Some("psx_r2"));
        assert_eq!(file(Button::Select, style), Some("psx_create"));
    }

    #[test]
    fn nintendo_shows_what_its_pad_prints_at_each_position() {
        let style = GlyphStyle::Nintendo;
        assert_eq!(file(Button::South, style), Some("nx_b"));
        assert_eq!(file(Button::East, style), Some("nx_a"));
        assert_eq!(file(Button::West, style), Some("nx_y"));
        assert_eq!(file(Button::North, style), Some("nx_x"));
        assert_eq!(file(Button::LeftTrigger, style), Some("nx_zl"));
        assert_eq!(file(Button::Start, style), Some("nx_plus"));
    }

    #[test]
    fn every_family_has_its_own_file_for_every_button() {
        for (style, prefix) in [
            (GlyphStyle::Xbox, "xb_"),
            (GlyphStyle::PlayStation, "psx_"),
            (GlyphStyle::Nintendo, "nx_"),
        ] {
            for button in Button::ALL {
                let name = file(button, style).unwrap();
                assert!(name.starts_with(prefix), "{button:?} {name}");
            }
        }
    }

    #[test]
    fn every_sprite_has_its_texture_at_twice_gui_size() {
        let images: Vec<_> = images().collect();
        assert_eq!(images.len(), ART.len() + 1, "the art and the cursor");
        for (name, w, h, pixels) in &images {
            assert_eq!(pixels.len(), (w * h * 4) as usize, "{name}");
            assert_eq!(*h as f32, DRAWN_HEIGHT * 2.0, "{name}");
        }
        for art in ART {
            let (_, w, ..) = images.iter().find(|i| i.0 == art.sprite).unwrap();
            assert_eq!(art_width(art), *w as f32 / 2.0 - 4.0, "{}", art.sprite);
        }
    }

    #[test]
    fn no_button_has_two_pictures_in_a_family() {
        for (i, art) in ART.iter().enumerate() {
            assert!(
                !ART[..i]
                    .iter()
                    .any(|a| a.family == art.family && a.button == art.button),
                "{}",
                art.sprite
            );
        }
    }

    #[test]
    fn art_measures_from_its_shape_and_text_from_its_label() {
        let atlas = atlas();
        assert_eq!(width(&atlas, Button::South, GlyphStyle::Xbox), 12.0);
        assert_eq!(width(&atlas, Button::LeftBumper, GlyphStyle::Xbox), 20.0);
        assert_eq!(
            width(&atlas, Button::LeftBumper, GlyphStyle::PlayStation),
            20.0
        );
        assert_eq!(width(&atlas, Button::DPadUp, GlyphStyle::Nintendo), 12.0);
        let mut no_art = atlas;
        no_art
            .sprites
            .remove(sprite(Button::LeftTrigger, GlyphStyle::Nintendo).unwrap());
        let label = HudBatch::text_width(&no_art, "ZL").ceil() + 4.0;
        assert_eq!(
            width(&no_art, Button::LeftTrigger, GlyphStyle::Nintendo),
            label
        );
    }

    #[test]
    fn an_atlas_without_the_art_falls_back_to_text() {
        let mut atlas = atlas();
        atlas
            .sprites
            .remove(sprite(Button::South, GlyphStyle::Xbox).unwrap());
        assert_eq!(art(&atlas, Button::South, GlyphStyle::Xbox), None);
        let label = HudBatch::text_width(&atlas, "A").ceil() + 4.0;
        assert_eq!(width(&atlas, Button::South, GlyphStyle::Xbox), label);
    }

    #[test]
    fn art_is_one_quad_centred_on_the_text_line_with_no_key_behind_it() {
        let atlas = atlas();
        let mut batch = HudBatch::new([640, 480]);
        draw(
            &mut batch,
            &atlas,
            Button::South,
            GlyphStyle::Xbox,
            10.0,
            20.0,
        );
        assert_eq!(quads(&batch), vec![[7.0, 16.0, 23.0, 32.0]]);
        let mut batch = HudBatch::new([640, 480]);
        draw(
            &mut batch,
            &atlas,
            Button::LeftBumper,
            GlyphStyle::Xbox,
            10.0,
            20.0,
        );
        assert_eq!(quads(&batch), vec![[7.0, 16.0, 31.0, 32.0]]);
    }

    #[test]
    fn text_sits_on_a_key_covering_what_rect_says() {
        let mut atlas = atlas();
        let style = GlyphStyle::Nintendo;
        atlas.sprites.remove(sprite(Button::South, style).unwrap());
        let mut batch = HudBatch::new([640, 480]);
        draw(&mut batch, &atlas, Button::South, style, 10.0, 20.0);
        let [x, y, w, h] = rect(&atlas, Button::South, style, 10.0, 20.0);
        assert_eq!(quads(&batch)[0], [x, y, x + w, y + h]);
    }
}
