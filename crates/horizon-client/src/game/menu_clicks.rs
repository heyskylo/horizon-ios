//! Clicks on an open container menu: `AbstractContainerMenu.clicked` run by the menu engine
//! (`menu_session`, `menu_click`, `menu_ops`), a shift-click's quick move, the creative screen's own
//! display and throw clicks, and a mouse button over the inventory (`click_inventory`).
use super::*;

impl Game<'_> {
    /// `quickMoveStack` of the open plain menu for a shift-click.
    pub(super) fn inventory_quick_move(&mut self, slot: usize) -> bool {
        self.menu_click(Some(slot), 0, horizon_client::menu::Input::QuickMove)
    }
    /// `AbstractContainerMenu.clicked` on the open inventory, crafting table,
    /// chest, hopper, dispenser or furnace menu, run by the verified menu
    /// engine (`horizon_client::menu`). `slot` None is outside the window.
    /// False when the open screen is not one the engine models.
    pub(super) fn menu_click(
        &mut self,
        slot: Option<usize>,
        button: i32,
        input: horizon_client::menu::Input,
    ) -> bool {
        if let Some(slot) = slot
            && slot >= Creative::DISPLAY
            && self.creative_tab_open()
        {
            self.creative_display_click(slot, button, input);
            return true;
        }
        if let Some(slot) = slot
            && input == horizon_client::menu::Input::Throw
            && self.carried.is_some()
            && self.remote.is_some()
            && ((slot < 9 && self.creative_tab_open()) || self.creative_inventory_open())
        {
            self.creative_throw_carrying(slot, button);
            return true;
        }
        self.menu_ops(&[(slot, button, input)])
    }
    /// `CreativeModeInventoryScreen.slotClicked`: THROW while a stack is carried, on an item
    /// tab's hotbar slot or any slot of the Survival Inventory tab. The menu's own THROW needs an
    /// empty hand, so the screen drops from the slot itself: one item, or with the control key
    /// the whole stack (on the inventory tab `Slot.remove(getMaxStackSize())`), through
    /// `handleCreativeModeItemDrop`, then sends the slot (`broadcastChanges`,
    /// `handleCreativeModeItemAdd`); the carried stack stays. Only the server drops it: this is
    /// the client of a server (`remote`).
    pub(super) fn creative_throw_carrying(&mut self, slot: usize, button: i32) {
        let Some(old) = self.slot_mut(slot).clone() else {
            return;
        };
        let count = if button == 0 {
            1
        } else if self.creative_inventory_open() {
            let max = world_items!(self)
                .get(&old.item)
                .map_or(64, |i| i.max_stack);
            old.count.min(max)
        } else {
            old.count
        };
        let before = self.player_equipment();
        let left = old.count - count;
        *self.slot_mut(slot) = (left > 0).then(|| ItemStack {
            count: left,
            ..old.clone()
        });
        self.player_equipment_changed(&before);
        self.apply_equipment();
        self.remote_creative_throw(ItemStack { count, ..old });
        self.send_creative_slots();
    }
    /// A click on an item tab's display slot: the screen's own handling
    /// (`creative::click_display`), never the menu's.
    pub(super) fn creative_display_click(
        &mut self,
        slot: usize,
        button: i32,
        input: horizon_client::menu::Input,
    ) {
        use horizon_client::creative::{DisplayClick, click_display};
        if self.creative_screen.tab().search {
            // CreativeModeInventoryScreen.slotClicked: moveCursorToEnd, then
            // setHighlightPos(0), so typing replaces the whole query.
            let search = &mut self.creative_screen.search;
            search.move_cursor_to_end(false);
            search.highlight = 0;
        }
        let first = self.creative_screen.first_row(&self.pack.names);
        let shown = self
            .creative_screen
            .shown(&self.pack.names, first * 9 + slot - Creative::DISPLAY);
        let catalog = world_items!(self);
        let max = |s: &ItemStack| catalog.get(&s.item).map_or(64, |i| i.max_stack);
        match click_display(&mut self.carried, shown.as_ref(), max, button, input) {
            DisplayClick::Hotbar(i, stack) if i < self.inventory.len() => {
                self.inventory[i] = Some(stack);
            }
            DisplayClick::Offhand(stack) => self.offhand = Some(stack),
            // handleCreativeModeItemDrop: on a server the stack goes to it (SetCreativeModeSlot
            // to slot -1), which drops it; a local drop here would never reach the server.
            DisplayClick::Drop(stack) if self.remote.is_some() => self.remote_creative_throw(stack),
            DisplayClick::Drop(stack) => self.player_drop(stack),
            _ => {}
        }
        // handleCreativeModeItemAdd / broadcastChanges: a swapped-in hotbar slot goes out now.
        if self.remote.is_some() {
            self.send_creative_slots();
        }
    }
    /// Several clicks on one menu instance (a quick craft drag is three).
    pub(super) fn menu_ops(
        &mut self,
        ops: &[(Option<usize>, i32, horizon_client::menu::Input)],
    ) -> bool {
        use horizon_client::menu::Input;
        // BundleMouseActions.onSlotClicked: a Shift click or a swap clears the bundle's selection.
        for &(slot, _, input) in ops {
            if let (Some(slot), Input::QuickMove | Input::Swap) = (slot, input) {
                self.unselect_bundle(slot);
            }
        }
        if self.remote.is_some() && self.screen == Screen::Inventory {
            // On a server the clicks are the server's: predicted and sent.
            return self.remote_menu_ops(ops);
        }
        if let [(Some(slot), button, input)] = ops {
            self.crafter_slot_clicked(*slot, *button, *input);
        }
        let ops = ops.to_vec();
        let done = self.menu_session(move |menu, env, to_menu| {
            let mut clicks = Vec::new();
            for &(slot, button, input) in &ops {
                let menu_slot = match slot {
                    Some(g) => {
                        if g == 200 && input != Input::QuickMove {
                            return None;
                        }
                        match to_menu(g) {
                            Some(i) => i as i32,
                            None => return None,
                        }
                    }
                    None => -999,
                };
                clicks.push((menu_slot, button, input));
            }
            for (menu_slot, button, input) in clicks {
                if menu.click(env, menu_slot, button, input).is_err() {
                    // Vanilla's server throws: nothing of this click is kept.
                    return Some(false);
                }
            }
            Some(true)
        });
        // CreativeModeInventoryScreen.slotClicked: the changed slots go out with the click.
        if done && self.remote.is_some() && self.screen == Screen::Creative {
            self.send_creative_slots();
        }
        done
    }
    /// The open menu as the verified engine (`horizon_client::menu`): the
    /// game's slots are copied in, `run` acts on it, and the slots, carried
    /// stack, menu data and drops are copied back when it returns Some(true)
    /// (Some(false): handled, nothing kept; None: not handled). False when the
    /// open screen is not one the engine models or `run` did not handle it.
    pub(super) fn menu_session(
        &mut self,
        run: impl FnOnce(
            &mut horizon_client::menu::Menu,
            &horizon_client::menu::Env,
            &dyn Fn(usize) -> Option<usize>,
        ) -> Option<bool>,
    ) -> bool {
        use horizon_client::menu::{Env, Kind, Menu};
        #[derive(Clone, Copy)]
        struct Map {
            container: usize,
            table: bool,
            cells: usize,
            plain: bool,
        }
        impl Map {
            fn main(&self) -> usize {
                match (self.plain, self.table) {
                    (false, _) => self.container,
                    (true, true) => 10,
                    (true, false) => 9,
                }
            }
            fn to_menu(self, g: usize) -> Option<usize> {
                let main = self.main();
                if self.plain {
                    match g {
                        200 => Some(0),
                        100..109 if g - 100 < self.cells => Some(1 + g - 100),
                        500..504 if !self.table => Some(5 + g - 500),
                        504 if !self.table => Some(45),
                        9..36 => Some(main + g - 9),
                        0..9 => Some(main + 27 + g),
                        _ => None,
                    }
                } else {
                    match g {
                        300.. if g < 300 + self.container => Some(g - 300),
                        9..36 => Some(main + g - 9),
                        0..9 => Some(main + 27 + g),
                        _ => None,
                    }
                }
            }
            fn to_game(self, i: usize) -> Option<usize> {
                let main = self.main();
                if self.plain {
                    match i {
                        0 => None,
                        1..=9 if i - 1 < self.cells => Some(100 + i - 1),
                        5..9 if !self.table => Some(500 + i - 5),
                        45 if !self.table => Some(504),
                        _ if i >= main && i < main + 27 => Some(9 + i - main),
                        _ if i >= main + 27 && i < main + 36 => Some(i - main - 27),
                        _ => None,
                    }
                } else if i < self.container {
                    Some(300 + i)
                } else if i < self.container + 27 {
                    Some(9 + i - self.container)
                } else {
                    Some(i - self.container - 27)
                }
            }
        }
        // Menu slot -> game slot, for every menu slot.
        let mut plain_map = None;
        let crafter_open = matches!(self.container_kind(), Some(BlockEntity::Crafter(_)));
        let loom_open = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == stations::Kind::Loom);
        let merchant_open = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == stations::Kind::Merchant);
        let stonecutter_open = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == stations::Kind::Stonecutter);
        let station_kind = self.station.as_ref().map(|s| s.kind);
        let grindstone_open = station_kind == Some(stations::Kind::Grindstone);
        let cartography_open = station_kind == Some(stations::Kind::Cartography);
        let anvil_open = station_kind == Some(stations::Kind::Anvil);
        let smithing_open = station_kind == Some(stations::Kind::Smithing);
        let enchanting_open = station_kind == Some(stations::Kind::Enchanting);
        let beacon_open = station_kind == Some(stations::Kind::Beacon);
        let (kind, game_of): (Kind, Vec<Option<usize>>) = if self.creative_tab_open() {
            // ItemPickerMenu: the display slots (clicked by the screen), then the hotbar.
            let mut v = vec![None; 45];
            v.extend((0..9).map(Some));
            (Kind::ItemPicker, v)
        } else if let Some(columns) = self
            .mount_menu_columns()
            .filter(|_| self.remote.is_some() && self.mount_active())
        {
            // A server's HorseInventoryMenu: the saddle, the body armour,
            // the chest, then the inventory.
            let mut v = vec![Some(mount_menu::SADDLE), Some(mount_menu::ARMOR)];
            v.extend((0..columns * 3).map(|i| Some(mount_menu::CHEST + i)));
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Mount(columns), v)
        } else if merchant_open {
            // MerchantMenu: two payment slots, the result, then the inventory.
            let mut v: Vec<Option<usize>> = vec![
                Some(stations::INPUT),
                Some(stations::INPUT + 1),
                Some(stations::RESULT),
            ];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Merchant, v)
        } else if crafter_open {
            // CrafterMenu: the grid, the inventory, then the result.
            let mut v: Vec<Option<usize>> = (300..309).map(Some).collect();
            v.extend((9..36).chain(0..9).map(Some));
            v.push(None);
            (Kind::CrafterBlock, v)
        } else if loom_open {
            // LoomMenu: banner, dye, pattern, result, then the inventory.
            let mut v: Vec<Option<usize>> = (0..3).map(|i| Some(stations::INPUT + i)).collect();
            v.push(Some(stations::RESULT));
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Loom, v)
        } else if stonecutter_open {
            // StonecutterMenu: the input, the result, then the inventory.
            let mut v = vec![Some(stations::INPUT), Some(stations::RESULT)];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Stonecutter, v)
        } else if grindstone_open || cartography_open {
            // GrindstoneMenu and CartographyTableMenu: the two inputs, the
            // result, then the inventory.
            let mut v = vec![
                Some(stations::INPUT),
                Some(stations::INPUT + 1),
                Some(stations::RESULT),
            ];
            v.extend((9..36).chain(0..9).map(Some));
            let kind = if grindstone_open {
                Kind::Grindstone
            } else {
                Kind::Cartography
            };
            (kind, v)
        } else if enchanting_open {
            // EnchantmentMenu: the item, the lapis, then the inventory.
            let mut v = vec![Some(stations::INPUT), Some(stations::INPUT + 1)];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Enchanting, v)
        } else if beacon_open {
            // BeaconMenu: the payment, then the inventory.
            let mut v = vec![Some(stations::INPUT)];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Beacon, v)
        } else if anvil_open || smithing_open {
            // ItemCombinerMenu: the inputs, the result, then the inventory.
            let inputs = if anvil_open { 2 } else { 3 };
            let mut v: Vec<Option<usize>> =
                (0..inputs).map(|i| Some(stations::INPUT + i)).collect();
            v.push(Some(stations::RESULT));
            v.extend((9..36).chain(0..9).map(Some));
            (
                if anvil_open {
                    Kind::Anvil
                } else {
                    Kind::Smithing
                },
                v,
            )
        } else {
            let (kind, map) = if self.container.is_some() {
                let size = self.container_size();
                let (kind, count) = match self.container_kind() {
                    Some(BlockEntity::Chest(_)) => {
                        let shulker = self.container.is_some_and(|k| {
                            let id = self.world.block(k.0, k.1, k.2);
                            storage::is_shulker_box(&self.res.facts().get(id).name)
                        });
                        let kind = match size {
                            5 => Kind::Hopper,
                            9 => Kind::Dispenser,
                            27 if shulker => Kind::Shulker,
                            n => Kind::Chest((n / 9).clamp(1, 6)),
                        };
                        (kind, size)
                    }
                    Some(BlockEntity::Brewing(_)) => (Kind::Brewing, 5),
                    // A server's content mod menu type: the server runs the
                    // same engine over it (its crafting grid, its linked
                    // slots), so its clicks are predicted as vanilla's.
                    Some(BlockEntity::Mod(_)) => {
                        match self
                            .remote_menu_type()
                            .and_then(horizon_client::containers::mod_menu_type)
                        {
                            Some(t) => (Kind::Mod(t), t.slots),
                            None => return false,
                        }
                    }
                    Some(BlockEntity::Furnace(f)) => (
                        Kind::Furnace(
                            match f.kind.recipe_type().trim_start_matches("minecraft:") {
                                "blasting" => "blasting",
                                "smoking" => "smoking",
                                _ => "smelting",
                            },
                        ),
                        3,
                    ),
                    _ => return false,
                };
                let map = Map {
                    container: count,
                    table: false,
                    cells: 0,
                    plain: false,
                };
                (kind, map)
            } else if self.station.is_none() && !self.mount_active() {
                let table = self.craft_size == 3;
                let map = Map {
                    container: 0,
                    table,
                    cells: self.craft_size * self.craft_size,
                    plain: true,
                };
                (
                    if table {
                        Kind::Crafting
                    } else {
                        Kind::Inventory
                    },
                    map,
                )
            } else {
                return false;
            };
            let total = Menu::new(kind).slot_count();
            plain_map = Some(map);
            (kind, (0..total).map(|i| map.to_game(i)).collect())
        };
        // The plain map also takes the crafting result slot (a shift-click on it).
        let to_menu = |g: usize| match plain_map {
            Some(map) => map.to_menu(g),
            None => game_of.iter().position(|x| *x == Some(g)),
        };
        let mut menu = Menu::new(kind);
        if matches!(kind, Kind::Mount(_)) {
            menu.mount = self.mount_menu_entity();
        }
        let total = menu.slot_count();
        let before: Vec<Option<ItemStack>> = game_of
            .iter()
            .map(|g| g.and_then(|g| self.slot_mut(g).clone()))
            .collect();
        let carried = self.carried.clone();
        let creative = self.creative;
        // CreativeModeInventoryScreen clicks the menu itself and sends every
        // changed slot, which the server sets by player.
        let equipment_before = (self.screen == Screen::Creative).then(|| self.player_equipment());
        let station_menu = self.station.as_ref().map(|s| {
            (
                s.loom.selected.map_or(-1, |i| i as i32),
                s.loom.patterns.clone(),
            )
        });
        let stone = self
            .station
            .as_ref()
            .map(|s| (s.selected.map_or(-1, |i| i as i32), s.stone.clone()));
        let crafter_data = match self.container_kind() {
            Some(BlockEntity::Crafter(c)) => Some((c.disabled, c.triggered)),
            _ => None,
        };
        // The merchant's session, lent to the menu for this click.
        let mut session = self
            .station
            .as_ref()
            .and_then(|s| s.merchant.as_ref())
            .map(|m| (m.session.clone(), m.mob));
        if let Some((s, mob)) = session.as_mut() {
            self.merchant_sync_in(s, *mob);
        }
        // Crafting and cartography read and derive the level's maps; a
        // server's (the client menu's ContainerLevelAccess.NULL) doesn't.
        let level_maps = (matches!(kind, Kind::Cartography | Kind::Crafting | Kind::Inventory)
            && self.remote.is_none())
        .then(|| {
            before
                .iter()
                .flatten()
                .chain(self.inventory.iter().flatten())
                .chain(self.carried.iter())
                .cloned()
                .collect::<Vec<_>>()
        })
        .filter(|held| {
            kind == Kind::Cartography
                || held
                    .iter()
                    .filter_map(maps::map_id)
                    .any(|id| self.map(id).is_some())
        })
        .map(|held| self.level_maps(&held));
        let next_map = if merchant_open { self.peek_map_id() } else { 0 };
        let tag = |t: &str| self.structure_tag(t);
        let maps = trading::GameMaps {
            terrain: self.end.locator(),
            structure_tag: &tag,
            made: Default::default(),
            next_id: std::cell::Cell::new(next_map),
        };
        let trade_env = horizon_client::merchant::TradeEnv {
            trades: &self.stations().trades,
            enchantments: &self.stations().enchantments,
            maps: Some(&maps),
        };
        let env = Env {
            items: self.items(),
            recipes: self.recipes(),
            brewing: &self.stations().brewing,
            banners: &self.stations().banners,
            station_recipes: &self.stations().recipes,
            enchantments: &self.stations().enchantments,
            names: &self.pack.names,
            trade: Some(&trade_env),
            maps: level_maps
                .as_ref()
                .map(|m| m as &dyn horizon_client::menu::Maps),
            limited_crafting: self.limited_crafting(),
            known_recipes: Some(&self.level.recipe_book),
        };
        menu.creative = creative;
        menu.selected = self.selected;
        menu.levels = self.experience.level;
        if anvil_open {
            // The rename box as AnvilScreen.onNameChanged gives it to the menu.
            menu.anvil.name = self.station.as_ref().map(|s| self.anvil_item_name(s));
        }
        for (i, stack) in before.iter().enumerate() {
            menu.set_slot(&env, i, stack.clone());
        }
        // The off hand is part of the inventory even when the menu has no slot for it.
        let offhand_mapped = game_of.contains(&Some(504));
        if !offhand_mapped {
            menu.set_player_item(40, self.offhand.clone());
        }
        menu.settle(&env);
        if kind == Kind::Loom
            && let Some((selected, patterns)) = station_menu
        {
            // The menu's state between clicks: what set_slot recomputed is replaced.
            menu.data[0] = selected;
            menu.loom_patterns = patterns;
            menu.set_slot(&env, 3, before[3].clone());
        }
        if kind == Kind::Stonecutter
            && let Some((selected, state)) = stone
        {
            // The same: the list and selection between clicks, then the result.
            menu.data[0] = selected;
            menu.stone = state;
            menu.set_slot(&env, 1, before[1].clone());
        }
        if anvil_open && let Some(s) = self.station.as_ref() {
            // The same: the cost, material cost and rename flag between clicks, as
            // the station's createResult and the mods' anvil_update left them (the
            // result came with the slots), so a take is predicted as the server makes it.
            menu.data[0] = s.anvil.cost;
            menu.anvil.repair_count = s.anvil.repair_item_count_cost;
            menu.anvil.only_renaming = s.anvil.only_renaming;
        }
        if kind == Kind::Enchanting
            && let Some(s) = self.station.as_ref()
        {
            // The offers between clicks (what copying the slots in flagged is not a change).
            menu.enchant_changed = false;
            for i in 0..3 {
                menu.data[i] = s.offers.costs[i];
                menu.data[7 + i] = s.offers.clues[i].as_ref().map_or(-1, |(_, l)| *l as i32);
            }
            menu.data[3] = self.level.xp_seed;
        }
        if let Some((disabled, triggered)) = crafter_data {
            for (i, d) in disabled.iter().enumerate() {
                menu.data[i] = i32::from(*d);
            }
            menu.data[9] = i32::from(triggered);
        }
        menu.carried = carried;
        menu.drops.clear();
        if kind == Kind::Merchant {
            // Attached after the slots were set: setting them is not a click.
            menu.merchant = session.map(|(s, _)| s);
        }
        // `FurnaceResultSlot.checkTakeAchievements` pays out on what left the output slot.
        let furnace_before = if matches!(kind, Kind::Furnace(_)) {
            menu.slot(2).cloned().map_or(0, |s| s.count)
        } else {
            0
        };
        let outcome = run(&mut menu, &env, &to_menu);
        let made = maps.made.take();
        match outcome {
            None => return false,
            Some(false) => return true,
            Some(true) => {}
        }
        // On a server the clicks only predict: the server drops, crafts and
        // makes maps itself.
        let predicting = self.remote.is_some();
        if !predicting {
            self.register_exploration_maps(made);
            for (item, count) in std::mem::take(&mut menu.crafted_counts) {
                self.award_stat("crafted", &item, count);
            }
        }
        if let Some(s) = menu.merchant.take() {
            self.merchant_sync_out(s);
        }
        let after: Vec<Option<ItemStack>> = (0..total).map(|i| menu.slot(i).cloned()).collect();
        // Only a furnace has a result slot to compare: an enchanting table has two slots.
        let furnace_after = if matches!(kind, Kind::Furnace(_)) {
            after[2].as_ref().map_or(0, |s| s.count)
        } else {
            0
        };
        let carried = menu.carried.take();
        let dropped = std::mem::take(&mut menu.dropped);
        let sounds = std::mem::take(&mut menu.sounds);
        // A bundle's sounds are the player's own (`Entity.playSound`): the
        // client plays them as it clicks, predicting or not.
        for sound in std::mem::take(&mut menu.player_sounds) {
            self.player_menu_sound(sound);
        }
        let crafted = std::mem::take(&mut menu.crafted);
        let brewed = std::mem::take(&mut menu.brewed);
        let equipped = std::mem::take(&mut menu.equipped);
        let item_crafted = std::mem::take(&mut menu.item_crafted);
        for (_, stack) in &item_crafted {
            self.tutorial_get_item(Some(stack));
        }
        for (i, stack) in after.into_iter().enumerate() {
            if let Some(g) = game_of[i] {
                *self.slot_mut(g) = stack;
            }
        }
        self.carried = carried;
        if !offhand_mapped {
            self.offhand = menu.player_item(40).cloned();
        }
        if !predicting {
            for stack in dropped {
                self.player_drop(stack);
            }
            self.award_crafted(&crafted);
            for stack in &brewed {
                self.trigger_brewed_potion(stack);
            }
            if furnace_before > furnace_after
                && self.remote.is_none()
                && let (Some(pos), Some(p)) = (self.container, self.player.as_ref())
            {
                let at = [p.position.x, p.position.y, p.position.z];
                self.award_furnace_recipes(pos, at);
            }
        } else if self.screen == Screen::Creative {
            // A THROW in the creative screen with an empty hand: on an item tab's hotbar
            // `AbstractContainerMenu.doClick` calls `Player.handleCreativeModeItemDrop`, on the
            // Survival Inventory tab the screen does. The stack goes to the server, the slot
            // follows with the others (SetCreativeModeSlot). Only the server can drop it; the
            // slot has already given it up.
            for stack in dropped {
                self.remote_creative_throw(stack);
            }
        }
        if kind == Kind::Loom {
            if let Some(s) = self.station.as_mut() {
                s.loom.selected = usize::try_from(menu.data[0]).ok();
                s.loom.patterns = std::mem::take(&mut menu.loom_patterns);
            }
            // A server's loom plays its own sounds (the client's menu has no level).
            for sound in sounds.into_iter().filter(|_| !predicting) {
                self.station_sound(sound);
            }
            self.loom_screen_changed();
        } else if kind == Kind::Cartography {
            for sound in sounds.into_iter().filter(|_| !predicting) {
                self.station_sound(sound);
            }
        } else if kind == Kind::Stonecutter {
            if let Some(s) = self.station.as_mut() {
                s.selected = usize::try_from(menu.data[0]).ok();
                s.stone = std::mem::take(&mut menu.stone);
                s.stonecutter_changed();
            }
            for sound in sounds.into_iter().filter(|_| !predicting) {
                self.station_sound(sound);
            }
        }
        if let Some(amount) = menu.ground.take().filter(|_| !predicting)
            && let Some(pos) = self.station.as_ref().map(|s| s.pos)
        {
            self.grindstone_taken(pos, amount);
        }
        if kind == Kind::Anvil || kind == Kind::Smithing {
            self.combiner_session_end(&menu, predicting);
        }
        if kind == Kind::Enchanting && (menu.enchant_changed || before[0] != menu.slot(0).cloned())
        {
            // EnchantmentMenu.slotsChanged: new offers for the item.
            self.station_changed();
        }
        if kind == Kind::CrafterBlock
            && let Some(BlockEntity::Crafter(c)) = self
                .container
                .and_then(|k| self.world.block_entities.get_mut(&k))
        {
            for (i, d) in c.disabled.iter_mut().enumerate() {
                *d = menu.data[i] == 1;
            }
        }
        if matches!(kind, Kind::Furnace(_))
            && let Some(BlockEntity::Furnace(f)) = self
                .container
                .and_then(|k| self.world.block_entities.get_mut(&k))
        {
            // AbstractFurnaceBlockEntity.setItem: only a new input restarts cooking.
            f.input_clicked(world_recipes!(self), before[0].clone());
        }
        match equipment_before {
            Some(before) => self.player_equipment_changed(&before),
            None => {
                for (slot, stack) in equipped {
                    self.player_equipped(slot, stack.as_ref());
                }
            }
        }
        self.apply_equipment();
        if let Some(maps) = level_maps {
            self.register_made_maps(maps);
        }
        true
    }
    pub(super) fn click_inventory(&mut self, right: bool) {
        if self.mod_browser_press(right) || self.mount_click(right) {
            return;
        }
        if self.recipe_book_click(right) {
            return;
        }
        if self.station_click(right) {
            return;
        }
        if self.remote.is_some() {
            return self.remote_click(right);
        }
        let Some(size) = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
        else {
            return;
        };
        let batch = HudBatch::new(size);
        let Some(slot) = self.slot_at(&batch, self.cursor.0, self.cursor.1) else {
            return;
        };
        let held_shift = self.has_shift_down();
        if self.crafter_click(slot, held_shift, right) {
            return;
        }
        if held_shift && self.container.is_some() && self.container_quick_move(slot) {
            if let Some(key) = self.container {
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
            return;
        }
        if let Some(c) = &self.carried
            && self.refuses(slot, c)
        {
            return;
        }
        let max = |items: &Items, s: &ItemStack| items.get(&s.item).map_or(64, |i| i.max_stack);
        let shift = held_shift;
        if slot >= 300
            && let Some(key) = self.container
        {
            self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        }
        let is_furnace = matches!(self.container_kind(), Some(BlockEntity::Furnace(_)));
        if is_furnace && slot == 302 {
            // FurnaceResultSlot: take only.
            let Some(result) = self.slot_mut(302).take() else {
                return;
            };
            match &mut self.carried {
                None => self.carried = Some(result),
                Some(c)
                    if c.item == result.item
                        && c.count + result.count <= max(world_items!(self), c) =>
                {
                    c.count += result.count
                }
                _ => *self.slot_mut(302) = Some(result),
            }
            return;
        }
        // Changing the input restarts cooking (AbstractFurnaceBlockEntity.setItem).
        let furnace_input = (is_furnace && slot == 300).then(|| self.slot_mut(300).clone());
        if slot == 200 {
            // ResultSlot: take the crafted stack and consume one of each ingredient.
            let mut crafted = 0;
            while let Some((recipe, result)) = self.craft_recipe() {
                let made = (result.item.clone(), result.count);
                let taken = result.clone();
                let cells = self.craft_size * self.craft_size;
                let ingredients = self.craft_grid[..cells].to_vec();
                let remainders = world_recipes!(self).remainders(
                    &recipe,
                    &self.craft_grid[..cells],
                    self.craft_size,
                );
                if shift {
                    if self.give(result).is_some() {
                        break;
                    }
                } else {
                    match &mut self.carried {
                        None => self.carried = Some(result),
                        Some(c)
                            if c.item == result.item
                                && c.count + result.count <= max(world_items!(self), c) =>
                        {
                            c.count += result.count
                        }
                        _ => return,
                    }
                }
                for cell in self.craft_grid.iter_mut() {
                    if let Some(s) = cell {
                        s.count -= 1;
                        if s.count == 0 {
                            *cell = None;
                        }
                    }
                }
                for (i, rest) in remainders.into_iter().enumerate() {
                    let Some(rest) = rest else { continue };
                    match &mut self.craft_grid[i] {
                        cell @ None => *cell = Some(rest),
                        Some(c) if c.same_item_same_components(&rest) => c.count += rest.count,
                        _ => self.give_or_drop(rest),
                    }
                }
                crafted += 1;
                // `ResultSlot.onTake`: the recipe and the stack taken
                // (native_item_crafted).
                let player = self.native_acting_id();
                self.native_item_crafted(player, &format!("minecraft:{recipe}"), &taken);
                self.tutorial_get_item(Some(&taken));
                self.award_stat("crafted", &made.0, made.1 as i32);
                self.award_crafted(&[(format!("minecraft:{recipe}"), ingredients)]);
                if !shift || crafted >= 64 {
                    break;
                }
            }
            return;
        }
        if shift
            && (slot < 36 || (500..504).contains(&slot) || (100..109).contains(&slot))
            && self.inventory_quick_move(slot)
        {
            return;
        }
        if (500..504).contains(&slot) {
            let wanted = ["head", "chest", "legs", "feet"][slot - 500];
            let fits = self.carried.as_ref().is_none_or(|c| {
                self.res
                    .items()
                    .get(&c.item)
                    .and_then(|i| i.equip_slot.as_deref())
                    == Some(wanted)
            });
            if !fits {
                return;
            }
            std::mem::swap(&mut self.carried, &mut self.armor[slot - 500]);
            self.apply_equipment();
            return;
        }
        let carried = self.carried.take();
        let current = self.slot_mut(slot).take();
        let items = self.items();
        let (new_slot, new_carried) = match (carried, current) {
            (None, Some(mut s)) if right => {
                let half = s.count.div_ceil(2);
                s.count -= half;
                let taken = ItemStack {
                    count: half,
                    ..s.clone()
                };
                ((s.count > 0).then_some(s), Some(taken))
            }
            (None, s) => (None, s),
            (Some(mut c), None) if right => {
                let one = ItemStack {
                    count: 1,
                    ..c.clone()
                };
                c.count -= 1;
                (Some(one), (c.count > 0).then_some(c))
            }
            (Some(c), None) => (Some(c), None),
            (Some(mut c), Some(mut s)) if c.same_item_same_components(&s) => {
                let room = max(items, &s).saturating_sub(s.count);
                let moved = if right {
                    1.min(room).min(c.count)
                } else {
                    room.min(c.count)
                };
                s.count += moved;
                c.count -= moved;
                (Some(s), (c.count > 0).then_some(c))
            }
            (Some(c), Some(s)) => (Some(c), Some(s)),
        };
        self.carried = new_carried;
        *self.slot_mut(slot) = new_slot;
        if let Some(before) = furnace_input
            && let Some(BlockEntity::Furnace(f)) = self
                .container
                .and_then(|k| self.world.block_entities.get_mut(&k))
        {
            f.input_clicked(world_recipes!(self), before);
        }
    }
}
