//! Minecraft Realms screen and logic.
//! Directly ported from vanilla `com.mojang.realmsclient.RealmsMainScreen` and `RealmsHeader`.
pub mod client;

use crate::hud::{HudAtlas, HudBatch, rgba};
use client::{PendingInviteDto, RealmAddress, RealmServer, RealmsClient};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const HEADER_HEIGHT: f32 = 44.0;
const CONTENT_WIDTH: f32 = 308.0;
const ROW_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 36.0;

const REALMS_INVITE_CODE: crate::clipboard::FieldId = ("realms.invite_code", 0);

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
pub enum SubScreen {
    Main,
    JoinCode {
        code: String,
        error: Option<String>,
        validating: bool,
    },
    PendingInvites {
        invites: Arc<Mutex<Option<Result<Vec<PendingInviteDto>, String>>>>,
    },
    Configure {
        server: RealmServer,
        status_msg: Option<String>,
        busy: bool,
    },
    AddRealm,
}

#[derive(Clone, Debug)]
pub struct Screen {
    pub state: Arc<Mutex<FetchState>>,
    pub subscreen: SubScreen,
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
            subscreen: SubScreen::Main,
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

    pub fn focused_box(&self, (w, _h): (f32, f32)) -> Option<[f32; 4]> {
        if let SubScreen::JoinCode { .. } = self.subscreen {
            Some([(w / 2.0).floor() - 100.0, 64.0, 200.0, 20.0])
        } else {
            None
        }
    }

