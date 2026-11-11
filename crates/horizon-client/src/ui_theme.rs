//! The design system's widgets drawn in the game: palette, type scale, radii,
//! control sizes and fonts from `menu_style`, given at startup (in the official
//! game `crates/horizon-theme`, `docs/design-system.md`). Every menu is vanilla's (`ui`); what is left
//! here draws Horizon's own overlays that vanilla has no screen for: the
//! on-screen keyboard a controller types with (`osk`), plus the
//! `horizon.txt` preferences.
//!
//! Build a [`Ui`] over the frame's `HudBatch` with [`Ui::new`], lay out in dp
//! (the launcher's logical pixels) and give every interactive widget a stable
//! `id`. Clicks run the same layout with [`Ui::dry`] and read [`Ui::hit`]: one
//! function draws and hit-tests, so they never disagree.
use crate::hud::{HudAtlas, HudBatch, Rect};
use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont as _};
use std::{cell::RefCell, collections::HashMap, path::Path};

/// A Horizon-only preference from `horizon.txt` beside `options.txt`
/// (`key:value` lines), so vanilla's options file stays exactly as vanilla
/// writes it.
pub fn preference(dir: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("horizon.txt")).ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| v.trim().to_owned())
    })
}
/// Set (or with None remove) one preference, keeping the others.
pub(crate) fn set_preference(dir: &Path, key: &str, value: Option<&str>) {
    let text = std::fs::read_to_string(dir.join("horizon.txt")).unwrap_or_default();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.split_once(':').is_none_or(|(k, _)| k.trim() != key))
        .map(str::to_owned)
        .collect();
    if let Some(v) = value {
        lines.push(format!("{key}:{v}"));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    let _ = std::fs::write(dir.join("horizon.txt"), out);
}

/// The overlays' palette (`menu_style`) in the HUD's colour encoding, plus the
/// translucent plates the overlays draw over the game.
pub(crate) mod color {
    use horizon_client_render::menu_style::{self, OverlayColours};
    pub use menu_style::hud_colour as hex;
    fn p() -> &'static OverlayColours {
        &menu_style::get().overlay
    }
    /// The page, under every veil.
    pub fn bg() -> u32 {
        hex(p().page, 255)
    }
    /// Hover plates, secondary buttons, fields, the segmented track: the
    /// dark palette's soft plate, as ink over the veil so the plates read the
    /// same over any backdrop (over the page colour they are `plate_soft`).
    pub fn raised() -> u32 {
        hex(p().ink, 20)
    }
    /// Pressed plates and a switch that is off (`plate`, the same way).
    pub fn hover() -> u32 {
        hex(p().ink, 36)
    }
    /// One-pixel rules between rows, slider tracks.
    pub fn border() -> u32 {
        hex(p().rule, 255)
    }
    /// Text and selection (`ink`).
    pub fn text() -> u32 {
        hex(p().ink, 255)
    }
    /// Values and secondary text (`ink_soft`).
    pub fn muted() -> u32 {
        hex(p().ink_soft, 255)
    }
    pub fn accent() -> u32 {
        hex(p().accent, 255)
    }
    pub fn on_accent() -> u32 {
        hex(p().on_accent, 255)
    }
    pub(crate) fn with_alpha(c: u32, a: f32) -> u32 {
        let base = (c >> 24) as f32;
        (c & 0x00ff_ffff) | ((base * a.clamp(0.0, 1.0)).round() as u32) << 24
    }
}

