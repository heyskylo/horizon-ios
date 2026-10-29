//! The spectator menu: `SpectatorGui` (the faded hotbar and the prompt under
//! it) over `SpectatorMenu` with its root page, Teleport to Player and
//! Teleport to Team Member, laid out as [`GuiDraw`] calls.
use crate::{
    font::Font,
    hud_layout::{Blend, GuiDraw, argb, color_from_float, fill, white, width},
    hud_overlays::text_with_backdrop,
    text::Text,
};
use std::borrow::Cow;

/// `SpectatorGui.FADE_OUT_DELAY` and `FADE_OUT_TIME`, in milliseconds.
const FADE_OUT_DELAY: i64 = 5000;
const FADE_OUT_TIME: f32 = 2000.0;

/// A player of the menu (`PlayerMenuItem`).
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub uuid: u128,
    pub name: String,
    /// Skin texture path, as the tab list draws it.
    pub skin: String,
    /// `PlayerInfo.getGameMode() != GameType.SPECTATOR`.
    pub enabled: bool,
}

/// A team with players to go to (`TeamSelectionItem`).
#[derive(Clone, Debug, PartialEq)]
pub struct Team {
    pub name: Text,
    /// The team colour's RGB.
    pub color: Option<u32>,
    pub players: Vec<Member>,
    /// The skin of the face drawn on the icon.
    pub icon: String,
}

/// A `SpectatorMenuItem`.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Empty,
    Close,
    Scroll { left: bool, enabled: bool },
    TeleportToPlayer { enabled: bool },
    TeleportToTeam { enabled: bool },
    Player(Member),
    Team(Team),
}
impl Item {
    pub fn enabled(&self) -> bool {
        match self {
            Item::Empty => false,
            Item::Close | Item::Team(_) => true,
            Item::Scroll { enabled, .. }
            | Item::TeleportToPlayer { enabled }
            | Item::TeleportToTeam { enabled } => *enabled,
            Item::Player(m) => m.enabled,
        }
    }
    /// `SpectatorMenuItem.getName`.
    pub fn name(&self) -> Text {
        match self {
            Item::Empty => Text::literal(""),
            Item::Close => Text::translate("spectatorMenu.close", &[]),
            Item::Scroll { left: true, .. } => Text::translate("spectatorMenu.previous_page", &[]),
            Item::Scroll { .. } => Text::translate("spectatorMenu.next_page", &[]),
            Item::TeleportToPlayer { .. } => Text::translate("spectatorMenu.teleport", &[]),
            Item::TeleportToTeam { .. } => Text::translate("spectatorMenu.team_teleport", &[]),
            Item::Player(m) => Text::literal(m.name.clone()),
            Item::Team(t) => t.name.clone(),
        }
    }
}

/// What a selected item asks the game to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    /// `SpectatorMenu.exit`.
    Exit,
    /// `ServerboundTeleportToEntityPacket` for this player.
    Teleport(u128),
}

enum Category {
    Root,
    Players,
    Teams,
    TeamPlayers(Vec<Member>),
}

/// `TeleportToPlayerMenuCategory`: players that are not spectators, ordered
/// by `UUID.compareTo` (signed halves).
pub fn players_page(mut players: Vec<Member>) -> Vec<Member> {
    players.retain(|m| m.enabled);
    players.sort_by_key(|m| ((m.uuid >> 64) as i64, m.uuid as i64));
    players
}

