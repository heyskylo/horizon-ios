//! A controller button as the player sees it: Horizon's own art where the pad's family has one
//! (`assets/horizon/textures/gui/controller/`, which a resource pack can replace), else the label
//! printed on the pad, in its colour, on a dark key. Whatever shows a button draws and measures it
//! here, from a [`Glyph`]: what the button does (`GlyphOf`), in the player's resolved style. A Nintendo
//! pad going by its letters is part of that style (`GlyphStyle::NintendoLetters`), so no caller
//! handles the letters, and nothing outside this module can turn a bare button into art or a label.
use crate::gamepad::{Button, GlyphOf, GlyphStyle, MenuJob, PadSettings};
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

/// A glyph ready to draw: the part a button plays and the player's resolved style (letter mode
/// included). It is only built from what the button does (`GlyphOf`), so a rebinding or a pad's
/// letters reach every glyph; there is no way to draw or name a bare button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    part: Button,
    style: GlyphStyle,
}

impl Glyph {
    /// The glyph for `of` on these settings in `style` (the player's: `PadSettings::glyph_style`).
    /// None while a gameplay action is unbound.
    pub fn of(of: GlyphOf, settings: &PadSettings, style: GlyphStyle) -> Option<Self> {
        let part = of.button(settings)?;
        Some(Self { part, style })
    }
    /// A menu job's glyph in `style`: menus aren't rebindable, so it is always there.
    pub fn menu(job: MenuJob, style: GlyphStyle) -> Self {
        Self {
            part: job.button(),
            style,
        }
    }
    /// A menu job's glyph by the buttons' places, whatever `style`'s letter mode: only for a
    /// question a controller nobody holds yet answers (splitscreen's join and lost-controller
    /// prompts), whose letter mode isn't known (docs/controllers.md, Nintendo pads).
    pub fn unheld(job: MenuJob, style: GlyphStyle) -> Self {
        Self::menu(job, style.by_position())
    }
    /// The part this glyph is for (what the pad reads, after any letters), for `pad status`.
    pub fn part(self) -> Button {
        self.part
    }
    /// What is printed on the button that plays the part: in letter mode, that button's letter.
    pub fn label(self) -> &'static str {
        printed(self.part, self.style)
    }
    /// The colour (RGB) of the button that plays the part.
    pub fn color(self) -> [u8; 3] {
        colour(self.part, self.style)
    }
}

/// What is printed on the button for a pad family (`Auto` reads as Xbox). For
/// `NintendoLetters` it is the print of the button that plays this part (`GlyphStyle::on_pad`).
fn printed(part: Button, style: GlyphStyle) -> &'static str {
    let (style, button) = style.on_pad(part);
    let i = button as usize;
    match style {
        GlyphStyle::PlayStation => [
            "×", "○", "□", "△", "L1", "R1", "L2", "R2", "Create", "Options", "L3", "R3", "↑", "↓",
            "←", "→",
        ][i],
        GlyphStyle::Nintendo | GlyphStyle::NintendoLetters => [
            "B", "A", "Y", "X", "L", "R", "ZL", "ZR", "−", "+", "LS", "RS", "↑", "↓", "←", "→",
        ][i],
        GlyphStyle::Xbox | GlyphStyle::Auto => [
            "A", "B", "X", "Y", "LB", "RB", "LT", "RT", "View", "Menu", "LS", "RS", "↑", "↓", "←",
            "→",
        ][i],
    }
}
/// The glyph's colour (RGB): face buttons in their family's colours,
/// the rest white.
fn colour(part: Button, style: GlyphStyle) -> [u8; 3] {
    let (style, button) = style.on_pad(part);
    let face = match button {
        Button::South => 0,
        Button::East => 1,
        Button::West => 2,
        Button::North => 3,
        _ => return [255, 255, 255],
    };
    match style {
        GlyphStyle::PlayStation => [
            [124, 178, 232],
            [235, 110, 110],
            [222, 145, 205],
            [92, 205, 170],
        ][face],
        GlyphStyle::Nintendo | GlyphStyle::NintendoLetters => [255, 255, 255],
        GlyphStyle::Xbox | GlyphStyle::Auto => [
            [110, 200, 90],
            [235, 90, 80],
            [80, 150, 240],
            [240, 200, 60],
        ][face],
    }
}

