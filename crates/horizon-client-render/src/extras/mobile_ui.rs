//! Settings for the native mobile extras.
use super::*;
/// The settings the Extras page shows, one per row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Fullbright,
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
    /// On the Horizon Settings page.
    DiscordPresence,
}
/// The Extras page: headers and items.
pub fn rows() -> Vec<Result<Item, &'static str>> {
    use Item::*;
    vec![
        Err("Lighting"),
        Ok(Fullbright),
        Ok(DynamicLights),
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
    ]
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
    pub fn caption(self) -> &'static str {
        match self {
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
            Item::DiscordPresence => "Discord Rich Presence",
        }
    }
    /// The switch this setting belongs to: it does nothing while that is off.
    pub fn parent(self) -> Option<Item> {
        use Item::*;
        Some(match self {
            ZoomKey | ZoomLevel | ZoomSmooth | ZoomSensitivity => Zoom,
            MinimapCorner | MinimapSize | MinimapShape | MinimapZoom | MinimapRotate => Minimap,
            FpsCorner | FpsLows => Fps,
            KeystrokesCorner | KeystrokesCps => Keystrokes,
            MouseTweaksWheel => MouseTweaks,
            SortKey => Sort,
            Fullbright | DynamicLights | OwnNameTag | Zoom | Minimap | Fps | Keystrokes
            | MouseTweaks | Sort | AutoReconnect | NoAi | AgentControl | DiscordPresence => {
                return None;
            }
        })
    }
    /// What the row says under its caption.
    pub fn description(self) -> &'static str {
        match self {
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
            Item::DiscordPresence => {
                "Shows Playing Horizon on your Discord profile while Discord runs: menus, single player or multiplayer, the dimension and the time played. Never a world's name or a server's address."
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
            _ => None,
        }
    }
    /// On/off items and their value.
    pub fn switch(self, e: &Extras) -> Option<bool> {
        Some(match self {
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
            Item::DiscordPresence => e.discord_presence,
            _ => return None,
        })
    }
    /// The value of the other items, as their button shows it.
    pub fn value(self, e: &Extras) -> String {
        match self {
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
            Item::DiscordPresence => e.discord_presence = !e.discord_presence,
            Item::ZoomKey | Item::SortKey => {}
        }
    }
    /// The key name the options screen waits on for a key item.
    pub fn key_name(self) -> Option<&'static str> {
        match self {
            Item::ZoomKey => Some(ZOOM_KEY),
            Item::SortKey => Some(SORT_KEY),
            _ => None,
        }
    }
}
