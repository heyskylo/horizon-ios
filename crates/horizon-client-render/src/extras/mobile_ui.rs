//! Settings for the native mobile extras.
#[cfg(not(target_os = "ios"))]
use super::native_ios::{ZOOM_KEY, ZOOM_LEVELS};
use super::*;
/// The settings the Extras page shows, one per row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    PackBrowser,
    Pregen,
    /// Shared PvP & HUD page options, with their upstream translations.
    Pvp(crate::pvp::Opt),
    CosmeticsHideFx,
    MusicPage,
    CosmeticsMine,
    CosmeticsOthers,
    CosmeticsSolo,
    HorizonMarks,
    HorizonMe,
    ReproKeys,
    WheelCyclesSettings,
    Fullbright,
    HideBlockParticles,
    UiSoundReserve,
    ChatAnimation,
    ChatIndicator,
    Splitscreen,

    DynamicLights,
    OwnNameTag,
    Zoom,
    ZoomKey,
    ZoomLevel,
    ZoomSmooth,
    ZoomSensitivity,
    Fps,
    FpsCorner,
    FpsLows,
    Keystrokes,
    KeystrokesCorner,
    KeystrokesCps,
    Minimap,
    MinimapCorner,
    MinimapSize,
    MinimapShape,
    MinimapZoom,
    MinimapRotate,
    AutoReconnect,
    MouseTweaks,
    MouseTweaksWheel,
    Sort,
    SortKey,
    /// Not on the Extras page: Horizon Settings shows No AI, its AI Agent page Agent Control.
    NoAi,
    AgentControl,
}
/// The Extras page: headers and items.
pub fn rows() -> Vec<Result<Item, &'static str>> {
    use self::Item::*;
    let mut rows = vec![
        Err("Lighting"),
        Ok(OwnNameTag),
        Err("Zoom"),
        Ok(Zoom),
        Ok(ZoomKey),
        Ok(ZoomLevel),
        Ok(ZoomSmooth),
        Ok(ZoomSensitivity),
        Err("Minimap"),
        Ok(Minimap),
        Ok(MinimapCorner),
        Ok(MinimapSize),
        Ok(MinimapShape),
        Ok(MinimapZoom),
        Ok(MinimapRotate),
        Err("FPS counter"),
        Ok(Fps),
        Ok(FpsCorner),
        Ok(FpsLows),
        Err("Keystrokes"),
        Ok(Keystrokes),
        Ok(KeystrokesCorner),
        Ok(KeystrokesCps),
        Err("Inventory"),
        Ok(MouseTweaks),
        Ok(MouseTweaksWheel),
        Ok(Sort),
        Ok(SortKey),
        Err("Multiplayer"),
        Ok(AutoReconnect),
        Ok(UiSoundReserve),
        Ok(MusicPage),
        Ok(PackBrowser),
        Ok(Pregen),
        Ok(ChatAnimation),
        Ok(ChatIndicator),
        Ok(WheelCyclesSettings),
    ];
    if horizon_cosmetics::enabled() {
        rows.extend([
            Err("horizon.extras.header.cosmetics"),
            Ok(CosmeticsMine),
            Ok(CosmeticsSolo),
            Ok(CosmeticsOthers),
            Ok(CosmeticsHideFx),
            Ok(HorizonMarks),
            Ok(HorizonMe),
        ]);
    }
    rows
}
/// "1 block", "2 blocks", "½ block".
fn block_count(n: f32) -> String {
    if n < 1.0 {
        "½ block".into()
    } else if n == 1.0 {
        "1 block".into()
    } else {
        format!("{n} blocks")
    }
}
fn next<T: Copy + PartialEq>(all: &[T], current: T, back: bool) -> T {
    let i = all.iter().position(|v| *v == current).unwrap_or(0);
    let n = all.len();
    all[if back { (i + n - 1) % n } else { (i + 1) % n }]
}
impl Item {
    fn shared(self) -> Option<super::desktop_ui::Item> {
        Some(match self {
            Self::Pvp(option) => super::desktop_ui::Item::Pvp(option),
            Self::PackBrowser => super::desktop_ui::Item::PackBrowser,
            Self::Pregen => super::desktop_ui::Item::Pregen,
            Self::CosmeticsHideFx => super::desktop_ui::Item::CosmeticsHideFx,
            Self::MusicPage => super::desktop_ui::Item::MusicPage,
            Self::CosmeticsMine => super::desktop_ui::Item::CosmeticsMine,
            Self::CosmeticsOthers => super::desktop_ui::Item::CosmeticsOthers,
            Self::CosmeticsSolo => super::desktop_ui::Item::CosmeticsSolo,
            Self::HorizonMarks => super::desktop_ui::Item::HorizonMarks,
            Self::HorizonMe => super::desktop_ui::Item::HorizonMe,
            Self::ReproKeys => super::desktop_ui::Item::ReproKeys,
            Self::WheelCyclesSettings => super::desktop_ui::Item::WheelCyclesSettings,
            Self::Fullbright => super::desktop_ui::Item::Fullbright,
            Self::DynamicLights => super::desktop_ui::Item::DynamicLights,
            Self::OwnNameTag => super::desktop_ui::Item::OwnNameTag,
            Self::AutoReconnect => super::desktop_ui::Item::AutoReconnect,
            Self::MouseTweaks => super::desktop_ui::Item::MouseTweaks,
            Self::MouseTweaksWheel => super::desktop_ui::Item::MouseTweaksWheel,
            Self::Sort => super::desktop_ui::Item::Sort,
            Self::SortKey => super::desktop_ui::Item::SortKey,
            Self::NoAi => super::desktop_ui::Item::NoAi,
            Self::AgentControl => super::desktop_ui::Item::AgentControl,
            Self::HideBlockParticles => super::desktop_ui::Item::HideBlockParticles,
            Self::UiSoundReserve => super::desktop_ui::Item::UiSoundReserve,
            Self::ChatAnimation => super::desktop_ui::Item::ChatAnimation,
            Self::ChatIndicator => super::desktop_ui::Item::ChatIndicator,
            Self::Splitscreen => super::desktop_ui::Item::Splitscreen,
            _ => return None,
        })
    }
    pub fn caption_text(self) -> String {
        self.shared()
            .map_or_else(|| self.caption().into(), |item| item.caption_text())
    }
    pub fn description_text(self) -> String {
        self.shared().map_or_else(
            || crate::lang::get_or(self.description_key(), self.description()),
            |item| item.description_text(),
        )
    }

