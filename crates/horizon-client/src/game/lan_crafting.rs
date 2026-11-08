//! Crafting for LAN guests: the inventory's 2x2 grid (window 0) and the
//! crafting table (`minecraft:crafting`) run the host's menu engine
//! (`horizon_client::menu`, `Kind::Inventory` / `Kind::Crafting`) over the
//! guest's inventory, with vanilla's packets: OpenScreen, container clicks
//! and their corrections, and the grid back into the inventory when the menu
//! closes (`AbstractContainerMenu.clearContainer`).
use super::{Game, lan::LanHost};
use horizon_client::{
    containers::BlockEntity,
    furnace::FurnaceCooking,
    items::ItemStack,
    lan::{guest::Guest, menu::Slots},
    menu::{
        Env, Input, Kind, Menu,
        place::{PostPlace, TransferError},
    },
};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// `AbstractContainerMenu.dropOrPlaceInInventory`: actual network disconnects
/// drop inputs; a live close still returns them to the inventory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuRemoval {
    Live,
    Disconnected,
}

/// The guest's menus the game runs: kept between clicks, so a drag (quick
/// craft) spans them as in vanilla.
#[derive(Default)]
pub(super) struct GuestCrafting {
    /// The inventory menu; its grid lives in the hub's guest.
    inventory: Option<Box<Menu>>,
    /// The open crafting table: its window, state id, menu and block.
    pub(super) table: Option<(i32, i32, Box<Menu>, [i32; 3])>,
    /// The open furnace's or brewing stand's window and menu (`AbstractFurnaceMenu`,
    /// `BrewingStandMenu`); its slots live in the block entity and the hub's
    /// container menu keeps the client in sync.
    pub(super) machine: Option<(i32, Box<Menu>)>,
}

/// ServerboundContainerClickPacket's input.
pub(super) fn input(id: i32) -> Input {
    match id {
        0 => Input::Pickup,
        1 => Input::QuickMove,
        2 => Input::Swap,
        3 => Input::Clone,
        4 => Input::Throw,
        5 => Input::QuickCraft,
        _ => Input::PickupAll,
    }
}

/// The armor's player-container slots, head to feet (`Inventory` 39..36).
const ARMOR: [usize; 4] = [39, 38, 37, 36];
const OFFHAND: usize = 40;

/// The guest's inventory, armor, off hand, selection and carried stack into `menu`.
fn load_player(menu: &mut Menu, g: &Guest) {
    for i in 0..36 {
        menu.set_player_item(i, g.inventory[i].clone());
    }
    for (k, slot) in ARMOR.into_iter().enumerate() {
        menu.set_player_item(slot, g.armor[k].clone());
    }
    menu.set_player_item(OFFHAND, g.offhand.clone());
    menu.selected = g.selected;
    menu.creative = g.creative();
    menu.levels = g.xp_level;
    menu.carried = g.carried.clone();
}

/// `load_player` back.
fn store_player(menu: &mut Menu, g: &mut Guest) {
    for i in 0..36 {
        g.inventory[i] = menu.player_item(i).cloned();
    }
    for (k, slot) in ARMOR.into_iter().enumerate() {
        g.armor[k] = menu.player_item(slot).cloned();
    }
    g.offhand = menu.player_item(OFFHAND).cloned();
    g.carried = menu.carried.take();
}

/// Inventory.placeItemBackInInventory for each stack: what doesn't fit is
/// left for Player.drop (creative players keep it too).
fn place_back(menu: &mut Menu, env: &Env, stacks: Vec<ItemStack>) {
    menu.creative = false;
    for mut stack in stacks {
        menu.give(env, &mut stack);
        if stack.count > 0 {
            menu.dropped.push(stack);
        }
    }
}

