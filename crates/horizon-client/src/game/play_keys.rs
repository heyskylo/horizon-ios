//! A key press while a world is open (`KeyboardHandler.keyPress` and the gameplay mappings).
use super::inventory_input::drop_whole_stack;
use super::*;

impl Game<'_> {
    /// A key press while a world is open (keyboard or control socket). `key`
    /// is the vanilla default the pressed mapping translates to, `raw` the
    /// physical key as pressed (the same where the caller had no window key):
    /// `Screen.keyPressed` hears the raw one, and only a raw key settles what
    /// the container's hotbar keys are now (`checkHotbarKeyPressed`).
    pub(super) fn key_pressed(&mut self, key: KeyCode, raw: KeyCode) -> Result<()> {
        self.direct_craft_stop();
        if self.screen == Screen::Playing && self.replay.playing() {
            self.studio_key(key);
            return Ok(());
        }
        // `CreativeModeInventoryScreen.keyPressed` clears `ignoreTextInput` on
        // every key, before it decides anything.
        self.creative_ignore_text = false;
        if self.sleeping.is_some()
            && self.screen == Screen::Playing
            && !matches!(
                key,
                KeyCode::F1 | KeyCode::F2 | KeyCode::F11 | KeyCode::KeyT | KeyCode::Slash
            )
        {
            if matches!(key, KeyCode::Escape | KeyCode::ShiftLeft) {
                self.wake_up();
            }
            return Ok(());
        }
        if self.sort_key_press(key)
            || horizon_client::keybinds::key_name(key)
                .is_some_and(|name| self.hud_editor_key_press(name))
        {
            return Ok(());
        }
        if key == KeyCode::Escape && self.mod_browser_back() {
            return Ok(());
        }
        if self.remote.is_some() && self.remote_key(key)? {
            return Ok(());
        }
        match key {
            KeyCode::Escape => match self.screen {
                Screen::Options => self.options_key(KeyCode::Escape, None)?,
                Screen::Playing => {
                    if self.replay.playing() && self.captured {
                        self.capture(false);
                        return Ok(());
                    }
                    self.pause_focus = None;
                    self.screen = Screen::Paused;
                    self.pause_menu = true;
                    self.capture(false);
                }
                Screen::Creative => {
                    self.pad_clear_focus();
                    self.mod_browser_closed();
                    self.carried = None;
                    self.screen = Screen::Playing;
                    self.capture(true);
                }
                Screen::Paused | Screen::Inventory => {
                    // RecipeBookComponent.keyPressed: a narrow window's open
                    // book takes Escape to close itself, not the screen.
                    if self.screen == Screen::Inventory && self.recipe_book_escape() {
                        return Ok(());
                    }
                    self.close_container();
                    self.pause_focus = None;
                    self.screen = Screen::Playing;
                    self.pause_menu = true;
                    self.capture(true);
                }
                Screen::GameModeSwitcher => self.close_gamemode_switcher(false),
                Screen::EmoteWheel => self.close_emote_wheel(None),
                Screen::Dead => {
                    if self.ui.death_confirm.is_some() {
                        self.death_confirm_answer(false)?;
                    }
                }
                Screen::Stats | Screen::Friends => self.screen = Screen::Paused,
                Screen::Social => self.close_social(),
                Screen::ServerLinks => self.screen = Screen::Paused,
                // ConfirmScreen.keyPressed: Escape answers no; both return to
                // the Server Links screen they came from.
                Screen::ConfirmLink => self.screen = self.confirm_link_back(),
                Screen::DialogWarning => self.screen = Screen::ServerLinks,
                Screen::Advancements => self.close_advancements(),
                Screen::WorldOptions => self.world_options_escape(),
                Screen::CommandBlock => self.close_command_block(),
                Screen::BlockEditor => self.close_block_editor(),
                Screen::Chat => self.close_chat(chat::ChatExit::Intentional),
                Screen::Win => self.close_win_screen()?,
                Screen::DebugOptions => self.close_debug_options(),
                Screen::Mod => self.close_mod_screen(),
                Screen::Studio => self.close_studio_screen(),
                Screen::HudEditor => self.close_hud_editor(),
            },
            // `checkHotbarKeyPressed`: the keys the controls bind to the off
            // hand and the hotbar slots, so a rebound key swaps instead of
            // reaching the screen. The list below keeps Q and the defaults.
            // The raw key asks what the bindings are now: the translated
            // `key` is a vanilla default and may itself be another mapping's
            // rebound key (the off hand on Left Alt while hotbar 7 is on F
            // would find that F a second time and swap with slot 7).
            _ if (self.screen == Screen::Inventory
                || (self.screen == Screen::Creative && !self.creative_screen.tab().search))
                && self
                    .settings
                    .keys
                    .container_hotbar_key(raw)
                    .is_some_and(|key| self.inventory_hotkey(key)) => {}
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5
            | KeyCode::Digit6
            | KeyCode::Digit7
            | KeyCode::Digit8
            | KeyCode::Digit9
            | KeyCode::KeyF
            | KeyCode::KeyQ
                if (self.screen == Screen::Inventory
                    || (self.screen == Screen::Creative && !self.creative_screen.tab().search))
                    && self.inventory_hotkey(key) => {}
            // The search tab's number keys (scripted presses; the window's
            // keys reach search_tab_hotbar_key before the search box).
            _ if self.screen == Screen::Creative
                && self.creative_screen.tab().search
                && self.search_tab_hotbar_key(key) => {}
            KeyCode::KeyT if self.screen == Screen::Inventory => self.recipe_focus_search(),
            KeyCode::KeyT if self.screen == Screen::Playing => self.open_chat_method(""),
            // CreativeModeInventoryScreen.keyPressed: the chat key opens the
            // search tab from another tab, and `ignoreTextInput` drops the
            // letter it would have typed.
            KeyCode::KeyT
                if self.screen == Screen::Creative && !self.creative_screen.tab().search =>
            {
                if let Some(tab) = self.creative_screen.search_tab() {
                    select_creative_tab(&mut self.creative_screen, &mut self.inv_clicks, tab);
                    self.creative_ignore_text = true;
                }
            }
            // key.advancements.
            KeyCode::KeyL if self.screen == Screen::Playing => {
                self.open_advancements(false);
                // Opened from the keybinds, like the inventory: vanilla releases
                // an active use in the same tick.
                self.release_use_for_keybind_screen();
            }
            // key.socialInteractions (Gui.handleKeybinds): only on a server; in single player
            // the action bar says so, and the narrator.
            KeyCode::KeyP if self.screen == Screen::Playing && !self.is_multiplayer_server() => {
                let message = horizon_client::text::Text::translate(
                    "multiplayer.socialInteractions.not_available",
                    &[],
                );
                self.titles.set_overlay(message.clone(), false);
                self.say_system_now(&message);
            }
            KeyCode::KeyP if self.screen == Screen::Playing => {
                self.open_social(false);
                self.release_use_for_keybind_screen();
            }
            KeyCode::KeyL if self.screen == Screen::Advancements => self.close_advancements(),
            // key.emote (Horizon's): the emote wheel, a tap again closes it.
            KeyCode::KeyB if self.screen == Screen::Playing && super::cosmetics::enabled() => {
                self.open_emote_wheel();
            }
            KeyCode::KeyB if self.screen == Screen::EmoteWheel => self.close_emote_wheel(None),
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5
            | KeyCode::Digit6
            | KeyCode::Digit7
            | KeyCode::Digit8
                if self.screen == Screen::EmoteWheel =>
            {
                let slot = (key as u32).wrapping_sub(KeyCode::Digit1 as u32) as usize;
                self.close_emote_wheel(Some(slot));
            }
            // key.quickActions: only open if a datapack provides dialog actions.
            KeyCode::KeyG if self.screen == Screen::Playing => {
                if self.has_quick_actions() {
                    self.open_quick_actions();
                }
            }
            KeyCode::Slash if self.screen == Screen::Playing => self.open_chat_method("/"),
            // key.toggleGui is a KeyMapping: it clicks only with no screen open.
            KeyCode::F1 if self.screen == Screen::Playing => self.toggle_hide_gui(),
            KeyCode::F11 => self.toggle_fullscreen(),
            KeyCode::F5 if self.screen == Screen::Playing && !self.replay.playing() => {
                self.cycle_camera()
            }
            KeyCode::KeyE
                if self.screen == Screen::Creative && self.creative_screen.tab().search => {}
            KeyCode::KeyE => match self.screen {
                Screen::Playing if self.spectator() || self.in_portal_transition() => {}
                Screen::Playing => {
                    // handleKeybinds: the number keys select before key.inventory.
                    self.select_queued_hotbar();
                    // key.inventory while riding a tamed mount opens its inventory.
                    if !self.open_ridden_mount_menu() {
                        if self.tutorial.open_inventory() {
                            self.persist_tutorial_step();
                        }
                        if self.creative {
                            self.screen = Screen::Creative;
                            self.refresh_creative_tabs();
                            self.creative_screen.reopen();
                            self.refresh_saved_hotbars();
                        } else {
                            self.screen = Screen::Inventory;
                        }
                        self.capture(false);
                    }
                }
                Screen::Creative => {
                    self.pad_clear_focus();
                    if let Some(c) = self.carried.take() {
                        let _ = self.give(c);
                    }
                    self.screen = Screen::Playing;
                    self.capture(true);
                }
                Screen::Inventory => {
                    self.close_container();
                    self.screen = Screen::Playing;
                    self.capture(true);
                }
                _ => {}
            },
            // key.swapOffhand: on a server, ServerboundPlayerActionPacket SWAP_ITEM_WITH_OFFHAND
            // (the server swaps the hands and sends both slots back; spectators can't).
            KeyCode::KeyF if self.screen == Screen::Playing && self.remote.is_some() => {
                if !self.spectator()
                    && let Some(r) = self.remote.as_mut()
                {
                    r.pending_keys.push(key);
                }
            }
            KeyCode::KeyF if self.screen == Screen::Playing => {
                // handleKeybinds: the number keys select before key.swapOffhand.
                self.select_queued_hotbar();
                // handlePlayerAction SWAP_ITEM_WITH_OFFHAND: setItemInHand for
                // the off hand, then the main hand.
                std::mem::swap(&mut self.offhand, &mut self.inventory[self.selected]);
                self.hand_stack_replaced(self.selected);
                self.hand_stack_replaced(shield::OFFHAND_SLOT);
                let (offhand, main) = (self.offhand.clone(), self.inventory[self.selected].clone());
                self.player_on_equip("offhand", main.as_ref(), offhand.as_ref());
                self.player_on_equip("mainhand", offhand.as_ref(), main.as_ref());
                self.using = None;
            }
            KeyCode::KeyQ if self.screen == Screen::Playing => {
                // handleKeybinds: the number keys select before key.drop.
                self.select_queued_hotbar();
                let all = drop_whole_stack(self.modifiers) || self.input_takes_stack_drop();
                self.drop_held(all);
            }
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5
            | KeyCode::Digit6
            | KeyCode::Digit7
            | KeyCode::Digit8
            | KeyCode::Digit9
                if self.screen == Screen::Playing =>
            {
                let slot = match key {
                    KeyCode::Digit1 => 0,
                    KeyCode::Digit2 => 1,
                    KeyCode::Digit3 => 2,
                    KeyCode::Digit4 => 3,
                    KeyCode::Digit5 => 4,
                    KeyCode::Digit6 => 5,
                    KeyCode::Digit7 => 6,
                    KeyCode::Digit8 => 7,
                    _ => 8,
                };
                // Minecraft.handleKeybinds: a spectator's number keys drive
                // the spectator menu, not the hotbar; a creative player
                // holding the load or save toolbar key loads or saves a
                // toolbar instead (load first when both are held).
                let load = self.mapped_key_down("key.loadToolbarActivator");
                if self.spectator() {
                    self.spectator_hotbar_key(slot as i32);
                } else if self.creative
                    && (load || self.mapped_key_down("key.saveToolbarActivator"))
                {
                    self.hotbar_load_or_save(slot, load);
                } else {
                    self.hotbar_presses.push(slot);
                }
            }
            _ if self.screen == Screen::Paused && self.pause_menu => {
                self.pause_key(key)?;
            }
            _ if self.screen == Screen::Dead => {
                self.death_key(key)?;
            }
            _ if self.screen == Screen::Stats => self.stats_key(raw),
            _ => {}
        }
        Ok(())
    }
}
