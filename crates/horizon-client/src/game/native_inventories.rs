//! Another player's inventories for server mods (API 132):
//! what an operator's `/invsee` or `/endersee` needs. A player online is read
//! and changed live; a player who isn't on the server is read from, and
//! written to, their file (`players/data/<uuid>.dat`, as the server saves
//! it), never while they are logging in, dead or swapped in. One stack moves
//! at a time, checked against what the mod last showed, so a menu built on
//! these never copies or loses an item. Server menus are in `native_menus.rs`;
//! maps a mod draws in `native_send_map` below. `native_open_inventory_menu`
//! shows another player's inventory in a menu whose clicks move their items by
//! vanilla's rules, live or on their file.
use super::{Game, lan::LanHost};
use horizon_client::{
    items::ItemStack,
    lan::{
        guest::Guest,
        menu::{self, NativeOwner, NativeStore, Slots},
        playerdata,
    },
    save::{Level, read_dat, write_dat},
    text::Text,
};
use std::path::Path;

/// Which of a player's inventories (`native_inventory`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeInventoryKind {
    /// 41 slots: 0..9 the hotbar, 9..36 the rest (as `Inventory`), 36..40
    /// the armour (feet, legs, chest, head), 40 the off hand.
    Player,
    /// The ender chest's 27 slots.
    Ender,
    /// The stack on the cursor: 1 slot. Live only: a player's cursor is not
    /// in their file.
    Cursor,
    /// The 2x2 crafting grid: 4 slots, row by row. Live only: the grid is not
    /// in their file.
    Crafting,
}

impl NativeInventoryKind {
    pub(crate) fn size(self) -> usize {
        match self {
            Self::Player => 41,
            Self::Ender => 27,
            Self::Cursor => 1,
            Self::Crafting => 4,
        }
    }

    /// Kinds only a player on the server has (`Cursor`, `Crafting`).
    pub(crate) fn live_only(self) -> bool {
        matches!(self, Self::Cursor | Self::Crafting)
    }
}

/// Why a call on another player's inventories failed. Its `Display` is the
/// text the mods' logs show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeInventoryError {
    /// The player is in a turn of their own (call from a player's hook, or
    /// from `native_menu_clicked` or `native_server_tick`).
    InsidePlayerTurn,
    /// No server is running.
    NoServer,
    /// The player is logging in.
    LoggingIn,
    /// The player is dead.
    Dead,
    /// The player is on the server (for a file made for an away player).
    Online,
    /// No world is open.
    NoWorld,
    /// The player has never played here: there is no file.
    NoSavedPlayer,
    /// The player has a file already.
    HasFile,
    /// The player is not on the server; the inventory is live only.
    NotOnline,
    /// Both players of a move are away.
    BothAway,
    /// The move's two inventories are the same one.
    SameInventory,
    /// The stack is not the one expected (it changed).
    StackChanged,
    /// The stack does not fit whole in the destination.
    NoRoom,
    /// The inventory takes `want` slots, not `got`.
    WrongSize { want: usize, got: usize },
    /// No such hotbar slot (0..9, `Player` only).
    NoSuchHotbarSlot,
    /// No such slot in this inventory.
    NoSuchSlot,
    /// A slot named twice in one take.
    SlotNamedTwice,
    /// The player's file could not be read or written.
    File(String),
}

impl std::fmt::Display for NativeInventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsidePlayerTurn => write!(f, "inside a player's turn"),
            Self::NoServer => write!(f, "no server"),
            Self::LoggingIn => write!(f, "the player is logging in"),
            Self::Dead => write!(f, "the player is dead"),
            Self::Online => write!(f, "the player is on the server"),
            Self::NoWorld => write!(f, "no world"),
            Self::NoSavedPlayer => write!(f, "no saved player"),
            Self::HasFile => write!(f, "the player has a file"),
            Self::NotOnline => write!(f, "the player is not on the server"),
            Self::BothAway => write!(f, "both players are away"),
            Self::SameInventory => write!(f, "the same inventory"),
            Self::StackChanged => write!(f, "that stack changed"),
            Self::NoRoom => write!(f, "no room"),
            Self::WrongSize { want, got } => write!(f, "{want} slots, not {got}"),
            Self::NoSuchHotbarSlot => write!(f, "no such hotbar slot"),
            Self::NoSuchSlot => write!(f, "no such slot"),
            Self::SlotNamedTwice => write!(f, "a slot named twice"),
            Self::File(e) => write!(f, "the player's file: {e}"),
        }
    }
}

impl std::error::Error for NativeInventoryError {}

/// A player's inventory as `native_inventory` read it: its slots, and
/// whether it is the live one (the player is on the server) or their file's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeInventory {
    pub slots: Vec<Option<ItemStack>>,
    pub online: bool,
}

/// `Level.inventory`'s slot numbers (the file's) for our 41.
fn file_slot(i: usize) -> Option<u8> {
    match i {
        0..36 => Some(i as u8),
        36..40 => Some(100 + (i - 36) as u8),
        40 => Some(150),
        _ => None,
    }
}