/// `SpectatorMenu`: the page, the selected slot and the page of its items.
pub struct SpectatorMenu {
    players: Vec<Member>,
    teams: Vec<Team>,
    category: Category,
    selected: i32,
    page: i32,
}
impl SpectatorMenu {
    /// `new SpectatorMenu`: the root page, whose categories take the players
    /// and teams as they are now.
    pub fn new(players: Vec<Member>, teams: Vec<Team>) -> Self {
        Self {
            players: players_page(players),
            teams,
            category: Category::Root,
            selected: -1,
            page: 0,
        }
    }
    /// `SpectatorMenuCategory.getItems`.
    fn category_items(&self) -> Vec<Item> {
        match &self.category {
            Category::Root => vec![
                Item::TeleportToPlayer {
                    enabled: !self.players.is_empty(),
                },
                Item::TeleportToTeam {
                    enabled: !self.teams.is_empty(),
                },
            ],
            Category::Players => self.players.iter().cloned().map(Item::Player).collect(),
            Category::Teams => self.teams.iter().cloned().map(Item::Team).collect(),
            Category::TeamPlayers(list) => list.iter().cloned().map(Item::Player).collect(),
        }
    }
    /// `SpectatorMenuCategory.getPrompt`.
    fn prompt(&self) -> Text {
        Text::translate(
            match self.category {
                Category::Root => "spectatorMenu.root.prompt",
                Category::Teams => "spectatorMenu.team_teleport.prompt",
                _ => "spectatorMenu.teleport.prompt",
            },
            &[],
        )
    }
    /// `SpectatorMenu.getItem`: six items a page between the scroll arrows.
    pub fn item(&self, slot: i32) -> Item {
        let items = self.category_items();
        let index = slot + self.page * 6;
        if self.page > 0 && slot == 0 {
            Item::Scroll {
                left: true,
                enabled: true,
            }
        } else if slot == 7 {
            Item::Scroll {
                left: false,
                enabled: index >= 0 && (index as usize) < items.len(),
            }
        } else if slot == 8 {
            Item::Close
        } else if index >= 0 && (index as usize) < items.len() {
            items[index as usize].clone()
        } else {
            Item::Empty
        }
    }
    pub fn selected_slot(&self) -> i32 {
        self.selected
    }
    /// `SpectatorMenu.selectSlot`: a first press selects, a second uses it.
    pub fn select_slot(&mut self, slot: i32) -> Outcome {
        let item = self.item(slot);
        if item == Item::Empty {
            return Outcome::None;
        }
        if self.selected != slot || !item.enabled() {
            self.selected = slot;
            return Outcome::None;
        }
        match item {
            Item::Close => return Outcome::Exit,
            Item::Scroll { left, .. } => self.page += if left { -1 } else { 1 },
            Item::TeleportToPlayer { .. } => self.select_category(Category::Players),
            Item::TeleportToTeam { .. } => self.select_category(Category::Teams),
            Item::Team(t) => self.select_category(Category::TeamPlayers(players_page(t.players))),
            Item::Player(m) => return Outcome::Teleport(m.uuid),
            Item::Empty => {}
        }
        Outcome::None
    }
    /// `SpectatorMenu.selectCategory`.
    fn select_category(&mut self, category: Category) {
        self.category = category;
        self.selected = -1;
        self.page = 0;
    }
}

