//! The launcher's view of Horizon's design tokens (`crates/horizon-theme`, described in
//! `docs/design-system.md`): the night look (`horizon_theme::night`, a night sky or a dawn with
//! Horizon's violet) in Bricolage Grotesque, with Geist Mono for paths and codes (all bundled, SIL
//! Open Font License). Screens not rebuilt yet draw the Blocks parts in the night's colours.
use gpui::{rgb, Hsla, WindowAppearance};
use horizon_launcher::backend::{Appearance, Settings};
use horizon_launcher::themes::{self, Accent};
use horizon_theme as tokens;
use std::time::Duration;

pub use tokens::night::{self, Night};
pub use tokens::{Step, CAPTION, DISPLAY, HEADING, LABEL, MICRO, TITLE, UI, WORDMARK};
pub const FONT: &str = night::FONT;
pub const DISPLAY_FONT: &str = night::DISPLAY_FONT;
pub const MONO: &str = tokens::MONO;
/// What the Blocks parts set in their pixel face (headings, navigation, buttons) until their
/// screens are rebuilt: the night's UI family.
pub const PIXEL: &str = night::FONT;

pub use tokens::{BLOCK_BEVEL as BEVEL, BLOCK_LIP as LIP};
/// Shapes and controls, shared with the game's menus and the website.
pub use tokens::{CONTROL_LARGE, DISABLED, FIELD, PRESSED, RISE, ROW, SEGMENT};

/// Blocks have square corners: these stay for the places that ask for a radius.
pub const RADIUS: f32 = 0.;
pub const RADIUS_PLATE: f32 = 0.;
/// Buttons are a little taller than the quiet look's, for their lip.
pub const CONTROL: f32 = 34.;
/// The top bar, and the sidebar of instances and servers.
pub const TOPBAR: f32 = 56.;
pub const SIDEBAR: f32 = 248.;
pub const PAGE_GUTTER: f32 = 32.;

/// Motion: short and eased out, never bouncy.
pub const QUICK: Duration = tokens::QUICK.duration();
pub const MOVE: Duration = tokens::MOVE.duration();
pub const ENTER: Duration = tokens::ENTER.duration();
pub const STAGGER: Duration = tokens::STAGGER;

#[derive(Clone, Copy)]
pub struct Theme {
    /// The glass look's colours for the accent (the dark appearance draws it).
    pub look: tokens::glass::Glass,
    /// The page behind everything the sky doesn't cover.
    pub page: Hsla,
    /// Hover plates, fields and the console: the raised panel colour.
    pub plate_soft: Hsla,
    /// Pressed plates, a switch that is off: the button colour.
    pub plate: Hsla,
    /// Dialogs and menus above the page: the panel colour.
    pub surface: Hsla,
    pub rule: Hsla,
    pub ink: Hsla,
    pub ink_soft: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
    pub warn: Hsla,
    /// The blocky parts (`horizon_theme::Blocks`).
    pub side: Hsla,
    pub panel: Hsla,
    pub raised: Hsla,
    pub button: Hsla,
    pub button_bevel: Hsla,
    pub well: Hsla,
    pub bevel: Hsla,
    pub lip: Hsla,
    pub outline: Hsla,
    pub face: Hsla,
    pub face_bevel: Hsla,
    pub face_lip: Hsla,
    pub on_face: Hsla,
    /// The night look's own tokens (the sky, plates with their opacity), for rebuilt screens.
    pub night: Night,
    /// The accent this look was built from, before the glass look's mixes.
    pub accent_choice: Accent,
    pub dark: bool,
    /// Whether the pages draw the redesign's glass look: both appearances do (light with ink
    /// for white and light plates, `glass_ui::light`).
    pub glass: bool,
}