fn guest_slots(g: &Guest, kind: NativeInventoryKind) -> Vec<Option<ItemStack>> {
    match kind {
        NativeInventoryKind::Player | NativeInventoryKind::Ender => {
            menu::owner_slots(g, kind == NativeInventoryKind::Ender)
        }
        NativeInventoryKind::Cursor => vec![g.carried.clone()],
        NativeInventoryKind::Crafting => g.craft_grid.to_vec(),
    }
}

fn set_guest_slots(g: &mut Guest, kind: NativeInventoryKind, s: Vec<Option<ItemStack>>) {
    match kind {
        NativeInventoryKind::Player | NativeInventoryKind::Ender => {
            menu::set_owner_slots(g, kind == NativeInventoryKind::Ender, s);
        }
        NativeInventoryKind::Cursor => g.carried = s.into_iter().next().flatten(),
        NativeInventoryKind::Crafting => {
            for (i, stack) in s.into_iter().take(4).enumerate() {
                g.craft_grid[i] = stack;
            }
        }
    }
}

/// The slots `guest_slots` reads, by reference (nothing is cloned), for
/// `native_players_inventory_changes` to compare each tick.
fn slot_refs(g: &Guest, kind: NativeInventoryKind) -> Vec<Option<&ItemStack>> {
    match kind {
        NativeInventoryKind::Player => (0..36)
            .map(|i| g.inventory.get(i).and_then(Option::as_ref))
            .chain([3, 2, 1, 0].map(|i| g.armor[i].as_ref()))
            .chain(std::iter::once(g.offhand.as_ref()))
            .collect(),
        NativeInventoryKind::Ender => (0..27)
            .map(|i| g.ender_items.get(i).and_then(Option::as_ref))
            .collect(),
        NativeInventoryKind::Cursor => vec![g.carried.as_ref()],
        NativeInventoryKind::Crafting => g.craft_grid.iter().map(Option::as_ref).collect(),
    }
}

/// Whether the slots `now` differ from what the mods last `heard` of them;
/// when they do, `heard` becomes `now`. `native_players_inventory_changes`
/// asks it once per player and kind each tick.
fn heard_changed(heard: &mut Vec<Option<ItemStack>>, now: &[Option<&ItemStack>]) -> bool {
    let same =
        now.len() == heard.len() && now.iter().zip(heard.iter()).all(|(n, h)| *n == h.as_ref());
    if !same {
        *heard = now.iter().map(|n| (*n).cloned()).collect();
    }
    !same
}

fn level_slots(level: &Level, kind: NativeInventoryKind) -> Vec<Option<ItemStack>> {
    match kind {
        // Live only: never read from a file (`native_inventory_side_for`).
        NativeInventoryKind::Cursor | NativeInventoryKind::Crafting => vec![None; kind.size()],
        NativeInventoryKind::Player => {
            let mut s = vec![None; 41];
            for (slot, item, count, damage) in &level.inventory {
                if let Some(i) = (0..41).find(|&i| file_slot(i) == Some(*slot)) {
                    s[i] = Some(ItemStack {
                        item: item.as_str().into(),
                        count: (*count).max(1),
                        damage: *damage,
                        extra: level.inventory_components.get(slot).cloned(),
                    });
                }
            }
            s
        }
        NativeInventoryKind::Ender => {
            let mut s = level.ender_items.clone();
            s.resize(27, None);
            s
        }
    }
}

fn set_level_slots(level: &mut Level, kind: NativeInventoryKind, s: Vec<Option<ItemStack>>) {
    match kind {
        NativeInventoryKind::Player => {
            level.inventory.clear();
            level.inventory_components.clear();
            for (i, stack) in s.into_iter().enumerate() {
                if let (Some(slot), Some(st)) = (file_slot(i), stack) {
                    level
                        .inventory
                        .push((slot, st.item.to_string(), st.count, st.damage));
                    if let Some(extra) = st.extra {
                        level.inventory_components.insert(slot, extra);
                    }
                }
            }
        }
        NativeInventoryKind::Ender => level.ender_items = s,
        // Live only: never written to a file.
        NativeInventoryKind::Cursor | NativeInventoryKind::Crafting => {}
    }
}

/// A saved player's file read as `playerdata::load` reads it, kept whole
/// (`player_extra`) so writing it back changes only what was edited.
fn read_level(world: &Path, uuid: u128) -> Option<Level> {
    let file = playerdata::path(world, uuid);
    let tag = read_dat(&file)?;
    let mut level = Level {
        health: 20.0,
        food: 20,
        saturation: 5.0,
        ..Level::default()
    };
    level.read_player(&tag);
    level.player_extra = Some(tag);
    Some(level)
}

/// Where one side of a move lives while it is made.
enum Side {
    /// A guest's index in the hub.
    Live(usize),
    /// A player's file, read.
    File(Box<Level>),
}

