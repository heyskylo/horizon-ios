//! Minecraft Realms screens: `RealmsMainScreen` with its `RealmsHeader`, the join-code and
//! pending-invite screens, the owner's open/close, `RealmsPopups.infoPopupScreen` for the
//! questions, and `GetServerDetailsTask` under `RealmsLongRunningMcoTaskScreen` for Play.
//! Layout from the vanilla screens' behaviour and the game's own sprites.
//!
//! Realms is used only with the signed-in player's own Microsoft account: the session the
//! launcher hands over after its owner check (`auth::current`). Without one (offline play, an
//! expired token) the screen says so and makes no request. Every request runs on a worker
//! thread; the title polls the results once a frame (`poll`).
pub mod client;

use crate::hud::{HudAtlas, HudBatch, rgba};
use crate::text::Text;
use client::{
    PendingInviteDto, RealmAddress, RealmServer, RealmsClient, RealmsError, Subscription,
};
use horizon_client_render::auth::{self, AuthSession};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER_HEIGHT: f32 = 44.0;
const CONTENT_WIDTH: f32 = 308.0;
const ROW_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 36.0;
/// `RealmsConfigureWorldScreen`'s `MenuTabBar` height (`gui/tab_header_background`).
const TAB_HEIGHT: f32 = 24.0;
/// `hud::rgba` packing (ABGR): 160 grey, vanilla's red and `RealmsPopups.COLOR_INFO`.
const GREY: u32 = 0xFFA0_A0A0;
const RED: u32 = 0xFF55_55FF;
const INFO: u32 = 0xFFBE_887D;

const REALMS_INVITE_CODE: crate::clipboard::FieldId = ("realms.invite_code", 0);

/// `CommonLinks.BUY_REALMS`.
const BUY_REALMS: &str = "https://aka.ms/BuyJavaRealms";
/// `CommonLinks.EXTEND_REALMS_LINK`.
const EXTEND_REALMS: &str = "https://aka.ms/ExtendJavaRealms";

/// What a worker thread hands back.
type Job<T> = Arc<Mutex<Option<T>>>;

fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Job<T> {
    let job = Arc::new(Mutex::new(None));
    let out = Arc::clone(&job);
    let started = thread::Builder::new().name("Realms".into()).spawn(move || {
        let result = work();
        if let Ok(mut slot) = out.lock() {
            *slot = Some(result);
        }
    });
    if let Err(e) = started {
        log::warn!("Horizon: could not start a Realms worker: {e}");
    }
    job
}

fn take<T>(job: &Job<T>) -> Option<T> {
    job.lock().ok()?.take()
}

/// The account Realms may use: the launcher's Microsoft session (it hands one over only for
/// an account that owns the game) while its token is good.
fn session() -> Option<Arc<AuthSession>> {
    auth::current().filter(|s| !s.expired())
}

fn client() -> Option<RealmsClient> {
    session().map(|s| RealmsClient::production(&s))
}

/// The address `/join/pc` gave, checked before it becomes `Action::Connect`: a host name,
/// IPv4 or `[IPv6]` host, then `:` and a port number. A scheme, a path, credentials or a
/// missing port are refused.
pub fn connect_address(address: &str) -> Option<String> {
    let address = address.trim();
    let (host, port, host_ok): (&str, &str, fn(u8) -> bool) =
        if let Some(rest) = address.strip_prefix('[') {
            let (host, port) = rest.split_once("]:")?;
            (host, port, |b| {
                b.is_ascii_hexdigit() || b == b':' || b == b'.'
            })
        } else {
            let (host, port) = address.rsplit_once(':')?;
            (host, port, |b| {
                b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_'
            })
        };
    if host.is_empty() || !host.bytes().all(host_ok) {
        return None;
    }
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    (port != 0).then(|| address.to_owned())
}

/// The world's resource pack the player agreed to download before joining
/// (`GetServerDetailsTask.scheduleResourcePackDownload`): the next connection to `address`
/// takes it as an allowed server pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RealmsPack {
    pub address: String,
    pub id: u128,
    pub url: String,
    pub hash: String,
}

static PENDING_PACK: Mutex<Option<RealmsPack>> = Mutex::new(None);

/// The pack for the connection to `address`, if the Realms screen asked for one.
pub fn take_pending_pack(address: &str) -> Option<RealmsPack> {
    let pack = PENDING_PACK.lock().ok()?.take()?;
    (pack.address == address).then_some(pack)
}

/// `UUID.nameUUIDFromBytes`.
fn name_uuid(bytes: &[u8]) -> u128 {
    let mut h = horizon_protocol::crypto::md5(bytes);
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    u128::from_be_bytes(h)
}

/// `GetServerDetailsTask.generatePackId`.
pub fn pack_id(server: &RealmServer) -> u128 {
    match &server.minigame_name {
        Some(minigame) => name_uuid(format!("minigame:{minigame}").as_bytes()),
        None => name_uuid(format!("realms:{}:{}", server.name, server.active_slot).as_bytes()),
    }
}

/// `CommonLinks.extendRealms`.
fn extend_link(server: &RealmServer, profile: u128) -> String {
    match &server.remote_subscription_id {
        None => EXTEND_REALMS.to_owned(),
        Some(sub) => {
            let reference = if server.expired_trial {
                "expiredTrial"
            } else {
                "expiredRealm"
            };
            format!("{EXTEND_REALMS}?subscriptionId={sub}&profileId={profile:032x}&ref={reference}")
        }
    }
}

/// `RealmsConfigureWorldScreen`'s tabs, in `MenuTabBar` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigureTab {
    Worlds,
    Players,
    Subscription,
    Settings,
}

impl ConfigureTab {
    /// The tabs in `MenuTabBar` order, which is also their index.
    const ALL: [Self; 4] = [
        Self::Worlds,
        Self::Players,
        Self::Subscription,
        Self::Settings,
    ];
    /// `RealmsWorldsTab.TITLE` and the other tabs' own titles.
    fn title(self) -> &'static str {
        match self {
            Self::Worlds => "mco.configure.worlds.title",
            Self::Players => "mco.configure.world.players.title",
            Self::Subscription => "mco.configure.world.subscription.tab",
            Self::Settings => "mco.configure.world.settings.title",
        }
    }
    /// `RealmsConfigureWorldScreen.init`'s `setTabActiveState(3, !serverData.expired)`: an
    /// expired Realm's settings tab is off (with the expired tooltip).
    fn enabled(self, expired: bool) -> bool {
        self != Self::Settings || !expired
    }
}

/// The tab bar's geometry over `MenuTabBar.builder(this, width)`: the bar is at most 400
/// wide, centred, and each of the four tabs is an eighth of its inner width rounded up to an
/// even number (`Java`'s integer division before the rounding).
fn tab_geometry(w: f32) -> (f32, f32) {
    let width = w.min(400.0) - 28.0;
    let x = (((w - width) / 2.0).floor() / 2.0).ceil() * 2.0;
    let tab = ((width / 4.0).floor() / 2.0).ceil() * 2.0;
    (x, tab)
}