/// The type scale and shapes (`menu_style`), in dp.
pub(crate) fn sizes() -> &'static horizon_client_render::menu_style::Sizes {
    &horizon_client_render::menu_style::get().sizes
}
pub(crate) fn small() -> f32 {
    sizes().caption
}
pub(crate) fn body() -> f32 {
    sizes().label
}
/// The boot screen's wordmark (`display`) and console lines (`micro`).
fn display() -> f32 {
    sizes().display
}
fn micro() -> f32 {
    sizes().micro
}
/// Fields; plates (hover rows, selection).
pub(crate) fn radius() -> f32 {
    sizes().radius
}
pub(crate) fn radius_plate() -> f32 {
    sizes().radius_plate
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Face {
    Regular,
    Medium,
    /// The wordmark and page titles (the design system's 500).
    SemiBold,
    /// Counts, timestamps and the boot screen's console lines.
    Mono,
}
#[cfg(not(test))]
const TEST_FONTS: Option<horizon_client_render::menu_style::Fonts> = None;
#[cfg(test)]
const TEST_FONTS: Option<horizon_client_render::menu_style::Fonts> =
    Some(horizon_client_render::menu_style::Fonts {
        regular: horizon_theme::fonts::GEIST_REGULAR,
        medium: horizon_theme::fonts::GEIST_MEDIUM,
        semibold: horizon_theme::fonts::GEIST_SEMIBOLD,
        mono: horizon_theme::fonts::GEIST_MONO,
    });
struct Fonts {
    faces: [FontRef<'static>; 4],
}
impl Fonts {
    /// The faces given in `menu_style`; none given, none (unit tests take the design system's).
    fn load() -> Option<Self> {
        let given = horizon_client_render::menu_style::get()
            .fonts
            .or(TEST_FONTS)?;
        let f = |b: &'static [u8]| FontRef::try_from_slice(b).expect("given font");
        Some(Self {
            faces: [
                f(given.regular),
                f(given.medium),
                f(given.semibold),
                f(given.mono),
            ],
        })
    }
    fn face(&self, f: Face) -> &FontRef<'static> {
        &self.faces[f as usize]
    }
    /// The ab_glyph scale for an em size in pixels.
    fn scale(&self, f: Face, px: f32) -> PxScale {
        let font = self.face(f);
        let upem = font.units_per_em().unwrap_or(1000.0);
        PxScale::from(px * font.height_unscaled() / upem)
    }
}
/// `text` without its `§` formatting codes (`StringDecomposer.iterateFormatted`
/// skips the `§` and the character after it, a lone trailing `§` too).
pub(crate) fn unformatted(text: &str) -> impl Iterator<Item = char> + '_ {
    let mut chars = text.chars();
    std::iter::from_fn(move || {
        loop {
            match chars.next()? {
                '§' => {
                    chars.next();
                }
                ch => return Some(ch),
            }
        }
    })
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Glyph(Face, u16, u16),
    /// A quarter disc (`false`) or a 1px quarter ring of this radius in pixels.
    Corner(u16, bool),
}
#[derive(Clone, Copy)]
struct Cached {
    rect: Option<Rect>,
    /// Offset of the bitmap from the pen (glyphs) in pixels.
    left: f32,
    top: f32,
}
/// Glyphs and corner masks rasterized into the atlas region `horizon/ui-text`.
struct Cache {
    /// None: no fonts were given, and text draws nothing.
    fonts: Option<Fonts>,
    entries: HashMap<Key, Cached>,
    shelf: (u32, u32, u32),
    pending: Vec<crate::font::Patch>,
    /// The atlas the entries were uploaded to (a new atlas starts over).
    atlas: usize,
}
thread_local! {
    static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) };
}
/// Reloaded atlases can occupy the same address with empty run-time slots.
pub fn reset_atlas() {
    CACHE.with(|cache| *cache.borrow_mut() = None);
}
fn with_cache<R>(atlas: &HudAtlas, f: impl FnOnce(&mut Cache) -> R) -> R {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let id = atlas as *const HudAtlas as usize;
        let cache = c.get_or_insert_with(|| Cache {
            fonts: Fonts::load(),
            entries: HashMap::new(),
            shelf: (0, 0, 0),
            pending: Vec::new(),
            atlas: id,
        });
        if cache.atlas != id {
            cache.atlas = id;
            cache.entries.clear();
            cache.shelf = (0, 0, 0);
            cache.pending.clear();
        }
        f(cache)
    })
}
impl Cache {
    fn region(atlas: &HudAtlas) -> Option<Rect> {
        atlas.sprites.get("horizon/ui-text").copied()
    }
    /// Reserve `w x h` pixels; on overflow forget everything and start over.
    fn place(&mut self, atlas: &HudAtlas, w: u32, h: u32) -> Option<(u32, u32, Rect)> {
        let region = Self::region(atlas)?;
        let (mut x, mut y, mut row) = self.shelf;
        if x + w + 1 > region.w {
            x = 0;
            y += row + 1;
            row = 0;
        }
        if y + h > region.h || w > region.w {
            self.entries.clear();
            self.shelf = (0, 0, 0);
            return None;
        }
        self.shelf = (x + w + 1, y, row.max(h));
        Some((x, y, region))
    }
    fn upload(&mut self, atlas: &HudAtlas, w: u32, h: u32, alpha: &[f32]) -> Option<Rect> {
        let (x, y, region) = self.place(atlas, w, h)?;
        let (ax, ay) = (
            (region.u0 * atlas.width as f32).round() as u32 + x,
            (region.v0 * atlas.height as f32).round() as u32 + y,
        );
        let rgba = alpha
            .iter()
            .flat_map(|a| [255, 255, 255, (a.clamp(0.0, 1.0) * 255.0).round() as u8])
            .collect();
        self.pending.push(crate::font::Patch {
            x: ax,
            y: ay,
            width: w,
            height: h,
            rgba,
        });
        let (aw, ah) = (atlas.width as f32, atlas.height as f32);
        Some(Rect {
            u0: ax as f32 / aw,
            v0: ay as f32 / ah,
            u1: (ax + w) as f32 / aw,
            v1: (ay + h) as f32 / ah,
            w,
            h,
        })
    }
    fn glyph(&mut self, atlas: &HudAtlas, face: Face, id: ab_glyph::GlyphId, px: f32) -> Cached {
        let size = (px * 4.0).round() as u16;
        let key = Key::Glyph(face, id.0, size);
        if let Some(c) = self.entries.get(&key) {
            return *c;
        }
        let Some(fonts) = &self.fonts else {
            return Cached {
                rect: None,
                left: 0.0,
                top: 0.0,
            };
        };
        let scale = fonts.scale(face, f32::from(size) / 4.0);
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
        let made = match fonts.face(face).outline_glyph(glyph) {
            Some(outline) => {
                let b = outline.px_bounds();
                let (w, h) = (b.width() as u32, b.height() as u32);
                let mut alpha = vec![0.0; (w * h) as usize];
                outline.draw(|x, y, c| {
                    if x < w && y < h {
                        alpha[(y * w + x) as usize] = c;
                    }
                });
                // Light text on dark backgrounds reads thin: a gentle boost.
                for a in &mut alpha {
                    *a = a.powf(0.85);
                }
                Cached {
                    rect: self.upload(atlas, w, h, &alpha),
                    left: b.min.x,
                    top: b.min.y,
                }
            }
            None => Cached {
                rect: None,
                left: 0.0,
                top: 0.0,
            },
        };
        if made.rect.is_some() || made.left == 0.0 {
            self.entries.insert(key, made);
        }
        made
    }
    fn corner(&mut self, atlas: &HudAtlas, r: u16, ring: bool) -> Option<Rect> {
        let key = Key::Corner(r, ring);
        if let Some(c) = self.entries.get(&key) {
            return c.rect;
        }
        // The top-left quadrant of a circle centred at (r, r), 4x4 supersampled.
        let n = u32::from(r);
        let rf = f32::from(r);
        let mut alpha = vec![0.0; (n * n) as usize];
        for y in 0..n {
            for x in 0..n {
                let mut hits = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = x as f32 + (sx as f32 + 0.5) / 4.0;
                        let py = y as f32 + (sy as f32 + 0.5) / 4.0;
                        let d = ((px - rf).powi(2) + (py - rf).powi(2)).sqrt();
                        if d <= rf && (!ring || d >= rf - 1.0) {
                            hits += 1;
                        }
                    }
                }
                alpha[(y * n + x) as usize] = hits as f32 / 16.0;
            }
        }
        let rect = self.upload(atlas, n, n, &alpha);
        if rect.is_some() {
            self.entries.insert(
                key,
                Cached {
                    rect,
                    left: 0.0,
                    top: 0.0,
                },
            );
        }
        rect
    }
}