/// Another player's inventories, for server mods. API 132.
#[allow(dead_code, reason = "native mod API (docs/native-mods.md)")]
impl Game<'_> {
    /// The player `uuid`'s inventory `kind`: the live one when they are on
    /// the server, else their file's. Err while they are logging in, dead,
    /// swapped in (inside a player's hook: call from `native_menu_clicked` or
    /// `native_server_tick`), or when they have never played here.
    pub(super) fn native_inventory_typed(
        &self,
        uuid: u128,
        kind: NativeInventoryKind,
    ) -> Result<NativeInventory, NativeInventoryError> {
        match self.native_inventory_side_for(uuid, kind)? {
            Side::Live(i) => {
                let host = self.lan.as_ref().expect("live");
                Ok(NativeInventory {
                    slots: guest_slots(&host.hub.guests[i], kind),
                    online: true,
                })
            }
            Side::File(level) => Ok(NativeInventory {
                slots: level_slots(&level, kind),
                online: false,
            }),
        }
    }

    fn native_inventory_side(&self, uuid: u128) -> Result<Side, NativeInventoryError> {
        if self.lan_acting.is_some() {
            return Err(NativeInventoryError::InsidePlayerTurn);
        }
        let Some(host) = self.lan.as_ref() else {
            return Err(NativeInventoryError::NoServer);
        };
        if let Some(i) = host.hub.guests.iter().position(|g| g.uuid == uuid) {
            let g = &host.hub.guests[i];
            return if !g.placed {
                Err(NativeInventoryError::LoggingIn)
            } else if g.dead {
                Err(NativeInventoryError::Dead)
            } else {
                Ok(Side::Live(i))
            };
        }
        if self.world_root.as_os_str().is_empty() {
            return Err(NativeInventoryError::NoWorld);
        }
        read_level(&self.world_root, uuid)
            .map(|l| Side::File(Box::new(l)))
            .ok_or(NativeInventoryError::NoSavedPlayer)
    }

    /// `native_inventory_side` for an inventory of `kind`: a live-only kind
    /// (`Cursor`, `Crafting`) has no file, so an away player is Err.
    fn native_inventory_side_for(
        &self,
        uuid: u128,
        kind: NativeInventoryKind,
    ) -> Result<Side, NativeInventoryError> {
        let side = self.native_inventory_side(uuid)?;
        if kind.live_only() && matches!(side, Side::File(_)) {
            return Err(NativeInventoryError::NotOnline);
        }
        Ok(side)
    }

    /// Creates the saved file of the player `uuid` (`name` is for the log),
    /// for one who has never played here: a new player's state with full
    /// health and food, no items and no position. Until they first join,
    /// `native_inventory` and the helpers above read and change it; when they
    /// join, they load it as a returning player. Err: no world, the player on
    /// the server (`Online`), a file already (`HasFile`), or the file cannot
    /// be written (`File`). API @API:create_player_file@.
    pub(super) fn native_create_player_file(
        &mut self,
        uuid: u128,
        name: &str,
    ) -> Result<(), NativeInventoryError> {
        if self.world_root.as_os_str().is_empty() {
            return Err(NativeInventoryError::NoWorld);
        }
        if self
            .lan
            .as_ref()
            .is_some_and(|h| h.hub.guests.iter().any(|g| g.uuid == uuid))
        {
            return Err(NativeInventoryError::Online);
        }
        if playerdata::exists(&self.world_root, uuid) {
            return Err(NativeInventoryError::HasFile);
        }
        let _ = playerdata::directory(&self.world_root);
        let level = Level {
            player_uuid: Some(playerdata::uuid_ints(uuid)),
            health: 20.0,
            food: 20,
            saturation: 5.0,
            ..Level::default()
        };
        write_dat(
            &playerdata::path(&self.world_root, uuid),
            level.player_tag(),
        )
        .map_err(|e| NativeInventoryError::File(e.to_string()))?;
        log::info!("Horizon: a player file is created for {name}");
        Ok(())
    }

    /// Moves the stack in slot `from_slot` of `from`'s inventory `from_kind`,
    /// which must still be exactly `expected`, into `to`'s inventory
    /// `to_kind` where it fits whole (onto equal stacks first, then empty
    /// slots; for `Player`, the 36 main slots, never armour or off hand), or
    /// nothing moves and the Err says why. Each side is live or its file
    /// (`native_inventory`); at most one may be a file, which is written
    /// before the live side changes. Their screens follow with the tick's
    /// sync. Typed helpers: API @API:inventory_typed@.
    pub(super) fn native_move_stack_typed(
        &mut self,
        from: (u128, NativeInventoryKind, usize),
        expected: &ItemStack,
        to: (u128, NativeInventoryKind),
    ) -> Result<(), NativeInventoryError> {
        let (from_uuid, from_kind, from_slot) = from;
        let (to_uuid, to_kind) = to;
        if from_uuid == to_uuid && from_kind == to_kind {
            return Err(NativeInventoryError::SameInventory);
        }
        // A just-disconnected player can already be absent from the hub.
        // Drain before reading or replacing their offline file.
        let _ = self.finish_player_autosaves(true);
        let a = self.native_inventory_side_for(from_uuid, from_kind)?;
        let b = if from_uuid == to_uuid {
            if to_kind.live_only() && matches!(a, Side::File(_)) {
                return Err(NativeInventoryError::NotOnline);
            }
            None
        } else {
            Some(self.native_inventory_side_for(to_uuid, to_kind)?)
        };
        if matches!(a, Side::File(_)) && matches!(b, Some(Side::File(_))) {
            return Err(NativeInventoryError::BothAway);
        }
        let items = self.res.items();
        let max = |s: &ItemStack| items.get(&s.item).map_or(64, |i| i.max_stack);
        let read = |side: &Side, kind| match side {
            Side::Live(i) => guest_slots(&self.lan.as_ref().expect("live").hub.guests[*i], kind),
            Side::File(level) => level_slots(level, kind),
        };
        let mut src = read(&a, from_kind);
        let mut dst = read(b.as_ref().unwrap_or(&a), to_kind);
        match src.get_mut(from_slot) {
            Some(s) if s.as_ref() == Some(expected) => *s = None,
            _ => return Err(NativeInventoryError::StackChanged),
        }
        let room = match to_kind {
            NativeInventoryKind::Player => 36,
            NativeInventoryKind::Ender => 27,
            NativeInventoryKind::Cursor => 1,
            NativeInventoryKind::Crafting => 4,
        };
        if !super::native_menus::fit(&mut dst[..room], std::slice::from_ref(expected), &max) {
            return Err(NativeInventoryError::NoRoom);
        }
        // The file first: when it can't be written, nothing has moved.
        let world = self.world_root.clone();
        let write = |side: Side,
                     kind,
                     slots: Vec<Option<ItemStack>>,
                     uuid|
         -> Result<Side, NativeInventoryError> {
            if let Side::File(mut level) = side {
                set_level_slots(&mut level, kind, slots);
                write_dat(&playerdata::path(&world, uuid), level.player_tag())
                    .map_err(|e| NativeInventoryError::File(e.to_string()))?;
                return Ok(Side::File(level));
            }
            Ok(side)
        };
        let (a, b) = match b {
            None => {
                // Two inventories of one player: their file or live.
                let mut both = a;
                if let Side::File(level) = &mut both {
                    set_level_slots(level, from_kind, src.clone());
                }
                let both = write(both, to_kind, dst.clone(), from_uuid)?;
                (both, None)
            }
            Some(b) if matches!(b, Side::File(_)) => {
                let b = write(b, to_kind, dst.clone(), to_uuid)?;
                (a, Some(b))
            }
            Some(b) => {
                let a = write(a, from_kind, src.clone(), from_uuid)?;
                (a, Some(b))
            }
        };
        let host = self.lan.as_mut().expect("checked");
        if let Side::Live(i) = a {
            set_guest_slots(&mut host.hub.guests[i], from_kind, src);
            if b.is_none() {
                set_guest_slots(&mut host.hub.guests[i], to_kind, dst);
                return Ok(());
            }
        }
        if let Some(Side::Live(i)) = b {
            set_guest_slots(&mut host.hub.guests[i], to_kind, dst);
        }
        Ok(())
    }

    /// Replaces the player `uuid`'s whole inventory `kind` with `slots`,
    /// `kind.size()` of them in `native_inventory`'s numbering (Bukkit's
    /// `PlayerInventory.setContents`). For `Player`, `selected` (0..9), when
    /// given, becomes the held hotbar slot, and the cursor and the 2x2
    /// crafting grid are emptied (what they held is gone), as for an
    /// inventory loaded from elsewhere. Live or their file, as
    /// `native_inventory`; a player on the server is sent their whole
    /// inventory at once. Err as `native_inventory`, or for a wrong number
    /// of slots or a `selected` past the hotbar. @API:set_inventory@.
    pub(super) fn native_set_inventory_typed(
        &mut self,
        uuid: u128,
        kind: NativeInventoryKind,
        slots: Vec<Option<ItemStack>>,
        selected: Option<usize>,
    ) -> Result<(), NativeInventoryError> {
        if slots.len() != kind.size() {
            return Err(NativeInventoryError::WrongSize {
                want: kind.size(),
                got: slots.len(),
            });
        }
        if selected.is_some_and(|s| s >= 9 || kind != NativeInventoryKind::Player) {
            return Err(NativeInventoryError::NoSuchHotbarSlot);
        }
        let ((), live) = self.native_edit_slots(uuid, kind, |s, held| {
            *s = slots;
            if let Some(selected) = selected {
                *held = selected;
            }
            Ok(())
        })?;
        if let (Some(i), NativeInventoryKind::Player) = (live, kind) {
            use horizon_protocol::packets::play::{Clientbound, SetHeldSlot};
            let hub = &mut self.lan.as_mut().expect("live").hub;
            let g = &mut hub.guests[i];
            g.carried = None;
            g.craft_grid = Default::default();
            g.craft_result = None;
            hub.send_inventory(i);
            let g = &hub.guests[i];
            g.send(&Clientbound::SetHeldSlot(SetHeldSlot {
                slot: g.selected as i32,
            }));
        }
        Ok(())
    }

    /// Puts `new` in slot `slot` of the player `uuid`'s inventory `kind`
    /// (`native_inventory`'s numbering: armour and the off hand too), which
    /// must still hold exactly `expected` (None: empty), and returns what was
    /// there (Bukkit's `setItem`, checked like `native_move_stack`). Live or
    /// their file; a player's screen follows with the tick's sync. Err as
    /// `native_inventory`, for no such slot, or when the slot changed.
    /// @API:set_inventory_slot@.
    pub(super) fn native_set_inventory_slot_typed(
        &mut self,
        uuid: u128,
        kind: NativeInventoryKind,
        slot: usize,
        expected: Option<&ItemStack>,
        new: Option<ItemStack>,
    ) -> Result<Option<ItemStack>, NativeInventoryError> {
        let (old, _) = self.native_edit_slots(uuid, kind, |s, _| {
            let at = s.get_mut(slot).ok_or(NativeInventoryError::NoSuchSlot)?;
            if at.as_ref() != expected {
                return Err(NativeInventoryError::StackChanged);
            }
            Ok(std::mem::replace(at, new))
        })?;
        Ok(old)
    }

    /// Takes the stacks out of the player `uuid`'s inventory (`Player`
    /// numbering) slots, each of which must still hold exactly its stack, all
    /// or none, and returns them in order. Live or their file; a player's
    /// screen follows with the tick's sync. Err as `native_inventory`, for a
    /// slot named twice, or when one changed (nothing is taken then).
    /// @API:set_inventory_slot@.
    pub(super) fn native_take_stacks_typed(
        &mut self,
        uuid: u128,
        slots: &[(usize, ItemStack)],
    ) -> Result<Vec<ItemStack>, NativeInventoryError> {
        let (taken, _) = self.native_edit_slots(uuid, NativeInventoryKind::Player, |s, _| {
            for (k, (slot, stack)) in slots.iter().enumerate() {
                if slots[..k].iter().any(|(other, _)| other == slot) {
                    return Err(NativeInventoryError::SlotNamedTwice);
                }
                if s.get(*slot).and_then(Option::as_ref) != Some(stack) {
                    return Err(NativeInventoryError::StackChanged);
                }
            }
            Ok(slots
                .iter()
                .filter_map(|(slot, _)| s[*slot].take())
                .collect())
        })?;
        Ok(taken)
    }

    /// Edits the player `uuid`'s inventory `kind` (and, for `Player`, the
    /// held hotbar slot) live or in their file, as `native_inventory` reads
    /// it: nothing is written when `edit` fails. The guest's hub index when
    /// they are on the server.
    fn native_edit_slots<R>(
        &mut self,
        uuid: u128,
        kind: NativeInventoryKind,
        edit: impl FnOnce(&mut Vec<Option<ItemStack>>, &mut usize) -> Result<R, NativeInventoryError>,
    ) -> Result<(R, Option<usize>), NativeInventoryError> {
        // A just-disconnected player can already be absent from the hub.
        let _ = self.finish_player_autosaves(true);
        match self.native_inventory_side_for(uuid, kind)? {
            Side::Live(i) => {
                let g = &mut self.lan.as_mut().expect("live").hub.guests[i];
                let mut slots = guest_slots(g, kind);
                let mut held = g.selected;
                let r = edit(&mut slots, &mut held)?;
                set_guest_slots(g, kind, slots);
                g.selected = held;
                Ok((r, Some(i)))
            }
            Side::File(mut level) => {
                let mut slots = level_slots(&level, kind);
                let mut held = usize::try_from(level.selected_slot).unwrap_or(0);
                let r = edit(&mut slots, &mut held)?;
                set_level_slots(&mut level, kind, slots);
                level.selected_slot = held as i32;
                write_dat(
                    &playerdata::path(&self.world_root, uuid),
                    level.player_tag(),
                )
                .map_err(|e| NativeInventoryError::File(e.to_string()))?;
                Ok((r, None))
            }
        }
    }

    /// `inventory_changed` for the players on the server: each of their
    /// inventory kinds whose slots differ from what the mods last heard is
    /// heard once (`native_inventory_changed`; API @API:inventory_changed@), after the players' equipment
    /// in each tick. A player new to the server counts from empty, as for
    /// equipment. A player away is not checked: changes made to their file
    /// come from the mods' own calls.
    pub(super) fn native_players_inventory_changes(&mut self, host: &mut LanHost) {
        const KINDS: [NativeInventoryKind; 4] = [
            NativeInventoryKind::Player,
            NativeInventoryKind::Ender,
            NativeInventoryKind::Cursor,
            NativeInventoryKind::Crafting,
        ];
        let mut changes = Vec::new();
        for g in host.hub.guests.iter().filter(|g| host.hub.here(g)) {
            let Some(play) = host.play.get_mut(&g.conn) else {
                continue;
            };
            let heard = play
                .inventory_heard
                .get_or_insert_with(|| Box::new(KINDS.map(|k| vec![None; k.size()])));
            for (i, kind) in KINDS.into_iter().enumerate() {
                if heard_changed(&mut heard[i], &slot_refs(g, kind)) {
                    changes.push((g.uuid, kind));
                }
            }
        }
        for (uuid, kind) in changes {
            self.native_inventory_changed(uuid, kind);
        }
    }

    /// Sends the player `uuid` the whole picture of map `map_id`: 128 x 128
    /// colours (`MapColor` ids times 4 plus the shade, row by row), scale 0,
    /// locked, no decorations, as `ClientboundMapItemDataPacket`. A filled
    /// map the mod gives with that `minecraft:map_id` then shows it in hand
    /// and in item frames; the server keeps nothing (a map id with no saved
    /// map is never sent again by the game), so send it again after the
    /// player joins. Use ids far from the world's own (they count up from 0).
    /// False for a player not here, or `colors` not 16384 long.
    /// API 132.
    pub(super) fn native_send_map(&mut self, uuid: u128, map_id: i32, colors: &[u8]) -> bool {
        use horizon_protocol::packets::play::{Clientbound, MapItemData, MapPatch};
        if colors.len() != 128 * 128 {
            return false;
        }
        self.native_send(
            uuid,
            Clientbound::MapItemData(MapItemData {
                map_id,
                scale: 0,
                locked: true,
                decorations: None,
                patch: Some(MapPatch {
                    width: 128,
                    height: 128,
                    start_x: 0,
                    start_y: 0,
                    colors: colors.to_vec(),
                }),
            }),
        )
    }
}