/// `RealmsSubscriptionTab.daysLeftPresentation`: what the days-left line says, in the
/// caller's language; empty when nothing is left to say.
fn days_left_text(days_left: i32, expired: bool) -> String {
    let t = crate::lang::get;
    if days_left < 0 && expired {
        return t("mco.configure.world.subscription.expired");
    }
    if days_left <= 1 {
        return t("mco.configure.world.subscription.less_than_a_day");
    }
    let (months, days) = (days_left / 30, days_left % 30);
    match (months > 0, days > 0) {
        (true, true) => crate::lang::tr(
            "mco.configure.world.subscription.remaining.months.days",
            &[&months, &days],
        ),
        (true, false) => crate::lang::tr(
            "mco.configure.world.subscription.remaining.months",
            &[&months],
        ),
        (false, true) => {
            crate::lang::tr("mco.configure.world.subscription.remaining.days", &[&days])
        }
        (false, false) => String::new(),
    }
}

/// `RealmsSubscriptionTab.localPresentation`: the start date in the player's own local
/// time. The process has no locale to format with, so the shape is fixed (`%Y-%m-%d`)
/// where vanilla asks for the locale's medium date.
fn start_date_text(epoch_millis: i64) -> String {
    let Some(time) = chrono::DateTime::from_timestamp_millis(epoch_millis) else {
        return String::new();
    };
    time.with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutState {
    Loading,
    NoRealms,
    List,
}

#[derive(Clone, Debug)]
pub enum FetchState {
    /// No Microsoft session: nothing is asked of the service.
    Unauthenticated,
    Loading(Job<Result<Vec<RealmServer>, RealmsError>>),
    Loaded(Vec<RealmServer>),
    Error(RealmsError),
}

/// A `RealmsPopups.infoPopupScreen`: Continue or Cancel.
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    /// `RealmsMainScreen.leaveClicked`.
    Leave(RealmServer),
    /// `GetServerDetailsTask.resourcePackDownloadConfirmationScreen`.
    ResourcePack { pack: RealmsPack },
    /// `RealmsSubscriptionTab`'s delete question (only offered once the Realm has expired).
    Delete(RealmServer),
}

#[derive(Clone, Debug)]
pub enum SubScreen {
    Main,
    JoinCode {
        code: String,
        error: Option<String>,
        job: Option<Job<Result<(), RealmsError>>>,
    },
    PendingInvites {
        invites: Option<Result<Vec<PendingInviteDto>, RealmsError>>,
        job: Option<Job<Result<Vec<PendingInviteDto>, RealmsError>>>,
    },
    Configure {
        server: RealmServer,
        tab: ConfigureTab,
        /// `RealmsClient.subscriptionFor` on a worker, then its answer (the subscription tab).
        subscription: Option<Result<Subscription, RealmsError>>,
        subscription_job: Option<Job<Result<Subscription, RealmsError>>>,
        /// `RealmsClient.deleteRealm` on a worker, from the subscription tab's question.
        deleting: Option<Job<Result<(), RealmsError>>>,
        error: Option<String>,
        /// `RealmsClient.open`/`close` on a worker.
        job: Option<Job<Result<(), RealmsError>>>,
    },
    AddRealm,
    Popup(Popup),
    /// `RealmsLongRunningMcoTaskScreen` running `GetServerDetailsTask`: the join request is on
    /// a worker (`joining`); an error stays on screen with a Back button.
    Connecting {
        server: RealmServer,
        error: Option<String>,
    },
}

/// The join request on its worker, and its abort flag (`LongRunningTask.aborted`).
type Joining = (Job<Result<RealmAddress, RealmsError>>, Arc<AtomicBool>);

#[derive(Clone, Debug)]
pub struct Screen {
    pub state: FetchState,
    pub subscreen: SubScreen,
    pub selected: Option<usize>,
    pub joining: Option<Joining>,
    /// Leaving a Realm (`RealmsMainScreen.leaveServer`): the list reloads when it's done.
    leaving: Option<Job<Result<(), RealmsError>>>,
    pub last_click: Option<(usize, SystemTime)>,
}

pub enum Outcome {
    None,
    Back,
    Connect(String),
    OpenUri(String),
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

/// `GetServerDetailsTask.fetchServerAddress`: up to 40 tries, waiting `Retry-After` between
/// them; an abort stops it.
fn fetch_address(
    client: &RealmsClient,
    realm_id: i64,
    aborted: &AtomicBool,
) -> Result<RealmAddress, RealmsError> {
    for _ in 0..40 {
        if aborted.load(Ordering::Relaxed) {
            return Err(RealmsError::Timeout);
        }
        match client.join_world(realm_id) {
            Err(RealmsError::Retry(seconds)) => {
                let until = Instant::now() + Duration::from_secs(seconds.min(60));
                while Instant::now() < until && !aborted.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                }
            }
            other => return other,
        }
    }
    Err(RealmsError::Timeout)
}

fn wrap_lines(text: &str, atlas: &HudAtlas, width: f32) -> Vec<String> {
    crate::multiplayer::wrap(text, width, atlas)
}

fn centered(batch: &mut HudBatch, atlas: &HudAtlas, text: &str, cx: f32, y: f32, color: u32) {
    let w = HudBatch::text_width(atlas, text);
    batch.text(atlas, text, cx - (w / 2.0).floor(), y, color, true);
}

impl Screen {
    pub fn new() -> Self {
        let mut screen = Self {
            state: FetchState::Unauthenticated,
            subscreen: SubScreen::Main,
            selected: None,
            joining: None,
            leaving: None,
            last_click: None,
        };
        screen.refresh();
        screen
    }

    pub fn layout_state(&self) -> LayoutState {
        match &self.state {
            FetchState::Loading(_) => LayoutState::Loading,
            FetchState::Loaded(servers) if !servers.is_empty() => LayoutState::List,
            _ => LayoutState::NoRealms,
        }
    }

    fn servers(&self) -> &[RealmServer] {
        match &self.state {
            FetchState::Loaded(servers) => servers,
            _ => &[],
        }
    }

    fn selected_server(&self) -> Option<&RealmServer> {
        self.servers().get(self.selected?)
    }

    /// `RealmsMainScreen.refreshServerList`.
    pub fn refresh(&mut self) {
        self.selected = None;
        self.state = match client() {
            Some(client) => FetchState::Loading(spawn(move || client.list_worlds())),
            None => FetchState::Unauthenticated,
        };
    }