    /// Shared controls retain their upstream keys; native controls have their own keys
    /// with English fallbacks in `description`, ready for the same language catalogs.
    pub fn description_key(self) -> &'static str {
        match self {
            Item::Zoom => "horizon.extras.zoom.tooltip",
            Item::ZoomKey => "horizon.extras.zoom_key.tooltip",
            Item::ZoomLevel => "horizon.extras.zoom_level.tooltip",
            Item::ZoomSmooth => "horizon.extras.zoom_smooth.tooltip",
            Item::ZoomSensitivity => "horizon.extras.zoom_sensitivity.tooltip",
            Item::Fps => "horizon.extras.fps.tooltip",
            Item::FpsCorner | Item::KeystrokesCorner | Item::MinimapCorner => {
                "horizon.extras.overlay_corner.tooltip"
            }
            Item::FpsLows => "horizon.extras.fps_lows.tooltip",
            Item::Keystrokes => "horizon.extras.keystrokes.tooltip",
            Item::KeystrokesCps => "horizon.extras.keystrokes_cps.tooltip",
            Item::Minimap => "horizon.extras.minimap.tooltip",
            Item::MinimapSize => "horizon.extras.minimap_size.tooltip",
            Item::MinimapShape => "horizon.extras.minimap_shape.tooltip",
            Item::MinimapZoom => "horizon.extras.minimap_zoom.tooltip",
            Item::MinimapRotate => "horizon.extras.minimap_rotate.tooltip",

            Item::Fullbright => "horizon.extras.fullbright.tooltip",
            Item::DynamicLights => "horizon.extras.dynamic_lights.tooltip",
            Item::OwnNameTag => "horizon.extras.own_name_tag.tooltip",
            Item::HideBlockParticles => "horizon.extras.hide_block_particles.tooltip",
            Item::AutoReconnect => "horizon.extras.auto_reconnect.tooltip",
            Item::UiSoundReserve => "horizon.extras.ui_sound_reserve.tooltip",
            Item::MusicPage => "horizon.extras.music_page.tooltip",
            Item::MouseTweaks => "horizon.extras.mouse_tweaks.tooltip",
            Item::MouseTweaksWheel => "horizon.extras.mouse_tweaks_wheel.tooltip",
            Item::Sort => "horizon.extras.sort.tooltip",
            Item::SortKey => "horizon.extras.sort_key.tooltip",
            Item::NoAi => "horizon.extras.no_ai.tooltip",
            Item::AgentControl => "horizon.extras.agent_control.tooltip",
            Item::ReproKeys => "horizon.extras.repro_keys.tooltip",
            Item::Splitscreen => "horizon.extras.splitscreen.tooltip",
            Item::ChatAnimation => "horizon.extras.chat_animation.tooltip",
            Item::ChatIndicator => "horizon.extras.chat_indicator.tooltip",
            Item::WheelCyclesSettings => "horizon.extras.wheel_cycles_settings.tooltip",
            Item::PackBrowser => "horizon.extras.pack_browser.tooltip",
            Item::Pregen => "horizon.extras.pregen.tooltip",
            Item::CosmeticsMine => "horizon.extras.cosmetics_mine.tooltip",
            Item::CosmeticsOthers => "horizon.extras.cosmetics_others.tooltip",
            Item::CosmeticsHideFx => "horizon.extras.cosmetics_hide_fx.tooltip",
            Item::CosmeticsSolo => "horizon.extras.cosmetics_solo.tooltip",
            Item::HorizonMarks => "horizon.extras.horizon_marks.tooltip",
            Item::HorizonMe => "horizon.extras.horizon_me.tooltip",
            Item::Pvp(o) => o.description_key(),
        }
    }

    pub fn caption(self) -> &'static str {
        match self {
            Item::Pvp(_)
            | Item::PackBrowser
            | Item::Pregen
            | Item::CosmeticsHideFx
            | Item::MusicPage
            | Item::CosmeticsMine
            | Item::CosmeticsOthers
            | Item::CosmeticsSolo
            | Item::HorizonMarks
            | Item::HorizonMe
            | Item::ReproKeys
            | Item::WheelCyclesSettings
            | Item::HideBlockParticles
            | Item::UiSoundReserve
            | Item::ChatAnimation
            | Item::ChatIndicator
            | Item::Splitscreen => self.shared().unwrap().caption(),
            Item::Fullbright => "Fullbright",
            Item::DynamicLights => "Dynamic lights",
            Item::OwnNameTag => "Own name tag",
            Item::Zoom => "Zoom",
            Item::ZoomKey => "Zoom key",
            Item::ZoomLevel => "Starting zoom",
            Item::ZoomSmooth => "Smooth zoom",
            Item::ZoomSensitivity => "Slower turning",
            Item::Fps => "FPS counter",
            Item::FpsCorner | Item::KeystrokesCorner | Item::MinimapCorner => "Corner",
            Item::Minimap => "Minimap",
            Item::MinimapSize => "Size",
            Item::MinimapShape => "Shape",
            Item::MinimapZoom => "Zoom",
            Item::MinimapRotate => "Turn with the player",
            Item::AutoReconnect => "Auto-reconnect",
            Item::MouseTweaks => "Mouse Tweaks controls",
            Item::MouseTweaksWheel => "Scroll wheel moves items",
            Item::Sort => "Sort buttons",
            Item::SortKey => "Sort key",
            Item::FpsLows => "Minimum and 1% low",
            Item::Keystrokes => "Keystrokes",
            Item::KeystrokesCps => "Clicks per second",
            Item::NoAi => "No AI",
            Item::AgentControl => "Agent Control",
        }
    }
    /// The switch this setting belongs to: it does nothing while that is off.
    pub fn parent(self) -> Option<Item> {
        use self::Item::*;
        Some(match self {
            Pvp(option) => return option.parent().map(Pvp),
            ChatIndicator => ChatAnimation,
            HideBlockParticles | UiSoundReserve | ChatAnimation | Splitscreen | PackBrowser
            | Pregen | CosmeticsHideFx | MusicPage | CosmeticsMine | CosmeticsOthers
            | CosmeticsSolo | HorizonMarks | HorizonMe | ReproKeys | WheelCyclesSettings => {
                return None;
            }
            ZoomKey | ZoomLevel | ZoomSmooth | ZoomSensitivity => Zoom,
            MinimapCorner | MinimapSize | MinimapShape | MinimapZoom | MinimapRotate => Minimap,
            FpsCorner | FpsLows => Fps,
            KeystrokesCorner | KeystrokesCps => Keystrokes,
            MouseTweaksWheel => MouseTweaks,
            SortKey => Sort,
            Fullbright | DynamicLights | OwnNameTag | Zoom | Minimap | Fps | Keystrokes
            | MouseTweaks | Sort | AutoReconnect | NoAi | AgentControl => {
                return None;
            }
        })
    }
    /// What the row says under its caption.
    pub fn description(self) -> &'static str {
        match self {
            Item::Pvp(_)
            | Item::PackBrowser
            | Item::Pregen
            | Item::CosmeticsHideFx
            | Item::MusicPage
            | Item::CosmeticsMine
            | Item::CosmeticsOthers
            | Item::CosmeticsSolo
            | Item::HorizonMarks
            | Item::HorizonMe
            | Item::ReproKeys
            | Item::WheelCyclesSettings
            | Item::HideBlockParticles
            | Item::UiSoundReserve
            | Item::ChatAnimation
            | Item::ChatIndicator
            | Item::Splitscreen => self.shared().unwrap().description(),
            Item::Fullbright => "Show blocks and entities at full light, even in dark areas.",
            Item::DynamicLights => {
                "Torches and other light sources held, dropped or on fire light the blocks around them."
            }
            Item::Zoom => "Hold the zoom key to zoom; the wheel changes it while held.",
            Item::ZoomKey => "Click, then press a key. Escape unbinds it.",
            Item::ZoomLevel => "The zoom the key starts at.",
            Item::ZoomSmooth => "Eases in and out like the spyglass.",
            Item::ZoomSensitivity => "Turns slower the more you zoom in.",
            Item::Fps => "Frames per second in a corner of the screen.",
            Item::FpsCorner | Item::KeystrokesCorner | Item::MinimapCorner => {
                "Overlays in one corner stack."
            }
            Item::Minimap => "A map of the loaded terrain around you, in map colours.",
            Item::MinimapSize => "How big the map is on screen.",
            Item::MinimapShape => "Square or round.",
            Item::MinimapZoom => "Blocks per pixel: smaller shows more detail, larger more land.",
            Item::MinimapRotate => "Forward is up; off keeps north up.",
            Item::OwnNameTag => "Show your own name above the player model in third person.",
            Item::AutoReconnect => {
                "Rejoins a server after a lost connection or a restart, never after a kick or ban."
            }
            Item::FpsLows => "The slowest frame and the 1% low of the last 5 seconds.",
            Item::MouseTweaks => {
                "Right-drag places one per slot, left-drag gathers the held item, Shift + left-drag moves every slot."
            }
            Item::MouseTweaksWheel => {
                "Down sends one item of the slot to the other side, up brings one back."
            }
            Item::Sort => {
                "Sorts the inventory, chests, barrels, ender chests and shulker boxes in creative tab order, merging stacks."
            }
            Item::SortKey => {
                "Sorts the part under the cursor. Click, then press a key; Escape unbinds it."
            }
            Item::Keystrokes => "Movement keys, jump and mouse buttons as you press them.",
            Item::KeystrokesCps => "Clicks in the last second under each mouse button.",
            Item::NoAi => {
                "Hides and turns off every AI feature: agent control, horizon-ctl and the MCP servers. The launcher's switch too."
            }
            Item::AgentControl => {
                "Your own AI agent can play this game through horizon-ctl or an MCP client, on this computer only."
            }
        }
    }
    /// The frame-time cost the main switch of an extra shows.
    pub fn cost(self) -> Option<crate::graphics_style::Cost> {
        use crate::graphics_style::Cost;
        match self {
            Item::Fullbright | Item::Zoom | Item::Fps | Item::Keystrokes => Some(Cost::None),
            Item::Minimap | Item::DynamicLights => Some(Cost::Low),
            Item::AutoReconnect | Item::MouseTweaks | Item::Sort => Some(Cost::None),
            Item::CosmeticsSolo | Item::HorizonMarks | Item::HorizonMe => {
                self.shared().unwrap().cost()
            }
            _ => None,
        }
    }
    /// On/off items and their value.
    pub fn switch(self, e: &Extras) -> Option<bool> {
        Some(match self {
            Item::Pvp(_)
            | Item::PackBrowser
            | Item::Pregen
            | Item::CosmeticsHideFx
            | Item::MusicPage
            | Item::CosmeticsMine
            | Item::CosmeticsOthers
            | Item::CosmeticsSolo
            | Item::HorizonMarks
            | Item::HorizonMe
            | Item::ReproKeys
            | Item::WheelCyclesSettings
            | Item::HideBlockParticles
            | Item::UiSoundReserve
            | Item::ChatAnimation
            | Item::ChatIndicator
            | Item::Splitscreen => return self.shared().unwrap().switch(e),
            Item::Fullbright => e.fullbright,
            Item::DynamicLights => e.dynamic_lights,
            Item::OwnNameTag => e.own_name_tag,
            Item::Zoom => e.zoom,
            Item::ZoomSmooth => e.zoom_smooth,
            Item::ZoomSensitivity => e.zoom_sensitivity,
            Item::Fps => e.fps,
            Item::FpsLows => e.fps_lows,
            Item::Keystrokes => e.keystrokes,
            Item::KeystrokesCps => e.keystrokes_cps,
            Item::Minimap => e.minimap,
            Item::MinimapRotate => e.minimap_rotate,
            Item::AutoReconnect => e.auto_reconnect,
            Item::MouseTweaks => e.mouse_tweaks,
            Item::MouseTweaksWheel => e.mouse_tweaks_wheel,
            Item::Sort => e.sort,
            Item::NoAi => e.no_ai,
            Item::AgentControl => e.agent_control,
            _ => return None,
        })
    }
    /// The value of the other items, as their button shows it.
    pub fn value(self, e: &Extras) -> String {
        match self {
            Item::Pvp(_)
            | Item::CosmeticsMine
            | Item::CosmeticsOthers
            | Item::CosmeticsSolo
            | Item::HorizonMarks
            | Item::HorizonMe => self.shared().unwrap().value(e),
            Item::ZoomKey => crate::keybinds::display_name(&e.zoom_key).plain(),
            Item::SortKey => crate::keybinds::display_name(&e.sort_key).plain(),
            Item::ZoomLevel => format!("{}x", e.zoom_level),
            Item::FpsCorner => e.fps_corner.label().into(),
            Item::KeystrokesCorner => e.keystrokes_corner.label().into(),
            Item::MinimapCorner => e.minimap_corner.label().into(),
            Item::MinimapSize => MINIMAP_SIZES
                .iter()
                .find(|s| s.0 == e.minimap_size)
                .map_or("Medium", |s| s.1)
                .into(),
            Item::MinimapShape => if e.minimap_round { "Round" } else { "Square" }.into(),
            Item::MinimapZoom => format!("{} per pixel", block_count(e.minimap_zoom)),
            other => match other.switch(e) {
                Some(true) => "On".into(),
                _ => "Off".into(),
            },
        }
    }
    /// A click (`back` for the right button). The zoom key binds instead.
    pub fn cycle(self, e: &mut Extras, back: bool) {
        match self {
            Item::Pvp(_)
            | Item::PackBrowser
            | Item::Pregen
            | Item::CosmeticsHideFx
            | Item::MusicPage
            | Item::CosmeticsMine
            | Item::CosmeticsOthers
            | Item::CosmeticsSolo
            | Item::HorizonMarks
            | Item::HorizonMe
            | Item::ReproKeys
            | Item::WheelCyclesSettings
            | Item::HideBlockParticles
            | Item::UiSoundReserve
            | Item::ChatAnimation
            | Item::ChatIndicator
            | Item::Splitscreen => self.shared().unwrap().cycle(e, back),
            Item::Fullbright => e.fullbright = !e.fullbright,
            Item::DynamicLights => e.dynamic_lights = !e.dynamic_lights,
            Item::OwnNameTag => e.own_name_tag = !e.own_name_tag,
            Item::Zoom => e.zoom = !e.zoom,
            Item::ZoomSmooth => e.zoom_smooth = !e.zoom_smooth,
            Item::ZoomSensitivity => e.zoom_sensitivity = !e.zoom_sensitivity,
            Item::Fps => e.fps = !e.fps,
            Item::FpsLows => e.fps_lows = !e.fps_lows,
            Item::Keystrokes => e.keystrokes = !e.keystrokes,
            Item::KeystrokesCps => e.keystrokes_cps = !e.keystrokes_cps,
            Item::ZoomLevel => e.zoom_level = next(&ZOOM_LEVELS, e.zoom_level, back),
            Item::FpsCorner => e.fps_corner = next(&Corner::ALL, e.fps_corner, back),
            Item::KeystrokesCorner => {
                e.keystrokes_corner = next(&Corner::ALL, e.keystrokes_corner, back)
            }
            Item::Minimap => e.minimap = !e.minimap,
            Item::MinimapRotate => e.minimap_rotate = !e.minimap_rotate,
            Item::AutoReconnect => e.auto_reconnect = !e.auto_reconnect,
            Item::MouseTweaks => e.mouse_tweaks = !e.mouse_tweaks,
            Item::MouseTweaksWheel => e.mouse_tweaks_wheel = !e.mouse_tweaks_wheel,
            Item::Sort => e.sort = !e.sort,
            Item::MinimapCorner => e.minimap_corner = next(&Corner::ALL, e.minimap_corner, back),
            Item::MinimapSize => {
                let sizes: Vec<u8> = MINIMAP_SIZES.iter().map(|s| s.0).collect();
                e.minimap_size = next(&sizes, e.minimap_size, back);
            }
            Item::MinimapShape => e.minimap_round = !e.minimap_round,
            Item::MinimapZoom => e.minimap_zoom = next(&MINIMAP_ZOOMS, e.minimap_zoom, back),
            Item::NoAi => e.no_ai = !e.no_ai,
            Item::AgentControl => e.agent_control = !e.agent_control,
            Item::ZoomKey | Item::SortKey => {}
        }
    }
    /// The key name the options screen waits on for a key item.
    pub fn key_name(self) -> Option<&'static str> {
        match self {
            Item::Pvp(_) => self.shared().unwrap().key_name(),
            Item::ZoomKey => Some(ZOOM_KEY),
            Item::SortKey => Some(SORT_KEY),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_rows_keep_native_controls_and_place_graphics_extras_only_in_quality() {
        let mobile: Vec<_> = rows().into_iter().filter_map(Result::ok).collect();
        for item in [
            Item::Zoom,
            Item::ZoomKey,
            Item::ZoomLevel,
            Item::ZoomSmooth,
            Item::ZoomSensitivity,
            Item::Fps,
            Item::FpsCorner,
            Item::FpsLows,
            Item::Keystrokes,
            Item::KeystrokesCorner,
            Item::KeystrokesCps,
            Item::Minimap,
            Item::MinimapCorner,
            Item::MinimapSize,
            Item::MinimapShape,
            Item::MinimapZoom,
            Item::MinimapRotate,
            Item::OwnNameTag,
        ] {
            assert_eq!(
                mobile.iter().filter(|&&row| row == item).count(),
                1,
                "{item:?}"
            );
        }
        for item in [
            Item::Fullbright,
            Item::DynamicLights,
            Item::HideBlockParticles,
        ] {
            assert!(!mobile.contains(&item));
        }
        for extra in [
            crate::extras::Item::Fullbright,
            crate::extras::Item::DynamicLights,
            crate::extras::Item::HideBlockParticles,
        ] {
            let pages: Vec<_> = crate::graphics_ui::rows()
                .into_iter()
                .flat_map(|(page, groups)| {
                    groups.into_iter().flat_map(move |(_, rows)| {
                        rows.into_iter().map(move |row| (page, row.item))
                    })
                })
                .filter_map(|(page, item)| {
                    (item == crate::graphics_ui::Item::Extra(extra)).then_some(page)
                })
                .collect();
            assert_eq!(pages, [crate::graphics_ui::Page::Quality]);
        }
    }

    #[test]
    fn mobile_native_controls_still_change_and_round_trip_their_settings() {
        let mut extras = Extras::default();
        for item in [
            Item::Zoom,
            Item::ZoomLevel,
            Item::ZoomSmooth,
            Item::ZoomSensitivity,
            Item::Fps,
            Item::FpsCorner,
            Item::FpsLows,
            Item::Keystrokes,
            Item::KeystrokesCorner,
            Item::KeystrokesCps,
            Item::Minimap,
            Item::MinimapCorner,
            Item::MinimapSize,
            Item::MinimapShape,
            Item::MinimapZoom,
            Item::MinimapRotate,
        ] {
            let before = extras.clone();
            item.cycle(&mut extras, false);
            assert_ne!(
                extras, before,
                "{item:?} must still edit its native setting"
            );
            assert_eq!(Extras::parse(&extras.serialize()), extras, "{item:?}");
            item.cycle(&mut extras, true);
            assert_eq!(extras, before, "{item:?} must still cycle backwards");
        }
        #[cfg(target_os = "ios")]
        assert!(extras.set_key(ZOOM_KEY, "key.keyboard.c".into()));
        #[cfg(not(target_os = "ios"))]
        {
            extras.zoom_key = "key.keyboard.c".into();
        }
        assert_eq!(
            Extras::parse(&extras.serialize()).zoom_key,
            "key.keyboard.c"
        );
    }

    #[test]
    fn mobile_descriptions_keep_shared_keys_and_native_fallbacks() {
        for item in rows().into_iter().filter_map(Result::ok).chain([
            Item::Fullbright,
            Item::DynamicLights,
            Item::HideBlockParticles,
        ]) {
            assert!(!item.description().is_empty(), "{item:?}");
            assert!(!item.description_text().is_empty(), "{item:?}");
            if let Some(shared) = item.shared() {
                assert_eq!(item.description_key(), shared.description_key());
                assert_eq!(item.description_text(), shared.description_text());
            } else {
                assert!(
                    item.description_key().starts_with("horizon.extras."),
                    "{item:?}"
                );
            }
        }
    }

    #[test]
    fn mobile_cosmetic_switches_round_trip_the_service_and_singleplayer_choices() {
        let mut extras = Extras::default();
        // Exercise the same mobile Items the options screen dispatches, including
        // switches absent from the old mobile adapter.
        for item in [Item::CosmeticsSolo, Item::HorizonMarks, Item::HorizonMe] {
            assert!(!item.caption_text().is_empty());
            assert!(!item.description_text().is_empty());
            assert!(item.parent().is_none());
            item.cycle(&mut extras, false);
        }
        let loaded = Extras::parse(&extras.serialize());
        assert!(loaded.cosmetics_solo);
        assert!(!loaded.horizon_marks);
        assert!(!loaded.horizon_listed);
        let home = std::env::temp_dir().join(format!(
            "horizon-mobile-cosmetic-prefs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(FILE), extras.serialize()).unwrap();
        assert!(!horizon_cosmetics::prefs::listed(&home));
        Item::HorizonMe.cycle(&mut extras, false);
        std::fs::write(home.join(FILE), extras.serialize()).unwrap();
        assert!(horizon_cosmetics::prefs::listed(&home));
        std::fs::remove_dir_all(home).unwrap();
        for item in [Item::CosmeticsSolo, Item::HorizonMarks, Item::HorizonMe] {
            assert_eq!(item.switch(&loaded), item.shared().unwrap().switch(&loaded));
            assert_eq!(item.value(&loaded), item.shared().unwrap().value(&loaded));
        }
        // Existing mobile-only controls keep their own policy.
        Item::Zoom.cycle(&mut extras, false);
        assert!(extras.zoom);
        assert_eq!(Item::ZoomKey.key_name(), Some(ZOOM_KEY));
    }
}