/// The helpers the mods of API 132 call, with their `String` errors as they
/// were; each is its `_typed` helper (the same checks) with the error as text.
#[allow(dead_code, reason = "native mod API (docs/native-mods.md)")]
impl Game<'_> {
    pub(super) fn native_inventory(
        &self,
        uuid: u128,
        kind: NativeInventoryKind,
    ) -> Result<NativeInventory, String> {
        self.native_inventory_typed(uuid, kind)
            .map_err(|e| e.to_string())
    }

    pub(super) fn native_move_stack(
        &mut self,
        from: (u128, NativeInventoryKind, usize),
        expected: &ItemStack,
        to: (u128, NativeInventoryKind),
    ) -> Result<(), String> {
        self.native_move_stack_typed(from, expected, to)
            .map_err(|e| e.to_string())
    }

    pub(super) fn native_set_inventory(
        &mut self,
        uuid: u128,
        kind: NativeInventoryKind,
        slots: Vec<Option<ItemStack>>,
        selected: Option<usize>,
    ) -> Result<(), String> {
        self.native_set_inventory_typed(uuid, kind, slots, selected)
            .map_err(|e| e.to_string())
    }

    pub(super) fn native_set_inventory_slot(
        &mut self,
        uuid: u128,
        kind: NativeInventoryKind,
        slot: usize,
        expected: Option<&ItemStack>,
        new: Option<ItemStack>,
    ) -> Result<Option<ItemStack>, String> {
        self.native_set_inventory_slot_typed(uuid, kind, slot, expected, new)
            .map_err(|e| e.to_string())
    }

    pub(super) fn native_take_stacks(
        &mut self,
        uuid: u128,
        slots: &[(usize, ItemStack)],
    ) -> Result<Vec<ItemStack>, String> {
        self.native_take_stacks_typed(uuid, slots)
            .map_err(|e| e.to_string())
    }
}