/// The light appearance's look: the soft accent (links, ticks, the sliders) is the accent
/// itself, which reads on white where the dark look's half-white mix would not.
fn light_look(mut look: tokens::glass::Glass) -> tokens::glass::Glass {
    look.accent_soft = look.accent;
    look
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Whether the launcher is pinned dark (`Some(true)`) or light, or follows the system (`None`):
/// `HORIZON_LAUNCHER_APPEARANCE=light` or `dark` (for screenshots and tests), else the player's
/// choice in Settings.
pub fn pinned_dark(choice: Appearance) -> Option<bool> {
    match std::env::var("HORIZON_LAUNCHER_APPEARANCE").as_deref() {
        Ok("light") => Some(false),
        Ok("dark") => Some(true),
        _ => match choice {
            Appearance::Dark => Some(true),
            Appearance::Light => Some(false),
            Appearance::System => None,
        },
    }
}

/// The look Settings ask for: the appearance, accent and launcher theme, at the player's Glass
/// and Blur.
pub fn from_settings(settings: &Settings, system: WindowAppearance) -> Theme {
    let mut theme = Theme::for_appearance(settings.appearance, settings.accent, system);
    if let Some(picked) = themes::theme(settings.theme.as_deref()) {
        theme = theme.with_launcher_theme(picked);
    }
    theme.with_glass(settings.glass_tint(), settings.glass_blur())
}

impl Theme {
    /// The pinned look (`pinned_dark`: dark unless the player picked another), the system's when
    /// they asked to follow it, in the player's accent.
    /// A launcher theme from Settings: its sky, or its whole palette (dark and light) when it
    /// brings one, keeping the player's Glass and Blur.
    pub fn with_launcher_theme(mut self, picked: &themes::Theme) -> Self {
        let Some((dark, light)) = picked.palette else {
            self.night.sky = picked.sky;
            return self;
        };
        let night = if self.dark { *dark } else { *light };
        let accent = self.accent_choice;
        // A theme with its own palette draws the square, solid Blocks look: no glass plates, no
        // rounded corners. The glass look's own colours stay the mockup's violet ones.
        let mut themed = Self::from(accent.apply(night.blocks()), night, accent);
        themed.look = tokens::glass::Glass::new(accent.rgb(), self.look.tint, self.look.blur);
        if !themed.dark {
            themed.look = light_look(themed.look);
        }
        themed
    }

    /// The glass look with the player's Glass (how much white the plates take, 0 to 1) and Blur
    /// (px).
    pub fn with_glass(mut self, tint: f32, blur: f32) -> Self {
        self.look = tokens::glass::Glass::new(self.look.accent, tint, blur);
        if !self.dark {
            self.look = light_look(self.look);
        }
        self
    }

    pub fn for_appearance(choice: Appearance, accent: Accent, system: WindowAppearance) -> Self {
        let system_dark = !matches!(
            system,
            WindowAppearance::Light | WindowAppearance::VibrantLight
        );
        let dark = pinned_dark(choice).unwrap_or(system_dark);
        let night = if dark { night::NIGHT } else { night::DAWN };
        Self::from(accent.apply(night.blocks()), night, accent).glass(accent)
    }

    /// The dark appearance draws the glass look ("Glass" in `docs/design-system.md`): what the
    /// pages not rebuilt for it yet draw with these tokens comes out as the mockup's parts do,
    /// white text and fills at a few opacities, no borders, no bevels.
    fn glass(self, accent: Accent) -> Self {
        use horizon_theme::glass::{fill, ink, Glass, BLUR, DANGER, LIVE, TINT, WARN};
        let dark = self.dark;
        // Light: the ink where dark has white, as `glass_ui::white` does.
        let white = |a: f32| {
            if dark {
                gpui::white().opacity(a)
            } else {
                c(horizon_theme::glass::ON_LIGHT).opacity(a)
            }
        };
        let none = gpui::transparent_black();
        let look = Glass::new(accent.rgb(), TINT, BLUR);
        let look = if dark { look } else { light_look(look) };
        Self {
            plate_soft: white(fill::ROW_HOVER),
            plate: white(fill::BUTTON),
            rule: white(fill::BUTTON),
            ink: white(ink::FULL),
            ink_soft: white(ink::SOFT),
            muted: white(ink::QUIET),
            accent: c(look.accent_soft),
            on_accent: c(look.on_accent),
            // Light: `glass_ui::danger`, `live` and `warn`'s darker shades.
            danger: c(if dark { DANGER } else { 0xdc2626 }),
            success: c(if dark { LIVE } else { 0x15803d }),
            warn: c(if dark { WARN } else { 0xb45309 }),
            panel: white(0.04),
            raised: white(fill::ROW_HOVER),
            button: white(fill::BUTTON),
            button_bevel: none,
            well: white(fill::FIELD),
            bevel: none,
            lip: none,
            outline: none,
            face: c(look.accent),
            face_bevel: none,
            face_lip: none,
            on_face: c(look.on_accent),
            look,
            accent_choice: accent,
            glass: true,
            ..self
        }
    }

    fn from(p: tokens::Blocks, night: Night, accent: Accent) -> Self {
        Self {
            look: tokens::glass::Glass::default_look(),
            page: c(p.page),
            plate_soft: c(p.raised),
            plate: c(p.button),
            surface: c(p.panel),
            rule: c(p.rule),
            ink: c(p.ink),
            ink_soft: c(p.ink_soft),
            muted: c(p.muted),
            accent: c(p.accent),
            on_accent: c(p.on_face),
            danger: c(p.danger),
            success: c(p.success),
            warn: c(p.warn),
            side: c(p.side),
            panel: c(p.panel),
            raised: c(p.raised),
            button: c(p.button),
            button_bevel: c(p.button_bevel),
            well: c(p.well),
            bevel: c(p.bevel),
            lip: c(p.lip),
            outline: c(p.outline),
            face: c(p.face),
            face_bevel: c(p.face_bevel),
            face_lip: c(p.face_lip),
            on_face: c(p.on_face),
            night,
            accent_choice: accent,
            dark: p.dark,
            glass: false,
        }
    }
}

/// How strongly a selected suggestion in a text field is tinted with the accent.
pub const SELECTION: f32 = 0.22;

#[cfg(test)]
mod tests {
    use super::{from_settings, tokens, SELECTION};
    use gpui::WindowAppearance;
    use horizon_launcher::backend::{Appearance, Settings};
    use horizon_launcher::themes::Accent;

    /// Nord asks for the solid, square Blocks look: no glass, and its own polar-night palette
    /// with the frost accent rather than Horizon's violet glass colours.
    #[test]
    fn a_palette_theme_is_solid_and_square() {
        let settings = Settings {
            appearance: Appearance::Dark,
            accent: Accent::from_rgb(0x88c0d0),
            theme: Some("nord".into()),
            ..Settings::default()
        };
        let theme = from_settings(&settings, WindowAppearance::Dark);
        assert!(!theme.glass, "Nord draws the blocks look");
        assert!(theme.dark);
        assert_eq!(theme.night.page, 0x2e3440);
        assert_eq!(theme.night.accent, 0x88c0d0);
        assert_eq!(theme.accent_choice.rgb(), 0x88c0d0);
    }

    /// `top` at `alpha` over `under`, as the GPU blends them.
    fn blend(top: u32, under: u32, alpha: f32) -> u32 {
        let channel = |shift: u32| {
            let (a, b) = ((top >> shift & 0xff) as f32, (under >> shift & 0xff) as f32);
            ((a * alpha + b * (1. - alpha)).round() as u32) << shift
        };
        channel(16) | channel(8) | channel(0)
    }

    /// What the Servers page draws besides text (whose colours horizon-theme's own test holds to
    /// WCAG AA on every background): the keyboard focus ring, the borders of a focused or refused
    /// field (WCAG 1.4.11 asks 3:1 for those), and text over a selected suggestion.
    #[test]
    fn marks_and_selections_read_clearly() {
        for p in [tokens::night::DAWN.blocks(), tokens::night::NIGHT.blocks()] {
            let theme = if p.dark { "dark" } else { "light" };
            for (name, bg) in [
                ("page", p.page),
                ("raised", p.raised),
                ("button", p.button),
                ("panel", p.panel),
                ("well", p.well),
            ] {
                let ring = tokens::contrast(p.accent, bg);
                assert!(ring >= 3.0, "{theme} focus ring on {name}: {ring:.2}");
                let refused = tokens::contrast(p.danger, bg);
                assert!(
                    refused >= 3.0,
                    "{theme} refused field on {name}: {refused:.2}"
                );
            }
            let selection = blend(p.accent, p.panel, SELECTION);
            let ratio = tokens::contrast(p.ink, selection);
            assert!(ratio >= 4.5, "{theme} text on a selection: {ratio:.2}");
        }
    }
}