/// One frame of design-system drawing, in dp: the
/// launcher's logical pixels, scaled so the screen reads like the launcher's
/// 1040x700 window at any size.
pub struct Ui<'a> {
    pub atlas: &'a HudAtlas,
    pub batch: &'a mut HudBatch,
    /// The screen in dp.
    pub w: f32,
    pub h: f32,
    /// The cursor in dp.
    pub cursor: (f32, f32),
    /// Physical pixels per dp.
    pub density: f32,
    scale: f32,
}
impl<'a> Ui<'a> {
    /// `cursor` is in GUI pixels, like every screen's input.
    pub fn new(atlas: &'a HudAtlas, batch: &'a mut HudBatch, cursor: (f32, f32)) -> Self {
        let scale = batch.scale;
        let (pw, ph) = (batch.width * scale, batch.height * scale);
        let density = (pw / 1040.0).min(ph / 700.0).clamp(1.0, 4.0);
        Self {
            atlas,
            w: pw / density,
            h: ph / density,
            cursor: (cursor.0 * scale / density, cursor.1 * scale / density),
            density,
            scale,
            batch,
        }
    }
    /// Move rasterized glyphs into the batch's atlas uploads (real frames only).
    pub fn finish(self) {
        let pending = with_cache(self.atlas, |c| std::mem::take(&mut c.pending));
        self.batch.patches.extend(pending);
        // Pack glyphs `layout` borrowed from the resource pack's font.
        self.batch.patches.extend(self.atlas.font.take_patches());
    }