/// Another player's inventory in a menu, for server mods. API @API:inventory_menu@.
#[allow(dead_code)]
impl Game<'_> {
    /// Opens a chest menu of `rows` rows titled `title` for the player
    /// `viewer` (in place of the menu they had open, as `native_open_menu`)
    /// that shows the inventory `kind` of the player `target`, live: menu slot
    /// `k` is the inventory's slot `layout[k]` (`native_inventory`'s
    /// numbering), and the menu's other slots stay the mod's to show
    /// (`native_set_menu_items`, their clicks heard by `native_menu_clicked`).
    /// Clicks on the shown slots move the target's own items by vanilla's
    /// click rules, as Bukkit's `openInventory(target.getInventory())` does:
    /// pickup, split, shift-click, number keys, drags, double-click, throw.
    /// An armour or off-hand slot takes any stack, as that view does. A target
    /// on the server is read before each click and each tick's sync and is
    /// changed in the same step; a target away is their file
    /// (`players/data/<uuid>.dat`), read before and written after each click
    /// that changes it (a click whose write fails is taken back whole). The
    /// menu closes once the target leaves, dies or, away, comes back. None
    /// for a viewer not here, the target being the viewer, logging in, dead or
    /// never seen here, a `Cursor` or `Crafting` kind (menus show only the
    /// inventory and the ender chest), a layout longer than the rows' slots, a slot number
    /// past the inventory or one shown twice, and inside a player's turn
    /// (call it from `native_menu_clicked` or `native_server_tick`).
    /// API @API:inventory_menu@.
    pub(super) fn native_open_inventory_menu(
        &mut self,
        viewer: u128,
        rows: u8,
        title: Text,
        target: (u128, NativeInventoryKind),
        layout: &[Option<usize>],
    ) -> Option<i32> {
        let (uuid, kind) = target;
        let size = usize::from(rows.clamp(1, 6)) * 9;
        let mut seen = std::collections::HashSet::new();
        if viewer == uuid
            || kind.live_only()
            || layout.len() > size
            || !layout
                .iter()
                .flatten()
                .all(|&slot| slot < kind.size() && seen.insert(slot))
        {
            return None;
        }
        let conn = self.native_conn(viewer)?;
        // A just-disconnected player's file may still be being written.
        let _ = self.finish_player_autosaves(true);
        let (slots, owner) = match self.native_inventory_side(uuid).ok()? {
            Side::Live(i) => {
                let g = &self.lan.as_ref()?.hub.guests[i];
                (guest_slots(g, kind), NativeOwner::Online(g.conn))
            }
            Side::File(level) => (level_slots(&level, kind), NativeOwner::Away(uuid)),
        };
        let items = layout
            .iter()
            .map(|at| at.and_then(|at| slots.get(at).cloned().flatten()))
            .collect();
        let window = self.native_open_menu(viewer, rows, title, items)?;
        let store = NativeStore::Inventory {
            owner,
            ender: kind == NativeInventoryKind::Ender,
            layout: layout.to_vec(),
        };
        self.lan.as_mut()?.hub.set_native_store(conn, store);
        Some(window)
    }
}

