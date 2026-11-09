//! The creative inventory screen (`CreativeModeInventoryScreen`): its tabs, pages, tooltips,
//! trash slot, search box and clicks.
use super::*;

impl Game<'_> {
    /// `CreativeModeInventoryScreen.hasPermissions` (`canUseGameMasterBlocks`
    /// and the Operator Items Tab option) given to `tryRefreshInvalidatedTabs`,
    /// as the screen's constructor and `containerTick` do.
    pub(super) fn refresh_creative_tabs(&mut self) {
        let permissions = self.can_use_game_master_blocks() && self.settings.operator_items_tab;
        let enchantments = Arc::clone(self.enchantments());
        self.creative_screen.set_enchantments(enchantments);
        self.creative_screen
            .set_permissions(permissions, &self.pack.names);
    }
    pub(super) fn creative_inventory_open(&self) -> bool {
        self.screen == Screen::Creative && self.creative_screen.tab().inventory
    }
    /// An item tab of the creative screen (its ItemPickerMenu).
    pub(super) fn creative_tab_open(&self) -> bool {
        self.screen == Screen::Creative && !self.creative_screen.tab().inventory
    }

    pub(super) fn creative_origin(batch: &HudBatch) -> (f32, f32) {
        batch.centered_origin(Creative::WIDTH, Creative::HEIGHT)
    }
    /// A creative tab's title: `itemGroup.*` for vanilla's; a mod tab's
    /// `itemGroup.<mod>.<name>`, else its `title` as a lang key (a mod's own
    /// `itemGroup.<mod>`), else `title` as English text.
    pub(super) fn creative_tab_title(&self, tab: &horizon_client::creative::Tab) -> String {
        match horizon_client::creative::item_group_key(&tab.id) {
            Some(key) => self.translate(key, &tab.title),
            None => {
                let own = self.translate(&tab.title, &tab.title);
                self.translate(&format!("itemGroup.{}", tab.id.replace(':', ".")), &own)
            }
        }
    }
    /// Vanilla tooltip: dark box with a purple gradient border, laid out and
    /// drawn like an item tooltip (4 px of padding on every side).
    pub(super) fn tooltip(&self, batch: &mut HudBatch, text: &str, x: f32, y: f32) {
        self.draw_tooltip_lines(batch, &[(text, u32::MAX)], x, y);
    }
    /// `CreativeModeInventoryScreen.getTooltipFromContainerItem`: the lines
    /// that follow the item's name, none for a category tab's own grid slots
    /// (`grid`): the names of the tabs containing the stack, in blue, then on
    /// the search tab's grid the tags a `#` search shows, in dark purple.
    pub(super) fn creative_tooltip_tabs(
        &self,
        stack: &ItemStack,
        grid: bool,
    ) -> Vec<(horizon_client::text::Text, u32)> {
        let screen = &self.creative_screen;
        let tab = screen.tab();
        let category = !tab.search && !tab.inventory;
        if !horizon_client::creative::tooltip_lists_tabs(category, grid) {
            return Vec::new();
        }
        let blue = rgba(85, 85, 255, 255);
        let dark_purple = rgba(170, 0, 170, 255);
        let mut lines: Vec<(horizon_client::text::Text, u32)> = screen
            .tabs_containing(stack)
            .into_iter()
            .map(|i| {
                let title = self.creative_tab_title(&screen.tabs[i]);
                (horizon_client::text::Text::literal(title), blue)
            })
            .collect();
        if tab.search && grid {
            for tag in screen.search_tag_lines(stack) {
                lines.push((horizon_client::text::Text::literal(tag), dark_purple));
            }
        }
        lines
    }
    /// `CreativeModeInventoryScreen.extractRenderState`: item slots share the
    /// container screen's hover target for their highlight and tooltip.
    pub(super) fn draw_creative(&mut self, batch: &mut HudBatch) {
        // CreativeModeInventoryScreen: `CommonComponents.EMPTY` for a title.
        screen_info::begin_container("creative");
        let (ox, oy) = Self::creative_origin(batch);
        let selected = self.creative_screen.selected;
        // Only what is drawn of the tabs: their stacks stay where they are.
        let tabs = self.creative_screen.tab_headers();
        let tab = &tabs[selected.min(tabs.len() - 1)];
        let tab_sprite = |t: &horizon_client::creative::Tab, on: bool| {
            format!(
                "gui/sprites/container/creative_inventory/tab_{}_{}_{}",
                if t.top { "top" } else { "bottom" },
                if on { "selected" } else { "unselected" },
                t.column + 1
            )
        };
        self.render_transparent_background("creative", batch);
        // Unselected tabs behind the panel, then the panel, then the selected tab.
        for (i, t) in tabs.iter().enumerate() {
            if i != selected && self.creative_screen.on_page(t) {
                let (tx, ty) = Creative::tab_position(t);
                batch.sprite(&self.gui.atlas, &tab_sprite(t, false), ox + tx, oy + ty);
                let mouse = (self.cursor.0 - ox, self.cursor.1 - oy);
                if Creative::tab_wants_hand(t, false, mouse) {
                    horizon_client::widgets::request_cursor(
                        horizon_client::widgets::Cursor::PointingHand,
                    );
                }
            }
        }
        let background = if tab.search {
            "gui/container/creative_inventory/tab_item_search"
        } else if tab.inventory {
            "gui/container/creative_inventory/tab_inventory"
        } else {
            "gui/container/creative_inventory/tab_items"
        };
        if let Some(mut r) = self.gui.atlas.sprites.get(background).copied() {
            r.u1 = r.u0 + (r.u1 - r.u0) * Creative::WIDTH / r.w as f32;
            r.v1 = r.v0 + (r.v1 - r.v0) * Creative::HEIGHT / r.h as f32;
            batch.rect_uv(ox, oy, Creative::WIDTH, Creative::HEIGHT, r, u32::MAX);
        }
        let (tx, ty) = Creative::tab_position(tab);
        batch.sprite(&self.gui.atlas, &tab_sprite(tab, true), ox + tx, oy + ty);
        for t in tabs.iter().filter(|t| self.creative_screen.on_page(t)) {
            let (tx, ty) = Creative::tab_position(t);
            let stack = ItemStack {
                item: Arc::from(t.icon.as_str()),
                count: 1,
                damage: 0,
                extra: None,
            };
            self.draw_item(
                batch,
                &stack,
                ox + tx + 5.0,
                oy + ty + if t.top { 9.0 } else { 7.0 },
            );
        }
        let atlas = &self.gui.atlas;
        if !tab.inventory {
            let title = self.creative_tab_title(tab);
            batch.text(
                atlas,
                &title,
                ox + 8.0,
                oy + 6.0,
                rgba(64, 64, 64, 255),
                false,
            );
        }
        if tab.search {
            // CreativeModeInventoryScreen: an unbordered EditBox of 50 at (82, 6).
            let search = &mut self.creative_screen.search;
            search.x = ox + 82.0;
            search.y = oy + 6.0;
            search.width = 80.0;
            search.bordered = false;
            search.focused = true;
            search.text_color = u32::MAX;
            search.max_length = clipboard::CREATIVE_SEARCH_CHARS;
            if horizon_client::widgets::narrating() {
                search.message = screen_info::tr("itemGroup.search", &[]);
            }
            search.draw(batch, atlas, None);
        }
        // The hovered stack, and whether it is a grid slot (`CustomCreativeSlot`).
        let mut hovered: Option<(ItemStack, bool)> = None;
        // Use the same padded slot bounds as clicks and the other menus, so
        // highlights and tooltips remain visible between adjacent items.
        let hovered_slot = self.slot_at(batch, self.cursor.0, self.cursor.1);
        if tab.inventory {
            self.draw_player_preview(
                batch,
                ox + 73.0,
                oy + 6.0,
                ox + 105.0,
                oy + 49.0,
                20.0,
                0.0625,
            );
            let trash = Self::creative_trash_hovered(batch, self.cursor);
            let (tx, ty) = Creative::TRASH_POSITION;
            if trash {
                self.draw_slot_highlight(batch, false, ox + tx, oy + ty);
            }
            self.draw_hovered_slot_highlight(batch, false, ox, oy);
            self.draw_inventory_slots(batch, ox, oy);
            if trash {
                self.draw_slot_highlight(batch, true, ox + tx, oy + ty);
            }
            self.draw_hovered_slot_highlight(batch, true, ox, oy);
        } else {
            let items = self.creative_screen.items(&self.pack.names);
            let first = self.creative_screen.first_row(&self.pack.names);
            for r in 0..5 {
                for c in 0..9 {
                    let Some(stack) = items.get((first + r) * 9 + c).filter(|s| s.count > 0) else {
                        continue;
                    };
                    let (x, y) = (ox + 9.0 + c as f32 * 18.0, oy + 18.0 + r as f32 * 18.0);
                    let over = hovered_slot == Some(Creative::DISPLAY + r * 9 + c);
                    if over {
                        self.draw_slot_highlight(batch, false, x, y);
                    }
                    self.draw_item(batch, stack, x, y);
                    if over {
                        self.draw_slot_highlight(batch, true, x, y);
                        hovered = Some((stack.clone(), true));
                    }
                }
            }
            let drag = self.drag_view();
            for c in 0..9 {
                let (x, y) = (ox + 9.0 + c as f32 * 18.0, oy + 112.0);
                let over = hovered_slot == Some(c);
                if over {
                    self.draw_slot_highlight(batch, false, x, y);
                }
                self.draw_slot_item(batch, drag.as_ref(), c, &self.inventory[c], x, y);
                if over {
                    self.draw_slot_highlight(batch, true, x, y);
                    hovered = self.inventory[c].clone().map(|s| (s, false));
                }
            }
            let scrollable = self.creative_screen.rows(&self.pack.names) > 5;
            let scroller = if scrollable {
                "gui/sprites/container/creative_inventory/scroller"
            } else {
                "gui/sprites/container/creative_inventory/scroller_disabled"
            };
            batch.sprite(
                &self.gui.atlas,
                scroller,
                ox + Creative::SCROLL_X,
                oy + Creative::SCROLL_Y + self.creative_screen.scroller_offset(),
            );
        }
        let effect_tip = self.draw_inventory_effects(batch, (ox, oy), Creative::WIDTH as i32);
        self.draw_creative_pages(batch, ox, oy);
        for t in tabs.iter().filter(|t| self.creative_screen.on_page(t)) {
            let (tx, ty) = Creative::tab_position(t);
            if self.cursor.0 >= ox + tx
                && self.cursor.0 < ox + tx + 26.0
                && self.cursor.1 >= oy + ty
                && self.cursor.1 < oy + ty + 32.0
            {
                self.tooltip(
                    batch,
                    &self.creative_tab_title(t),
                    self.cursor.0,
                    self.cursor.1,
                );
            }
        }
        self.draw_carried(batch);
        if tab.inventory {
            if Self::creative_trash_hovered(batch, self.cursor) {
                // `CreativeModeInventoryScreen`: the destroy slot's tooltip is
                // `inventory.binSlot`, not an English literal.
                let destroy = self.translate("inventory.binSlot", "Destroy Item");
                self.tooltip(batch, &destroy, self.cursor.0, self.cursor.1);
            } else if let Some(slot) = self.slot_at(batch, self.cursor.0, self.cursor.1)
                && let Some(stack) = self.slot_view(slot)
                && self.tooltip_with_carried(&stack)
            {
                let tabs = self.creative_tooltip_tabs(&stack, false);
                self.draw_stack_tooltip_with(
                    batch,
                    &stack,
                    false,
                    self.cursor.0,
                    self.cursor.1,
                    tabs,
                );
            }
        } else if let Some((item, grid)) = hovered
            && self.tooltip_with_carried(&item)
        {
            let tabs = self.creative_tooltip_tabs(&item, grid);
            // A `CustomCreativeSlot`: TooltipFlag.asCreative.
            self.draw_stack_tooltip_with(batch, &item, grid, self.cursor.0, self.cursor.1, tabs);
        }
        if let Some(lines) = effect_tip {
            self.draw_tooltip_lines(batch, &lines, self.cursor.0, self.cursor.1);
        }
    }
    pub(super) fn creative_trash_hovered(batch: &HudBatch, cursor: (f32, f32)) -> bool {
        let (ox, oy) = Self::creative_origin(batch);
        let (x, y) = Creative::TRASH_POSITION;
        cursor.0 >= ox + x
            && cursor.0 < ox + x + 16.0
            && cursor.1 >= oy + y
            && cursor.1 < oy + y + 16.0
    }
    /// NeoForge's creative page buttons, 20 square, 50 above the screen at
    /// its left and right edges: -1 or 1 for the one at `cursor`, when there
    /// is more than one page.
    pub(super) fn creative_page_button(
        ox: f32,
        oy: f32,
        cursor: (f32, f32),
        pages: u32,
    ) -> Option<i64> {
        if pages <= 1 {
            return None;
        }
        let y = oy - 50.0;
        let inside =
            |x: f32| cursor.0 >= x && cursor.0 < x + 20.0 && cursor.1 >= y && cursor.1 < y + 20.0;
        if inside(ox) {
            Some(-1)
        } else if inside(ox + Creative::WIDTH - 20.0) {
            Some(1)
        } else {
            None
        }
    }
    /// The page buttons and "page / pages" between them.
    pub(super) fn draw_creative_pages(&self, batch: &mut HudBatch, ox: f32, oy: f32) {
        let pages = self.creative_screen.pages();
        if pages <= 1 {
            return;
        }
        let atlas = &self.gui.atlas;
        let y = oy - 50.0;
        for (x, label, step) in [(ox, "<", -1), (ox + Creative::WIDTH - 20.0, ">", 1)] {
            let hover = Self::creative_page_button(ox, oy, self.cursor, pages) == Some(step);
            let page = self.creative_screen.page as i64 + step;
            let active = (0..pages as i64).contains(&page);
            let sprite = match (active, hover) {
                (false, _) => "gui/sprites/widget/button_disabled",
                (true, true) => "gui/sprites/widget/button_highlighted",
                (true, false) => "gui/sprites/widget/button",
            };
            horizon_client::ui::blit_sprite(batch, atlas, sprite, x, y, 20.0, 20.0, u32::MAX);
            let color = if active {
                u32::MAX
            } else {
                rgba(160, 160, 160, 255)
            };
            let w = HudBatch::text_width(atlas, label);
            batch.text(atlas, label, x + (20.0 - w) / 2.0, y + 6.0, color, true);
        }
        let text = format!("{} / {}", self.creative_screen.page + 1, pages);
        let w = HudBatch::text_width(atlas, &text);
        batch.text(
            atlas,
            &text,
            ox + (Creative::WIDTH - w) / 2.0,
            oy - 44.0,
            u32::MAX,
            true,
        );
    }
    pub(super) fn click_creative(&mut self, right: bool) {
        if self.mod_browser_press(right) {
            return;
        }
        let Some(size) = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
        else {
            return;
        };
        let batch = HudBatch::new(size);
        let (ox, oy) = Self::creative_origin(&batch);
        let (cx, cy) = self.cursor;
        // NeoForge's page buttons, when mods add tabs.
        if let Some(step) =
            Self::creative_page_button(ox, oy, (cx, cy), self.creative_screen.pages())
        {
            // Page buttons are outside the inventory panel; their release must
            // not throw a carried stack, including when the button is disabled.
            self.inv_clicks.skip_release();
            let page = self.creative_screen.page as i64 + step;
            if (0..self.creative_screen.pages() as i64).contains(&page) {
                self.audio.play_button_click();
                self.creative_screen.set_page(page as u32);
                self.refresh_saved_hotbars();
            }
            return;
        }
        let tabs = self.creative_screen.tabs.clone();
        for (i, t) in tabs.iter().enumerate() {
            if !self.creative_screen.on_page(t) {
                continue;
            }
            let (tx, ty) = Creative::tab_position(t);
            if cx >= ox + tx && cx < ox + tx + 26.0 && cy >= oy + ty && cy < oy + ty + 32.0 {
                select_creative_tab(&mut self.creative_screen, &mut self.inv_clicks, i);
                self.refresh_saved_hotbars();
                self.inv_drag = None;
                self.last_inv_click = None;
                return;
            }
        }
        if self.creative_inventory_open() {
            if Self::creative_trash_hovered(&batch, self.cursor) {
                if self.has_shift_down() {
                    let before = self.player_equipment();
                    self.inventory.fill(None);
                    self.armor.fill(None);
                    self.offhand = None;
                    self.craft_grid[..4].fill(None);
                    self.player_equipment_changed(&before);
                    self.apply_equipment();
                } else {
                    self.carried = None;
                }
                self.inv_drag = None;
                self.inv_clicks = Default::default();
            } else if self.slot_at(&batch, cx, cy).is_some() || self.clicked_outside(&batch) {
                self.inventory_press(right);
            }
            return;
        }
        if !right && self.creative_search_click(ox, oy) {
            return;
        }
        // Holding the scrollbar scrolls as the mouse moves (mouseDragged).
        if !right
            && self
                .creative_screen
                .press_scrollbar(cx - ox, cy - oy, &self.pack.names)
        {
            return;
        }
        if self.slot_at(&batch, cx, cy).is_some() || self.clicked_outside(&batch) {
            self.inventory_press(right);
        }
    }
    /// A click on the search tab's box (`EditBox.onClick`).
    pub(super) fn creative_search_click(&mut self, ox: f32, oy: f32) -> bool {
        if !self.creative_screen.tab().search {
            return false;
        }
        let (x, y, w, h) = (ox + 82.0, oy + 6.0, 80.0, 9.0);
        let (mx, my) = self.cursor;
        if mx < x || my < y || mx >= x + w || my >= y + h {
            return false;
        }
        let shift = self.has_shift_down();
        let search = &mut self.creative_screen.search;
        search.x = x;
        search.y = y;
        search.width = w;
        search.click(&self.gui.atlas, mx, shift, false);
        true
    }
}
