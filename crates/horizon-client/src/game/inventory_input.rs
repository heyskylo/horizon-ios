//! Keys and mouse buttons over an open container screen: hotbar and pick keys, modifier keys,
//! drags, throws and the press/release of a slot click.
use super::*;

impl Game<'_> {
    /// `CreativeModeInventoryScreen.keyPressed` on the search tab: a hotbar
    /// key over a slot swaps before the search box sees it.
    pub(super) fn search_tab_hotbar_key(&mut self, code: KeyCode) -> bool {
        use horizon_client::creative::{Hover, search_key_swaps};
        let numeric = HOTBAR_DIGITS.contains(&code) || NUMERIC_KEYS.contains(&code);
        if !numeric {
            return false;
        }
        let Some(key) = self.settings.keys.container_hotbar_key(code) else {
            return false;
        };
        let Some(size) = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
        else {
            return false;
        };
        let batch = HudBatch::new(size);
        let hover = match self.slot_at(&batch, self.cursor.0, self.cursor.1) {
            None => Hover::Nothing,
            Some(slot) if slot >= Creative::DISPLAY => {
                let first = self.creative_screen.first_row(&self.pack.names);
                Hover::Display {
                    filled: self
                        .creative_screen
                        .shown(&self.pack.names, first * 9 + slot - Creative::DISPLAY)
                        .is_some(),
                }
            }
            Some(_) => Hover::Slot,
        };
        search_key_swaps(numeric, true, hover, self.carried.is_some()) && self.inventory_hotkey(key)
    }
    /// Number keys, F and Q over a slot of the open menu: SWAP with a hotbar
    /// slot or the off hand, THROW one or the whole stack.
    pub(super) fn inventory_hotkey(&mut self, key: KeyCode) -> bool {
        use horizon_client::menu::Input;
        let Some(size) = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
        else {
            return false;
        };
        let batch = HudBatch::new(size);
        let Some(slot) = self.slot_at(&batch, self.cursor.0, self.cursor.1) else {
            return false;
        };
        let digit = HOTBAR_DIGITS.iter().position(|k| *k == key);
        let done = match (digit, key) {
            (Some(n), _) => self.menu_click(Some(slot), n as i32, Input::Swap),
            (None, KeyCode::KeyF) => self.menu_click(Some(slot), 40, Input::Swap),
            (None, KeyCode::KeyQ) => {
                let all = drop_whole_stack(self.modifiers);
                self.menu_click(Some(slot), i32::from(all), Input::Throw)
            }
            _ => false,
        };
        if done && let Some(key) = self.container {
            self.container_changed(key);
        }
        done
    }
    /// The pick block key bound to a keyboard key, over a slot of the open
    /// menu (`AbstractContainerScreen.keyPressed`): `CLONE` with button 0. The
    /// search tab's box takes the key instead.
    pub(super) fn inventory_pick_key(&mut self) {
        let menu = (self.screen == Screen::Inventory
            || (self.screen == Screen::Creative && !self.creative_screen.tab().search))
            && !self.dialog_open();
        let Some(size) = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
        else {
            return;
        };
        let batch = HudBatch::new(size);
        let Some(slot) = self
            .slot_at(&batch, self.cursor.0, self.cursor.1)
            .filter(|_| menu)
        else {
            return;
        };
        if self.menu_click(Some(slot), 0, horizon_client::menu::Input::Clone)
            && let Some(key) = self.container
        {
            self.container_changed(key);
        }
    }
    /// The plain inventory mouse press: starts a quick craft drag when a
    /// stack is held, gathers on a double click, else the normal click.
    pub(super) fn inventory_press(&mut self, right: bool) {
        use horizon_client::container_screen::Target;
        use horizon_client::menu::Input;
        if self.mod_browser_press(right) {
            return;
        }
        if !right && self.sort_press() {
            return;
        }
        if self.native_screen_press(right) {
            self.inv_clicks.skip_release();
            return;
        }
        // AbstractRecipeBookScreen lets the book handle its buttons before
        // AbstractContainerScreen starts a drag or chooses an outside slot.
        // Toggling it can move the inventory under the pointer before release.
        if self.recipe_book_click(right) {
            self.inv_clicks.skip_release();
            return;
        }
        let (slot, outside) = self.inventory_target();
        let shift = self.has_shift_down();
        let target = Target::of(slot, outside);
        let input = self.inv_clicks.press(
            &target,
            Self::screen_button(right),
            self.carried.is_none(),
            shift,
            false,
        );
        if outside {
            // slotId -999: an empty hand throws nothing; a carried stack
            // starts a drag whose release drops it.
            match input {
                Some(input) => self.throw_click(right, input),
                None if self.inv_clicks.quick_crafting().is_some() => {
                    self.inv_drag = Some((i32::from(right), Vec::new()));
                }
                None => {}
            }
            return;
        }
        if self.tweak_press(slot, right, shift) {
            return;
        }
        if let Some(slot) = slot
            && shift
            && slot != 200
        {
            let now = std::time::Instant::now();
            let double = self.last_inv_click.is_some_and(|(at, last, was_right)| {
                last == slot && was_right == right && now.duration_since(at).as_millis() < 250
            });
            self.last_inv_click = Some((now, slot, right));
            // mouseClicked's QUICK_MOVE (and Horizon's at-press QUICK_MOVE
            // of a carried stack, mouseReleased's in vanilla) remember the
            // stack it moves.
            if input == Some(Input::QuickMove) || (self.carried.is_some() && !double) {
                self.last_quick_moved = self.slot_view(slot);
            }
            if double && !right {
                // mouseReleased: every stack of lastQuickMoved's item in the
                // clicked slot's container is quick-moved.
                if let Some(input) = input
                    && !self.menu_click(Some(slot), 0, input)
                {
                    // A screen the menu engine doesn't model: its own click.
                    self.click_inventory(right);
                    return;
                }
                if let Some(moved) = self.last_quick_moved.clone() {
                    // The creative screen clicks its own menu, on a server too.
                    if self.remote.is_some() && self.screen != Screen::Creative {
                        self.remote_quick_move_all(slot, &moved);
                    } else {
                        self.menu_session(|menu, env, to_menu| {
                            Some(menu.quick_move_all(env, to_menu(slot)?, &moved).is_ok())
                        });
                        if self.remote.is_some() {
                            self.send_creative_slots();
                        }
                    }
                }
                self.last_inv_click = None;
                self.inv_clicks.skip_release();
                return;
            }
        }
        if let Some(slot) = slot
            && slot != 200
        {
            // An item tab's display slots take no drags or PICKUP_ALL.
            let display = slot >= Creative::DISPLAY;
            let now = std::time::Instant::now();
            let double = !shift
                && !right
                && !display
                && self.carried.is_some()
                && self.last_inv_click.is_some_and(|(at, last, was_right)| {
                    last == slot && !was_right && now.duration_since(at).as_millis() < 250
                });
            self.last_inv_click = Some((now, slot, right));
            if double && self.menu_click(Some(slot), 0, Input::PickupAll) {
                self.last_inv_click = None;
                // The double click's own release (`mouseReleased`'s PICKUP_ALL).
                self.inv_clicks.skip_release();
                return;
            }
            if self.carried.is_some() {
                let ty = i32::from(right);
                // `mouseClicked` starts a quick craft drag whenever the hand holds a stack,
                // Shift or not: `mouseDragged` fills the slots the cursor crosses, so a
                // Shift held over the drag spreads the same way (#4166). The drag starts
                // empty on a Shift press: a release with nothing spread is `mouseReleased`'s
                // click on the hovered slot (Pickup, or QuickMove under Shift).
                if !shift && !display && self.quick_craft_accepts(slot, ty, 0) {
                    self.inv_drag = Some((ty, vec![slot]));
                } else {
                    self.inv_drag = Some((ty, Vec::new()));
                }
                return;
            }
        }
        if self.creative_tab_open() {
            // CreativeModeInventoryScreen goes through the same mouseClicked:
            // a click now, or a drag that its release ends.
            match input {
                Some(input) => {
                    self.menu_click(slot, i32::from(right), input);
                }
                None if self.inv_clicks.quick_crafting().is_some() => {
                    self.inv_drag = Some((i32::from(right), Vec::new()));
                }
                None => {}
            }
            return;
        }
        // mouseClicked: an empty hand's PICKUP is the menu's click at once
        // (a right click on a bundle takes its first stack out).
        if let (Some(input), Some(_)) = (input, slot)
            && self.remote.is_none()
            && self.menu_click(slot, i32::from(right), input)
        {
            if let Some(key) = self.container {
                self.container_changed(key);
            }
            return;
        }
        self.click_inventory(right);
    }
    /// Mouse movement while a quick craft drag is running.
    pub(super) fn inventory_drag_move(&mut self) {
        if self.tweaks_on() && self.tweak_drag_move(self.inventory_target().0) {
            return;
        }
        let Some((ty, slots)) = self.inv_drag.clone() else {
            return;
        };
        let Some(r) = self.renderer.as_ref() else {
            return;
        };
        let batch = HudBatch::new([r.config.width, r.config.height]);
        if let Some(slot) = self.slot_at(&batch, self.cursor.0, self.cursor.1)
            && slot != 200
            && slot < Creative::DISPLAY
            && !slots.contains(&slot)
            && self.quick_craft_accepts(slot, ty, slots.len())
            && let Some((_, slots)) = self.inv_drag.as_mut()
        {
            slots.push(slot);
        }
    }
    /// The hovered slot and whether the cursor is outside the window.
    pub(super) fn inventory_target(&self) -> (Option<usize>, bool) {
        let Some(r) = self.renderer.as_ref() else {
            return (None, false);
        };
        let batch = HudBatch::new([r.config.width, r.config.height]);
        (
            self.slot_at(&batch, self.cursor.0, self.cursor.1),
            self.clicked_outside(&batch),
        )
    }
    pub(super) fn screen_button(right: bool) -> horizon_client::container_screen::Button {
        use horizon_client::container_screen::Button;
        if right { Button::Right } else { Button::Left }
    }
    /// `Screen.hasShiftDown` / `InputWithModifiers.hasShiftDown`: 26.3 keeps
    /// no key cache — every event answers with its own modifiers bit
    /// (`InputWithModifiers.java:54-56`, `keys = 3`), and the pointer and
    /// keyboard events carry the OS's live word, refreshed at the focus. The
    /// window's `ModifiersChanged` is that word here; key releases can die
    /// on the way from the OS (a stuck Shift then kept every screen's Shift
    /// and a sneak the key no longer held: #4166's in-game report), so a
    /// cached key set is not vanilla's answer.
    pub(super) fn has_shift_down(&self) -> bool {
        self.modifiers.shift_key()
    }
    /// The same live word for Control (`InputWithModifiers.hasControlDown`).
    pub(super) fn has_control_down(&self) -> bool {
        self.modifiers.control_key()
    }
    /// The same live word for Alt (`InputWithModifiers.hasAltDown`).
    pub(super) fn has_alt_down(&self) -> bool {
        self.modifiers.alt_key()
    }
    /// Real events word the modifiers by their own `ModifiersChanged`; a
    /// control socket's `key` is synthesized, so it words the bit directly
    /// and every reader can ask the one live state.
    pub(super) fn press_modifier_key(&mut self, key: KeyCode, down: bool) {
        horizon_client_input::modifiers::set_key(
            &mut self.modifiers,
            &mut self.physical_keys,
            key,
            down,
        );
    }
    /// `hasClickedOutside` of the open screen at the cursor.
    pub(super) fn clicked_outside(&self, batch: &HudBatch) -> bool {
        use horizon_client::container_screen::{Book, clicked_outside};
        let image = self.container_image(batch);
        let book = self
            .recipe_book_area(batch)
            .map(|(visible, tab_hovered)| Book {
                visible,
                selected_tab_hovered: tab_hovered,
            });
        clicked_outside(
            f64::from(self.cursor.0),
            f64::from(self.cursor.1),
            image,
            book,
        )
    }
    /// The open container screen's window: its place and size.
    pub(super) fn container_image(
        &self,
        batch: &HudBatch,
    ) -> horizon_client::container_screen::Image {
        let (ox, oy) = self.inventory_origin(batch);
        let (shift, width, height) = if self.screen == Screen::Creative {
            (0.0, Creative::WIDTH as i32, Creative::HEIGHT as i32)
        } else {
            let (shift, width) = match self.station.as_ref().map(|s| s.kind) {
                Some(stations::Kind::Merchant) => (-50.0, 276),
                Some(stations::Kind::Beacon) => (beacon::SHIFT, 230),
                _ => (0.0, self.native_screen_width().map_or(176, |w| w as i32)),
            };
            (shift, width, self.screen_height() as i32)
        };
        horizon_client::container_screen::Image {
            left: (ox + shift) as i32,
            top: oy as i32,
            width,
            height,
        }
    }
    /// A click outside the window (slot id -999): `THROW` does nothing; a
    /// `PICKUP` drops the carried stack (left) or one of it (right), thrown
    /// from the hand (`AbstractContainerMenu.doClick`, `LivingEntity.drop`).
    pub(super) fn throw_click(&mut self, right: bool, input: horizon_client::menu::Input) {
        use horizon_client::menu::Input;
        let button = i32::from(right);
        if self.remote.is_some() {
            if self.screen == Screen::Creative {
                self.remote_creative_drop(right);
            } else {
                self.remote_throw(right, input);
            }
            return;
        }
        let before = self.carried.clone();
        if !self.menu_click(None, button, input) && input == Input::Pickup {
            // A screen the menu engine doesn't model: the same doClick.
            if let Some(c) = self.carried.as_mut() {
                let thrown = if right {
                    let one = ItemStack {
                        count: 1,
                        ..c.clone()
                    };
                    c.count -= 1;
                    if c.count == 0 {
                        self.carried = None;
                    }
                    one
                } else {
                    self.carried.take().expect("some")
                };
                self.player_drop(thrown);
            }
        }
        if self.carried != before {
            self.swing_arm();
        }
        if let Some(key) = self.container {
            self.container_changed(key);
        }
    }
    /// Mouse release over an inventory screen: `mouseReleased` ends or
    /// cancels the drag, drops a stack released outside, or clicks.
    pub(super) fn inventory_mouse_release(&mut self, right: bool) {
        use horizon_client::container_screen::{Release, Target};
        self.tweak_release();
        let (slot, outside) = self.inventory_target();
        let dragged = self.inv_drag.as_ref().is_some_and(|(_, s)| !s.is_empty());
        let release = self.inv_clicks.release(
            &Target::of(slot, outside),
            Self::screen_button(right),
            self.carried.is_none(),
            dragged,
            self.has_shift_down(),
            false,
        );
        match (release, Target::of(slot, outside)) {
            (Release::QuickCraft, _) => self.inventory_release(),
            (Release::Click(input), Target::Outside) => {
                self.inv_drag = None;
                self.throw_click(right, input);
            }
            (Release::Click(input), Target::Slot(slot)) if self.inv_drag.is_some() => {
                // The drag never reached a slot: a plain click where it ends.
                self.inv_drag = None;
                if !self.menu_click(Some(slot), i32::from(right), input) {
                    self.click_inventory(right);
                }
                if let Some(key) = self.container {
                    self.container_changed(key);
                }
            }
            _ => self.inv_drag = None,
        }
    }
    /// A press of the pick block button (`keyPickItem`, the middle button by
    /// default) over an inventory screen: `mouseClicked` with `cloning`
    /// (infinite materials). An empty hand clones the hovered stack at its
    /// full size (`CLONE`), a carried stack starts a quick craft drag of type
    /// 2 that fills every slot it crosses; without infinite materials the
    /// press does nothing.
    pub(super) fn inventory_pick_press(&mut self) {
        use horizon_client::container_screen::{Button, Target};
        let (slot, outside) = self.inventory_target();
        let was_crafting = self.inv_clicks.quick_crafting().is_some();
        let input = self.inv_clicks.press(
            &Target::of(slot, outside),
            Button::Middle,
            self.carried.is_none(),
            self.has_shift_down(),
            self.creative,
        );
        match (input, slot) {
            // Slot -999 (outside the window) clones nothing.
            (Some(input), Some(slot)) if !outside => {
                self.menu_click(Some(slot), 2, input);
            }
            (None, _) if !was_crafting && self.inv_clicks.quick_crafting().is_some() => {
                // Item tab display slots take no drags.
                let first = slot.filter(|&s| {
                    s != 200 && s < Creative::DISPLAY && self.quick_craft_accepts(s, 2, 0)
                });
                self.inv_drag = Some((2, first.into_iter().collect()));
            }
            _ => {}
        }
    }
    /// The pick block button's release (`mouseReleased`): ends its quick
    /// craft drag, cancels another button's, or clones onto the slot under a
    /// carried stack (`CLONE`, which only an item tab's display slot takes).
    pub(super) fn inventory_pick_release(&mut self) {
        use horizon_client::container_screen::{Button, Release, Target};
        let (slot, outside) = self.inventory_target();
        let dragged = self.inv_drag.as_ref().is_some_and(|(_, s)| !s.is_empty());
        let release = self.inv_clicks.release(
            &Target::of(slot, outside),
            Button::Middle,
            self.carried.is_none(),
            dragged,
            self.has_shift_down(),
            true,
        );
        match (release, slot) {
            (Release::QuickCraft, _) => self.inventory_release(),
            (Release::Click(input), Some(slot)) if !outside => {
                self.inv_drag = None;
                self.menu_click(Some(slot), 2, input);
            }
            _ => self.inv_drag = None,
        }
    }
    /// Mouse release: finishes a quick craft drag (one slot is a plain click).
    pub(super) fn inventory_release(&mut self) {
        use horizon_client::menu::Input;
        let Some((ty, slots)) = self.inv_drag.take() else {
            return;
        };
        if slots.len() == 1 {
            if !self.menu_click(Some(slots[0]), ty, Input::Pickup) {
                self.click_inventory(ty == 1);
            }
        } else {
            let mask = |header: i32| header | (ty << 2);
            let mut ops = vec![(None, mask(0), Input::QuickCraft)];
            ops.extend(
                slots
                    .into_iter()
                    .map(|slot| (Some(slot), mask(1), Input::QuickCraft)),
            );
            ops.push((None, mask(2), Input::QuickCraft));
            if !self.menu_ops(&ops) {
                // Not a menu the engine models: the drag is a plain click.
                self.click_inventory(ty == 1);
            }
        }
        if let Some(key) = self.container {
            self.container_changed(key);
        }
    }
}
