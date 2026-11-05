//! Every input the game names: vanilla's key mappings (`Options.keyMappings`) and Horizon's
//! pad-only actions. Held state is asked by [`Action`] ([`crate::held::HeldKeys::held`]), not by a
//! key: code that wants a mapping says which one, and every device that can hold it counts.
use winit::keyboard::KeyCode;

macro_rules! actions {
    ($($variant:ident, $name:literal, $key:expr;)*) => {
        /// A key mapping, or a pad-only action.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Action {
            $($variant,)*
        }
        impl Action {
            /// In `Options.keyMappings` order, the pad-only actions last.
            pub const ALL: &[Action] = &[$(Action::$variant,)*];
            /// The saved name: vanilla's for a mapping, `pad.*` for Horizon's own.
            pub fn name(self) -> &'static str {
                match self {
                    $(Action::$variant => $name,)*
                }
            }
            /// The default key the game's held state keeps this action under. `None` for what
            /// isn't held as a key there: a mouse mapping, one with no default, a pad-only action,
            /// and the debug keys, which act on the physical key (`debug_keys.rs`) and whose
            /// defaults are other mappings' keys.
            pub fn key(self) -> Option<KeyCode> {
                match self {
                    $(Action::$variant => $key,)*
                }
            }
        }
    };
}

actions! {
    Attack, "key.attack", None;
    Use, "key.use", None;
    Forward, "key.forward", Some(KeyCode::KeyW);
    Left, "key.left", Some(KeyCode::KeyA);
    Back, "key.back", Some(KeyCode::KeyS);
    Right, "key.right", Some(KeyCode::KeyD);
    Jump, "key.jump", Some(KeyCode::Space);
    Sneak, "key.sneak", Some(KeyCode::ShiftLeft);
    Sprint, "key.sprint", Some(KeyCode::ControlLeft);
    Drop, "key.drop", Some(KeyCode::KeyQ);
    Inventory, "key.inventory", Some(KeyCode::KeyE);
    Chat, "key.chat", Some(KeyCode::KeyT);
    Playerlist, "key.playerlist", Some(KeyCode::Tab);
    PickItem, "key.pickItem", None;
    Command, "key.command", Some(KeyCode::Slash);
    Friends, "key.friends", Some(KeyCode::KeyO);
    Emote, "key.emote", Some(KeyCode::KeyB);
    SocialInteractions, "key.socialInteractions", Some(KeyCode::KeyP);
    ToggleGui, "key.toggleGui", Some(KeyCode::F1);
    ToggleSpectatorShaderEffects, "key.toggleSpectatorShaderEffects", Some(KeyCode::F4);
    Screenshot, "key.screenshot", Some(KeyCode::F2);
    TogglePerspective, "key.togglePerspective", Some(KeyCode::F5);
    SmoothCamera, "key.smoothCamera", None;
    Fullscreen, "key.fullscreen", Some(KeyCode::F11);
    SpectatorOutlines, "key.spectatorOutlines", None;
    SpectatorHotbar, "key.spectatorHotbar", None;
    SwapOffhand, "key.swapOffhand", Some(KeyCode::KeyF);
    SaveToolbarActivator, "key.saveToolbarActivator", Some(KeyCode::KeyC);
    LoadToolbarActivator, "key.loadToolbarActivator", Some(KeyCode::KeyX);
    Advancements, "key.advancements", Some(KeyCode::KeyL);
    QuickActions, "key.quickActions", Some(KeyCode::KeyG);
    DebugOverlay, "key.debug.overlay", None;
    DebugModifier, "key.debug.modifier", None;
    Hotbar1, "key.hotbar.1", Some(KeyCode::Digit1);
    Hotbar2, "key.hotbar.2", Some(KeyCode::Digit2);
    Hotbar3, "key.hotbar.3", Some(KeyCode::Digit3);
    Hotbar4, "key.hotbar.4", Some(KeyCode::Digit4);
    Hotbar5, "key.hotbar.5", Some(KeyCode::Digit5);
    Hotbar6, "key.hotbar.6", Some(KeyCode::Digit6);
    Hotbar7, "key.hotbar.7", Some(KeyCode::Digit7);
    Hotbar8, "key.hotbar.8", Some(KeyCode::Digit8);
    Hotbar9, "key.hotbar.9", Some(KeyCode::Digit9);
    DebugReloadChunk, "key.debug.reloadChunk", None;
    DebugShowHitboxes, "key.debug.showHitboxes", None;
    DebugClearChat, "key.debug.clearChat", None;
    DebugCrash, "key.debug.crash", None;
    DebugShowChunkBorders, "key.debug.showChunkBorders", None;
    DebugShowAdvancedTooltips, "key.debug.showAdvancedTooltips", None;
    DebugCopyRecreateCommand, "key.debug.copyRecreateCommand", None;
    DebugSpectate, "key.debug.spectate", None;
    DebugSwitchGameMode, "key.debug.switchGameMode", None;
    DebugDebugOptions, "key.debug.debugOptions", None;
    DebugFocusPause, "key.debug.focusPause", None;
    DebugDumpDynamicTextures, "key.debug.dumpDynamicTextures", None;
    DebugReloadResourcePacks, "key.debug.reloadResourcePacks", None;
    DebugProfiling, "key.debug.profiling", None;
    DebugCopyLocation, "key.debug.copyLocation", None;
    DebugDumpVersion, "key.debug.dumpVersion", None;
    DebugProfilingChart, "key.debug.profilingChart", None;
    DebugFpsCharts, "key.debug.fpsCharts", None;
    DebugNetworkCharts, "key.debug.networkCharts", None;
    DebugLightmapTexture, "key.debug.lightmapTexture", None;
    DebugImprovedTransparency, "key.debug.improvedTransparency", None;
    HotbarPrevious, "pad.hotbarPrevious", None;
    HotbarNext, "pad.hotbarNext", None;
}

impl Action {
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinds::{self, KeyBinds, MAPPINGS};

    #[test]
    fn every_mapping_is_an_action_in_order() {
        let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        let mappings: Vec<&str> = MAPPINGS.iter().map(|m| m.0).collect();
        assert_eq!(names[..mappings.len()], mappings[..]);
        for action in &names[mappings.len()..] {
            assert!(action.starts_with("pad."), "{action}");
        }
    }

    #[test]
    fn an_actions_key_is_its_mappings_default_key() {
        for (name, _, category) in MAPPINGS {
            let action = Action::parse(name).unwrap();
            let want = if *category == "debug" {
                None
            } else {
                keybinds::key_code(KeyBinds::default_of(name))
            };
            assert_eq!(action.key(), want, "{name}");
        }
    }

    #[test]
    fn every_bindable_action_is_an_action() {
        for (name, _) in crate::gamepad::ACTIONS {
            assert!(Action::parse(name).is_some(), "{name}");
        }
    }
}
