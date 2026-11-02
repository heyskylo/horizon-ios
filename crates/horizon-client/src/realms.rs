//! Minecraft Realms screen and logic.
//! Directly ported from vanilla `com.mojang.realmsclient.RealmsMainScreen` and `RealmsHeader`.
pub mod client;

use crate::hud::{HudAtlas, HudBatch, rgba};
use client::{RealmAddress, RealmServer, RealmsClient};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const HEADER_HEIGHT: f32 = 44.0;
const CONTENT_WIDTH: f32 = 308.0;
const ROW_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutState {
    Loading,
    NoRealms,
    List,
}

#[derive(Clone, Debug)]
pub enum FetchState {
    Unauthenticated,
    Loading,
    Loaded(Vec<RealmServer>),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Screen {
    pub state: Arc<Mutex<FetchState>>,
    pub selected: Option<usize>,
    pub joining: Arc<Mutex<Option<Result<RealmAddress, String>>>>,
    pub last_click: Option<(usize, SystemTime)>,
}

fn open_link(url: &str) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Ok(mut child) = std::process::Command::new(program)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

pub enum Outcome {
    None,
    Back,
    Connect(String),
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(FetchState::Loading));
        let joining = Arc::new(Mutex::new(None));

        if let Some(session) = horizon_client_render::auth::current() {
            let state_clone = Arc::clone(&state);
            let name = session.name.clone();
            let uuid = format!("{:032x}", session.uuid);
            let token = session.access_token().to_owned();

            thread::spawn(move || {
                let client = RealmsClient::default_client(&name, &uuid, &token);
                let result = match client.list_worlds() {
                    Ok(worlds) => FetchState::Loaded(worlds),
                    Err(e) => FetchState::Error(e),
                };
                if let Ok(mut lock) = state_clone.lock() {
                    *lock = result;
                }
            });
        } else if let Ok(mut lock) = state.lock() {
            *lock = FetchState::Unauthenticated;
        }