impl Game<'_> {
    /// `LocalPlayer.doLimitedCrafting`: whether limited crafting is active.
    pub fn limited_crafting(&self) -> bool {
        self.remote.as_ref().map_or_else(
            || self.ui.rules.bool("limited_crafting"),
            |r| r.do_limited_crafting,
        )
    }

    pub(super) fn menu_env(&self) -> Env<'_> {
        Env {
            items: self.items(),
            recipes: self.recipes(),
            brewing: &self.stations().brewing,
            banners: &self.stations().banners,
            station_recipes: &self.stations().recipes,
            enchantments: &self.stations().enchantments,
            names: &self.pack.names,
            trade: None,
            maps: None,
            limited_crafting: self.limited_crafting(),
            known_recipes: Some(&self.level.recipe_book),
        }
    }

    /// Runs `op` on the guest's menu with their inventory copied in and back.
    pub(super) fn guest_menu_op<R>(
        &mut self,
        host: &mut LanHost,
        gi: usize,
        menu: &mut Menu,
        op: impl FnOnce(&mut Menu, &Env) -> R,
    ) -> R {
        // Crafting and cartography read and derive this level's saved maps.
        let maps = matches!(
            menu.kind(),
            Kind::Cartography | Kind::Crafting | Kind::Inventory
        )
        .then(|| {
            let g = &host.hub.guests[gi];
            (0..menu.slot_count())
                .filter_map(|s| menu.slot(s).cloned())
                .chain(g.inventory.iter().flatten().cloned())
                .chain(g.carried.clone())
                .collect::<Vec<_>>()
        })
        .filter(|held| {
            menu.kind() == Kind::Cartography
                || held
                    .iter()
                    .filter_map(super::maps::map_id)
                    .any(|id| self.map(id).is_some())
        })
        .map(|held| self.level_maps(&held));
        let env = Env {
            maps: maps.as_ref().map(|m| m as &dyn horizon_client::menu::Maps),
            limited_crafting: host.hub.limited_crafting,
            known_recipes: Some(&host.hub.guests[gi].recipe_book),
            ..self.menu_env()
        };
        load_player(menu, &host.hub.guests[gi]);
        let result = op(menu, &env);
        if let Some(maps) = maps {
            self.register_made_maps(maps);
        }
        // A taken anvil result waits for AnvilCraftEvent.Pre: what the click
        // dropped waits with it (lan_station_op throws it or refuses the take).
        let dropped = if menu.anvil_craft.is_some() {
            Vec::new()
        } else {
            std::mem::take(&mut menu.dropped)
        };
        let equipped = std::mem::take(&mut menu.equipped);
        let brewed = std::mem::take(&mut menu.brewed);
        let crafted = std::mem::take(&mut menu.crafted);
        let item_crafted = std::mem::take(&mut menu.item_crafted);
        menu.drops.clear();
        store_player(menu, &mut host.hub.guests[gi]);
        if !brewed.is_empty() {
            let conn = host.hub.guests[gi].conn;
            self.as_guest(host, conn, |g| {
                for stack in &brewed {
                    g.trigger_brewed_potion(stack);
                }
            });
        }
        if !crafted.is_empty() {
            let conn = host.hub.guests[gi].conn;
            self.as_guest(host, conn, |g| {
                for (recipe, ingredients) in &crafted {
                    g.trigger_recipe_crafted(recipe, ingredients);
                }
            });
        }
        if !item_crafted.is_empty() {
            let conn = host.hub.guests[gi].conn;
            self.as_guest(host, conn, |g| {
                let player = g.native_acting_id();
                for (recipe, stack) in &item_crafted {
                    g.native_item_crafted(player, recipe, stack);
                }
            });
        }
        if !equipped.is_empty() {
            let conn = host.hub.guests[gi].conn;
            self.as_guest(host, conn, |g| {
                for (slot, stack) in equipped {
                    g.player_equipped(slot, stack.as_ref());
                }
            });
        }
        self.drop_guest_items(host, gi, dropped);
        result
    }

    /// A click on the guest's inventory menu (window 0): `InventoryMenu`,
    /// crafting grid included. False for other windows.
    pub(super) fn lan_inventory_click(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        if p.container != 0 {
            return false;
        }
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return true;
        };
        if host.hub.guests[gi].spectator() {
            return true;
        }
        let crafting = host.play.entry(conn).or_default();
        let mut menu = crafting
            .crafting
            .inventory
            .take()
            .unwrap_or_else(|| Box::new(Menu::new(Kind::Inventory)));
        let grid = host.hub.guests[gi].craft_grid.clone();
        let changed = self.guest_menu_op(host, gi, &mut menu, |menu, env| {
            for (k, s) in grid.into_iter().enumerate() {
                menu.set_slot(env, 1 + k, s);
            }
            let before: Vec<_> = (0..menu.slot_count())
                .map(|s| menu.slot(s).cloned())
                .collect();
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            (0..menu.slot_count())
                .filter(|&s| menu.slot(s) != before[s].as_ref())
                .collect::<Vec<_>>()
        });
        let guest = &host.hub.guests[gi];
        let (uuid, name) = (guest.uuid, guest.name.clone());
        let mut result = menu.slot(0).cloned();
        self.native_prepare_item_craft(uuid, &name, None, &mut result);
        let env = self.menu_env();
        menu.set_slot(&env, 0, result);
        let g = &mut host.hub.guests[gi];
        for k in 0..4 {
            g.craft_grid[k] = menu.slot(1 + k).cloned();
        }
        g.craft_result = menu.slot(0).cloned();
        host.play.entry(conn).or_default().crafting.inventory = Some(menu);
        host.hub.inventory_clicked(conn, p, &self.world, &changed);
        true
    }

    /// A click on the guest's open furnace, blast furnace, smoker or brewing stand:
    /// the host's `AbstractFurnaceMenu` (fuel slot, take-only result, quick moves by
    /// recipe and fuel), `BrewingStandMenu` (one bottle per slot, ingredient and
    /// blaze powder slots) or a content mod's menu type (`Kind::Mod`) over the
    /// block entity's slots. False for other windows.
    pub(super) fn lan_machine_click(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let Some((pos, mod_type, hidden)) = host.hub.guests[gi]
            .menu
            .as_ref()
            .filter(|m| p.container != 0 && m.window == p.container)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some((pos, m.mod_type, m.hidden.clone())),
                _ => None,
            })
        else {
            return false;
        };
        let (kind, items) = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                let recipe = match f.kind.recipe_type() {
                    "minecraft:blasting" => "blasting",
                    "minecraft:smoking" => "smoking",
                    _ => "smelting",
                };
                (Kind::Furnace(recipe), f.items.to_vec())
            }
            Some(BlockEntity::Brewing(b)) => (Kind::Brewing, b.items.to_vec()),
            Some(BlockEntity::Crafter(c)) => (Kind::CrafterBlock, c.items.to_vec()),
            // A content mod's menu type, with its slots' rules.
            Some(BlockEntity::Mod(m)) => match mod_type {
                Some(t) => {
                    let mut items = m.items.clone();
                    items.resize(t.slots, None);
                    (Kind::Mod(t), items)
                }
                None => return false,
            },
            _ => return false,
        };
        let furnace = matches!(kind, Kind::Furnace(_));
        // CrafterBlockEntity.containerData: the disabled slots, then triggered.
        let crafter_data = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Crafter(c)) => Some((c.disabled, c.triggered)),
            _ => None,
        };
        if host.hub.guests[gi].spectator() {
            // handleContainerClick: nothing changes, and sendAllDataToRemote
            // takes back what the client predicted.
            host.hub.resend_menu(conn, p.container, &self.world);
            return true;
        }
        let mut menu = host
            .play
            .entry(conn)
            .or_default()
            .crafting
            .machine
            .take()
            .filter(|(window, m)| *window == p.container && m.kind() == kind)
            .map_or_else(|| Box::new(Menu::new(kind)), |(_, m)| m);
        for &slot in &hidden {
            menu.hide_slot(slot);
        }
        let changed = self.guest_menu_op(host, gi, &mut menu, |menu, env| {
            for (k, s) in items.iter().enumerate() {
                menu.set_slot(env, k, s.clone());
            }
            // After the slots: setting a disabled slot (CrafterBlockEntity.setItem) enables it.
            if let Some((disabled, triggered)) = crafter_data {
                for (k, d) in disabled.iter().enumerate() {
                    menu.data[k] = i32::from(*d);
                }
                menu.data[9] = i32::from(triggered);
            }
            let before: Vec<_> = (0..menu.slot_count())
                .map(|s| menu.slot(s).cloned())
                .collect();
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            (0..menu.slot_count())
                .filter(|&s| menu.slot(s) != before[s].as_ref())
                .collect::<Vec<_>>()
        });
        match self.world.block_entities.get_mut(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                for k in 0..3 {
                    f.items[k] = menu.slot(k).cloned();
                }
                // AbstractFurnaceBlockEntity.setItem: only a new input restarts cooking.
                f.input_clicked(world_recipes!(self), items[0].clone());
            }
            // BrewingStandBlockEntity.serverTick notices a new ingredient itself.
            Some(BlockEntity::Brewing(b)) => {
                for k in 0..5 {
                    b.items[k] = menu.slot(k).cloned();
                }
            }
            // The mod's items past its menu's slots stay as they were.
            Some(BlockEntity::Mod(m)) => {
                let n = items.len();
                if m.items.len() < n {
                    m.items.resize(n, None);
                }
                for (k, slot) in m.items.iter_mut().take(n).enumerate() {
                    *slot = menu.slot(k).cloned();
                }
            }
            // CrafterBlockEntity.setItem: filling a disabled slot enables it.
            Some(BlockEntity::Crafter(c)) => {
                for k in 0..9 {
                    c.items[k] = menu.slot(k).cloned();
                    c.disabled[k] = menu.data[k] == 1;
                }
            }
            _ => {}
        }
        // FurnaceResultSlot.checkTakeAchievements: the guest took from the output.
        let count = |s: Option<&ItemStack>| s.map_or(0, |s| s.count);
        if furnace && count(menu.slot(2)) < count(items[2].as_ref()) {
            let at = host.hub.guests[gi].position;
            let guest = &host.hub.guests[gi];
            let output = items[2].as_ref();
            if let Some(output) = output {
                self.native_furnace_extract(
                    guest.uuid,
                    &guest.name,
                    [pos.0, pos.1, pos.2],
                    &output.item,
                    output.count - count(menu.slot(2)),
                );
            }
            self.award_furnace_recipes(pos, at);
        }
        // Slot.setChanged on a changed container slot: BlockEntity.setChanged,
        // so the chunk is saved and comparators read the block again.
        if changed.iter().any(|&s| s < items.len()) {
            self.container_changed(pos);
        }
        host.play.entry(conn).or_default().crafting.machine = Some((p.container, menu));
        host.hub.inventory_clicked(conn, p, &self.world, &changed);
        true
    }

    /// `ServerGamePacketListenerImpl.handleContainerSlotStateChanged`: a
    /// guest's crafter slot toggle (`CrafterBlockEntity.setSlotState`) in
    /// their open crafter window.
    pub(super) fn lan_crafter_slot_state(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: i32,
        slot: i32,
        enabled: bool,
    ) {
        let Some(pos) = host
            .hub
            .guest(conn)
            .filter(|g| !g.spectator())
            .and_then(|g| g.menu.as_ref())
            .filter(|m| window != 0 && m.window == window)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some(pos),
                _ => None,
            })
        else {
            return;
        };
        if let (Some(BlockEntity::Crafter(c)), Ok(slot)) = (
            self.world.block_entities.get_mut(&pos),
            usize::try_from(slot),
        ) {
            c.set_slot_state(slot, enabled);
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        }
    }

    /// ServerGamePacketListenerImpl.handlePlaceRecipe: the guest's recipe
    /// book places a known recipe in their inventory grid, crafting table or furnace
    /// (`ServerPlaceRecipe`), and answers with a ghost recipe when the items
    /// are missing.
    pub(super) fn lan_place_recipe(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        p: &horizon_protocol::packets::recipes::PlaceRecipe,
    ) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let g = &host.hub.guests[gi];
        if g.spectator() {
            return;
        }
        // The server menu uses the world's active data-pack tables, the same
        // tables used by the guest menu below. The base resources can have a
        // different display registry or placement for an overridden recipe.
        let Some(id) = world_stations!(self)
            .recipe_index
            .recipe_of_display(p.recipe)
            .map(str::to_owned)
        else {
            return;
        };
        // An unknown or impossible recipe is refused outright.
        if !g.recipe_book.knows(&id)
            || world_recipes!(self)
                .placement(&id)
                .is_none_or(|placement| placement.impossible())
        {
            return;
        }
        let (player, name) = (g.uuid, g.name.clone());
        self.native_player_recipe_book_click(player, &name, &id, p.use_max);
        let creative = g.creative();
        // AbstractFurnaceMenu.handlePlacement: the current furnace's slots,
        // rather than the separate crafting-table menu, own cooking inputs.
        let furnace = host.hub.guests[gi]
            .menu
            .as_ref()
            .filter(|m| p.container != 0 && m.window == p.container)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some(pos),
                _ => None,
            })
            .and_then(|pos| match self.world.block_entities.get(&pos) {
                Some(BlockEntity::Furnace(f)) => {
                    let recipe = match f.kind.recipe_type() {
                        "minecraft:blasting" => "blasting",
                        "minecraft:smoking" => "smoking",
                        _ => "smelting",
                    };
                    Some((pos, Kind::Furnace(recipe), f.items.clone()))
                }
                _ => None,
            });
        let placed = if let Some((pos, kind, items)) = furnace {
            // Opening need not have created the click/drag cache yet.
            let mut menu = host
                .play
                .entry(conn)
                .or_default()
                .crafting
                .machine
                .take()
                .filter(|(window, m)| *window == p.container && m.kind() == kind)
                .map_or_else(|| Box::new(Menu::new(kind)), |(_, m)| m);
            let placed = self.guest_menu_op(host, gi, &mut menu, |menu, env| {
                for (k, item) in items.iter().enumerate() {
                    menu.set_slot(env, k, item.clone());
                }
                menu.place_recipe(env, &id, p.use_max, creative, None)
            });
            let changed = (0..3).any(|k| menu.slot(k) != items[k].as_ref());
            if let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get_mut(&pos) {
                for k in 0..3 {
                    f.items[k] = menu.slot(k).cloned();
                }
                f.input_clicked(world_recipes!(self), items[0].clone());
            }
            if changed {
                self.container_changed(pos);
            }
            host.play.entry(conn).or_default().crafting.machine = Some((p.container, menu));
            // Hub::sync_containers broadcasts the actual changed slots.
            Some(placed)
        } else {
            self.lan_apply_recipe(host, gi, p.container, |menu, env| {
                menu.place_recipe(env, &id, p.use_max, creative, None)
            })
        };
        if placed == Some(PostPlace::Ghost) {
            host.hub.send_ghost_recipe(conn, p.container, &id);
        }
    }

    pub(super) fn guest_recipe_transfer(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        use horizon_protocol::buf::{Reader, Writer};
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let g = &host.hub.guests[gi];
        if g.brand != "horizon" || g.spectator() {
            return;
        }
        let mut reader = Reader::new(data);
        let (Ok(container), Ok(id)) = (reader.varint(), reader.string(512)) else {
            return;
        };
        if reader.remaining() != 0 {
            return;
        }
        let result = if container == 0 && (g.menu.is_some() || g.creative()) {
            Err(TransferError::Unavailable)
        } else {
            self.lan_apply_recipe(host, gi, container, |menu, env| {
                menu.transfer_recipe(env, &id)
            })
            .unwrap_or(Err(TransferError::Unavailable))
        };
        let mut reply = Writer::new();
        reply.varint(container);
        reply.u8(match result {
            Ok(()) => 0,
            Err(TransferError::Missing) => 1,
            Err(TransferError::NoRoom) => 2,
            Err(TransferError::Unavailable) => 3,
        });
        host.hub.guests[gi].send(&C::CustomPayload(
            horizon_protocol::packets::configuration::CustomPayload {
                channel: horizon_client::lan::recipes::TRANSFER_CHANNEL.into(),
                data: reply.data,
            },
        ));
    }

    fn lan_apply_recipe<R>(
        &mut self,
        host: &mut LanHost,
        gi: usize,
        container: i32,
        apply: impl FnOnce(&mut Menu, &Env) -> R,
    ) -> Option<R> {
        let conn = host.hub.guests[gi].conn;
        let placed = if container == 0 {
            let crafting = host.play.entry(conn).or_default();
            let mut menu = crafting
                .crafting
                .inventory
                .take()
                .unwrap_or_else(|| Box::new(Menu::new(Kind::Inventory)));
            let grid = host.hub.guests[gi].craft_grid.clone();
            let placed = self.guest_menu_op(host, gi, &mut menu, |menu, env| {
                for (k, s) in grid.into_iter().enumerate() {
                    menu.set_slot(env, 1 + k, s);
                }
                apply(menu, env)
            });
            let g = &mut host.hub.guests[gi];
            for k in 0..4 {
                g.craft_grid[k] = menu.slot(1 + k).cloned();
            }
            g.craft_result = menu.slot(0).cloned();
            host.play.entry(conn).or_default().crafting.inventory = Some(menu);
            host.hub.send_inventory(gi);
            placed
        } else {
            let (window, state, mut menu, pos) = host
                .play
                .get_mut(&conn)
                .and_then(|play| play.crafting.table.take_if(|(w, ..)| *w == container))?;
            let placed = self.guest_menu_op(host, gi, &mut menu, apply);
            host.play.entry(conn).or_default().crafting.table = Some((window, state, menu, pos));
            self.lan_send_table_content(host, conn);
            placed
        };
        Some(placed)
    }

    /// The guest opened a crafting table (`CraftingTableBlock.getMenuProvider`).
    pub(super) fn lan_open_crafting(&mut self, host: &mut LanHost, conn: u32, pos: [i32; 3]) {
        if !self.guest_may_open(host, conn, "minecraft:crafting", Some(pos)) {
            return;
        }
        // ServerPlayer.openMenu closes the menu that was open.
        self.guest_close_all(host, conn);
        let title = horizon_protocol::text::translatable("container.crafting", &[]);
        let Some(window) = host.hub.open_window(conn, "minecraft:crafting", title) else {
            return;
        };
        let menu = Box::new(Menu::new(Kind::Crafting));
        host.play.entry(conn).or_default().crafting.table = Some((window, 0, menu, pos));
        self.lan_send_table_content(host, conn);
    }

    /// A click on the guest's crafting table window; false for other windows.
    pub(super) fn lan_crafting_click(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        let Some(play) = host.play.get_mut(&conn) else {
            return false;
        };
        let Some((window, state, mut menu, pos)) = play
            .crafting
            .table
            .take_if(|(window, ..)| *window == p.container)
        else {
            return false;
        };
        // handleContainerClick: broadcastFullState only for a click on an old
        // state, else only what the client got wrong. The whole menu after every
        // click would undo the clicks it has predicted since.
        let mut agreed = false;
        // A table that is no longer valid takes no clicks (handleContainerClick).
        if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn)
            && !host.hub.guests[gi].spectator()
            && self.lan_table_valid(&host.hub.guests[gi], pos)
        {
            let mut changed = self.guest_menu_op(host, gi, &mut menu, |menu, env| {
                let before: Vec<_> = (0..menu.slot_count())
                    .map(|s| menu.slot(s).cloned())
                    .collect();
                let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
                (0..menu.slot_count())
                    .filter(|&s| menu.slot(s) != before[s].as_ref())
                    .collect::<Vec<_>>()
            });
            let guest = &host.hub.guests[gi];
            let (uuid, name) = (guest.uuid, guest.name.clone());
            let mut result = menu.slot(0).cloned();
            let vanilla = result.clone();
            self.native_prepare_item_craft(uuid, &name, Some(pos), &mut result);
            if result != vanilla && !changed.contains(&0) {
                changed.push(0);
            }
            let env = self.menu_env();
            menu.set_slot(&env, 0, result);
            agreed = p.state == state
                && host.hub.click_agreed(
                    gi,
                    p,
                    menu.slot_count(),
                    &|s| menu.slot(s).cloned(),
                    &changed,
                );
            if agreed {
                // The inventory menu's sync isn't to send what the click settled.
                let g = &mut host.hub.guests[gi];
                for &s in changed.iter().filter(|&&s| s >= 10) {
                    g.menu_sent[s - 1] = menu.slot(s).cloned();
                }
            }
        }
        host.play.entry(conn).or_default().crafting.table = Some((window, state, menu, pos));
        if !agreed {
            self.lan_send_table_content(host, conn);
        }
        true
    }

    /// `CraftingMenu.stillValid`: the block is still a crafting table and the
    /// guest is within reach of it.
    pub(super) fn lan_table_valid(&self, g: &Guest, pos: [i32; 3]) -> bool {
        let name = &self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name;
        &**name == "minecraft:crafting_table" && g.within_block_range(pos, 4.0)
    }

    /// `ServerPlayer.tick`'s `stillValid`: a guest's crafting table closes
    /// once it is gone or out of reach, giving its grid back.
    pub(super) fn lan_tables_tick(&mut self, host: &mut LanHost) {
        let invalid: Vec<(u32, i32)> = host
            .play
            .iter()
            .filter_map(|(c, p)| p.crafting.table.as_ref().map(|t| (*c, t.0, t.3)))
            .filter(|&(c, _, pos)| {
                host.hub
                    .guests
                    .iter()
                    .find(|g| g.conn == c)
                    .is_some_and(|g| host.hub.here(g) && !self.lan_table_valid(g, pos))
            })
            .map(|(c, window, _)| (c, window))
            .collect();
        for (conn, window) in invalid {
            self.lan_crafting_closed(host, conn, Some(window));
            host.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose { container: window }),
            );
            if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
                self.guest_carried_back(host, gi);
            }
        }
    }

    /// The whole crafting table menu for the client (`sendAllDataToRemote`).
    fn lan_send_table_content(&mut self, host: &mut LanHost, conn: u32) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let Some((window, state, menu, _)) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.crafting.table.as_mut())
        else {
            return;
        };
        *state = (*state + 1) & 0x7FFF;
        let (window, state) = (*window, *state);
        let mut items: Vec<Option<ItemStack>> = (0..10).map(|s| menu.slot(s).cloned()).collect();
        let g = &host.hub.guests[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let items = items.iter().map(|s| host.hub.stack(s)).collect();
        let carried = host.hub.stack(&carried);
        host.hub.send_to(
            conn,
            &C::ContainerSetContent(play::ContainerSetContent {
                container: window,
                state,
                items,
                carried,
            }),
        );
        host.hub.guests[gi].inventory_told();
    }

    /// `AbstractContainerMenu.removed` of a crafting menu: the closed window
    /// (`window`; None closes the table whichever it is, and 0 the inventory
    /// menu) gives its grid back to the inventory, what doesn't fit is
    /// dropped. False when no such window is open.
    /// The open crafting table's own slots (result and grid) before the
    /// player's inventory, if the guest has one open.
    pub(super) fn lan_crafting_table_own_slots(host: &LanHost, conn: u32) -> Option<usize> {
        let (_, _, menu, _) = host.play.get(&conn)?.crafting.table.as_ref()?;
        Some(menu.slot_count() - 36)
    }

    pub(super) fn lan_crafting_closed(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        self.lan_crafting_removed(host, conn, window, MenuRemoval::Live)
    }

    /// `AbstractContainerMenu.clearContainer`: only input slots are removed;
    /// `dropOrPlaceInInventory` chooses drops for a disconnected player.
    pub(super) fn lan_crafting_removed(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: Option<i32>,
        removal: MenuRemoval,
    ) -> bool {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let crafting = &mut host.play.entry(conn).or_default().crafting;
        let grid: Vec<ItemStack> = if window == Some(0) {
            crafting.inventory = None;
            let g = &mut host.hub.guests[gi];
            g.craft_result = None;
            g.craft_grid.iter_mut().filter_map(Option::take).collect()
        } else {
            match crafting
                .table
                .take_if(|(w, ..)| window.is_none_or(|window| *w == window))
            {
                Some((_, _, menu, _)) => (1..10).filter_map(|s| menu.slot(s).cloned()).collect(),
                None => return false,
            }
        };
        if !grid.is_empty() {
            match removal {
                MenuRemoval::Live => self.lan_place_back(host, gi, grid),
                MenuRemoval::Disconnected => self.drop_guest_items(host, gi, grid),
            }
        }
        true
    }

    /// `Inventory.placeItemBackInInventory` of `stacks` for guest `gi`, what
    /// doesn't fit dropped (`clearContainer`).
    pub(super) fn lan_place_back(&mut self, host: &mut LanHost, gi: usize, stacks: Vec<ItemStack>) {
        let mut menu = Menu::new(Kind::Inventory);
        self.guest_menu_op(host, gi, &mut menu, |menu, env| {
            place_back(menu, env, stacks)
        });
    }

    /// A guest left: what their crafting grids and workstation held goes back into their
    /// inventory before it is saved (dropped where they stood if full).
    pub(super) fn lan_crafting_left(&mut self, host: &mut LanHost, g: &mut Guest) {
        // The guest's play state stays: their statistics are awarded and saved
        // after this (`HostEvent::Left`), which removes it.
        let play = host.play.get_mut(&g.conn);
        let mut grid: Vec<ItemStack> = g.craft_grid.iter_mut().filter_map(Option::take).collect();
        g.craft_result = None;
        if let Some(play) = play {
            if let Some((_, _, menu, _)) = play.crafting.table.take() {
                grid.extend((1..10).filter_map(|s| menu.slot(s).cloned()));
            }
            // A workstation's inputs too.
            if let Some(station) = play.station.take() {
                grid.extend(super::lan_stations::station_inputs(&station));
            }
        }
        if grid.is_empty() {
            return;
        }
        let mut menu = Menu::new(Kind::Inventory);
        load_player(&mut menu, g);
        place_back(&mut menu, &self.menu_env(), grid);
        let dropped = std::mem::take(&mut menu.dropped);
        store_player(&mut menu, g);
        for stack in dropped {
            self.spawn_item(stack, g.position, [0.0, 0.1, 0.0], 40);
        }
    }
}

