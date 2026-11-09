//! Horizon's look, given to the engine's extension points at startup (docs/layers.md): the
//! windows' icon and the colours, sizes and fonts of the menus vanilla doesn't have
//! (`docs/design-system.md`).
//! Its own module so it can leave the engine crate as a pure file move, for the official
//! executable to call.
use horizon_client::menu_style::{Fonts, MenuStyle, OverlayColours, SettingsColours, Sizes};
use horizon_theme as theme;

/// Give the engine Horizon's look; call it once, before the first window opens.
pub(crate) fn give() {
    horizon_client::window::set_icon_png(theme::ICON_PNG);
    let (blocks, dark) = (theme::BLOCKS_DARK, theme::DARK);
    horizon_client::menu_style::install(MenuStyle {
        settings: SettingsColours {
            page: blocks.page,
            ink: blocks.ink,
            ink_soft: blocks.ink_soft,
            muted: blocks.muted,
            outline: blocks.outline,
            accent: blocks.accent,
            face: blocks.face,
            // The face's hover, lighter than `face`: the settings screen's own shade.
            face_hover: 0x6a50e6,
            success: blocks.success,
            warn: blocks.warn,
            danger: blocks.danger,
        },
        overlay: OverlayColours {
            page: dark.page,
            backdrop_muted: theme::BACKDROP_MUTED,
            ink: dark.ink,
            ink_soft: dark.ink_soft,
            rule: dark.rule,
            accent: dark.accent,
            on_accent: dark.on_accent,
            lit: theme::night::NIGHT.accent,
            lit_edge: theme::night::NIGHT.accent_hi,
        },
        sizes: Sizes {
            caption: theme::CAPTION.px,
            label: theme::LABEL.px,
            ui: theme::UI.px,
            body: theme::BODY.px,
            title: theme::TITLE.px,
            display: theme::DISPLAY.px,
            micro: theme::MICRO.px,
            radius: theme::RADIUS,
            radius_plate: theme::RADIUS_PLATE,
            control_large: theme::CONTROL_LARGE,
        },
        fonts: Some(Fonts {
            regular: theme::fonts::GEIST_REGULAR,
            medium: theme::fonts::GEIST_MEDIUM,
            semibold: theme::fonts::GEIST_SEMIBOLD,
            mono: theme::fonts::GEIST_MONO,
        }),
    });
}