    /// Picks up what the workers finished; called once a frame.
    pub fn poll(&mut self) -> Outcome {
        if let FetchState::Loading(job) = &self.state
            && let Some(result) = take(job)
        {
            self.state = match result {
                Ok(servers) => FetchState::Loaded(servers),
                Err(e) => FetchState::Error(e),
            };
        }
        if let Some(job) = &self.leaving
            && let Some(result) = take(job)
        {
            self.leaving = None;
            match result {
                Ok(()) => self.refresh(),
                Err(e) => self.state = FetchState::Error(e),
            }
        }
        let mut refresh = false;
        match &mut self.subscreen {
            SubScreen::JoinCode { error, job, .. } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        Ok(()) => {
                            self.subscreen = SubScreen::Main;
                            refresh = true;
                        }
                        Err(_) => {
                            *error = Some(crate::lang::get("mco.selectServer.joinCode.invalid"));
                        }
                    }
                }
            }
            SubScreen::PendingInvites { invites, job } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    *invites = Some(result);
                }
            }
            SubScreen::Configure {
                server,
                subscription,
                subscription_job,
                deleting,
                error,
                job,
                ..
            } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        Ok(()) => {
                            let open = server.is_open();
                            server.state = if open { "CLOSED" } else { "OPEN" }.to_owned();
                            refresh = true;
                        }
                        Err(e) => *error = Some(e.text()),
                    }
                }
                if let Some(result) = subscription_job.as_ref().and_then(take) {
                    *subscription_job = None;
                    *subscription = Some(result);
                }
                if let Some(result) = deleting.as_ref().and_then(take) {
                    *deleting = None;
                    match result {
                        // The Realm is gone: the list reloads without it.
                        Ok(()) => {
                            self.subscreen = SubScreen::Main;
                            self.selected = None;
                            refresh = true;
                        }
                        Err(e) => *error = Some(e.text()),
                    }
                }
            }
            SubScreen::Connecting { .. } => return self.poll_join(),
            _ => {}
        }
        if refresh {
            self.refresh();
        }
        Outcome::None
    }

    /// `GetServerDetailsTask.run` once the address came back.
    fn poll_join(&mut self) -> Outcome {
        let Some((job, _)) = &self.joining else {
            return Outcome::None;
        };
        let Some(result) = take(job) else {
            return Outcome::None;
        };
        self.joining = None;
        let SubScreen::Connecting { server, error } = &mut self.subscreen else {
            return Outcome::None;
        };
        let address = match result {
            Ok(address) => address,
            Err(e) => {
                // 6006: the world is out of date; only its owner can reset it.
                let owner = session().is_some_and(|s| server.owned_by(s.uuid));
                *error = Some(if e.code() == 6006 && !owner {
                    format!(
                        "{} {}",
                        crate::lang::get("mco.brokenworld.nonowner.title"),
                        crate::lang::get("mco.brokenworld.nonowner.error")
                    )
                } else {
                    e.text()
                });
                return Outcome::None;
            }
        };
        let Some(target) = address.address.as_deref().and_then(connect_address) else {
            log::warn!("Horizon: Realms gave no usable address to join");
            *error = Some(crate::lang::get("mco.errorMessage.connectionFailure"));
            return Outcome::None;
        };
        match (address.resource_pack_url, address.resource_pack_hash) {
            // `new URL(..)` fails in scheduleResourcePackDownload: the pack can't download.
            (Some(url), Some(_)) if !crate::server_packs::valid_url(&url) => {
                *error = Some(crate::lang::get("mco.download.resourcePack.fail"));
                Outcome::None
            }
            (Some(url), Some(hash)) => {
                let pack = RealmsPack {
                    address: target,
                    id: pack_id(server),
                    url,
                    hash,
                };
                self.subscreen = SubScreen::Popup(Popup::ResourcePack { pack });
                Outcome::None
            }
            _ => {
                self.subscreen = SubScreen::Main;
                Outcome::Connect(target)
            }
        }
    }

    pub fn focused_box(&self, (w, _h): (f32, f32)) -> Option<[f32; 4]> {
        matches!(self.subscreen, SubScreen::JoinCode { .. })
            .then(|| [(w / 2.0).floor() - 100.0, 64.0, 200.0, 20.0])
    }

    pub fn focused_text(&mut self) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
        if let SubScreen::JoinCode { ref mut code, .. } = self.subscreen {
            Some((REALMS_INVITE_CODE, code, 128))
        } else {
            None
        }
    }

    pub fn tab(&mut self) {}

    /// Escape: a sub-screen goes back to the list (cancelling a join); the list goes back to
    /// the title (true).
    pub fn escape(&mut self) -> bool {
        if matches!(self.subscreen, SubScreen::Main) {
            return true;
        }
        self.back_to_list();
        false
    }

    fn back_to_list(&mut self) {
        if let Some((_, aborted)) = self.joining.take() {
            aborted.store(true, Ordering::Relaxed);
        }
        let reload = matches!(self.subscreen, SubScreen::PendingInvites { .. });
        self.subscreen = SubScreen::Main;
        if reload {
            self.refresh();
        }
    }

    /// Play: `RealmsMainScreen.play` → `GetServerDetailsTask` on a worker. The screen
    /// shows "Connecting to the Realm..." and stays responsive.
    fn play_selected(&mut self) -> Outcome {
        match self.selected_server() {
            Some(server) => self.start_join(server.clone()),
            None => Outcome::None,
        }
    }

    /// `RealmsMainScreen.play` for `server`, the list's or the configuration screen's.
    fn start_join(&mut self, server: RealmServer) -> Outcome {
        if !server.is_open() {
            return Outcome::None;
        }
        let Some(client) = client() else {
            return Outcome::None;
        };
        let aborted = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&aborted);
        let id = server.id;
        let job = spawn(move || fetch_address(&client, id, &flag));
        self.joining = Some((job, aborted));
        self.subscreen = SubScreen::Connecting {
            server,
            error: None,
        };
        Outcome::None
    }

    /// `RealmsConfigureWorldScreen.init`'s `fetchSubscription`: the subscription tab's
    /// data, on a worker.
    fn start_configure_subscription(&mut self) {
        let SubScreen::Configure {
            server,
            subscription_job,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        if subscription_job.is_some() {
            return;
        }
        if let Some(client) = client() {
            let id = server.id;
            *subscription_job = Some(spawn(move || client.subscription_for(id)));
        }
    }

    pub fn click(&mut self, hit: Option<u8>, x: f32, y: f32, (w, h): (f32, f32)) -> Outcome {
        match hit {
            Some(1) => self.play_selected(),
            Some(2) => Outcome::Back,
            Some(10) => {
                if let Some(server) = self.selected_server().cloned() {
                    self.subscreen = SubScreen::Configure {
                        server,
                        tab: ConfigureTab::Worlds,
                        subscription: None,
                        subscription_job: None,
                        deleting: None,
                        error: None,
                        job: None,
                    };
                    self.start_configure_subscription();
                }
                Outcome::None
            }
            Some(11) => match (self.selected_server(), session()) {
                // RealmsMainScreen.onRenew.
                (Some(server), Some(s)) => Outcome::OpenUri(extend_link(server, s.uuid)),
                _ => Outcome::None,
            },
            Some(12) => {
                self.leave_clicked(session().map(|s| s.uuid));
                Outcome::None
            }
            Some(13) => {
                self.subscreen = SubScreen::AddRealm;
                Outcome::None
            }
            Some(14) => {
                self.subscreen = SubScreen::JoinCode {
                    code: String::new(),
                    error: None,
                    job: None,
                };
                Outcome::None
            }
            Some(15) => {
                let job = client().map(|c| spawn(move || c.pending_invites()));
                self.subscreen = SubScreen::PendingInvites { invites: None, job };
                Outcome::None
            }
            Some(17) => {
                self.refresh();
                Outcome::None
            }
            Some(20) => {
                if let SubScreen::JoinCode { code, error, job } = &mut self.subscreen
                    && job.is_none()
                    && !code.trim().is_empty()
                {
                    match client() {
                        Some(client) => {
                            let code = code.trim().to_owned();
                            *error = None;
                            *job = Some(spawn(move || client.redeem_invite_code(&code)));
                        }
                        None => *error = Some(crate::lang::get("horizon.realms.not_signed_in")),
                    }
                }
                Outcome::None
            }
            Some(21) | Some(22) | Some(32) | Some(41) | Some(51) | Some(52) => {
                self.back_to_list();
                Outcome::None
            }
            Some(30) => {
                if let SubScreen::Configure {
                    server, error, job, ..
                } = &mut self.subscreen
                    && job.is_none()
                    && let Some(client) = client()
                {
                    let (id, open) = (server.id, server.is_open());
                    *error = None;
                    *job = Some(spawn(move || {
                        if open {
                            client.close_realm(id)
                        } else {
                            client.open_realm(id)
                        }
                    }));
                }
                Outcome::None
            }
            // The configuration screen's tabs (60..=63) and its footer: Play (64).
            Some(sel @ 60..=63) => {
                if let SubScreen::Configure { server, tab, .. } = &mut self.subscreen
                    && let Some(wanted) = ConfigureTab::ALL.get(usize::from(sel - 60))
                    && wanted.enabled(server.expired)
                {
                    *tab = *wanted;
                }
                Outcome::None
            }
            Some(64) => match &self.subscreen {
                SubScreen::Configure { server, .. } => self.start_join(server.clone()),
                _ => Outcome::None,
            },
            // The subscription tab's own buttons: Delete (expired only, after its question)
            // and Extend.
            Some(65) => {
                if let SubScreen::Configure {
                    server, deleting, ..
                } = &self.subscreen
                    && server.expired
                    && deleting.is_none()
                {
                    let server = server.clone();
                    self.subscreen = SubScreen::Popup(Popup::Delete(server));
                }
                Outcome::None
            }
            Some(66) => match (&self.subscreen, session()) {
                (SubScreen::Configure { server, .. }, Some(s)) => {
                    Outcome::OpenUri(extend_link(server, s.uuid))
                }
                _ => Outcome::None,
            },
            Some(40) => Outcome::OpenUri(BUY_REALMS.to_owned()),
            Some(50) => self.popup_continue(),
            Some(btn @ 100..=200) => {
                if let SubScreen::PendingInvites { invites, job } = &mut self.subscreen
                    && job.is_none()
                    && let Some(Ok(list)) = invites
                    && let Some(invite) = list.get(usize::from((btn - 100) / 2))
                    && let Some(client) = client()
                {
                    let accept = (btn - 100) % 2 == 0;
                    let id = invite.invitation_id.clone();
                    *job = Some(spawn(move || {
                        let done = if accept {
                            client.accept_invite(&id)
                        } else {
                            client.reject_invite(&id)
                        };
                        done.and_then(|()| client.pending_invites())
                    }));
                }
                Outcome::None
            }
            // The tab bar draws its own tabs (`MenuTabBar`), so a click there carries no
            // button id: the tab bar's geometry decides.
            None if matches!(self.subscreen, SubScreen::Configure { .. }) => {
                let (bar_x, tab_w) = tab_geometry(w);
                if y < TAB_HEIGHT && x >= bar_x {
                    let index = ((x - bar_x) / tab_w).floor() as usize;
                    if let SubScreen::Configure { server, tab, .. } = &mut self.subscreen
                        && let Some(wanted) = ConfigureTab::ALL.get(index)
                        && wanted.enabled(server.expired)
                    {
                        *tab = *wanted;
                    }
                }
                Outcome::None
            }
            _ => self.click_list(x, y, (w, h)),
        }
    }

    /// `RealmsMainScreen.leaveClicked`: someone else's Realm only, and only the question:
    /// nothing is sent until Continue.
    fn leave_clicked(&mut self, me: Option<u128>) {
        if let Some(server) = self.selected_server().cloned()
            && let Some(me) = me
            && !server.owned_by(me)
        {
            self.subscreen = SubScreen::Popup(Popup::Leave(server));
        }
    }

    /// Continue in a popup.
    fn popup_continue(&mut self) -> Outcome {
        let SubScreen::Popup(popup) = &self.subscreen else {
            return Outcome::None;
        };
        let popup = popup.clone();
        self.subscreen = SubScreen::Main;
        match popup {
            Popup::Leave(server) => {
                // leaveServer: on a thread, then the list reloads.
                if let Some(client) = client() {
                    let id = server.id;
                    self.leaving = Some(spawn(move || client.uninvite_myself(id)));
                    self.selected = None;
                }
                Outcome::None
            }
            Popup::ResourcePack { pack } => {
                let address = pack.address.clone();
                if let Ok(mut slot) = PENDING_PACK.lock() {
                    *slot = Some(pack);
                }
                Outcome::Connect(address)
            }
            // RealmsSubscriptionTab.deleteRealm: the question was asked over the
            // configuration screen, so it answers there; the list returns once the
            // Realm is gone.
            Popup::Delete(server) => {
                let Some(client) = client() else {
                    return Outcome::None;
                };
                let id = server.id;
                self.subscreen = SubScreen::Configure {
                    server,
                    tab: ConfigureTab::Subscription,
                    subscription: None,
                    subscription_job: None,
                    deleting: Some(spawn(move || client.delete_realm(id))),
                    error: None,
                    job: None,
                };
                // A failed delete stays here and the tab reads again.
                self.start_configure_subscription();
                Outcome::None
            }
        }
    }

    fn click_list(&mut self, x: f32, y: f32, (w, h): (f32, f32)) -> Outcome {
        if !matches!(self.subscreen, SubScreen::Main) {
            return Outcome::None;
        }
        let footer_h = if self.layout_state() == LayoutState::List {
            60.0
        } else {
            48.0
        };
        let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
        let rx = ((w - ROW_WIDTH) / 2.0).floor();
        if y < top || y > bottom || x < rx || x > rx + ROW_WIDTH {
            return Outcome::None;
        }
        let row = ((y - top - 2.0) / ROW_HEIGHT).floor() as usize;
        let Some(can_play) = self.servers().get(row).map(RealmServer::is_open) else {
            return Outcome::None;
        };
        let now = SystemTime::now();
        let double = matches!(self.last_click, Some((last, at)) if last == row
            && now.duration_since(at).is_ok_and(|d| d.as_millis() < 350));
        self.selected = Some(row);
        self.last_click = Some((row, now));
        if double && can_play {
            return self.play_selected();
        }
        Outcome::None
    }

    pub fn buttons(
        &self,
        w: f32,
        h: f32,
    ) -> Vec<(f32, f32, f32, String, bool, u8, Option<(&'static str, f32)>)> {
        let cx = (w / 2.0).floor();
        let left = cx - (CONTENT_WIDTH / 2.0).floor();
        let mid = (h / 2.0).floor();
        let t = crate::lang::get;
        let mut buttons = Vec::new();

        match &self.subscreen {
            SubScreen::JoinCode { code, job, .. } => {
                buttons.push((
                    cx - 100.0,
                    mid + 10.0,
                    200.0,
                    t("mco.selectServer.joinCode.select"),
                    !code.trim().is_empty() && job.is_none(),
                    20,
                    None,
                ));
                buttons.push((
                    cx - 100.0,
                    mid + 34.0,
                    200.0,
                    t("gui.cancel"),
                    true,
                    21,
                    None,
                ));
            }
            SubScreen::PendingInvites { invites, job } => {
                buttons.push((cx - 100.0, h - 36.0, 200.0, t("gui.done"), true, 22, None));
                if let Some(Ok(list)) = invites {
                    let rx = ((w - ROW_WIDTH) / 2.0).floor();
                    for (i, _) in list.iter().enumerate().take(40) {
                        let row_y = 36.0 + 2.0 + i as f32 * ROW_HEIGHT;
                        if row_y + ROW_HEIGHT > h - 48.0 {
                            break;
                        }
                        let id = 100 + i as u8 * 2;
                        buttons.push((
                            rx + ROW_WIDTH - 46.0,
                            row_y + 8.0,
                            20.0,
                            String::new(),
                            job.is_none(),
                            id,
                            Some(("pending_invite/accept", 14.0)),
                        ));
                        buttons.push((
                            rx + ROW_WIDTH - 22.0,
                            row_y + 8.0,
                            20.0,
                            String::new(),
                            job.is_none(),
                            id + 1,
                            Some(("pending_invite/reject", 14.0)),
                        ));
                    }
                }
            }
            SubScreen::Configure {
                server,
                tab,
                subscription,
                deleting,
                job,
                ..
            } => {
                // The footer: Play, the open/close toggle (vanilla keeps it on the settings
                // tab; it stays here until that tab lands) and Back.
                buttons.push((
                    cx - 155.0,
                    h - 26.0,
                    100.0,
                    t("mco.selectServer.play"),
                    server.is_open(),
                    64,
                    None,
                ));
                let toggle = if server.is_open() {
                    t("mco.configure.world.buttons.close")
                } else {
                    t("mco.configure.world.buttons.open")
                };
                buttons.push((cx - 50.0, h - 26.0, 100.0, toggle, job.is_none(), 30, None));
                buttons.push((cx + 55.0, h - 26.0, 100.0, t("gui.back"), true, 32, None));
                // RealmsSubscriptionTab's own two buttons.
                if *tab == ConfigureTab::Subscription && matches!(subscription, Some(Ok(_))) {
                    buttons.push((
                        cx - 100.0,
                        92.0,
                        200.0,
                        t("mco.configure.world.subscription.extend"),
                        true,
                        66,
                        None,
                    ));
                    buttons.push((
                        cx - 100.0,
                        118.0,
                        200.0,
                        t("mco.configure.world.delete.button"),
                        server.expired && deleting.is_none(),
                        65,
                        None,
                    ));
                }
            }
            SubScreen::AddRealm => {
                buttons.push((
                    cx - 100.0,
                    mid + 30.0,
                    200.0,
                    t("mco.selectServer.buy"),
                    true,
                    40,
                    None,
                ));
                buttons.push((cx - 100.0, h - 36.0, 200.0, t("gui.back"), true, 41, None));
            }
            SubScreen::Popup(_) => {
                buttons.push((
                    cx - 102.0,
                    mid + 30.0,
                    100.0,
                    t("gui.continue"),
                    true,
                    50,
                    None,
                ));
                buttons.push((cx + 2.0, mid + 30.0, 100.0, t("gui.cancel"), true, 51, None));
            }
            SubScreen::Connecting { error, .. } => {
                let label = if error.is_some() {
                    t("gui.back")
                } else {
                    t("gui.cancel")
                };
                buttons.push((cx - 100.0, mid + 40.0, 200.0, label, true, 52, None));
            }
            SubScreen::Main => {
                let signed_in = session().is_some();
                buttons.push((
                    cx - 68.0 - 112.0,
                    12.0,
                    112.0,
                    t("mco.selectServer.joinRealm"),
                    signed_in,
                    14,
                    None,
                ));
                buttons.push((
                    left + CONTENT_WIDTH - 44.0,
                    12.0,
                    20.0,
                    String::new(),
                    signed_in,
                    15,
                    Some(("icon/invite", 14.0)),
                ));
                // RealmsHeader's news button opens the news link the service sent; Horizon
                // doesn't fetch the news, so it has none (vanilla's newsLink == null).
                buttons.push((
                    left + CONTENT_WIDTH - 20.0,
                    12.0,
                    20.0,
                    String::new(),
                    false,
                    16,
                    Some(("icon/news", 14.0)),
                ));

                match self.layout_state() {
                    LayoutState::List => {
                        let server = self.selected_server();
                        let uuid = session().map(|s| s.uuid);
                        let owned = server.zip(uuid).is_some_and(|(s, u)| s.owned_by(u));
                        let can_join = server.is_some_and(RealmServer::is_open);
                        let expired = server.is_some_and(|s| s.expired);
                        let row1 = h - 52.0;
                        let row2 = h - 28.0;
                        buttons.push((
                            left,
                            row1,
                            100.0,
                            t("mco.selectServer.play"),
                            can_join,
                            1,
                            None,
                        ));
                        buttons.push((
                            left + 104.0,
                            row1,
                            100.0,
                            t("mco.selectServer.configure"),
                            owned,
                            10,
                            None,
                        ));
                        buttons.push((
                            left + 208.0,
                            row1,
                            100.0,
                            t("mco.selectServer.expiredRenew"),
                            owned && expired,
                            11,
                            None,
                        ));
                        buttons.push((
                            left,
                            row2,
                            100.0,
                            t("mco.selectServer.leave"),
                            server.is_some() && uuid.is_some() && !owned && self.leaving.is_none(),
                            12,
                            None,
                        ));
                        buttons.push((
                            left + 104.0,
                            row2,
                            100.0,
                            t("mco.selectServer.purchase"),
                            true,
                            13,
                            None,
                        ));
                        buttons.push((left + 208.0, row2, 100.0, t("gui.back"), true, 2, None));
                    }
                    LayoutState::NoRealms | LayoutState::Loading => {
                        if matches!(self.state, FetchState::Error(_)) {
                            buttons.push((
                                cx - 154.0,
                                h - 36.0,
                                100.0,
                                t("mco.errorMessage.retry"),
                                true,
                                17,
                                None,
                            ));
                            buttons.push((
                                cx - 50.0,
                                h - 36.0,
                                100.0,
                                t("mco.selectServer.purchase"),
                                true,
                                13,
                                None,
                            ));
                            buttons.push((
                                cx + 54.0,
                                h - 36.0,
                                100.0,
                                t("gui.back"),
                                true,
                                2,
                                None,
                            ));
                        } else {
                            buttons.push((
                                cx - 102.0,
                                h - 36.0,
                                100.0,
                                t("mco.selectServer.purchase"),
                                true,
                                13,
                                None,
                            ));
                            buttons.push((cx + 2.0, h - 36.0, 100.0, t("gui.back"), true, 2, None));
                        }
                    }
                }
            }
        }
        buttons
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        caret: &mut crate::ui::Caret,
        focused_at: Instant,
        cursor: (f32, f32),
        w: f32,
        h: f32,
    ) {
        let (w, h) = (w.ceil(), h.ceil());
        let cx = (w / 2.0).floor();
        let mid = (h / 2.0).floor();
        let t = crate::lang::get;
        crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);

        match &self.subscreen {
            SubScreen::JoinCode { code, error, job } => {
                centered(
                    batch,
                    atlas,
                    &t("mco.selectServer.joinCode.title"),
                    cx,
                    20.0,
                    u32::MAX,
                );
                batch.text(
                    atlas,
                    &t("mco.selectServer.joinCode"),
                    cx - 100.0,
                    50.0,
                    GREY,
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
                if job.is_some() {
                    centered(
                        batch,
                        atlas,
                        &t("mco.selectServer.joinCode.validating"),
                        cx,
                        88.0,
                        u32::MAX,
                    );
                } else if let Some(err) = error {
                    centered(batch, atlas, err, cx, 88.0, RED);
                }
            }
            SubScreen::PendingInvites { invites, .. } => {
                centered(batch, atlas, &t("mco.invites.title"), cx, 14.0, u32::MAX);
                let (top, bottom) = (36.0, h - 48.0);
                crate::ui::list_background(batch, atlas, top, bottom, false);
                match invites {
                    None => {}
                    Some(Err(e)) => {
                        for (i, line) in wrap_lines(&e.text(), atlas, w - 40.0).iter().enumerate() {
                            centered(batch, atlas, line, cx, mid + i as f32 * 10.0, RED);
                        }
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        centered(batch, atlas, &t("mco.invites.nopending"), cx, mid, GREY);
                    }
                    Some(Ok(list)) => {
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
                            batch.text(
                                atlas,
                                &invite.world_owner_name,
                                rx + 10.0,
                                y + 16.0,
                                GREY,
                                true,
                            );
                        }
                    }
                }
            }
            SubScreen::Configure {
                server,
                tab,
                subscription,
                deleting,
                error,
                job,
                ..
            } => {
                // `RealmsConfigureWorldScreen`: the tab bar over `gui/tab_header_background`,
                // the header separators either side of it, and the footer's separator.
                crate::ui::tiled(
                    batch,
                    atlas,
                    "gui/tab_header_background",
                    0.0,
                    0.0,
                    w,
                    TAB_HEIGHT,
                    16.0,
                    u32::MAX,
                );
                let (bar_x, tab_w) = tab_geometry(w);
                let now = crate::hud_overlays::now_ms();
                for (i, which) in ConfigureTab::ALL.iter().enumerate() {
                    let x = bar_x + i as f32 * tab_w;
                    let selected = which == tab;
                    let hover = crate::ui::hovered(cursor, x, 0.0, tab_w, TAB_HEIGHT);
                    let sprite = match (selected, hover) {
                        (true, true) => "gui/sprites/widget/tab_selected_highlighted",
                        (true, false) => "gui/sprites/widget/tab_selected",
                        (false, true) => "gui/sprites/widget/tab_highlighted",
                        (false, false) => "gui/sprites/widget/tab",
                    };
                    crate::ui::nine_slice(
                        batch,
                        atlas,
                        sprite,
                        x,
                        0.0,
                        tab_w,
                        TAB_HEIGHT,
                        u32::MAX,
                    );
                    if selected {
                        crate::ui::menu_background(
                            batch,
                            atlas,
                            x + 2.0,
                            2.0,
                            tab_w - 4.0,
                            22.0,
                            false,
                        );
                        batch.fill(atlas, x + 2.0, 22.0, tab_w - 4.0, 1.0, u32::MAX);
                    }
                    crate::ui::menu_tab_label(
                        batch,
                        atlas,
                        &Text::translate(which.title(), &[]),
                        (x, 0.0, tab_w, TAB_HEIGHT),
                        selected,
                        which.enabled(server.expired),
                        now,
                    );
                }
                for (left, width) in [(0.0, bar_x), (bar_x + 4.0 * tab_w, w - bar_x - 4.0 * tab_w)]
                {
                    crate::ui::tiled_sized(
                        batch,
                        atlas,
                        "gui/header_separator",
                        left,
                        TAB_HEIGHT - 2.0,
                        width,
                        2.0,
                        [32.0, 2.0],
                        u32::MAX,
                    );
                }
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    h - 35.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                let left = cx - 100.0;
                match tab {
                    ConfigureTab::Subscription => match subscription {
                        Some(Ok(sub)) => {
                            let mut label = |y: f32, text: &str, color: u32| {
                                batch.text(atlas, text, left, y, color, true);
                            };
                            label(26.0, &t("mco.configure.world.subscription.start"), u32::MAX);
                            label(41.0, &start_date_text(sub.start_date), GREY);
                            let days_label = if sub.kind == "RECURRING" {
                                t("mco.configure.world.subscription.recurring.daysleft")
                            } else {
                                t("mco.configure.world.subscription.timeleft")
                            };
                            label(56.0, &days_label, u32::MAX);
                            label(71.0, &days_left_text(sub.days_left, server.expired), GREY);
                            for (i, line) in wrap_lines(
                                &t("mco.configure.world.subscription.recurring.info"),
                                atlas,
                                200.0,
                            )
                            .iter()
                            .enumerate()
                            {
                                label(146.0 + i as f32 * 10.0, line, GREY);
                            }
                        }
                        Some(Err(e)) => {
                            for (i, line) in
                                wrap_lines(&e.text(), atlas, w - 40.0).iter().enumerate()
                            {
                                centered(batch, atlas, line, cx, 60.0 + i as f32 * 10.0, RED);
                            }
                        }
                        None => centered(
                            batch,
                            atlas,
                            &t("mco.configure.world.loading"),
                            cx,
                            60.0,
                            u32::MAX,
                        ),
                    },
                    // The other tabs land in follow-ups; vanilla shows this while its own
                    // fetches run.
                    _ => centered(
                        batch,
                        atlas,
                        &t("mco.configure.world.loading"),
                        cx,
                        60.0,
                        u32::MAX,
                    ),
                }
                let status = if job.is_some() {
                    Some((
                        t(if server.is_open() {
                            "mco.configure.world.closing"
                        } else {
                            "mco.configure.world.opening"
                        }),
                        u32::MAX,
                    ))
                } else if deleting.is_some() {
                    Some((t("mco.configure.world.buttons.delete"), u32::MAX))
                } else {
                    error.clone().map(|e| (e, RED))
                };
                if let Some((text, color)) = status {
                    for (i, line) in wrap_lines(&text, atlas, w - 40.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, h - 60.0 + i as f32 * 10.0, color);
                    }
                }
            }
            SubScreen::AddRealm => {
                centered(batch, atlas, &t("mco.title"), cx, 24.0, u32::MAX);
                let lines = wrap_lines(&t("mco.selectServer.popup"), atlas, CONTENT_WIDTH);
                let top = mid - 10.0 * lines.len() as f32;
                for (i, line) in lines.iter().enumerate() {
                    centered(
                        batch,
                        atlas,
                        line,
                        cx,
                        top + i as f32 * 10.0,
                        rgba(180, 180, 180, 255),
                    );
                }
            }
            SubScreen::Popup(popup) => {
                let message = match popup {
                    Popup::Leave(_) => t("mco.configure.world.leave.question.line1"),
                    Popup::ResourcePack { .. } => t("mco.configure.world.resourcepack.question"),
                    Popup::Delete(_) => t("mco.configure.world.delete.question.line1"),
                };
                let lines = wrap_lines(&message, atlas, CONTENT_WIDTH);
                let top = mid - 10.0 - 10.0 * lines.len() as f32;
                centered(batch, atlas, &t("mco.info"), cx, top - 16.0, INFO);
                for (i, line) in lines.iter().enumerate() {
                    centered(batch, atlas, line, cx, top + i as f32 * 10.0, u32::MAX);
                }
            }
            SubScreen::Connecting { error, .. } => {
                centered(
                    batch,
                    atlas,
                    &t("mco.connect.connecting"),
                    cx,
                    mid - 30.0,
                    u32::MAX,
                );
                match error {
                    Some(e) => {
                        for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                            centered(batch, atlas, line, cx, mid - 10.0 + i as f32 * 10.0, RED);
                        }
                    }
                    None => centered(batch, atlas, loading_dots(), cx, mid - 10.0, GREY),
                }
            }
            SubScreen::Main => self.draw_main(batch, atlas, w, h),
        }
    }

    fn draw_main(&self, batch: &mut HudBatch, atlas: &HudAtlas, w: f32, h: f32) {
        let cx = (w / 2.0).floor();
        let mid = (h / 2.0).floor();
        let t = crate::lang::get;
        // RealmsHeader: the Realms logo.
        if atlas.sprites.contains_key("gui/title/realms") {
            batch.sprite_region(
                atlas,
                "gui/title/realms",
                [cx - 64.0, 5.0, 128.0, 34.0],
                128.0,
                64.0,
            );
        } else {
            centered(batch, atlas, &t("mco.title"), cx, 12.0, u32::MAX);
        }
        let footer_h = if self.layout_state() == LayoutState::List {
            60.0
        } else {
            48.0
        };
        let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
        crate::ui::list_background(batch, atlas, top, bottom, false);
        let rx = ((w - ROW_WIDTH) / 2.0).floor();
        let paragraph = |batch: &mut HudBatch, text: &str, color: u32| {
            for (i, line) in wrap_lines(text, atlas, CONTENT_WIDTH).iter().enumerate() {
                centered(batch, atlas, line, cx, mid - 10.0 + i as f32 * 10.0, color);
            }
        };

        match &self.state {
            FetchState::Unauthenticated => {
                paragraph(batch, &t("horizon.realms.not_signed_in"), GREY)
            }
            FetchState::Loading(_) => {
                centered(
                    batch,
                    atlas,
                    &t("mco.selectServer.loading"),
                    cx,
                    mid - 12.0,
                    u32::MAX,
                );
                centered(
                    batch,
                    atlas,
                    loading_dots(),
                    cx,
                    mid + 4.0,
                    rgba(128, 128, 128, 255),
                );
            }
            FetchState::Error(e) => paragraph(batch, &e.text(), RED),
            FetchState::Loaded(servers) if servers.is_empty() => {
                let img_y = mid - 54.0;
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
                let lines = wrap_lines(&t("mco.selectServer.noRealms"), atlas, CONTENT_WIDTH);
                for (i, line) in lines.iter().enumerate() {
                    centered(batch, atlas, line, cx, img_y + 72.0 + i as f32 * 12.0, GREY);
                }
            }
            FetchState::Loaded(servers) => {
                for (i, server) in servers.iter().enumerate() {
                    let y = top + 2.0 + i as f32 * ROW_HEIGHT;
                    if y + ROW_HEIGHT > bottom {
                        break;
                    }
                    if self.selected == Some(i) {
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
                    batch.sprite_scaled(
                        atlas,
                        "misc/unknown_server",
                        rx + 2.0,
                        y + 2.0,
                        32.0,
                        32.0,
                        u32::MAX,
                    );
                    let name_color = if server.is_open() { u32::MAX } else { GREY };
                    batch.text(atlas, &server.name, rx + 38.0, y + 1.0, name_color, true);
                    let second = if server.expired {
                        (t("mco.selectServer.expiredList"), RED)
                    } else {
                        (server.motd.clone(), rgba(128, 128, 128, 255))
                    };
                    batch.text(atlas, &second.0, rx + 38.0, y + 12.0, second.1, true);
                    batch.text(
                        atlas,
                        server.owner.as_deref().unwrap_or(""),
                        rx + 38.0,
                        y + 23.0,
                        GREY,
                        true,
                    );
                    let light = if server.expired {
                        "gui/sprites/realm_status/expired"
                    } else if server.is_open() {
                        "gui/sprites/realm_status/open"
                    } else {
                        "gui/sprites/realm_status/closed"
                    };
                    if atlas.sprites.contains_key(light) {
                        batch.sprite_scaled(
                            atlas,
                            light,
                            rx + ROW_WIDTH - 17.0,
                            y + 2.0,
                            10.0,
                            28.0,
                            u32::MAX,
                        );
                    }
                }
            }
        }
    }
}