    // ---- geometry, snapped to physical pixels ----
    fn px_x(&self, v: f32) -> f32 {
        (v * self.density).round()
    }
    fn px_y(&self, v: f32) -> f32 {
        (v * self.density).round()
    }
    fn raw_fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u32) {
        if x1 > x0 && y1 > y0 {
            let s = self.scale;
            self.batch.fill(
                self.atlas,
                x0 / s,
                y0 / s,
                (x1 - x0) / s,
                (y1 - y0) / s,
                color,
            );
        }
    }
    fn corner_quad(&mut self, rect: Rect, x: f32, y: f32, r: f32, flip: (bool, bool), color: u32) {
        let (u0, u1) = if flip.0 {
            (rect.u1, rect.u0)
        } else {
            (rect.u0, rect.u1)
        };
        let (v0, v1) = if flip.1 {
            (rect.v1, rect.v0)
        } else {
            (rect.v0, rect.v1)
        };
        let s = self.scale;
        let (gx, gy, gr) = (x / s, y / s, r / s);
        self.batch.quad(
            [[gx, gy], [gx + gr, gy], [gx + gr, gy + gr], [gx, gy + gr]],
            [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            color,
        );
    }
    fn corners(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, ring: bool, color: u32) {
        if r < 1.0 {
            return;
        }
        let atlas = self.atlas;
        if let Some(rect) = with_cache(atlas, |c| c.corner(atlas, r as u16, ring)) {
            self.corner_quad(rect, x0, y0, r, (false, false), color);
            self.corner_quad(rect, x1 - r, y0, r, (true, false), color);
            self.corner_quad(rect, x1 - r, y1 - r, r, (true, true), color);
            self.corner_quad(rect, x0, y1 - r, r, (false, true), color);
        }
    }
    /// A filled rectangle with rounded corners.
    pub(crate) fn rounded(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: u32) {
        if color >> 24 == 0 {
            return;
        }
        let (x0, y0, x1, y1) = (
            self.px_x(x),
            self.px_y(y),
            self.px_x(x + w),
            self.px_y(y + h),
        );
        let r = (radius * self.density)
            .round()
            .min(((x1 - x0) / 2.0).floor())
            .min(((y1 - y0) / 2.0).floor())
            .max(0.0);
        if r < 1.0 {
            self.raw_fill(x0, y0, x1, y1, color);
            return;
        }
        self.raw_fill(x0 + r, y0, x1 - r, y1, color);
        self.raw_fill(x0, y0 + r, x0 + r, y1 - r, color);
        self.raw_fill(x1 - r, y0 + r, x1, y1 - r, color);
        self.corners(x0, y0, x1, y1, r, false, color);
    }
    /// A 1px rounded border.
    pub(crate) fn border(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: u32) {
        if color >> 24 == 0 {
            return;
        }
        let (x0, y0, x1, y1) = (
            self.px_x(x),
            self.px_y(y),
            self.px_x(x + w),
            self.px_y(y + h),
        );
        let r = (radius * self.density)
            .round()
            .min(((x1 - x0) / 2.0).floor())
            .min(((y1 - y0) / 2.0).floor())
            .max(0.0);
        self.raw_fill(x0 + r, y0, x1 - r, y0 + 1.0, color);
        self.raw_fill(x0 + r, y1 - 1.0, x1 - r, y1, color);
        self.raw_fill(x0, y0 + r, x0 + 1.0, y1 - r, color);
        self.raw_fill(x1 - 1.0, y0 + r, x1, y1 - r, color);
        self.corners(x0, y0, x1, y1, r, true, color);
    }

    // ---- text ----
    /// `text` laid out at `px` pixels to the em: its advance, the face's ascent
    /// and descent, and (`draw`) its glyphs as `(x, y, w, h, rect)` from the pen
    /// on the baseline, all in physical pixels. `§` codes are dropped (the
    /// design system keeps its own colours). A character Geist lacks, such as a
    /// pack's private-use icon, is the resource pack's default-font glyph:
    /// vanilla's 9px line scaled onto Geist's, or at GUI scale as the pack
    /// draws it when every character is such a glyph (a picture title).
    fn layout(
        &self,
        face: Face,
        px: f32,
        text: &str,
        draw: bool,
    ) -> (f32, f32, f32, Vec<(f32, f32, f32, f32, Rect)>) {
        let atlas = self.atlas;
        let gui = self.scale;
        with_cache(atlas, |c| {
            let Some(fonts) = &c.fonts else {
                return (0.0, 0.0, 0.0, Vec::new());
            };
            let scale = fonts.scale(face, px);
            let (ascent, descent) = {
                let f = fonts.face(face).as_scaled(scale);
                (f.ascent(), f.descent())
            };
            let chars: Vec<(char, Option<crate::font::Glyph>)> = unformatted(text)
                .map(|ch| {
                    let pack = (!ch.is_whitespace() && fonts.face(face).glyph_id(ch).0 == 0)
                        .then(|| atlas.font.glyph(ch))
                        // The pack lacks it too (its missing glyph): Geist's box, like vanilla's.
                        .filter(|g| g.rect.is_some() || g.right <= 0.0);
                    (ch, pack)
                })
                .collect();
            let k = if chars.iter().all(|(_, g)| g.is_some()) {
                gui
            } else {
                (ascent - descent) / 9.0
            };
            let (mut pen, mut prev, mut quads) = (0.0, None, Vec::new());
            for (ch, pack) in chars {
                if let Some(g) = pack {
                    if let (true, Some(rect)) = (draw, g.rect) {
                        // Vanilla's baseline is 7 below the line's top.
                        let (x, y) = ((pen + g.left * k).round(), ((g.top - 7.0) * k).round());
                        quads.push((x, y, (g.right - g.left) * k, (g.bottom - g.top) * k, rect));
                    }
                    pen += g.advance * k;
                    prev = None;
                    continue;
                }
                let (id, advance, kern) = {
                    let Some(fonts) = &c.fonts else {
                        break;
                    };
                    let font = fonts.face(face);
                    let id = font.glyph_id(ch);
                    let f = font.as_scaled(scale);
                    (id, f.h_advance(id), prev.map_or(0.0, |p| f.kern(p, id)))
                };
                pen += kern;
                if draw && !ch.is_whitespace() {
                    let g = c.glyph(atlas, face, id, px);
                    if let Some(rect) = g.rect {
                        quads.push((
                            (pen + g.left).round(),
                            g.top,
                            rect.w as f32,
                            rect.h as f32,
                            rect,
                        ));
                    }
                }
                pen += advance;
                prev = Some(id);
            }
            (pen, ascent, descent, quads)
        })
    }
    /// The advance of `text` in dp.
    pub(crate) fn measure(&self, face: Face, size: f32, text: &str) -> f32 {
        let px = (size * self.density * 4.0).round() / 4.0;
        self.layout(face, px, text, false).0 / self.density
    }
    /// Draw a line of text in a `line`-tall box whose top is `y` (the
    /// launcher's line heights: about 1.45 × size for the interface);
    /// returns its width in dp.
    pub(crate) fn text_line(
        &mut self,
        face: Face,
        size: f32,
        line: f32,
        x: f32,
        y: f32,
        color: u32,
        text: &str,
    ) -> f32 {
        let px = (size * self.density * 4.0).round() / 4.0;
        let (x0, y0) = (self.px_x(x), self.px_y(y));
        let (width, ascent, descent, quads) = self.layout(face, px, text, color >> 24 != 0);
        let baseline = y0 + ((line * self.density - (ascent - descent)) / 2.0 + ascent).round();
        let s = self.scale;
        for (gx, gy, w, h, rect) in quads {
            self.batch.rect_uv(
                (x0 + gx) / s,
                (baseline + gy) / s,
                w / s,
                h / s,
                rect,
                color,
            );
        }
        width / self.density
    }
    /// Text in a `1.45 × size` line box whose top is `y`.
    pub(crate) fn text(
        &mut self,
        face: Face,
        size: f32,
        x: f32,
        y: f32,
        color: u32,
        text: &str,
    ) -> f32 {
        self.text_line(face, size, (size * 1.45).round(), x, y, color, text)
    }
    pub(crate) fn text_centered(
        &mut self,
        face: Face,
        size: f32,
        cx: f32,
        y: f32,
        color: u32,
        text: &str,
    ) {
        let w = self.measure(face, size, text);
        self.text(face, size, cx - w / 2.0, y, color, text);
    }

    // ---- the launcher's components ----
}