#[cfg(test)]
mod machine_recipe_placement_tests {
    use super::*;
    use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use horizon_core::position::ChunkPos;
    use horizon_protocol::packets::{
        configuration::ClientInformation,
        play::{Serverbound, UseItemOn},
        recipes::PlaceRecipe,
    };
    use std::sync::{Arc, atomic::AtomicI32, mpsc};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h234_registered_place_recipe_reaches_actual_furnace_and_preserves_controls() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.world.loaded(8, 8));
        game.lan_host(
            crate::game::lan::LanSettings {
                port: 0,
                guest_commands: true,
                force_game_mode: false,
            },
            NetConfig {
                announce: false,
                ..NetConfig::loopback(0, "machine-place-test".into(), 2)
            },
            HubConfig {
                default_game_mode: 0,
                guest_commands: true,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
            },
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn: 2,
                uuid: 234,
                name: "RecipeGuest".into(),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            2,
            [8.5, 200.0, 8.5],
            2,
        );
        guest.dimension = "minecraft:overworld".into();
        guest.placed = true;
        guest.loaded = true;
        guest.selected = 1;
        guest.permission = 2;
        let mut host = game.lan.take().unwrap();
        host.hub.guests.push(guest);
        let clock = game.world.game_time;
        let planks = game
            .stations()
            .recipe_index
            .first_display("minecraft:oak_planks")
            .unwrap();
        let charcoal = game
            .stations()
            .recipe_index
            .first_display("minecraft:charcoal")
            .unwrap();
        let command = |game: &mut Game<'_>, host: &mut LanHost, text: &str| {
            assert!(
                game.as_guest(host, 2, |g| g.run_command(text)).unwrap(),
                "{text}"
            );
        };
        let place = |game: &mut Game<'_>, host: &mut LanHost, container, recipe| {
            game.lan_guest_packet(
                host,
                2,
                Serverbound::PlaceRecipe(PlaceRecipe {
                    container,
                    recipe,
                    use_max: false,
                }),
            );
        };
        let open = |game: &mut Game<'_>, host: &mut LanHost, pos| {
            game.lan_guest_packet(
                host,
                2,
                Serverbound::UseItemOn(UseItemOn {
                    hand: 0,
                    pos,
                    face: 1,
                    cursor: [0.5, 1.0, 0.5],
                    inside: false,
                    world_border: false,
                    sequence: 234,
                }),
            );
            assert!(host.hub.guest(2).unwrap().ack_up_to.get() >= 234);
            host.play
                .get(&2)
                .and_then(|p| p.crafting.table.as_ref())
                .map(|(window, ..)| *window)
                .or_else(|| host.hub.guest(2).unwrap().menu.as_ref().map(|m| m.window))
                .expect("actual table or machine window opened")
        };
        command(&mut game, &mut host, "recipe give @s minecraft:oak_planks");
        command(&mut game, &mut host, "recipe give @s minecraft:charcoal");
        assert!(
            host.hub
                .guest(2)
                .unwrap()
                .recipe_book
                .knows("minecraft:charcoal")
        );
        command(
            &mut game,
            &mut host,
            "item replace entity @s hotbar.0 with oak_log 3",
        );
        place(&mut game, &mut host, 0, planks);
        assert_eq!(
            host.hub.guest(2).unwrap().craft_grid[0],
            Some(ItemStack::new("minecraft:oak_log", 1))
        );
        assert_eq!(
            host.hub.guest(2).unwrap().craft_result,
            Some(ItemStack::new("minecraft:oak_planks", 4))
        );
        assert_eq!(
            host.hub.guest(2).unwrap().inventory[0]
                .as_ref()
                .unwrap()
                .count,
            2
        );
        game.lan_crafting_closed(&mut host, 2, Some(0));
        assert!(game.run_command("setblock 10 200 8 crafting_table"));
        let window = open(&mut game, &mut host, [10, 200, 8]);
        place(&mut game, &mut host, window, planks);
        let (_, _, table, _) = host.play[&2].crafting.table.as_ref().unwrap();
        assert_eq!(
            table.slot(0),
            Some(&ItemStack::new("minecraft:oak_planks", 4))
        );
        game.guest_close_all(&mut host, 2);
        let mut failures = Vec::new();
        for (label, count, known) in [
            ("place", 3, true),
            ("ghost", 0, true),
            ("forgotten", 3, false),
        ] {
            game.as_guest(&mut host, 2, |g| {
                g.run_command("clear @s");
            })
            .unwrap();
            if count > 0 {
                command(
                    &mut game,
                    &mut host,
                    "item replace entity @s hotbar.0 with oak_log 3",
                );
            }
            command(
                &mut game,
                &mut host,
                "item replace entity @s hotbar.8 with stick[minecraft:custom_data={h234:1}] 7",
            );
            assert!(game.run_command("setblock 10 200 8 air"));
            assert!(game.run_command("setblock 10 200 8 furnace"));
            assert!(game.run_command("item replace block 10 200 8 container.1 with coal[minecraft:custom_data={h234:2}] 2"));
            let window = open(&mut game, &mut host, [10, 200, 8]);
            if !known {
                command(&mut game, &mut host, "recipe take @s minecraft:charcoal");
            }
            assert_eq!(
                host.hub
                    .guest(2)
                    .unwrap()
                    .recipe_book
                    .knows("minecraft:charcoal"),
                known
            );
            let before = host.hub.guest(2).unwrap().inventory.clone();
            let pos = (10, 200, 8);
            let Some(BlockEntity::Furnace(f)) = game.world.block_entities.get(&pos) else {
                panic!("actual furnace missing")
            };
            assert!(f.items[0].is_none() && f.items[2].is_none());
            let fuel = f.items[1].clone();
            assert_eq!(fuel.as_ref().unwrap().count, 2);
            place(&mut game, &mut host, window, charcoal);
            let Some(BlockEntity::Furnace(f)) = game.world.block_entities.get(&pos) else {
                panic!("furnace removed")
            };
            assert_eq!(f.items[1], fuel);
            assert!(f.items[2].is_none());
            let mut expected = before.clone();
            let input = if label == "place" {
                expected[0].as_mut().unwrap().count = 2;
                Some(ItemStack::new("minecraft:oak_log", 1))
            } else {
                None
            };
            if f.items[0] != input || host.hub.guest(2).unwrap().inventory != expected {
                failures.push((
                    label,
                    f.items.clone(),
                    host.hub.guest(2).unwrap().inventory.clone(),
                ));
            }
            assert_eq!(host.hub.guest(2).unwrap().uuid, 234);
            assert_eq!(host.hub.guest(2).unwrap().health, 20.0);
            assert_eq!(host.hub.guest(2).unwrap().position, [8.5, 200.0, 8.5]);
            assert_eq!(game.world.game_time, clock);
            game.guest_close_all(&mut host, 2);
        }
        assert!(
            failures.is_empty(),
            "actual registered PlaceRecipe furnace failures: {failures:?}"
        );
    }
}