/// The art for `part`: in letter mode, the picture of the button that plays it (`on_pad`).
fn find(part: Button, style: GlyphStyle) -> Option<&'static Art> {
    let (family, button) = style.on_pad(part);
    let family = match family {
        GlyphStyle::Auto => GlyphStyle::Xbox,
        family => family,
    };
    ART.iter()
        .find(|art| art.family == family && art.button == button)
}

/// The sprite of a part in a style, if it has one.
fn sprite(part: Button, style: GlyphStyle) -> Option<&'static str> {
    find(part, style).map(|art| art.sprite)
}

/// The sprite this atlas can draw for the glyph; none (the boot screen's atlas) means text.
pub fn art(atlas: &HudAtlas, glyph: Glyph) -> Option<&'static str> {
    sprite(glyph.part, glyph.style).filter(|name| atlas.sprites.contains_key(*name))
}

/// How wide the art shows: half its texture (IHDR's width, in a PNG's first bytes) less its edge.
/// A pack's own texture of another size is drawn in the same place.
fn art_width(art: &Art) -> f32 {
    let texels = u32::from_be_bytes([art.png[16], art.png[17], art.png[18], art.png[19]]);
    texels as f32 / 2.0 - 2.0 * MARGIN
}

/// How wide the glyph shows, in GUI pixels.
pub fn width(atlas: &HudAtlas, glyph: Glyph) -> f32 {
    match find(glyph.part, glyph.style).filter(|a| atlas.sprites.contains_key(a.sprite)) {
        Some(art) => art_width(art),
        None => HudBatch::text_width(atlas, glyph.label()).ceil() + 4.0,
    }
}

/// What the glyph covers with its label's text at `(x, y)`: `[x, y, w, h]` in GUI pixels.
pub fn rect(atlas: &HudAtlas, glyph: Glyph, x: f32, y: f32) -> [f32; 4] {
    [x - 1.0, y - 2.0, width(atlas, glyph), HEIGHT]
}