// ---- the boot screen ----

/// The boot screen's palette on the pure black: a white wordmark and dimmed
/// console lines (the theme's ink is for light pages).
const BOOT_TEXT: u32 = color::hex(0xFFFFFF, 255);
const BOOT_LOG: u32 = color::hex(0x9BA1A6, 255);

/// The boot screen while the menu is not ready yet: the wordmark centred on
/// pure black and the console's last lines at the bottom right, as the
/// terminal prints them.
pub fn draw_boot(atlas: &HudAtlas, batch: &mut HudBatch, logs: &[String]) {
    // Pure black under everything: the frame's panorama (the title artwork
    // once the bundle is in) must not leak through.
    batch.fill(
        atlas,
        0.0,
        0.0,
        batch.width,
        batch.height,
        color::hex(0x000000, 255),
    );
    wordmark(atlas, batch);
    draw_boot_log(atlas, batch, logs);
}

/// The wordmark with its tagline, centred.
fn wordmark(atlas: &HudAtlas, batch: &mut HudBatch) {
    let mut ui = Ui::new(atlas, batch, (0.0, 0.0));
    let size = (ui.w / 16.0).clamp(display(), 96.0);
    let small = (size * 0.25).clamp(micro(), 18.0);
    let block = size * 1.45 + small * 0.6 + small * 1.5;
    let top = (ui.h - block) / 2.0;
    ui.text_centered(Face::SemiBold, size, ui.w / 2.0, top, BOOT_TEXT, "Horizon");
    ui.text_centered(
        Face::Mono,
        small,
        ui.w / 2.0,
        top + size * 1.45 + small * 0.6,
        BOOT_LOG,
        "Rust based minecraft client",
    );
    ui.finish();
}