    pub fn focused_text(&mut self) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
        if let SubScreen::JoinCode { ref mut code, .. } = self.subscreen {
            Some((REALMS_INVITE_CODE, code, 128))
        } else {
            None
        }
    }

    pub fn tab(&mut self) {}

    pub fn escape(&mut self) -> bool {
        if !matches!(self.subscreen, SubScreen::Main) {
            self.subscreen = SubScreen::Main;
            false
        } else {
            true
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
            Some(10) => {
                // Configure Realm: open in-game configure subscreen
                if let Some(idx) = self.selected
                    && let Ok(lock) = self.state.lock()
                    && let FetchState::Loaded(ref list) = *lock
                    && let Some(server) = list.get(idx)
                {
                    self.subscreen = SubScreen::Configure {
                        server: server.clone(),
                        status_msg: None,
                        busy: false,
                    };
                }
                Outcome::None
            }
            Some(11) => {
                // Renew Realm
                open_link("https://www.minecraft.net/realms");
                Outcome::None
            }
            Some(12) => {
                // Leave Realm (uninvite myself)
                if let Some(idx) = self.selected {
                    let server_id = if let Ok(lock) = self.state.lock()
                        && let FetchState::Loaded(ref list) = *lock
                        && let Some(server) = list.get(idx)
                    {
                        Some(server.id)
                    } else {
                        None
                    };

                    if let Some(id) = server_id
                        && let Some(session) = horizon_client_render::auth::current()
                    {
                        let name = session.name.clone();
                        let uuid = format!("{:032x}", session.uuid);
                        let token = session.access_token().to_owned();
                        thread::spawn(move || {
                            let client = RealmsClient::default_client(&name, &uuid, &token);
                            let _ = client.uninvite_myself(id);
                        });
                        self.selected = None;
                        self.refresh();
                    }
                }
                Outcome::None
            }
            Some(13) => {
                // Add Realm: open in-game info & purchase subscreen
                self.subscreen = SubScreen::AddRealm;
                Outcome::None
            }
            Some(14) => {
                // Join Realm with Code button in header
                self.subscreen = SubScreen::JoinCode {
                    code: String::new(),
                    error: None,
                    validating: false,
                };
                Outcome::None
            }
            Some(15) => {
                // Pending invites header button
                let invites_arc = Arc::new(Mutex::new(None));
                let arc_clone = Arc::clone(&invites_arc);
                if let Some(session) = horizon_client_render::auth::current() {
                    let name = session.name.clone();
                    let uuid = format!("{:032x}", session.uuid);
                    let token = session.access_token().to_owned();
                    thread::spawn(move || {
                        let client = RealmsClient::default_client(&name, &uuid, &token);
                        let res = client.pending_invites();
                        if let Ok(mut lock) = arc_clone.lock() {
                            *lock = Some(res);
                        }
                    });
                }
                self.subscreen = SubScreen::PendingInvites {
                    invites: invites_arc,
                };
                Outcome::None
            }
            Some(16) => {
                // Realms News header button
                open_link("https://www.minecraft.net/realms");
                Outcome::None
            }
            Some(17) => {
                // Retry / Refresh
                self.refresh();
                Outcome::None
            }
            Some(20) => {
                // Join button inside JoinCode subscreen
                if let SubScreen::JoinCode {
                    ref code,
                    ref mut error,
                    ref mut validating,
                } = self.subscreen
                {
                    let trimmed = code.trim().to_string();
                    if !trimmed.is_empty() {
                        *validating = true;
                        *error = None;
                        if let Some(session) = horizon_client_render::auth::current() {
                            let name = session.name.clone();
                            let uuid = format!("{:032x}", session.uuid);
                            let token = session.access_token().to_owned();
                            let client = RealmsClient::default_client(&name, &uuid, &token);
                            match client.redeem_invite_code(&trimmed) {
                                Ok(()) => {
                                    self.subscreen = SubScreen::Main;
                                    self.refresh();
                                    return Outcome::None;
                                }
                                Err(e) => {
                                    *error = Some(e);
                                    *validating = false;
                                }
                            }
                        } else {
                            *error = Some("Not signed in".to_string());
                            *validating = false;
                        }
                    }
                }
                Outcome::None
            }
            Some(21) | Some(22) | Some(32) | Some(41) => {
                // Cancel/Back or Done in subscreens
                self.subscreen = SubScreen::Main;
                Outcome::None
            }
            Some(30) => {
                // Toggle Realm open / close in Configure subscreen
                let action_info = if let SubScreen::Configure {
                    ref server, busy, ..
                } = self.subscreen
                {
                    if !busy {
                        Some((server.id, server.is_open()))
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some((s_id, is_open)) = action_info {
                    if let SubScreen::Configure { ref mut busy, .. } = self.subscreen {
                        *busy = true;
                    }
                    let res = if let Some(session) = horizon_client_render::auth::current() {
                        let name = session.name.clone();
                        let uuid = format!("{:032x}", session.uuid);
                        let token = session.access_token().to_owned();
                        let client = RealmsClient::default_client(&name, &uuid, &token);
                        if is_open {
                            client.close_realm(s_id)
                        } else {
                            client.open_realm(s_id)
                        }
                    } else {
                        Err("Not signed in".to_string())
                    };

                    match res {
                        Ok(()) => {
                            if let SubScreen::Configure {
                                ref mut server,
                                ref mut status_msg,
                                ref mut busy,
                            } = self.subscreen
                            {
                                server.state = if is_open {
                                    "CLOSED".to_string()
                                } else {
                                    "OPEN".to_string()
                                };
                                *status_msg = Some(if is_open {
                                    "Realm został pomyślnie zamknięty.".to_string()
                                } else {
                                    "Realm został pomyślnie otwarty.".to_string()
                                });
                                *busy = false;
                            }
                            self.refresh();
                        }
                        Err(e) => {
                            if let SubScreen::Configure {
                                ref mut status_msg,
                                ref mut busy,
                                ..
                            } = self.subscreen
                            {
                                *status_msg = Some(format!("Błąd: {e}"));
                                *busy = false;
                            }
                        }
                    }
                }
                Outcome::None
            }
            Some(31) | Some(40) => {
                // Open Minecraft Realms web portal
                open_link("https://www.minecraft.net/realms");
                Outcome::None
            }
            Some(btn @ 100..=200) => {
                // Accept or Reject in PendingInvites
                let invites_arc = if let SubScreen::PendingInvites { ref invites } = self.subscreen
                {
                    Some(Arc::clone(invites))
                } else {
                    None
                };

                if let Some(invites) = invites_arc {
                    let idx = ((btn - 100) / 2) as usize;
                    let is_accept = (btn - 100) % 2 == 0;
                    let invite_id = if let Ok(lock) = invites.lock()
                        && let Some(Ok(ref list)) = *lock
                        && let Some(item) = list.get(idx)
                    {
                        Some(item.invitation_id.clone())
                    } else {
                        None
                    };

                    if let Some(inv_id) = invite_id
                        && let Some(session) = horizon_client_render::auth::current()
                    {
                        let name = session.name.clone();
                        let uuid = format!("{:032x}", session.uuid);
                        let token = session.access_token().to_owned();
                        let client = RealmsClient::default_client(&name, &uuid, &token);
                        if is_accept {
                            let _ = client.accept_invite(&inv_id);
                            self.refresh();
                        } else {
                            let _ = client.reject_invite(&inv_id);
                        }
                        if let Ok(res) = client.pending_invites()
                            && let Ok(mut lock) = invites.lock()
                        {
                            *lock = Some(Ok(res));
                        }
                    }
                }
                Outcome::None
            }
            _ => {
                if !matches!(self.subscreen, SubScreen::Main) {
                    return Outcome::None;
                }
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

    pub fn buttons(
        &self,
        w: f32,
        h: f32,
    ) -> Vec<(f32, f32, f32, String, bool, u8, Option<(&'static str, f32)>)> {
        let cx = (w / 2.0).floor();
        let left = cx - (CONTENT_WIDTH / 2.0).floor();
        let mut buttons = Vec::new();

        match &self.subscreen {
            SubScreen::JoinCode {
                code, validating, ..
            } => {
                buttons.push((
                    cx - 100.0,
                    (h / 2.0).floor() + 10.0,
                    200.0,
                    crate::lang::get_or("mco.selectServer.joinCode.select", "Join"),
                    !code.trim().is_empty() && !validating,
                    20,
                    None,
                ));
                buttons.push((
                    cx - 100.0,
                    (h / 2.0).floor() + 34.0,
                    200.0,
                    crate::lang::get_or("gui.cancel", "Cancel"),
                    true,
                    21,
                    None,
                ));
            }
            SubScreen::PendingInvites { invites } => {
                buttons.push((
                    cx - 100.0,
                    h - 36.0,
                    200.0,
                    crate::lang::get_or("gui.done", "Done"),
                    true,
                    22,
                    None,
                ));

                if let Ok(lock) = invites.lock()
                    && let Some(Ok(ref list)) = *lock
                {
                    let top = 36.0;
                    let rx = ((w - ROW_WIDTH) / 2.0).floor();
                    for (i, _) in list.iter().enumerate() {
                        let row_y = top + 2.0 + i as f32 * ROW_HEIGHT;
                        if row_y + ROW_HEIGHT > h - 48.0 {
                            break;
                        }
                        if i < 40 {
                            buttons.push((
                                rx + ROW_WIDTH - 46.0,
                                row_y + 8.0,
                                20.0,
                                String::new(),
                                true,
                                100 + i as u8 * 2,
                                Some(("pending_invite/accept", 14.0)),
                            ));
                            buttons.push((
                                rx + ROW_WIDTH - 22.0,
                                row_y + 8.0,
                                20.0,
                                String::new(),
                                true,
                                101 + i as u8 * 2,
                                Some(("pending_invite/reject", 14.0)),
                            ));
                        }
                    }
                }
            }
            SubScreen::Configure { server, busy, .. } => {
                let is_open = server.is_open();
                let toggle_label = if is_open {
                    crate::lang::get_or("mco.configure.world.buttons.close", "Zamknij Realm")
                } else {
                    crate::lang::get_or("mco.configure.world.buttons.open", "Otwórz Realm")
                };
                buttons.push((
                    cx - 100.0,
                    (h / 2.0).floor() + 10.0,
                    200.0,
                    toggle_label,
                    !busy,
                    30,
                    None,
                ));
                buttons.push((
                    cx - 100.0,
                    (h / 2.0).floor() + 34.0,
                    200.0,
                    "Zarządzaj w przeglądarce".to_string(),
                    true,
                    31,
                    None,
                ));
                buttons.push((
                    cx - 100.0,
                    h - 36.0,
                    200.0,
                    crate::lang::get_or("gui.back", "Wstecz"),
                    true,
                    32,
                    None,
                ));
            }
            SubScreen::AddRealm => {
                buttons.push((
                    cx - 110.0,
                    (h / 2.0).floor() + 20.0,
                    220.0,
                    "Kup Realm / Otwórz stronę".to_string(),
                    true,
                    40,
                    None,
                ));
                buttons.push((
                    cx - 100.0,
                    h - 36.0,
                    200.0,
                    crate::lang::get_or("gui.back", "Wstecz"),
                    true,
                    41,
                    None,
                ));
            }
            SubScreen::Main => {
                // RealmsHeader buttons:
                // Left: Join Realm button (comfortably fits localized text)
                let join_text = crate::lang::get_or("mco.selectServer.joinRealm", "Join Realm");
                let join_w = 112.0;
                let join_x = cx - 68.0 - join_w;
                buttons.push((join_x, 12.0, join_w, join_text, true, 14, None));

                // Right: Invites (20px) and News (20px) with icons!
                buttons.push((
                    left + CONTENT_WIDTH - 44.0,
                    12.0,
                    20.0,
                    String::new(),
                    true,
                    15,
                    Some(("icon/invite", 14.0)),
                ));
                buttons.push((
                    left + CONTENT_WIDTH - 20.0,
                    12.0,
                    20.0,
                    String::new(),
                    true,
                    16,
                    Some(("icon/news", 14.0)),
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

                        let selected_can_join =
                            selected_server.as_ref().is_some_and(|s| s.is_open());
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
                            None,
                        ));
                        buttons.push((
                            left + 104.0,
                            h - 52.0,
                            100.0,
                            crate::lang::get_or("mco.selectServer.configure", "Configure"),
                            is_self_owned,
                            10,
                            None,
                        ));
                        buttons.push((
                            left + 208.0,
                            h - 52.0,
                            100.0,
                            crate::lang::get_or("mco.selectServer.expiredRenew", "Renew"),
                            is_self_owned && is_expired,
                            11,
                            None,
                        ));

                        // Row 2: Leave (100) | Add Realm (100) | Back (100)
                        buttons.push((
                            left,
                            h - 28.0,
                            100.0,
                            crate::lang::get_or("mco.selectServer.leave", "Leave Realm"),
                            selected_server.is_some() && !is_self_owned,
                            12,
                            None,
                        ));
                        buttons.push((
                            left + 104.0,
                            h - 28.0,
                            100.0,
                            crate::lang::get_or("mco.selectServer.purchase", "Add Realm"),
                            true,
                            13,
                            None,
                        ));
                        buttons.push((
                            left + 208.0,
                            h - 28.0,
                            100.0,
                            crate::lang::get_or("gui.back", "Back"),
                            true,
                            2,
                            None,
                        ));
                    }
                    LayoutState::NoRealms | LayoutState::Loading => {
                        let has_error = if let Ok(lock) = self.state.lock() {
                            matches!(&*lock, FetchState::Error(_))
                        } else {
                            false
                        };

                        if has_error {
                            buttons.push((
                                cx - 154.0,
                                h - 36.0,
                                100.0,
                                crate::lang::get_or("mco.selectServer.retry", "Retry"),
                                true,
                                17,
                                None,
                            ));
                            buttons.push((
                                cx - 50.0,
                                h - 36.0,
                                100.0,
                                crate::lang::get_or("mco.selectServer.purchase", "Add Realm"),
                                true,
                                13,
                                None,
                            ));
                            buttons.push((
                                cx + 54.0,
                                h - 36.0,
                                100.0,
                                crate::lang::get_or("gui.back", "Back"),
                                true,
                                2,
                                None,
                            ));
                        } else {
                            buttons.push((
                                cx - 102.0,
                                h - 36.0,
                                100.0,
                                crate::lang::get_or("mco.selectServer.purchase", "Add Realm"),
                                true,
                                13,
                                None,
                            ));
                            buttons.push((
                                cx + 2.0,
                                h - 36.0,
                                100.0,
                                crate::lang::get_or("gui.back", "Back"),
                                true,
                                2,
                                None,
                            ));
                        }
                    }
                }
            }
        }

        buttons
    }

    pub fn draw(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        caret: &mut crate::ui::Caret,
        focused_at: Instant,
        _cursor: (f32, f32),
        w: f32,
        h: f32,
    ) {
        let (w, h) = (w.ceil(), h.ceil());
        let cx = (w / 2.0).floor();
        let _left = cx - (CONTENT_WIDTH / 2.0).floor();

        match &self.subscreen {
            SubScreen::JoinCode {
                code,
                error,
                validating,
            } => {
                crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);
                let title = crate::lang::get_or("mco.selectServer.joinCode.title", "Join Realm");
                let tw = HudBatch::text_width(atlas, &title);
                batch.text(atlas, &title, cx - (tw / 2.0).floor(), 20.0, u32::MAX, true);

                let label = crate::lang::get_or("mco.selectServer.joinCode", "Invite Code");
                batch.text(
                    atlas,
                    &label,
                    cx - 100.0,
                    50.0,
                    rgba(160, 160, 160, 255),
                    true,
                );

                caret.draw(
                    batch,
                    atlas,
                    (REALMS_INVITE_CODE, code, 128),
                    [cx - 100.0, 64.0, 200.0, 20.0],
                    true,
                    focused_at,
                );

                if *validating {
                    let val_txt = crate::lang::get_or(
                        "mco.selectServer.joinCode.validating",
                        "Validating code...",
                    );
                    let vw = HudBatch::text_width(atlas, &val_txt);
                    batch.text(
                        atlas,
                        &val_txt,
                        cx - (vw / 2.0).floor(),
                        88.0,
                        u32::MAX,
                        true,
                    );
                } else if let Some(err) = error {
                    let ew = HudBatch::text_width(atlas, err);
                    batch.text(
                        atlas,
                        err,
                        cx - (ew / 2.0).floor(),
                        88.0,
                        rgba(255, 85, 85, 255),
                        true,
                    );
                }
            }
            SubScreen::PendingInvites { invites } => {
                crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);
                let title = crate::lang::get_or("mco.invites.title", "Pending Invites");
                let tw = HudBatch::text_width(atlas, &title);
                batch.text(atlas, &title, cx - (tw / 2.0).floor(), 14.0, u32::MAX, true);

                let (top, bottom) = (36.0, h - 48.0);
                crate::ui::list_background(batch, atlas, top, bottom, false);

                let lock = invites.lock().ok();
                match lock.as_deref() {
                    None | Some(None) => {
                        let msg = "Loading invites...";
                        let mw = HudBatch::text_width(atlas, msg);
                        batch.text(
                            atlas,
                            msg,
                            cx - (mw / 2.0).floor(),
                            (h / 2.0).floor(),
                            rgba(160, 160, 160, 255),
                            true,
                        );
                    }
                    Some(Some(Err(e))) => {
                        let msg = format!("Error: {e}");
                        let mw = HudBatch::text_width(atlas, &msg);
                        batch.text(
                            atlas,
                            &msg,
                            cx - (mw / 2.0).floor(),
                            (h / 2.0).floor(),
                            rgba(255, 85, 85, 255),
                            true,
                        );
                    }
                    Some(Some(Ok(list))) if list.is_empty() => {
                        let msg = crate::lang::get_or(
                            "mco.invites.nopending",
                            "You have no pending invites",
                        );
                        let mw = HudBatch::text_width(atlas, &msg);
                        batch.text(
                            atlas,
                            &msg,
                            cx - (mw / 2.0).floor(),
                            (h / 2.0).floor(),
                            rgba(160, 160, 160, 255),
                            true,
                        );
                    }
                    Some(Some(Ok(list))) => {
                        let rx = ((w - ROW_WIDTH) / 2.0).floor();
                        for (i, invite) in list.iter().enumerate() {
                            let y = top + 2.0 + i as f32 * ROW_HEIGHT;
                            if y + ROW_HEIGHT > bottom {
                                break;
                            }
                            batch.text(
                                atlas,
                                &invite.world_name,
                                rx + 10.0,
                                y + 4.0,
                                u32::MAX,
                                true,
                            );
                            let owner_txt = format!("Owner: {}", invite.world_owner_name);
                            batch.text(
                                atlas,
                                &owner_txt,
                                rx + 10.0,
                                y + 16.0,
                                rgba(160, 160, 160, 255),
                                true,
                            );
                        }
                    }
                }
            }
            SubScreen::Configure {
                server, status_msg, ..
            } => {
                crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);
                let title = format!("Konfiguracja: {}", server.name);
                let tw = HudBatch::text_width(atlas, &title);
                batch.text(atlas, &title, cx - (tw / 2.0).floor(), 20.0, u32::MAX, true);

                let info_y = (h / 2.0).floor() - 60.0;
                let motd = if server.motd.is_empty() {
                    "Minecraft Realm"
                } else {
                    &server.motd
                };
                let motd_w = HudBatch::text_width(atlas, motd);
                batch.text(
                    atlas,
                    motd,
                    cx - (motd_w / 2.0).floor(),
                    info_y,
                    rgba(160, 160, 160, 255),
                    true,
                );

                let (state_text, state_col) = if server.is_open() {
                    ("Stan: Otwarty", rgba(85, 255, 85, 255))
                } else {
                    ("Stan: Zamknięty", rgba(255, 85, 85, 255))
                };
                let st_w = HudBatch::text_width(atlas, state_text);
                batch.text(
                    atlas,
                    state_text,
                    cx - (st_w / 2.0).floor(),
                    info_y + 14.0,
                    state_col,
                    true,
                );

                let days_txt = format!("Wygasa za: {} dni", server.days_left);
                let days_w = HudBatch::text_width(atlas, &days_txt);
                batch.text(
                    atlas,
                    &days_txt,
                    cx - (days_w / 2.0).floor(),
                    info_y + 28.0,
                    rgba(160, 160, 160, 255),
                    true,
                );

                if let Some(msg) = status_msg {
                    let mw = HudBatch::text_width(atlas, msg);
                    batch.text(
                        atlas,
                        msg,
                        cx - (mw / 2.0).floor(),
                        info_y + 46.0,
                        u32::MAX,
                        true,
                    );
                }
            }
            SubScreen::AddRealm => {
                crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);
                let title = "Minecraft Realms";
                let tw = HudBatch::text_width(atlas, title);
                batch.text(atlas, title, cx - (tw / 2.0).floor(), 24.0, u32::MAX, true);

                let info_y = (h / 2.0).floor() - 40.0;
                let l1 = "Realms to Twój własny prywatny serwer Minecraft.";
                let l2 = "Działa w bezpiecznej chmurze 24/7 dla Ciebie i przyjaciół.";
                let l3 = "Możesz w każdej chwili zmienić minigrę lub wgrać własny świat.";
                for (i, line) in [l1, l2, l3].into_iter().enumerate() {
                    let lw = HudBatch::text_width(atlas, line);
                    batch.text(
                        atlas,
                        line,
                        cx - (lw / 2.0).floor(),
                        info_y + i as f32 * 14.0,
                        rgba(180, 180, 180, 255),
                        true,
                    );
                }
            }
            SubScreen::Main => {
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
                    let title = crate::lang::get_or("mco.title", "Minecraft Realms");
                    let tw = HudBatch::text_width(atlas, &title);
                    batch.text(atlas, &title, cx - (tw / 2.0).floor(), 12.0, u32::MAX, true);
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
                        let msg = "Please sign in with a Microsoft account in Horizon Launcher to play Realms.";
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
                        let msg =
                            crate::lang::get_or("mco.selectServer.loading", "Loading Realms List");
                        let mw = HudBatch::text_width(atlas, &msg);
                        let mid_y = (h / 2.0).floor();
                        batch.text(atlas, &msg, cx - mw / 2.0, mid_y - 12.0, u32::MAX, true);

                        let millis = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis());
                        let dots =
                            ["O o o", "o O o", "o o O", "o O o"][(millis / 300 % 4) as usize];
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
                                    batch.fill(
                                        atlas,
                                        rx - 2.0,
                                        y - 2.0,
                                        ROW_WIDTH,
                                        ROW_HEIGHT,
                                        u32::MAX,
                                    );
                                    batch.fill(
                                        atlas,
                                        rx - 1.0,
                                        y - 1.0,
                                        ROW_WIDTH - 2.0,
                                        ROW_HEIGHT - 2.0,
                                        rgba(0, 0, 0, 255),
                                    );
                                }

                                batch.sprite_scaled(
                                    atlas,
                                    "misc/unknown_server",
                                    rx + 2.0,
                                    y + 2.0,
                                    32.0,
                                    32.0,
                                    u32::MAX,
                                );

                                let name_color = if server.is_open() {
                                    u32::MAX
                                } else {
                                    rgba(160, 160, 160, 255)
                                };
                                batch.text(
                                    atlas,
                                    &server.name,
                                    rx + 38.0,
                                    y + 1.0,
                                    name_color,
                                    true,
                                );

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

                                let owner = server.owner.as_deref().unwrap_or("");
                                batch.text(
                                    atlas,
                                    owner,
                                    rx + 38.0,
                                    y + 23.0,
                                    rgba(160, 160, 160, 255),
                                    true,
                                );

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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_buttons_have_proper_icons() {
        let screen = Screen::new();
        let btns = screen.buttons(800.0, 600.0);

        let invite_btn = btns
            .iter()
            .find(|b| b.5 == 15)
            .expect("Invites button exists");
        assert_eq!(invite_btn.6, Some(("icon/invite", 14.0)));

        let news_btn = btns.iter().find(|b| b.5 == 16).expect("News button exists");
        assert_eq!(news_btn.6, Some(("icon/news", 14.0)));

        let join_btn = btns
            .iter()
            .find(|b| b.5 == 14)
            .expect("Join Realm button exists");
        assert_eq!(join_btn.6, None);
    }

    #[test]
    fn join_realm_button_navigates_to_join_code_screen() {
        let mut screen = Screen::new();
        assert!(matches!(screen.subscreen, SubScreen::Main));

        screen.click(Some(14), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(screen.subscreen, SubScreen::JoinCode { .. }));

        let btns = screen.buttons(800.0, 600.0);
        let join_btn = btns.iter().find(|b| b.5 == 20).expect("Join button exists");
        assert!(
            !join_btn.4,
            "Join button should be disabled when code is empty"
        );

        let cancel_btn = btns
            .iter()
            .find(|b| b.5 == 21)
            .expect("Cancel button exists");
        assert!(cancel_btn.4);

        // Escape key returns to Main
        assert!(!screen.escape());
        assert!(matches!(screen.subscreen, SubScreen::Main));
    }
}
