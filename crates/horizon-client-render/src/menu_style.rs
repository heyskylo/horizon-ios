//! The look of the screens vanilla doesn't have that the engine still draws: the settings pages
//! (`graphics_style`) and the HUD kit's tab strip. Vanilla's screens never read it.
//!
//! The layer above gives it once at startup ([`install`]), before the first frame; the
//! official game gives Horizon's design system (`docs/design-system.md`). With none given, the
//! screens draw in [`NEUTRAL`], vanilla's greys and chat colours. A read is one atomic load, and
//! only those screens make it.
use std::sync::OnceLock;

/// Colours as `0xRRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuStyle {
    pub settings: SettingsColours,
    pub overlay: OverlayColours,
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

/// Overlays drawn over the game (the HUD kit's tab strip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayColours {
    /// The dark page under text and tabs.
    pub page: u32,
    /// Muted text over a veiled backdrop.
    pub backdrop_muted: u32,
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
    },
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