/// The console's last lines at the bottom left, over any loading screen: long
/// lines wrap word-wise, the block grows upwards.
pub fn draw_boot_log(atlas: &HudAtlas, batch: &mut HudBatch, logs: &[String]) {
    if logs.is_empty() {
        return;
    }
    let mut ui = Ui::new(atlas, batch, (0.0, 0.0));
    let (size, line, pad) = (micro(), micro() * 1.5, 16.0);
    let max_w = ui.w / 2.0;
    let max_rows = (((ui.h - pad) / line) as usize).clamp(1, 24);
    let mut y = ui.h - pad - line;
    let mut drawn = 0;
    'lines: for text in logs.iter().rev() {
        let chunks = wrapping_lines(&ui, Face::Mono, size, text, max_w);
        for chunk in chunks.iter().rev() {
            if drawn == max_rows {
                break 'lines;
            }
            ui.text(Face::Mono, size, pad, y, BOOT_LOG, chunk);
            y -= line;
            drawn += 1;
        }
    }
    ui.finish();
}

/// The lines `text` wraps into at `max_w`, word-wise; a word wider than
/// `max_w` on its own breaks at the edge.
fn wrapping_lines(ui: &Ui, face: Face, size: f32, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let mut word = word;
        loop {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if ui.measure(face, size, &candidate) <= max_w {
                line = candidate;
                break;
            }
            if line.is_empty() {
                // The word alone does not fit: hard-break it at the edge.
                let mut end = 0;
                for (i, ch) in word.char_indices() {
                    if i > 0 && ui.measure(face, size, &word[..i]) > max_w {
                        break;
                    }
                    end = i + ch.len_utf8();
                }
                lines.push(word[..end].to_owned());
                word = &word[end..];
            } else {
                lines.push(std::mem::take(&mut line));
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_reload_regenerates_text_and_corners_at_the_same_address() {
        let atlas = HudAtlas::build(Path::new("/nonexistent/horizon-test-assets")).unwrap();
        reset_atlas();
        let draw = |cache: &mut Cache| {
            let glyph = cache
                .fonts
                .as_ref()
                .unwrap()
                .face(Face::Regular)
                .glyph_id('A');
            assert!(
                cache
                    .glyph(&atlas, Face::Regular, glyph, 16.0)
                    .rect
                    .is_some()
            );
            assert!(cache.corner(&atlas, 8, false).is_some());
            std::mem::take(&mut cache.pending)
        };
        let before = with_cache(&atlas, draw);
        assert_eq!(before.len(), 2);
        assert!(with_cache(&atlas, draw).is_empty());
        reset_atlas();
        let after = with_cache(&atlas, draw);
        assert_eq!(after.len(), 2);
        for (a, b) in before.iter().zip(&after) {
            assert_eq!((a.x, a.y, a.width, a.height), (b.x, b.y, b.width, b.height));
            assert_eq!(a.rgba, b.rgba);
        }
        reset_atlas();
    }

    #[test]
    fn the_boot_atlas_holds_the_run_time_glyph_region_without_a_pack() {
        let atlas = HudAtlas::boot();
        assert_eq!((atlas.width, atlas.height), (1024, 1024));
        assert_eq!(
            atlas.sprites["horizon/ui-text"].texels((atlas.width, atlas.height)),
            (1024, 512)
        );
        assert_eq!(
            atlas.sprites["horizon/white"].texels((atlas.width, atlas.height)),
            (4, 4)
        );
    }

    #[test]
    fn the_boot_screen_centres_the_wordmark_and_left_aligns_the_console() {
        reset_atlas();
        let atlas = HudAtlas::boot();
        let mut batch = HudBatch::new([1280, 720]);
        draw_boot(&atlas, &mut batch, &[]);
        // The first quad is the pure black backdrop over the frame's panorama.
        let (w, h) = (batch.width, batch.height);
        let bg = &batch.vertices[..6];
        assert!(bg.iter().all(|v| v.color == 0xFF00_0000));
        assert!(bg.iter().all(|v| v.pos[0] == 0.0 || v.pos[0] == w));
        assert!(bg.iter().all(|v| v.pos[1] == 0.0 || v.pos[1] == h));
        assert!(
            !batch.patches.is_empty(),
            "the glyphs are queued for the atlas"
        );

        let mut batch = HudBatch::new([1280, 720]);
        wordmark(&atlas, &mut batch);
        let (min, max) = batch
            .vertices
            .iter()
            .map(|v| v.pos[0])
            .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!(
            min < batch.width / 2.0 && max > batch.width / 2.0,
            "the wordmark straddles the centre"
        );
        assert!(((min + max) / 2.0 - batch.width / 2.0).abs() < 1.0);

        let long = "[12:34:56] [Render thread/INFO]: Horizon: resources loaded in 1.0s; 1240 block states, 1500 items, 62 biomes; 8 worker threads".to_owned();
        let mut batch = HudBatch::new([1280, 720]);
        draw_boot_log(&atlas, &mut batch, &[long]);
        let (min, max) = batch
            .vertices
            .iter()
            .map(|v| v.pos[0])
            .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!(min >= 4.0, "the line stays inside the left inset");
        assert!(
            max - min <= batch.width / 2.0 + 1.0,
            "a too wide line is cut to half the screen"
        );

        let mut batch = HudBatch::new([1280, 720]);
        let ui = Ui::new(&atlas, &mut batch, (0.0, 0.0));
        let lines = wrapping_lines(
            &ui,
            Face::Mono,
            micro(),
            "the quick brown fox jumps over the lazy dog",
            90.0,
        );
        assert!(lines.len() > 1, "a long line wraps");
        assert!(
            lines
                .iter()
                .all(|l| ui.measure(Face::Mono, micro(), l) <= 90.0),
            "every wrapped line fits"
        );
        assert_eq!(
            lines
                .iter()
                .map(|l| l.split_whitespace().count())
                .sum::<usize>(),
            9,
            "the wrap keeps every word"
        );

        let mut batch = HudBatch::new([1280, 720]);
        draw_boot_log(&atlas, &mut batch, &[]);
        assert!(batch.vertices.is_empty() && batch.patches.is_empty());
        reset_atlas();
    }

    /// The quads' top-left corners and UVs `text_line` draws for `text`.
    fn drawn(atlas: &HudAtlas, text: &str) -> (f32, Vec<([f32; 2], [f32; 2])>) {
        let mut batch = HudBatch::new([1280, 720]);
        let mut ui = Ui::new(atlas, &mut batch, (0.0, 0.0));
        let w = ui.text_line(Face::Medium, body(), 20.0, 10.0, 10.0, color::text(), text);
        let quads = batch
            .vertices
            .iter()
            .step_by(6)
            .map(|v| (v.pos, v.uv))
            .collect();
        (w, quads)
    }

    #[test]
    fn section_sign_codes_are_formatting_not_text() {
        reset_atlas();
        let atlas = HudAtlas::boot();
        // A pack's lang gives `menu.options` as "§6Options..." (vanilla draws it gold).
        assert_eq!(drawn(&atlas, "§6Options..."), drawn(&atlas, "Options..."));
        assert_eq!(drawn(&atlas, "§lA§r§"), drawn(&atlas, "A"));
        reset_atlas();
    }

    #[test]
    fn a_private_use_glyph_geist_lacks_is_the_packs_bitmap() {
        let root = std::env::temp_dir().join(format!("hz-ui-pua-{}", std::process::id()));
        let font = root.join("assets/minecraft/font");
        let tex = root.join("assets/minecraft/textures/font");
        std::fs::create_dir_all(&font).unwrap();
        std::fs::create_dir_all(&tex).unwrap();
        std::fs::write(
            font.join("default.json"),
            r#"{"providers":[{"type":"bitmap","file":"minecraft:font/icon.png","ascent":7,"chars":[""]}]}"#,
        )
        .unwrap();
        let mut png = Vec::new();
        let mut enc = png::Encoder::new(&mut png, 8, 8);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[255; 256])
            .unwrap();
        std::fs::write(tex.join("icon.png"), png).unwrap();
        reset_atlas();
        let atlas = HudAtlas::build(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let icon = atlas.font.glyph('\u{e238}');
        let uv = icon
            .rect
            .map(|r| [r.u0, r.v0])
            .expect("the pack draws U+E238");
        // Alone (a picture label) and inside a Geist line, the quad is the pack's bitmap.
        for text in ["\u{e238}", "Quit \u{e238}"] {
            let (w, quads) = drawn(&atlas, text);
            assert_eq!(quads.last().unwrap().1, uv, "{text:?}");
            assert!(w > 0.0);
        }
        // Alone it is drawn as the pack draws it: 8 GUI pixels wide, advance 9.
        let (w, quads) = drawn(&atlas, "\u{e238}");
        assert_eq!(quads.len(), 1);
        let mut batch = HudBatch::new([1280, 720]);
        let ui = Ui::new(&atlas, &mut batch, (0.0, 0.0));
        assert!(
            (w * ui.density / ui.scale - icon.advance).abs() < 0.01,
            "{w}"
        );
        reset_atlas();
    }
}

pub fn touch_control(atlas: &HudAtlas, batch: &mut HudBatch, r: [f32; 4], label: &str, held: bool) {
    let scale = batch.scale;
    let mut ui = Ui::new(atlas, batch, (0.0, 0.0));
    let k = scale / ui.density;
    let [x, y, w, h] = r.map(|v| v * k);
    ui.rounded(
        x,
        y,
        w,
        h,
        w.min(h) / 2.0,
        crate::hud::rgba(0, 0, 0, if held { 120 } else { 77 }),
    );
    let size = (h / if label.contains('\n') { 3.0 } else { 2.8 })
        .min(w / label.lines().map(str::len).max().unwrap_or(1) as f32 * 1.6);
    let count = label.lines().count() as f32;
    for (i, line) in label.lines().enumerate() {
        ui.text_centered(
            Face::Regular,
            size,
            x + w / 2.0,
            y + (h - count * size * 1.45) / 2.0 + i as f32 * size * 1.45,
            crate::hud::rgba(255, 255, 255, 255),
            line,
        );
    }
    ui.finish();
}

/// Antialiased touch joystick disc, using the same masks as native controls.
pub fn touch_disc(
    atlas: &HudAtlas,
    batch: &mut HudBatch,
    centre: (f32, f32),
    radius: f32,
    color: u32,
) {
    let scale = batch.scale;
    let mut ui = Ui::new(atlas, batch, (0.0, 0.0));
    let k = scale / ui.density;
    ui.rounded(
        (centre.0 - radius) * k,
        (centre.1 - radius) * k,
        radius * 2.0 * k,
        radius * 2.0 * k,
        radius * k,
        color,
    );
    ui.finish();
}