/// Draw the glyph over the 8 pixel text line at `(x, y)`, filling [`rect`]. A dark key backs text
/// only: the art brings its own.
pub fn draw(batch: &mut HudBatch, atlas: &HudAtlas, glyph: Glyph, x: f32, y: f32) {
    let [left, top, wide, tall] = rect(atlas, glyph, x, y);
    if let Some(name) = art(atlas, glyph) {
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
        let [r, g, b] = glyph.color();
        batch.fill(atlas, left, top, wide, tall, rgba(40, 40, 40, 255));
        batch.text(atlas, glyph.label(), x + 1.0, y, rgba(r, g, b, 255), false);
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

    /// A glyph for any part: the tests reach the table directly.
    fn g(part: Button, style: GlyphStyle) -> Glyph {
        Glyph { part, style }
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
        assert_eq!(width(&atlas, g(Button::South, GlyphStyle::Xbox)), 12.0);
        assert_eq!(width(&atlas, g(Button::LeftBumper, GlyphStyle::Xbox)), 20.0);
        assert_eq!(
            width(&atlas, g(Button::LeftBumper, GlyphStyle::PlayStation)),
            20.0
        );
        assert_eq!(width(&atlas, g(Button::DPadUp, GlyphStyle::Nintendo)), 12.0);
        let mut no_art = atlas;
        no_art
            .sprites
            .remove(sprite(Button::LeftTrigger, GlyphStyle::Nintendo).unwrap());
        let label = HudBatch::text_width(&no_art, "ZL").ceil() + 4.0;
        assert_eq!(
            width(&no_art, g(Button::LeftTrigger, GlyphStyle::Nintendo)),
            label
        );
    }

    #[test]
    fn an_atlas_without_the_art_falls_back_to_text() {
        let mut atlas = atlas();
        atlas
            .sprites
            .remove(sprite(Button::South, GlyphStyle::Xbox).unwrap());
        assert_eq!(art(&atlas, g(Button::South, GlyphStyle::Xbox)), None);
        let label = HudBatch::text_width(&atlas, "A").ceil() + 4.0;
        assert_eq!(width(&atlas, g(Button::South, GlyphStyle::Xbox)), label);
    }

    #[test]
    fn art_is_one_quad_centred_on_the_text_line_with_no_key_behind_it() {
        let atlas = atlas();
        let mut batch = HudBatch::new([640, 480]);
        draw(
            &mut batch,
            &atlas,
            g(Button::South, GlyphStyle::Xbox),
            10.0,
            20.0,
        );
        assert_eq!(quads(&batch), vec![[7.0, 16.0, 23.0, 32.0]]);
        let mut batch = HudBatch::new([640, 480]);
        draw(
            &mut batch,
            &atlas,
            g(Button::LeftBumper, GlyphStyle::Xbox),
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
        draw(&mut batch, &atlas, g(Button::South, style), 10.0, 20.0);
        let [x, y, w, h] = rect(&atlas, g(Button::South, style), 10.0, 20.0);
        assert_eq!(quads(&batch)[0], [x, y, x + w, y + h]);
    }

    /// The part a pad going by its letters reads when `button` is pressed (`PadState::by_letter`,
    /// as `Pads` reads the pad), so the expectation doesn't come from the glyph code.
    fn part_read_from(button: Button) -> Button {
        let mut pad = crate::gamepad::PadState::default();
        pad.set(button, true);
        let read = pad.by_letter();
        Button::ALL.into_iter().find(|b| read.held(*b)).unwrap()
    }

    #[test]
    fn letter_mode_shows_the_button_that_plays_each_part() {
        let atlas = atlas();
        for button in Button::ALL {
            let part = part_read_from(button);
            let letters = g(part, GlyphStyle::NintendoLetters);
            let printed = g(button, GlyphStyle::Nintendo);
            assert_eq!(letters.label(), printed.label(), "{button:?}");
            assert_eq!(art(&atlas, letters), art(&atlas, printed), "{button:?}");
        }
        // As a Switch: the A on the right confirms and jumps.
        assert_eq!(g(Button::South, GlyphStyle::NintendoLetters).label(), "A");
        assert_eq!(
            file(Button::South, GlyphStyle::NintendoLetters),
            Some("nx_a")
        );
    }

    #[test]
    fn position_mode_names_each_part_by_its_place() {
        assert_eq!(g(Button::South, GlyphStyle::Nintendo).label(), "B");
        assert_eq!(g(Button::South, GlyphStyle::Xbox).label(), "A");
        assert_eq!(g(Button::South, GlyphStyle::PlayStation).label(), "×");
        assert_eq!(
            g(Button::RightTrigger, GlyphStyle::PlayStation).label(),
            "R2"
        );
        assert_eq!(g(Button::DPadLeft, GlyphStyle::Xbox).label(), "←");
        // The splitscreen exception keeps places whatever the player's letters.
        let unheld = Glyph::unheld(MenuJob::Confirm, GlyphStyle::NintendoLetters);
        assert_eq!(unheld.label(), "B");
    }

    #[test]
    fn rebinding_an_action_changes_its_glyph() {
        let mut settings = PadSettings::default();
        let jump = GlyphOf::Bound("key.jump");
        let before = Glyph::of(jump, &settings, GlyphStyle::Xbox).unwrap();
        settings.bind("key.jump", Some(Button::North));
        let after = Glyph::of(jump, &settings, GlyphStyle::Xbox).unwrap();
        assert_ne!(before.label(), after.label());
        assert_eq!(after.label(), "Y");
        settings.bind("key.jump", None);
        assert_eq!(Glyph::of(jump, &settings, GlyphStyle::Xbox), None);
    }

    /// Glyphs are asked for by what the button does. The one way round that is `Glyph::unheld`,
    /// splitscreen's questions to a controller nobody holds yet: a new use has to be argued for
    /// here and in docs/ui-kit.md.
    #[test]
    fn only_the_documented_exception_draws_by_place() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = Vec::new();
        for dir in ["horizon-client/src", "horizon-client-render/src"] {
            let mut stack = vec![root.join(dir)];
            while let Some(path) = stack.pop() {
                if path.is_dir() {
                    stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("pad_glyph.rs")
                    && std::fs::read_to_string(&path)
                        .unwrap()
                        .contains("Glyph::unheld(")
                {
                    found.push(path.strip_prefix(&root).unwrap().to_owned());
                }
            }
        }
        assert_eq!(
            found,
            [std::path::PathBuf::from(
                "horizon-client/src/game/split/hud.rs"
            )]
        );
    }
}
