//! Key and mouse mappings in play: toggled sneak and sprint, keys after rebinding, keys held
//! across screens, the hotbar's wheel and bumper steps, and mouse buttons while playing.
use super::*;

fn studio_replay_shortcut(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Escape
            | KeyCode::KeyP
            | KeyCode::KeyF
            | KeyCode::KeyT
            | KeyCode::KeyM
            | KeyCode::Home
            | KeyCode::KeyL
            | KeyCode::KeyK
            | KeyCode::Delete
    )
}

fn studio_freecam_movement_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::KeyW
            | KeyCode::KeyA
            | KeyCode::KeyS
            | KeyCode::KeyD
            | KeyCode::KeyE
            | KeyCode::KeyQ
            | KeyCode::Space
            | KeyCode::ControlRight
    )
}

impl Game<'_> {
    /// Keep the held-key state of toggled sneak and sprint in step with their mapping.
    /// `KeyMapping.resetToggleKeys`: every toggle mapping up, and the latched sneak and sprint
    /// keys with it.
    pub(super) fn reset_toggle_keys(&mut self) {
        for key in &mut self.toggles.0 {
            key.reset();
        }
        self.sync_toggle(0);
        self.sync_toggle(1);
        self.left = false;
        self.right = false;
    }
    /// key.smoothCamera from any device toggles the cinematic camera in play.
    pub(super) fn smooth_camera_action(&mut self) {
        if self.screen == Screen::Playing {
            self.smooth_camera = !self.smooth_camera;
        }
    }
    pub(super) fn sync_toggle(&mut self, slot: usize) {
        let key = [KeyCode::ShiftLeft, KeyCode::ControlLeft][slot];
        if self.toggles.is_down(slot) {
            self.keys.insert(key);
        } else {
            self.keys.remove(&key);
        }
    }
    pub(super) fn update_modifiers(&mut self, state: winit::keyboard::ModifiersState) {
        self.modifiers = state;
        horizon_client_input::modifiers::reconcile(
            self.modifiers,
            &mut self.physical_keys,
            &self.settings.keys,
            &mut self.toggles,
            [self.settings.toggle_crouch, self.settings.toggle_sprint],
            [
                self.pad.holds_action("key.sneak")
                    || self.keys.held_by(&KeyCode::ShiftLeft, Source::Touch),
                self.pad.holds_action("key.sprint")
                    || self.keys.held_by(&KeyCode::ControlLeft, Source::Touch),
            ],
        );
        self.sync_toggle(SNEAK);
        self.sync_toggle(SPRINT);
    }
    /// Let a held sneak, sprint or modifier key go when the keyboard reports
    /// its modifier up. A release does not always reach the mapping: a screen
    /// reads presses only, and an unfocused window is given no release at all,
    /// so the latch would keep the player sneaking or sprinting until the key
    /// is tapped again. The modifier state is what the keyboard itself
    /// asserts, so it settles the question. A mapping that toggles, or one a
    /// pad latched, keeps its state: `set_down(false, needs_toggle)` leaves
    /// those alone, and a toggle the player asked for is meant to outlive the
    /// key. A mapping rebound away from its modifier is left alone too: the
    /// modifier says nothing about it, and so is one a controller holds.
    ///
    /// Only `physical_keys` is cleared directly here; the logical `self.keys`
    /// entries for sneak and sprint are left to the toggle loop below, because
    /// they stand for the mapping and not the physical Shift or Control.
    /// `Screen.hasShiftDown` and `hasControlDown` read the physical state
    /// (vanilla's `InputConstants.isKeyDown`), so a Shift no one is holding
    /// makes every inventory click a shift-click and every throw a whole-stack
    /// drop. Those do not care what sneak and sprint are bound to. A modifier
    /// says nothing about which side is down, so one being up means both
    /// Shifts, or both Controls, are.
    pub(super) fn heal_hold_latches(&mut self) {
        let (shift, control) = (self.modifiers.shift_key(), self.modifiers.control_key());
        if !shift {
            for key in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
                self.physical_keys.remove(&key);
            }
        }
        if !control {
            for key in [KeyCode::ControlLeft, KeyCode::ControlRight] {
                self.physical_keys.remove(&key);
            }
        }
        if !self.modifiers.alt_key() {
            for key in [KeyCode::AltLeft, KeyCode::AltRight] {
                self.physical_keys.remove(&key);
            }
        }
        if !self.modifiers.super_key() {
            for key in [KeyCode::SuperLeft, KeyCode::SuperRight] {
                self.physical_keys.remove(&key);
            }
        }
        for (slot, mapping, needs_toggle) in [
            (0, "key.sneak", self.settings.toggle_crouch),
            (1, "key.sprint", self.settings.toggle_sprint),
        ] {
            let held = match horizon_client::keybinds::key_code(self.settings.keys.get(mapping)) {
                Some(KeyCode::ShiftLeft | KeyCode::ShiftRight) => shift,
                Some(KeyCode::ControlLeft | KeyCode::ControlRight) => control,
                _ => continue,
            };
            // A controller's stick click acts as the key without setting the modifier: it is held
            // while the pad holds it (or its release waits for a tick), not when the keyboard says.
            let pad = self.input_holds(mapping);
            let logical_key = [KeyCode::ShiftLeft, KeyCode::ControlLeft][slot];
            let touch = self.keys.held_by(&logical_key, Source::Touch);
            if !held && !needs_toggle {
                self.keys.release(&logical_key, Source::Keyboard);
            }
            if !held && !pad && !touch {
                self.toggles.0[slot].set_down(false, needs_toggle);
                self.sync_toggle(slot);
            }
        }
    }
    /// Whether the other device still holds the sneak or sprint mapping `source` released.
    pub(super) fn toggle_held_elsewhere(&self, slot: usize, key: KeyCode, source: Source) -> bool {
        if source == Source::Agent {
            return false;
        }
        let touch = source != Source::Touch && self.keys.held_by(&key, Source::Touch);
        let pad = source != Source::Pad
            && !(slot == SNEAK && self.settings.controller.toggle_sneak)
            && self.pad.holds_action(["key.sneak", "key.sprint"][slot]);
        let keyboard = matches!(source, Source::Pad | Source::Touch)
            && self.physical_keys.iter().any(|physical| {
                self.settings
                    .keys
                    .translate_all(*physical)
                    .contains(&Ok(key))
            });
        touch || pad || keyboard
    }
    /// Apply a physical key to every mapping bound to it.
    pub(super) fn mapped_key_event(
        &mut self,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> Result<()> {
        let source = if self.touch_synthetic_input() {
            Source::Touch
        } else {
            Source::Keyboard
        };
        for mapped in self.settings.keys.translate_all(key) {
            match mapped {
                Ok(k) => {
                    self.mapped_key(k, pressed, repeat, key, source)?;
                }
                Err(button) => {
                    let action = match (pressed, repeat) {
                        (false, _) => KeyAction::Release,
                        (_, false) => KeyAction::Press,
                        (_, true) => KeyAction::Repeat,
                    };
                    if let Some(pressed) = keyboard_button(
                        gameplay_has_keyboard(
                            self.screen,
                            self.title.is_some(),
                            self.screen_over_playing(),
                        ) || (cfg!(target_os = "ios") && !pressed),
                        action,
                    ) {
                        if self.shared_default_acts(button, horizon_client::keybinds::key_name(key))
                        {
                            self.mouse_mapping(button, pressed, source)?;
                        }
                    } else if container_key_clones(button, action) {
                        self.inventory_pick_key();
                    }
                }
            }
        }
        Ok(())
    }
    /// A key event after rebinding, as the default key of its mapping (the
    /// keyboard and controllers both end here). `raw` is the physical key
    /// the event started from (`key` where the caller started from no window
    /// key): the screens act in that keyspace (`Screen.keyPressed`), where
    /// `key`, the vanilla default the mapping translated to, may itself be
    /// another mapping's rebound key. `source` is the device the event came
    /// from: a key stays held while any device holds it.
    pub(super) fn mapped_key(
        &mut self,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
        raw: KeyCode,
        source: Source,
    ) -> Result<()> {
        if self.screen == Screen::Playing && self.replay.playing() {
            if !pressed {
                self.keys.release(&key, source);
                return Ok(());
            }
            if studio_replay_shortcut(key) {
                if !repeat {
                    self.key_pressed(key, raw)?;
                }
                return Ok(());
            }
            if !studio_freecam_movement_key(key) || !self.studio.flying() {
                return Ok(());
            }
        }
        // Sneak and sprint are ToggleKeyMappings: a press (or a key
        // repeat) flips them when toggling; only releases reach them
        // behind a screen (KeyboardHandler.keyPress).
        let toggle = match key {
            KeyCode::ShiftLeft => Some((0, self.settings.toggle_crouch)),
            KeyCode::ControlLeft => Some((1, self.settings.toggle_sprint)),
            _ => None,
        };
        if let Some((slot, option)) = toggle {
            let keyboard_held = self.keys.held_by(&key, Source::Keyboard);
            if !pressed && !option && !self.keys.release(&key, source) {
                return Ok(());
            }
            let action = match (pressed, repeat) {
                (false, _) => KeyAction::Release,
                (_, false) => KeyAction::Press,
                (_, true) => KeyAction::Repeat,
            };
            // One device letting go doesn't let go for the other still holding it.
            if !pressed && self.toggle_held_elsewhere(slot, key, source) {
                return Ok(());
            }
            let screen_open = !gameplay_has_keyboard(
                self.screen,
                self.title.is_some(),
                self.screen_over_playing(),
            );
            self.toggles.key(slot, action, screen_open, option);
            self.sync_toggle(slot);
            if !pressed {
                return Ok(());
            }
            if self.keys.raw(&key) {
                self.keys.press(key, source);
                if source != Source::Keyboard && !keyboard_held {
                    self.keys.release(&key, Source::Keyboard);
                }
            }
        } else if !pressed {
            self.keys.release(&key, source);
            return Ok(());
        } else {
            self.keys.press(key, source);
        }
        // KeyboardHandler.keyPress: a repeat (InputConstants.REPEAT) reaches
        // Screen.keyPressed and KeyMapping.click like a press, so holding
        // key.drop keeps throwing, in game and over a menu slot.
        if repeat && key != KeyCode::KeyQ {
            return Ok(());
        }
        self.key_pressed(key, raw)
    }
    /// `KeyMapping.releaseAll` when a screen opens and
    /// `restoreToggleStatesOnScreenClosed` when it closes.
    pub(super) fn track_toggle_screens(&mut self) {
        let dialog = self.dialog_open();
        drop_held_behind_dialog(dialog, &mut self.keys, &mut self.left, &mut self.right);
        let playing = gameplay_has_keyboard(
            self.screen,
            self.title.is_some(),
            self.screen_over_playing(),
        );
        if playing == self.was_playing {
            return;
        }
        self.was_playing = playing;
        let options = [
            self.settings.toggle_crouch,
            self.settings.toggle_sprint,
            self.settings.toggle_attack,
            self.settings.toggle_use,
        ];
        if playing {
            self.toggles.screen_closed(options);
        } else {
            self.toggles.screen_opened(options);
        }
        self.sync_toggle(0);
        self.sync_toggle(1);
        self.left = self.toggles.is_down(2);
        self.right = self.toggles.is_down(3);
    }

    /// `MouseHandler.onScroll`: the hotbar (or the spectator menu) takes the wheel only when no
    /// screen is open; a book, lectern, sign or prompt drawn over gameplay is one.
    pub(super) fn hotbar_takes_wheel(&self) -> bool {
        self.screen == Screen::Playing
            && !self.screen_over_playing()
            && self.sleeping.is_none()
            && self.player.is_some()
    }
    /// A mod's `MouseScrolling` cancels the step about to turn the hotbar.
    pub(super) fn hotbar_scroll_canceled(&mut self, scroll: [f64; 2], steps: [i32; 2]) -> bool {
        let mut canceled = false;
        self.native_mouse_scrolling(
            scroll,
            steps,
            [
                self.physical_buttons.contains(&MouseButton::Left),
                self.physical_buttons.contains(&MouseButton::Middle),
                self.physical_buttons.contains(&MouseButton::Right),
            ],
            [self.cursor.0, self.cursor.1],
            glfw_modifiers(self.modifiers),
            &mut canceled,
        );
        canceled
    }
    /// What one whole wheel step does in play (`MouseHandler.onScroll`): a spectator's menu or
    /// flying speed, else the selected hotbar slot.
    pub(super) fn hotbar_wheel_step(&mut self, wheel: i32, vertical_steps: i32) {
        if self.spectator() && self.spectator_gui.is_menu_active() {
            self.spectator_scroll(-wheel);
        } else if self.spectator() {
            if let Some(player) = &mut self.player {
                player.flying_speed = horizon_client::mouse::spectator_flying_speed(
                    player.flying_speed,
                    vertical_steps,
                );
            }
        } else {
            self.selected = horizon_client::mouse::ScrollWheelHandler::next_selection(
                f64::from(wheel),
                self.selected as i32,
                9,
            ) as usize;
        }
    }
    /// A step of the hotbar from a controller's bumper: one wheel notch, down for the next slot,
    /// through everything the wheel goes through.
    pub(super) fn hotbar_step(&mut self, next: bool) {
        let wheel = if next { -1 } else { 1 };
        if self.hotbar_takes_wheel()
            && !self.native_wheel_event(wheel as f32)
            && !self.hotbar_scroll_canceled([0.0, f64::from(wheel)], [0, wheel])
        {
            self.hotbar_wheel_step(wheel, wheel);
        }
    }
    /// A mouse mapping pressed or let go by `source`. A release waits while another device holds
    /// the mapping, so letting go of one can't end what the other still presses, as for keys
    /// (`HeldKeys`).
    pub(super) fn mouse_mapping(
        &mut self,
        button: MouseButton,
        pressed: bool,
        source: Source,
    ) -> Result<()> {
        if !pressed && self.mouse_mapping_held_elsewhere(button, source) {
            return Ok(());
        }
        self.play_mouse(button, pressed)
    }
    /// Whether a device other than `source` holds what `button` stands for. Asks each device's own
    /// live state rather than keeping a count: a screen takes a mouse release without telling the
    /// mappings, and a count would then hold the attack for good. A script keeps no such state, so
    /// nothing waits for a script's mouse hold.
    pub(super) fn mouse_mapping_held_elsewhere(&self, button: MouseButton, source: Source) -> bool {
        let mapping = match button {
            MouseButton::Left => "key.attack",
            MouseButton::Right => "key.use",
            _ => return false,
        };
        let touch = source != Source::Touch && self.touch_holds_mouse_mapping(button);
        let pad = source != Source::Pad && self.pad.holds_action(mapping);
        let keyboard = matches!(source, Source::Pad | Source::Touch) && {
            let keys = &self.settings.keys;
            self.physical_buttons
                .iter()
                .any(|held| keys.translate_mouse_all(*held).contains(&Err(button)))
                || self
                    .physical_keys
                    .iter()
                    .any(|held| keys.translate_all(*held).contains(&Err(button)))
        };
        touch || pad || keyboard
    }
    /// Two mappings with one default button are separate mappings all the same
    /// (`Minecraft.handleKeybinds`): whether the middle-button action a physical `input` translated
    /// to is one of those it is bound to, for this player now.
    pub(super) fn shared_default_acts(&self, button: MouseButton, input: Option<&str>) -> bool {
        button != MouseButton::Middle
            || input.is_none_or(|input| self.settings.keys.middle_acts(input, self.spectator()))
    }
    /// A mouse button while playing (window or control socket).
    pub(super) fn play_mouse(&mut self, button: MouseButton, pressed: bool) -> Result<()> {
        if pressed {
            self.pvp_click(button);
        }
        if self.sleeping.is_some() {
            return Ok(());
        }
        // key.spectatorHotbar (the middle button) opens the spectator menu
        // and uses its selected item.
        if button == MouseButton::Middle && self.spectator() {
            if pressed {
                self.spectator_action_key();
            }
            return Ok(());
        }
        if self.remote.is_some() {
            return self.remote_mouse(button, pressed);
        }
        match button {
            MouseButton::Left => {
                // KeyMapping.click: Minecraft.tick starts the attack.
                if pressed {
                    self.attack_clicks += 1;
                }
                self.toggles
                    .button(2, pressed, false, self.settings.toggle_attack);
                self.left = self.toggles.is_down(2);
            }
            MouseButton::Right => {
                self.toggles
                    .button(3, pressed, false, self.settings.toggle_use);
                self.right = self.toggles.is_down(3);
                // handleKeybinds consumes use clicks without acting while an
                // item is in use. Minecraft.tick runs them after the attack
                // clicks, so a place and a break in one tick replace the block.
                if pressed && self.using.is_none() {
                    self.use_clicks += 1;
                }
            }
            MouseButton::Middle if pressed => {
                // handleKeybinds: the number keys select before key.pickItem.
                self.select_queued_hotbar();
                // Minecraft.pickBlockOrEntity -> the server's handlePickItemFromBlock:
                // the block's clone item, with its block entity's data on Ctrl in
                // creative, picked from the inventory (`pick_item`).
                if let Some(entity) = self.pick_entity_hit() {
                    // handlePickItemFromEntity: the entity's pick result.
                    if let Some(stack) = entity.and_then(|id| self.entity_pick_result(id)) {
                        let infinite = self.creative;
                        horizon_client::pick_item::pick(
                            &mut self.inventory[..36],
                            &mut self.selected,
                            &stack,
                            infinite,
                        );
                    }
                } else if let Some(hit) = self.pick() {
                    let include_data = self.creative && self.has_control_down();
                    if let Some(stack) = self.picked_stack(hit.pos, include_data) {
                        let infinite = self.creative;
                        horizon_client::pick_item::pick(
                            &mut self.inventory[..36],
                            &mut self.selected,
                            &stack,
                            infinite,
                        );
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// The input source (docs/layers.md, "Controllers, splitscreen, replays and UI pages"): what
/// gameplay asks of a device beyond the keyboard and mouse, and tells it. Gameplay calls these,
/// never the controller modules; today `pad` answers.
impl Game<'_> {
    /// The analog move vector (`[left, forward]`, -1..1) while a device moves the player.
    pub(super) fn input_move(&self) -> Option<[f32; 2]> {
        self.pad_move()
    }

    /// Whether a device holds the key mapping named `mapping` (`key.use`).
    pub(super) fn input_holds(&self, mapping: &str) -> bool {
        self.pad.holds_action(mapping)
    }

    /// Turns the view by what the devices give this frame (`Entity.turn`).
    pub(super) fn input_look(&mut self, seconds: f32) {
        self.pad_look(seconds);
    }

    /// The keyboard or mouse acted in play (a key or button pressed): it has control again, though
    /// only moving the mouse takes a pad out of use.
    pub(super) fn input_keyboard_used(&mut self) {
        self.pad.keyboard_took_over = true;
    }

    /// Whether the pointer event being handled is one a device made (a pad's menu cursor
    /// clicking), not the mouse's.
    pub(super) fn input_made_pointer_event(&self) -> bool {
        self.pad.menu.synthetic
    }

    /// A new player entity took over (spawn, respawn, a server joined, a dimension change).
    pub(super) fn input_player_replaced(&mut self) {
        self.pad_player_replaced();
    }

    /// Whether the drop about to happen is a device's whole-stack drop (it has no Control key to
    /// hold). Asking answers once.
    pub(super) fn input_takes_stack_drop(&mut self) -> bool {
        std::mem::take(&mut self.pad.drop_stack)
    }

    /// The HUD starts a frame: what a device drew on the last one is gone.
    pub(super) fn input_hud_begin(&mut self) {
        self.pad.guide_shown.clear();
    }

    /// Something happened to the player that a device may let them feel.
    pub(super) fn input_feedback(&mut self, event: horizon_client::input_source::Feedback) {
        self.pad_rumble(event.into());
    }

    /// Whether `key` is held, a movement key by the analog move vector too: as the keys it holds
    /// (`eight_direction_keys`), whatever moves the player. For readers of the keys that aren't
    /// the movement itself.
    pub(super) fn movement_key_held(&self, key: KeyCode) -> bool {
        if self.keys.raw(&key) {
            return true;
        }
        let Some(stick) = self.input_move() else {
            return false;
        };
        let [forward, backward, left, right] = horizon_client::gamepad::eight_direction_keys(stick);
        match key {
            KeyCode::KeyW => forward,
            KeyCode::KeyS => backward,
            KeyCode::KeyA => left,
            KeyCode::KeyD => right,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{studio_freecam_movement_key, studio_replay_shortcut};
    use winit::keyboard::KeyCode;

    #[test]
    fn replay_shortcuts_are_distinct_from_freecam_movement() {
        for key in [
            KeyCode::Escape,
            KeyCode::KeyP,
            KeyCode::KeyF,
            KeyCode::KeyT,
            KeyCode::KeyM,
            KeyCode::Home,
            KeyCode::KeyL,
            KeyCode::KeyK,
            KeyCode::Delete,
        ] {
            assert!(studio_replay_shortcut(key));
        }
        for key in [
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyQ,
        ] {
            assert!(studio_freecam_movement_key(key));
            assert!(!studio_replay_shortcut(key));
        }
        assert!(!studio_freecam_movement_key(KeyCode::KeyI));
    }
}

impl Game<'_> {
    pub(super) fn reload_blocks_input(&self) -> bool {
        self.pad.reload_pending || self.pack_reload.is_some() || self.content_reload_busy()
    }
    pub(super) fn begin_reload_input_block(&mut self) {
        self.pad.reload_pending = true;
        self.release_use_for_keybind_screen();
        self.keys.clear();
        self.physical_keys.clear();
        self.physical_buttons.clear();
        self.left = false;
        self.right = false;
        self.mining = None;
        self.drag_button = None;
        self.dragging = false;
        self.inv_drag = None;
        self.cancel_emote_controller();
        self.cancel_touch_for_reload();
        self.suspend_system_keyboard();
        self.ui.options.release();
        if let Some(title) = &mut self.title {
            title.release();
            title.release_field();
        }
        self.hud_editor_release();
        self.capture(false);
    }
}