/// `SpectatorGui`: the menu while it is up and when it was last used.
#[derive(Default)]
pub struct SpectatorGui {
    menu: Option<SpectatorMenu>,
    last_selection: i64,
}
impl SpectatorGui {
    pub fn is_menu_active(&self) -> bool {
        self.menu.is_some()
    }
    fn apply(&mut self, outcome: Outcome) -> Outcome {
        if outcome == Outcome::Exit {
            // onSpectatorMenuClosed.
            self.menu = None;
            self.last_selection = 0;
        }
        outcome
    }
    /// `SpectatorGui.onHotbarSelected`: a number key.
    /// `fresh` is the menu to open when none is up.
    pub fn on_hotbar_selected(
        &mut self,
        slot: i32,
        now: i64,
        fresh: Option<SpectatorMenu>,
    ) -> Outcome {
        self.last_selection = now;
        match &mut self.menu {
            Some(menu) => {
                let outcome = menu.select_slot(slot);
                self.apply(outcome)
            }
            None => {
                self.menu = fresh;
                Outcome::None
            }
        }
    }
    /// `SpectatorGui.onHotbarActionKeyPressed`: the key that opens the menu
    /// and uses the selected item.
    pub fn on_action_key(&mut self, now: i64, fresh: Option<SpectatorMenu>) -> Outcome {
        self.last_selection = now;
        match &mut self.menu {
            Some(menu) => {
                let slot = menu.selected_slot();
                if slot == -1 {
                    return Outcome::None;
                }
                let outcome = menu.select_slot(slot);
                self.apply(outcome)
            }
            None => {
                self.menu = fresh;
                Outcome::None
            }
        }
    }
    /// `SpectatorGui.onMouseScrolled`: the next usable slot that way.
    pub fn on_mouse_scrolled(&mut self, wheel: i32, now: i64) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let usable = |menu: &SpectatorMenu, slot: i32| {
            let item = menu.item(slot);
            item != Item::Empty && item.enabled()
        };
        let mut slot = menu.selected_slot() + wheel;
        while (0..=8).contains(&slot) && !usable(menu, slot) {
            slot += wheel;
        }
        if (0..=8).contains(&slot) {
            menu.select_slot(slot);
            self.last_selection = now;
        }
    }
    /// `SpectatorGui.getHotbarAlpha`.
    fn alpha(&self, now: i64) -> f32 {
        let delta = self.last_selection - now + FADE_OUT_DELAY;
        (delta as f32 / FADE_OUT_TIME).clamp(0.0, 1.0)
    }
    /// `SpectatorGui.extractHotbar`: the page's nine slots; a menu that has
    /// faded out exits instead. `key_label` is the hotbar key of a slot.
    pub fn hotbar(
        &mut self,
        w: i32,
        h: i32,
        now: i64,
        key_label: &dyn Fn(usize) -> Text,
        font: &Font,
    ) -> Vec<GuiDraw> {
        let mut out = Vec::new();
        let alpha = self.alpha(now);
        if self.menu.is_some() && alpha <= 0.0 {
            self.apply(Outcome::Exit);
        }
        let Some(menu) = &self.menu else {
            return out;
        };
        let center = w / 2;
        let y = (h as f32 - 22.0 * alpha).floor() as i32;
        let color = white(alpha);
        out.push(sprite("hud/hotbar", center - 91, y, 182, 22, color));
        if menu.selected_slot() >= 0 {
            out.push(sprite(
                "hud/hotbar_selection",
                center - 91 - 1 + menu.selected_slot() * 20,
                y - 1,
                24,
                23,
                color,
            ));
        }
        for slot in 0..9 {
            let item = menu.item(slot);
            if item == Item::Empty {
                continue;
            }
            let x = center - 90 + slot * 20 + 2;
            let brightness = if item.enabled() { 1.0 } else { 0.25 };
            icon(&mut out, &item, x, y + 3, brightness, alpha);
            if item.enabled() {
                let label = key_label(slot as usize);
                let lw = width(font, &label);
                out.push(GuiDraw::Text {
                    text: label,
                    x: x + 19 - 2 - lw,
                    y: y + 3 + 6 + 3,
                    color,
                    shadow: true,
                });
            }
        }
        out
    }
    /// `SpectatorGui.extractAction`: the selected item's name, or the
    /// page's prompt, over the hotbar.
    pub fn action(&self, w: i32, h: i32, now: i64, font: &Font, background: u32) -> Vec<GuiDraw> {
        let mut out = Vec::new();
        let alpha = self.alpha(now);
        let Some(menu) = &self.menu else {
            return out;
        };
        if alpha <= 0.0 {
            return out;
        }
        let item = menu.item(menu.selected_slot());
        let action = if item == Item::Empty {
            menu.prompt()
        } else {
            item.name()
        };
        let str_width = width(font, &action);
        text_with_backdrop(
            &mut out,
            action,
            (w - str_width) / 2,
            h - 35,
            str_width,
            white(alpha),
            background,
        );
        out
    }
}

fn sprite(name: &'static str, x: i32, y: i32, w: i32, h: i32, color: u32) -> GuiDraw {
    GuiDraw::Sprite {
        sprite: Cow::Borrowed(name),
        x,
        y,
        w,
        h,
        region: None,
        color,
        blend: Blend::Textured,
    }
}

/// `SpectatorMenuItem.extractIcon` at the slot's corner.
fn icon(out: &mut Vec<GuiDraw>, item: &Item, x: i32, y: i32, brightness: f32, alpha: f32) {
    let tint = color_from_float(alpha, brightness, brightness, brightness);
    match item {
        Item::Empty => {}
        Item::Close => out.push(sprite("spectator/close", x, y, 16, 16, tint)),
        Item::Scroll { left, .. } => out.push(sprite(
            if *left {
                "spectator/scroll_left"
            } else {
                "spectator/scroll_right"
            },
            x,
            y,
            16,
            16,
            tint,
        )),
        Item::TeleportToPlayer { .. } => {
            out.push(sprite("spectator/teleport_to_player", x, y, 16, 16, tint));
        }
        Item::TeleportToTeam { .. } => {
            out.push(sprite("spectator/teleport_to_team", x, y, 16, 16, tint));
        }
        Item::Player(m) => crate::tab_list::face(out, &m.skin, x + 2, y + 2, 12, true, false, tint),
        Item::Team(t) => {
            if let Some(rgb) = t.color {
                // ARGB.scaleRGB.
                let c = |s: u32| (((rgb >> s) & 255) as f32 * brightness) as u32;
                out.push(fill(
                    x + 1,
                    y + 1,
                    x + 15,
                    y + 15,
                    argb(255, c(16), c(8), c(0)),
                ));
            }
            crate::tab_list::face(out, &t.icon, x + 2, y + 2, 12, true, false, tint);
        }
    }
}