        Self {
            state,
            selected: None,
            joining,
            last_click: None,
        }
    }

    pub fn layout_state(&self) -> LayoutState {
        if let Ok(lock) = self.state.lock() {
            match &*lock {
                FetchState::Loading => LayoutState::Loading,
                FetchState::Loaded(servers) if !servers.is_empty() => LayoutState::List,
                _ => LayoutState::NoRealms,
            }
        } else {
            LayoutState::Loading
        }
    }

    pub fn refresh(&mut self) {
        self.selected = None;
        if let Some(session) = horizon_client_render::auth::current() {
            if let Ok(mut lock) = self.state.lock() {
                *lock = FetchState::Loading;
            }
            let state_clone = Arc::clone(&self.state);
            let name = session.name.clone();
            let uuid = format!("{:032x}", session.uuid);
            let token = session.access_token().to_owned();

            thread::spawn(move || {
                let client = RealmsClient::default_client(&name, &uuid, &token);
                let result = match client.list_worlds() {
                    Ok(worlds) => FetchState::Loaded(worlds),
                    Err(e) => FetchState::Error(e),
                };
                if let Ok(mut lock) = state_clone.lock() {
                    *lock = result;
                }
            });
        }
    }

    fn play_selected(&mut self) -> Outcome {
        let target = if let Some(idx) = self.selected
            && let Ok(lock) = self.state.lock()
            && let FetchState::Loaded(servers) = &*lock
            && let Some(server) = servers.get(idx)
            && let Some(session) = horizon_client_render::auth::current()
        {
            Some((
                server.id,
                session.name.clone(),
                format!("{:032x}", session.uuid),
                session.access_token().to_owned(),
            ))
        } else {
            None
        };

        if let Some((realm_id, name, uuid, token)) = target {
            let client = RealmsClient::default_client(&name, &uuid, &token);
            match client.join_world(realm_id) {
                Ok(addr) => return Outcome::Connect(addr.address),
                Err(e) => {
                    if let Ok(mut lock) = self.state.lock() {
                        *lock = FetchState::Error(e);
                    }
                }
            }
        }
        Outcome::None
    }

    pub fn click(&mut self, hit: Option<u8>, x: f32, y: f32, (w, h): (f32, f32)) -> Outcome {
        match hit {
            Some(1) => self.play_selected(),
            Some(2) => Outcome::Back,
            Some(11) => {
                // Renew Realm
                open_link("https://www.minecraft.net/realms");
                Outcome::None
            }
            Some(13) => {
                // Add Realm / Free Trial
                open_link("https://www.minecraft.net/realms");
                Outcome::None
            }
            Some(15) | Some(16) => {
                // Pending invites / news
                Outcome::None
            }
            _ => {
                // Clicking in list area
                let footer_h = if self.layout_state() == LayoutState::List {
                    60.0
                } else {
                    48.0
                };
                let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
                if y >= top && y <= bottom {
                    let rx = ((w - ROW_WIDTH) / 2.0).floor();
                    if x >= rx && x <= rx + ROW_WIDTH {
                        let row = ((y - top - 2.0) / ROW_HEIGHT).floor() as usize;
                        let (is_valid_row, can_play) = if let Ok(lock) = self.state.lock()
                            && let FetchState::Loaded(servers) = &*lock
                            && row < servers.len()
                        {
                            (true, servers[row].is_open())
                        } else {
                            (false, false)
                        };

                        if is_valid_row {
                            let now = SystemTime::now();
                            let is_double_click = match self.last_click {
                                Some((last_row, last_time)) if last_row == row => now
                                    .duration_since(last_time)
                                    .is_ok_and(|d| d.as_millis() < 350),
                                _ => false,
                            };
                            self.selected = Some(row);
                            self.last_click = Some((row, now));

                            if is_double_click && can_play {
                                return self.play_selected();
                            }
                        }
                    }
                }
                Outcome::None
            }
        }
    }

    pub fn buttons(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, String, bool, u8)> {
        let cx = (w / 2.0).floor();
        let left = cx - (CONTENT_WIDTH / 2.0).floor();

        let mut buttons = Vec::new();

        // RealmsHeader buttons:
        // Left: Join Realm button (88px)
        buttons.push((
            left,
            12.0,
            88.0,
            crate::lang::get_or("mco.selectServer.joinRealm", "Join Realm"),
            true,
            14,
        ));

        // Right: Invites (20px) and News (20px)
        buttons.push((
            left + CONTENT_WIDTH - 44.0,
            12.0,
            20.0,
            "".to_string(),
            true,
            15,
        ));
        buttons.push((
            left + CONTENT_WIDTH - 20.0,
            12.0,
            20.0,
            "".to_string(),
            true,
            16,
        ));

        // Footer buttons (vanilla RealmsMainScreen.createFooter):
        match self.layout_state() {
            LayoutState::List => {
                let selected_server = self.selected.and_then(|idx| {
                    if let Ok(lock) = self.state.lock()
                        && let FetchState::Loaded(ref list) = *lock
                    {
                        list.get(idx).cloned()
                    } else {
                        None
                    }
                });

                let selected_can_join = selected_server.as_ref().is_some_and(|s| s.is_open());
                let is_self_owned = selected_server.as_ref().is_some_and(|s| {
                    horizon_client_render::auth::current()
                        .is_some_and(|u| s.owner.as_deref() == Some(&u.name))
                });
                let is_expired = selected_server.as_ref().is_some_and(|s| s.expired);

                // Row 1: Play (100) | Configure (100) | Renew (100)
                buttons.push((
                    left,
                    h - 52.0,
                    100.0,
                    crate::lang::get_or("mco.selectServer.play", "Play"),
                    selected_can_join,
                    1,
                ));
                buttons.push((
                    left + 104.0,
                    h - 52.0,
                    100.0,
                    crate::lang::get_or("mco.selectServer.configure", "Configure"),
                    is_self_owned,
                    10,
                ));
                buttons.push((
                    left + 208.0,
                    h - 52.0,
                    100.0,
                    crate::lang::get_or("mco.selectServer.expiredRenew", "Renew"),
                    is_self_owned && is_expired,
                    11,
                ));

                // Row 2: Leave (100) | Add Realm (100) | Back (100)
                buttons.push((
                    left,
                    h - 28.0,
                    100.0,
                    crate::lang::get_or("mco.selectServer.leave", "Leave Realm"),
                    selected_server.is_some() && !is_self_owned,
                    12,
                ));
                buttons.push((
                    left + 104.0,
                    h - 28.0,
                    100.0,
                    crate::lang::get_or("mco.selectServer.purchase", "Add Realm"),
                    true,
                    13,
                ));
                buttons.push((
                    left + 208.0,
                    h - 28.0,
                    100.0,
                    crate::lang::get_or("gui.back", "Back"),
                    true,
                    2,
                ));
            }
            LayoutState::NoRealms | LayoutState::Loading => {
                // Vanilla footer for NO_REALMS: Add Realm (100) and Back (100) centered
                buttons.push((
                    cx - 102.0,
                    h - 36.0,
                    100.0,
                    crate::lang::get_or("mco.selectServer.purchase", "Add Realm"),
                    true,
                    13,
                ));
                buttons.push((
                    cx + 2.0,
                    h - 36.0,
                    100.0,
                    crate::lang::get_or("gui.back", "Back"),
                    true,
                    2,
                ));
            }
        }

        buttons
    }

    pub fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas, w: f32, h: f32) {
        let (w, h) = (w.ceil(), h.ceil());
        let cx = (w / 2.0).floor();
        let left = cx - (CONTENT_WIDTH / 2.0).floor();

        // 1. Classic Minecraft tiled dirt menu background
        crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);

        // 2. Official Realms Header:
        // Center: Realms Logo texture (128 x 34 px from textures/gui/title/realms.png)
        if atlas.sprites.contains_key("gui/title/realms") {
            batch.sprite_region(
                atlas,
                "gui/title/realms",
                [cx - 64.0, 5.0, 128.0, 34.0],
                128.0,
                64.0,
            );
        } else {
            // High-fidelity fallback header text if sprite isn't unpacked
            let title = crate::lang::get_or("mco.title", "Minecraft Realms");
            let tw = HudBatch::text_width(atlas, &title);
            batch.text(
                atlas,
                &title,
                cx - (tw / 2.0).floor(),
                ((HEADER_HEIGHT - 9.0) / 2.0).floor(),
                u32::MAX,
                true,
            );
        }

        // Header icons: Invites (envelope) & News
        let inv_x = left + CONTENT_WIDTH - 44.0 + 3.0;
        let news_x = left + CONTENT_WIDTH - 20.0 + 3.0;
        if atlas.sprites.contains_key("gui/sprites/icon/invite") {
            batch.sprite_scaled(
                atlas,
                "gui/sprites/icon/invite",
                inv_x,
                15.0,
                14.0,
                14.0,
                u32::MAX,
            );
        }
        if atlas.sprites.contains_key("gui/sprites/icon/news") {
            batch.sprite_scaled(
                atlas,
                "gui/sprites/icon/news",
                news_x,
                15.0,
                14.0,
                14.0,
                u32::MAX,
            );
        }

        // 3. Selection list background with standard separators
        let footer_h = if self.layout_state() == LayoutState::List {
            60.0
        } else {
            48.0
        };
        let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
        crate::ui::list_background(batch, atlas, top, bottom, false);

        let rx = ((w - ROW_WIDTH) / 2.0).floor();

        let lock = match self.state.lock() {
            Ok(l) => l,
            Err(_) => return,
        };

        match &*lock {
            FetchState::Unauthenticated => {
                let msg =
                    "Please sign in with a Microsoft account in Horizon Launcher to play Realms.";
                let mw = HudBatch::text_width(atlas, msg);
                batch.text(
                    atlas,
                    msg,
                    cx - mw / 2.0,
                    (h / 2.0).floor() - 10.0,
                    rgba(160, 160, 160, 255),
                    true,
                );
            }
            FetchState::Loading => {
                let msg = crate::lang::get_or("mco.selectServer.loading", "Loading Realms List");
                let mw = HudBatch::text_width(atlas, &msg);
                let mid_y = (h / 2.0).floor();
                batch.text(atlas, &msg, cx - mw / 2.0, mid_y - 12.0, u32::MAX, true);

                // Vanilla animated dots under loading message
                let millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis());
                let dots = ["O o o", "o O o", "o o O", "o O o"][(millis / 300 % 4) as usize];
                let dw = HudBatch::text_width(atlas, dots);
                batch.text(
                    atlas,
                    dots,
                    cx - dw / 2.0,
                    mid_y + 4.0,
                    rgba(128, 128, 128, 255),
                    true,
                );
            }
            FetchState::Error(err) => {
                let msg = format!("Error: {err}");
                let mw = HudBatch::text_width(atlas, &msg);
                batch.text(
                    atlas,
                    &msg,
                    cx - mw / 2.0,
                    (h / 2.0).floor() - 10.0,
                    rgba(255, 85, 85, 255),
                    true,
                );
            }
            FetchState::Loaded(servers) => {
                if servers.is_empty() {
                    // Vanilla createNoRealmsContent layout:
                    // 1. Texture gui/realms/no_realms.png (130 x 64 px)
                    let img_y = (h / 2.0).floor() - 54.0;
                    if atlas.sprites.contains_key("gui/realms/no_realms") {
                        batch.sprite_scaled(
                            atlas,
                            "gui/realms/no_realms",
                            cx - 65.0,
                            img_y,
                            130.0,
                            64.0,
                            u32::MAX,
                        );
                    }

                    // 2. Focused text message under the illustration
                    let line1 = "You don't seem to have a Realm.";
                    let line2 = "Add a Realm to play together with your friends.";
                    let w1 = HudBatch::text_width(atlas, line1);
                    let w2 = HudBatch::text_width(atlas, line2);
                    batch.text(
                        atlas,
                        line1,
                        cx - (w1 / 2.0).floor(),
                        img_y + 72.0,
                        rgba(160, 160, 160, 255),
                        true,
                    );
                    batch.text(
                        atlas,
                        line2,
                        cx - (w2 / 2.0).floor(),
                        img_y + 84.0,
                        rgba(160, 160, 160, 255),
                        true,
                    );
                } else {
                    for (i, server) in servers.iter().enumerate() {
                        let y = top + 2.0 + i as f32 * ROW_HEIGHT;
                        if y + ROW_HEIGHT > bottom {
                            break;
                        }

                        let is_selected = self.selected == Some(i);
                        if is_selected {
                            batch.fill(atlas, rx - 2.0, y - 2.0, ROW_WIDTH, ROW_HEIGHT, u32::MAX);
                            batch.fill(
                                atlas,
                                rx - 1.0,
                                y - 1.0,
                                ROW_WIDTH - 2.0,
                                ROW_HEIGHT - 2.0,
                                rgba(0, 0, 0, 255),
                            );
                        }

                        // 32x32 realm icon
                        batch.sprite_scaled(
                            atlas,
                            "misc/unknown_server",
                            rx + 2.0,
                            y + 2.0,
                            32.0,
                            32.0,
                            u32::MAX,
                        );

                        // Line 1: Server name (white, bold with shadow)
                        let name_color = if server.is_open() {
                            u32::MAX
                        } else {
                            rgba(160, 160, 160, 255)
                        };
                        batch.text(atlas, &server.name, rx + 38.0, y + 1.0, name_color, true);

                        // Line 2: MOTD / description
                        let motd = if !server.motd.is_empty() {
                            &server.motd
                        } else {
                            "Minecraft Realm"
                        };
                        batch.text(
                            atlas,
                            motd,
                            rx + 38.0,
                            y + 12.0,
                            rgba(128, 128, 128, 255),
                            true,
                        );

                        // Line 3: Owner
                        let owner = server.owner.as_deref().unwrap_or("");
                        batch.text(
                            atlas,
                            owner,
                            rx + 38.0,
                            y + 23.0,
                            rgba(160, 160, 160, 255),
                            true,
                        );

                        // Status light (width 10, height 28) at rx + ROW_WIDTH - 17.0
                        let light_x = rx + ROW_WIDTH - 17.0;
                        let light_sprite = if server.expired {
                            "gui/sprites/realm_status/expired"
                        } else if server.is_open() {
                            "gui/sprites/realm_status/open"
                        } else {
                            "gui/sprites/realm_status/closed"
                        };

                        if atlas.sprites.contains_key(light_sprite) {
                            batch.sprite_scaled(
                                atlas,
                                light_sprite,
                                light_x,
                                y + 2.0,
                                10.0,
                                28.0,
                                u32::MAX,
                            );
                        } else {
                            // High-contrast badge fallback
                            let (badge_text, badge_col) = if server.is_open() {
                                ("ONLINE", rgba(85, 255, 85, 255))
                            } else {
                                ("EXPIRED", rgba(255, 85, 85, 255))
                            };
                            batch.text(
                                atlas,
                                badge_text,
                                rx + ROW_WIDTH - 50.0,
                                y + 2.0,
                                badge_col,
                                true,
                            );
                        }
                    }
                }
            }
        }
    }
}
