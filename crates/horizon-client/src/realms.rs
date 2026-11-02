//! Minecraft Realms screen and logic.
pub mod client;

use crate::hud::{HudAtlas, HudBatch, rgba};
use client::{RealmAddress, RealmServer, RealmsClient};
use std::sync::{Arc, Mutex};
use std::thread;

const HEADER: f32 = 33.0;
const FOOTER: f32 = 60.0;
const ROW_HEIGHT: f32 = 36.0;
const ROW_WIDTH: f32 = 308.0;

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

    pub fn click(&mut self, hit: Option<u8>, x: f32, y: f32, (w, h): (f32, f32)) -> Outcome {
        match hit {
            Some(1) => {
                // Play / Connect to Realm
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
            Some(2) => Outcome::Back,
            _ => {
                // Clicking on a realm list row
                let (top, bottom) = (HEADER, h - FOOTER);
                if y >= top && y <= bottom {
                    let rx = ((w - ROW_WIDTH) / 2.0).floor();
                    if x >= rx && x <= rx + ROW_WIDTH {
                        let row = ((y - top - 2.0) / ROW_HEIGHT).floor() as usize;
                        if let Ok(lock) = self.state.lock()
                            && let FetchState::Loaded(servers) = &*lock
                            && row < servers.len()
                        {
                            self.selected = Some(row);
                        }
                    }
                }
                Outcome::None
            }
        }
    }

    pub fn buttons(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, String, bool, u8)> {
        let cx = (w / 2.0).floor();
        let selected_can_join = self.selected.is_some()
            && self.state.lock().ok().is_some_and(|lock| {
                if let FetchState::Loaded(ref list) = *lock {
                    self.selected
                        .and_then(|i| list.get(i))
                        .is_some_and(|s| s.is_open())
                } else {
                    false
                }
            });

        // Exact vanilla footer layout: Play (100) and Back (100), centered
        vec![
            (
                cx - 104.0,
                h - 52.0,
                100.0,
                crate::lang::get_or("mco.selectServer.play", "Play"),
                selected_can_join,
                1,
            ),
            (
                cx + 4.0,
                h - 52.0,
                100.0,
                crate::lang::get_or("gui.back", "Back"),
                true,
                2,
            ),
        ]
    }

    pub fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas, w: f32, h: f32) {
        let (w, h) = (w.ceil(), h.ceil());
        let cx = (w / 2.0).floor();

        // 1. Classic Minecraft tiled dirt menu background
        crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);

        // 2. Official Header title centered in the 33px header
        let title = crate::lang::get_or("mco.title", "Minecraft Realms");
        let tw = HudBatch::text_width(atlas, &title);
        let tx = ((w - tw.ceil()) / 2.0).floor();
        batch.text(
            atlas,
            &title,
            tx,
            ((HEADER - 9.0) / 2.0).floor(),
            u32::MAX,
            true,
        );

        // 3. Selection list background with standard separators
        let (top, bottom) = (HEADER, h - FOOTER);
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
                batch.text(
                    atlas,
                    &msg,
                    cx - mw / 2.0,
                    (h / 2.0).floor() - 10.0,
                    rgba(160, 160, 160, 255),
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
                    let msg = crate::lang::get_or(
                        "mco.selectServer.noRealms",
                        "You don't seem to have a Realm. Add a Realm to play together with your friends.",
                    );
                    let mw = HudBatch::text_width(atlas, &msg);
                    if mw < w - 40.0 {
                        batch.text(
                            atlas,
                            &msg,
                            cx - mw / 2.0,
                            (h / 2.0).floor() - 10.0,
                            rgba(160, 160, 160, 255),
                            true,
                        );
                    } else {
                        let line1 = "You don't seem to have a Realm.";
                        let line2 = "Add a Realm to play together with your friends.";
                        let w1 = HudBatch::text_width(atlas, line1);
                        let w2 = HudBatch::text_width(atlas, line2);
                        batch.text(
                            atlas,
                            line1,
                            cx - w1 / 2.0,
                            (h / 2.0).floor() - 15.0,
                            rgba(160, 160, 160, 255),
                            true,
                        );
                        batch.text(
                            atlas,
                            line2,
                            cx - w2 / 2.0,
                            (h / 2.0).floor() + 2.0,
                            rgba(160, 160, 160, 255),
                            true,
                        );
                    }
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

                        // 32x32 server icon
                        batch.sprite_scaled(
                            atlas,
                            "misc/unknown_server",
                            rx + 2.0,
                            y + 2.0,
                            32.0,
                            32.0,
                            u32::MAX,
                        );

                        // Server name (White with shadow)
                        let name_color = if server.is_open() {
                            u32::MAX
                        } else {
                            rgba(160, 160, 160, 255)
                        };
                        batch.text(atlas, &server.name, rx + 38.0, y + 3.0, name_color, true);

                        // State & MOTD
                        let motd = if !server.motd.is_empty() {
                            &server.motd
                        } else {
                            "Minecraft Realm"
                        };
                        batch.text(
                            atlas,
                            motd,
                            rx + 38.0,
                            y + 14.0,
                            rgba(128, 128, 128, 255),
                            true,
                        );

                        // Owner / Info
                        let owner = server.owner.as_deref().unwrap_or("");
                        batch.text(
                            atlas,
                            owner,
                            rx + 38.0,
                            y + 24.0,
                            rgba(160, 160, 160, 255),
                            true,
                        );

                        // Status badge (ONLINE / EXPIRED)
                        let (status_str, status_col) = if server.is_open() {
                            ("ONLINE", rgba(85, 255, 85, 255))
                        } else {
                            ("EXPIRED", rgba(255, 85, 85, 255))
                        };
                        batch.text(
                            atlas,
                            status_str,
                            rx + ROW_WIDTH - 52.0,
                            y + 3.0,
                            status_col,
                            true,
                        );
                    }
                }
            }
        }
    }
}
