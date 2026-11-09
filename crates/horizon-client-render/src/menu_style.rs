//! The look of the screens vanilla doesn't have that the engine still draws: the settings pages
//! (`graphics_style`), the HUD kit's tab strip, and the overlays drawn with `ui_theme` (the
//! on-screen keyboard, the boot screen, the missing-resources window). Vanilla's screens never
//! read it.
//!
//! The layer above gives it once at startup ([`install`]), before the first frame; the
//! official game gives Horizon's design system (`docs/design-system.md`). With none given, the
//! screens draw in [`NEUTRAL`]: vanilla's greys and chat colours, and no text where they would
//! use their own fonts. A read is one atomic load, and only those screens make it.
use std::sync::OnceLock;

/// Colours as `0xRRGGBB`, sizes in dp (the launcher's logical pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuStyle {
    pub settings: SettingsColours,
    pub overlay: OverlayColours,
    pub sizes: Sizes,
    /// The overlays' TrueType faces; none: they draw no text.
    pub fonts: Option<Fonts>,
}

/// The settings pages: square panels of dark glass over the world, flat controls, one face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsColours {
    /// The plate behind everything, drawn see-through.
    pub page: u32,
    /// Text, and the light the panels, rules, hovers and controls are drawn in.
    pub ink: u32,
    /// Secondary text and values.
    pub ink_soft: u32,
    /// Labels, descriptions, what can't be used.
    pub muted: u32,
    /// Wells and the dark edge of small marks.
    pub outline: u32,
    /// Focus, a checked box, a slider's fill.
    pub accent: u32,
    /// The one main button, and while hovered.
    pub face: u32,
    pub face_hover: u32,
    pub success: u32,
    pub warn: u32,
    pub danger: u32,
}

/// Overlays drawn over the game (the HUD kit's tab strip, `ui_theme`'s overlays).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayColours {
    /// The dark page under text and tabs.
    pub page: u32,
    /// Muted text over a veiled backdrop.
    pub backdrop_muted: u32,
    /// Text and the light hover and pressed plates are drawn in.
    pub ink: u32,
    /// Secondary text and values.
    pub ink_soft: u32,
    /// One-pixel rules between rows.
    pub rule: u32,
    /// The main action, and its text.
    pub accent: u32,
    pub on_accent: u32,
    /// A lit choice and its bright edge (the controller dial's sector).
    pub lit: u32,
    pub lit_edge: u32,
}

/// `ui_theme`'s type scale and shapes, in dp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub caption: f32,
    pub label: f32,
    pub ui: f32,
    pub body: f32,
    pub title: f32,
    /// The boot screen's wordmark at its smallest, and its console lines.
    pub display: f32,
    pub micro: f32,
    /// Fields; plates (hover rows, selection); a large control's height.
    pub radius: f32,
    pub radius_plate: f32,
    pub control_large: f32,
}

/// TrueType faces (`ui_theme::Face`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fonts {
    pub regular: &'static [u8],
    pub medium: &'static [u8],
    pub semibold: &'static [u8],
    pub mono: &'static [u8],
}

/// The look with nothing given: vanilla's greys and chat colours.
pub const NEUTRAL: MenuStyle = MenuStyle {
    settings: SettingsColours {
        page: 0x101010,
        ink: 0xffffff,
        ink_soft: 0xe0e0e0,
        muted: 0xa0a0a0,
        outline: 0x000000,
        accent: 0xffffff,
        face: 0x555555,
        face_hover: 0x6f6f6f,
        success: 0x55ff55,
        warn: 0xffff55,
        danger: 0xff5555,
    },
    overlay: OverlayColours {
        page: 0x101010,
        backdrop_muted: 0xa0a0a0,
        ink: 0xffffff,
        ink_soft: 0xe0e0e0,
        rule: 0x303030,
        accent: 0xffffff,
        on_accent: 0x000000,
        lit: 0xffffff,
        lit_edge: 0xffffff,
    },
    sizes: Sizes {
        caption: 12.0,
        label: 13.0,
        ui: 14.0,
        body: 15.0,
        title: 28.0,
        display: 44.0,
        micro: 11.0,
        radius: 0.0,
        radius_plate: 0.0,
        control_large: 40.0,
    },
    fonts: None,
};

static STYLE: OnceLock<MenuStyle> = OnceLock::new();

/// Give the menus their look: call it once at startup. A second call keeps the first look and
/// returns false.
pub fn install(style: MenuStyle) -> bool {
    STYLE.set(style).is_ok()
}

/// The look given to [`install`], or [`NEUTRAL`].
#[inline]
pub fn get() -> &'static MenuStyle {
    STYLE.get().unwrap_or(&NEUTRAL)
}

/// `0xRRGGBB` and an alpha in the HUD's colour encoding (`0xAABBGGRR`).
pub const fn hud_colour(rgb: u32, alpha: u8) -> u32 {
    (alpha as u32) << 24 | (rgb & 0xff) << 16 | (rgb >> 8 & 0xff) << 8 | (rgb >> 16 & 0xff)
}