fn loading_dots() -> &'static str {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    ["O o o", "o O o", "o o O", "o O o"][(millis / 300 % 4) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: i64, owner_uuid: Option<&str>) -> RealmServer {
        RealmServer {
            id,
            name: "Friends".into(),
            state: "OPEN".into(),
            owner_uuid: owner_uuid.map(str::to_owned),
            active_slot: 1,
            ..Default::default()
        }
    }

    fn listed(servers: Vec<RealmServer>) -> Screen {
        Screen {
            state: FetchState::Loaded(servers),
            subscreen: SubScreen::Main,
            selected: Some(0),
            joining: None,
            leaving: None,
            last_click: None,
        }
    }

    #[test]
    fn join_addresses_need_a_host_and_a_numeric_port() {
        for good in [
            "1.2.3.4:25565",
            "eu-west.realms.example:30001",
            "[2001:db8::1]:25565",
            " a.b:1 ",
        ] {
            assert_eq!(
                connect_address(good).as_deref(),
                Some(good.trim()),
                "{good}"
            );
        }
        for bad in [
            "",
            ":25565",
            "1.2.3.4",
            "1.2.3.4:",
            "1.2.3.4:http",
            "1.2.3.4:65536",
            "1.2.3.4:0",
            "1.2.3.4:-1",
            "https://1.2.3.4:25565",
            "tcp://host:25565",
            "user@host:25565",
            "host/path:25565",
            "2001:db8::1:25565",
            "[]:25565",
            "host name:25565",
        ] {
            assert_eq!(connect_address(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn leave_asks_first_and_cancel_leaves_nothing() {
        const ME: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;
        let theirs = server(7, Some("fedcba9876543210fedcba9876543210"));
        // Our own Realm: no question, nothing to leave.
        let mut screen = listed(vec![server(7, Some("0123456789abcdef0123456789abcdef"))]);
        screen.leave_clicked(Some(ME));
        assert!(matches!(screen.subscreen, SubScreen::Main));

        // Someone else's: the first click only asks.
        let mut screen = listed(vec![theirs.clone()]);
        screen.leave_clicked(Some(ME));
        assert!(matches!(&screen.subscreen, SubScreen::Popup(Popup::Leave(s)) if *s == theirs));
        assert!(screen.leaving.is_none());
        let ids: Vec<u8> = screen.buttons(800.0, 600.0).iter().map(|b| b.5).collect();
        assert_eq!(ids, [50, 51]);

        // Cancel goes back with nothing sent and the Realm still selected.
        let _ = screen.click(Some(51), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(screen.subscreen, SubScreen::Main));
        assert!(screen.leaving.is_none());
        assert_eq!(screen.selected, Some(0));

        // Escape closes the question too.
        screen.leave_clicked(Some(ME));
        assert!(!screen.escape());
        assert!(matches!(screen.subscreen, SubScreen::Main));
        assert!(screen.leaving.is_none());

        // Signed out: no question at all.
        screen.leave_clicked(None);
        assert!(matches!(screen.subscreen, SubScreen::Main));
    }

    #[test]
    fn the_pack_question_connects_only_after_continue_with_the_pack_queued() {
        let pack = RealmsPack {
            address: "9.8.7.6:25565".into(),
            id: 5,
            url: "https://packs.test/p.zip".into(),
            hash: "0".repeat(40),
        };
        let mut screen = listed(vec![]);
        screen.subscreen = SubScreen::Popup(Popup::ResourcePack { pack: pack.clone() });
        let _ = screen.click(Some(51), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(screen.subscreen, SubScreen::Main));

        screen.subscreen = SubScreen::Popup(Popup::ResourcePack { pack: pack.clone() });
        let outcome = screen.click(Some(50), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(outcome, Outcome::Connect(ref a) if a == "9.8.7.6:25565"));
        assert_eq!(take_pending_pack("other:1"), None);
        // A taken pack is gone, whatever the address.
        assert_eq!(take_pending_pack("9.8.7.6:25565"), None);

        screen.subscreen = SubScreen::Popup(Popup::ResourcePack { pack: pack.clone() });
        let _ = screen.click(Some(50), 0.0, 0.0, (800.0, 600.0));
        assert_eq!(take_pending_pack("9.8.7.6:25565"), Some(pack));
    }

    #[test]
    fn a_join_answer_becomes_a_checked_connect_or_the_pack_question() {
        let mut screen = listed(vec![server(7, None)]);
        let answer = |address: &str, pack: bool| RealmAddress {
            address: Some(address.into()),
            resource_pack_url: pack.then(|| "https://packs.test/p.zip".into()),
            resource_pack_hash: pack.then(|| "ab".repeat(20)),
        };
        let start = |screen: &mut Screen, result: Result<RealmAddress, RealmsError>| {
            let job = Arc::new(Mutex::new(Some(result)));
            screen.joining = Some((job, Arc::new(AtomicBool::new(false))));
            screen.subscreen = SubScreen::Connecting {
                server: server(7, None),
                error: None,
            };
            screen.poll()
        };
        assert!(
            matches!(start(&mut screen, Ok(answer("1.2.3.4:25565", false))), Outcome::Connect(a) if a == "1.2.3.4:25565")
        );
        assert!(matches!(screen.subscreen, SubScreen::Main));

        assert!(matches!(
            start(&mut screen, Ok(answer("http://evil:25565", false))),
            Outcome::None
        ));
        assert!(matches!(
            screen.subscreen,
            SubScreen::Connecting { error: Some(_), .. }
        ));

        assert!(matches!(
            start(&mut screen, Ok(answer("1.2.3.4:25565", true))),
            Outcome::None
        ));
        match &screen.subscreen {
            SubScreen::Popup(Popup::ResourcePack { pack }) => {
                assert_eq!(pack.address, "1.2.3.4:25565");
                assert_eq!(pack.id, pack_id(&server(7, None)));
            }
            other => panic!("{other:?}"),
        }

        assert!(matches!(
            start(&mut screen, Err(RealmsError::Timeout)),
            Outcome::None
        ));
        assert!(matches!(
            screen.subscreen,
            SubScreen::Connecting { error: Some(_), .. }
        ));
    }

    #[test]
    fn cancelling_a_join_aborts_its_worker() {
        let mut screen = listed(vec![server(7, None)]);
        let aborted = Arc::new(AtomicBool::new(false));
        screen.joining = Some((Arc::new(Mutex::new(None)), Arc::clone(&aborted)));
        screen.subscreen = SubScreen::Connecting {
            server: server(7, None),
            error: None,
        };
        let _ = screen.click(Some(52), 0.0, 0.0, (800.0, 600.0));
        assert!(aborted.load(Ordering::Relaxed));
        assert!(screen.joining.is_none());
        assert!(matches!(screen.subscreen, SubScreen::Main));
    }

    #[test]
    fn pack_ids_are_javas_name_uuids() {
        // UUID.nameUUIDFromBytes("realms:Friends:1"): MD5 with the version-3 bits,
        // computed outside Rust.
        assert_eq!(
            horizon_protocol::crypto::uuid_string(pack_id(&server(7, None))),
            "9b833ced-6630-3283-b4a6-778948c32b5f"
        );
    }

    #[test]
    fn without_a_session_nothing_is_requested() {
        let screen = Screen::new();
        assert!(matches!(screen.state, FetchState::Unauthenticated));
        let buttons = screen.buttons(800.0, 600.0);
        let enabled = |id| buttons.iter().find(|b| b.5 == id).map(|b| b.4);
        assert_eq!(enabled(14), Some(false), "Join Realm");
        assert_eq!(enabled(15), Some(false), "invites");
    }

    #[test]
    fn join_realm_button_navigates_to_join_code_screen() {
        let mut screen = listed(vec![]);
        let _ = screen.click(Some(14), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(screen.subscreen, SubScreen::JoinCode { .. }));
        let btns = screen.buttons(800.0, 600.0);
        let join_btn = btns.iter().find(|b| b.5 == 20).expect("Join button exists");
        assert!(!join_btn.4, "Join is off while the code is empty");
        assert!(!screen.escape());
        assert!(matches!(screen.subscreen, SubScreen::Main));
    }

    /// `RealmsConfigureWorldScreen`'s `MenuTabBar`: Worlds, Players, Subscription,
    /// Settings, and an expired Realm's settings tab is off.
    #[test]
    fn configure_tabs_follow_the_vanilla_order_and_expiry() {
        assert_eq!(
            ConfigureTab::ALL,
            [
                ConfigureTab::Worlds,
                ConfigureTab::Players,
                ConfigureTab::Subscription,
                ConfigureTab::Settings,
            ]
        );
        for tab in ConfigureTab::ALL {
            assert!(tab.enabled(false), "{}", tab.title());
        }
        assert!(!ConfigureTab::Settings.enabled(true));
        for tab in [
            ConfigureTab::Worlds,
            ConfigureTab::Players,
            ConfigureTab::Subscription,
        ] {
            assert!(tab.enabled(true), "{}", tab.title());
        }
    }

    /// `MenuTabBar.builder(this, width)`: at most 400 wide, centred, four even tabs.
    #[test]
    fn the_tab_bar_is_centred_and_four_tabs_wide() {
        // 854x480 at GUI scale 2 is 427x240: 372 inner width, 28 from the left, 94 a tab.
        assert_eq!(tab_geometry(427.0), (28.0, 94.0));
        // A window narrower than 400 keeps its own width less the bar's margins:
        // 272 inner width over four tabs, 14 from the left.
        assert_eq!(tab_geometry(300.0), (14.0, 68.0));
    }

    /// `RealmsSubscriptionTab.daysLeftPresentation`, compared with the same translations
    /// so the check reads in any language.
    #[test]
    fn days_left_says_what_vanilla_says() {
        let t = crate::lang::get;
        assert_eq!(
            days_left_text(-1, true),
            t("mco.configure.world.subscription.expired")
        );
        let less = t("mco.configure.world.subscription.less_than_a_day");
        assert_eq!(days_left_text(0, false), less);
        assert_eq!(days_left_text(1, false), less);
        assert_eq!(
            days_left_text(45, false),
            crate::lang::tr(
                "mco.configure.world.subscription.remaining.months.days",
                &[&1, &15]
            )
        );
        assert_eq!(
            days_left_text(60, false),
            crate::lang::tr("mco.configure.world.subscription.remaining.months", &[&2])
        );
        assert_eq!(
            days_left_text(5, false),
            crate::lang::tr("mco.configure.world.subscription.remaining.days", &[&5])
        );
        // An expired Realm still shows its remaining time while it has some.
        assert_eq!(
            days_left_text(60, true),
            crate::lang::tr("mco.configure.world.subscription.remaining.months", &[&2])
        );
    }

    /// An unreadable start date says nothing rather than a wrong date.
    #[test]
    fn an_unreadable_start_date_is_empty() {
        assert_eq!(start_date_text(i64::MAX), "");
    }
}