/// An away player's file read for one click on a menu showing it
/// (`native_open_inventory_menu`), and the clicking guest as they were.
pub(super) struct AwayClick {
    uuid: u128,
    kind: NativeInventoryKind,
    layout: Vec<Option<usize>>,
    level: Box<Level>,
    window: i32,
    /// The guest's inventory, off hand, cursor and menu before the click.
    before: (
        Vec<Option<ItemStack>>,
        Option<ItemStack>,
        Option<ItemStack>,
        Vec<Option<ItemStack>>,
    ),
}

/// Where the game keeps an away player's inventory menu on their file.
impl Game<'_> {
    /// Before guest `gi`'s click on `window`: when that is a native menu
    /// showing an away player's inventory, their file is read again into it,
    /// so the click moves what the file holds now (another viewer may have
    /// changed it). The menu closes instead when the player is back or the
    /// file can't be read. None for any other click.
    pub(super) fn away_menu_click_start(
        &mut self,
        host: &mut LanHost,
        gi: usize,
        window: i32,
    ) -> Option<AwayClick> {
        let g = &host.hub.guests[gi];
        g.menu
            .as_ref()
            .filter(|m| m.window == window && m.slots == Slots::Native)?;
        let Some(NativeStore::Inventory {
            owner: NativeOwner::Away(uuid),
            ender,
            layout,
        }) = &g.native_store
        else {
            return None;
        };
        let kind = if *ender {
            NativeInventoryKind::Ender
        } else {
            NativeInventoryKind::Player
        };
        let (uuid, layout, conn) = (*uuid, layout.clone(), g.conn);
        let _ = self.finish_player_autosaves(true);
        let back = host.hub.guests.iter().any(|o| o.uuid == uuid);
        let Some(level) = (!back)
            .then(|| read_level(&self.world_root, uuid))
            .flatten()
        else {
            host.hub.close_container(conn);
            return None;
        };
        let slots = level_slots(&level, kind);
        let g = &mut host.hub.guests[gi];
        for (k, at) in layout.iter().enumerate() {
            if let (Some(at), Some(slot)) = (at, g.native_menu.get_mut(k)) {
                *slot = slots.get(*at).cloned().flatten();
            }
        }
        let before = (
            g.inventory.clone(),
            g.offhand.clone(),
            g.carried.clone(),
            g.native_menu.clone(),
        );
        Some(AwayClick {
            uuid,
            kind,
            layout,
            level: Box::new(level),
            window,
            before,
        })
    }

    /// After the click `away_menu_click_start` prepared: the file gets what
    /// the click left in the menu's slots that show it, when it changed them.
    /// When the file can't be written the click is taken back whole (the
    /// guest's inventory, off hand, cursor and menu as they were, resent) and
    /// nothing it threw out is dropped. The stacks to drop.
    pub(super) fn away_menu_click_end(
        &mut self,
        host: &mut LanHost,
        gi: usize,
        click: AwayClick,
        drops: Vec<ItemStack>,
    ) -> Vec<ItemStack> {
        let AwayClick {
            uuid,
            kind,
            layout,
            mut level,
            window,
            before,
        } = click;
        let g = &host.hub.guests[gi];
        let mut slots = level_slots(&level, kind);
        let mut changed = false;
        for (k, at) in layout.iter().enumerate() {
            if let Some(at) = at
                && let (Some(now), Some(file)) = (g.native_menu.get(k), slots.get_mut(*at))
                && now != file
            {
                file.clone_from(now);
                changed = true;
            }
        }
        if !changed {
            return drops;
        }
        set_level_slots(&mut level, kind, slots);
        let Err(e) = write_dat(
            &playerdata::path(&self.world_root, uuid),
            level.player_tag(),
        ) else {
            return drops;
        };
        log::warn!("a menu could not write an away player's file: {e}");
        let conn = g.conn;
        let g = &mut host.hub.guests[gi];
        (g.inventory, g.offhand, g.carried, g.native_menu) = before;
        host.hub.resend_menu(conn, window, &self.world);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(item: &str, count: u32) -> ItemStack {
        ItemStack {
            item: item.into(),
            count,
            damage: 0,
            extra: None,
        }
    }

    /// `inventory_changed` is heard for a change only: an empty inventory
    /// against the empty start, or the same slots again, is no change; a new
    /// stack, or the same item in another count, is one, once.
    #[test]
    fn a_change_is_heard_once() {
        let mut heard = vec![None; 4];
        assert!(!heard_changed(&mut heard, &[None, None, None, None]));
        let stick = ItemStack::new("minecraft:stick", 1);
        assert!(heard_changed(&mut heard, &[None, Some(&stick), None, None]));
        assert!(!heard_changed(
            &mut heard,
            &[None, Some(&stick), None, None]
        ));
        let sticks = ItemStack::new("minecraft:stick", 2);
        assert!(heard_changed(
            &mut heard,
            &[None, Some(&sticks), None, None]
        ));
        assert_eq!(heard[1].as_ref(), Some(&sticks));
    }

    #[test]
    fn cursor_and_crafting_are_live_only_and_their_errors_read_as_before() {
        assert_eq!(NativeInventoryKind::Cursor.size(), 1);
        assert_eq!(NativeInventoryKind::Crafting.size(), 4);
        assert!(NativeInventoryKind::Cursor.live_only());
        assert!(NativeInventoryKind::Crafting.live_only());
        assert!(!NativeInventoryKind::Player.live_only());
        assert!(!NativeInventoryKind::Ender.live_only());
        assert_eq!(
            NativeInventoryError::WrongSize { want: 41, got: 3 }.to_string(),
            "41 slots, not 3"
        );
        assert_eq!(
            NativeInventoryError::File("disk full".into()).to_string(),
            "the player's file: disk full"
        );
    }

    #[test]
    fn a_files_slots_map_to_the_41_and_back() {
        let mut level = Level::default();
        let mut s = vec![None; 41];
        s[0] = Some(stack("minecraft:stone", 5));
        s[35] = Some(stack("minecraft:dirt", 1));
        s[36] = Some(stack("minecraft:iron_boots", 1));
        s[39] = Some(stack("minecraft:iron_helmet", 1));
        s[40] = Some(stack("minecraft:shield", 1));
        set_level_slots(&mut level, NativeInventoryKind::Player, s.clone());
        let slots: Vec<u8> = level.inventory.iter().map(|e| e.0).collect();
        assert_eq!(slots, [0, 35, 100, 103, 150]);
        assert_eq!(level_slots(&level, NativeInventoryKind::Player), s);
    }

    #[test]
    fn a_saved_player_file_round_trips_its_inventory_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("native-inv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let uuid = 0x1234_u128;
        let file = playerdata::path(&dir, uuid);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut level = Level {
            health: 13.0,
            xp_level: 7,
            ..Level::default()
        };
        level.ender_items = vec![None; 27];
        level.ender_items[2] = Some(stack("minecraft:diamond", 3));
        let mut s = vec![None; 41];
        s[4] = Some(stack("minecraft:bread", 12));
        set_level_slots(&mut level, NativeInventoryKind::Player, s);
        write_dat(&file, level.player_tag()).unwrap();
        let mut back = read_level(&dir, uuid).unwrap();
        assert_eq!(back.health, 13.0);
        assert_eq!(back.xp_level, 7);
        assert_eq!(
            level_slots(&back, NativeInventoryKind::Ender)[2],
            Some(stack("minecraft:diamond", 3))
        );
        let mut inv = level_slots(&back, NativeInventoryKind::Player);
        assert_eq!(inv[4], Some(stack("minecraft:bread", 12)));
        inv[4] = None;
        set_level_slots(&mut back, NativeInventoryKind::Player, inv);
        write_dat(&file, back.player_tag()).unwrap();
        let again = read_level(&dir, uuid).unwrap();
        assert!(
            level_slots(&again, NativeInventoryKind::Player)
                .iter()
                .all(Option::is_none)
        );
        assert_eq!(again.xp_level, 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
