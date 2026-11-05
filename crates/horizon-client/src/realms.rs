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
mod transport;
mod workers;
pub(crate) use workers::Workers;
pub use workers::{poll_retired_workers, shutdown_workers};

use crate::hud::{HudAtlas, HudBatch, rgba};
use crate::text::Text;
use client::{
    PendingInviteDto, PlayerInfo, RealmAddress, RealmServer, RealmSlot, RealmWorldOptions,
    RealmsClient, RealmsError, RegionDataDto, RegionSelectionPreferenceDto, Subscription,
};
use horizon_client_render::auth::{self, AuthSession};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER_HEIGHT: f32 = 44.0;
const CONTENT_WIDTH: f32 = 308.0;
const ROW_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 36.0;
const PLAYERS_TOP: f32 = 60.0;
const PLAYERS_BOTTOM: f32 = 68.0;
// Two controls per visible row, inside the u8 widget id space 100..=255.
const MAX_PLAYER_ROWS: usize = (u8::MAX as usize + 1 - 100) / 2;
/// `RealmsConfigureWorldScreen`'s `MenuTabBar` height (`gui/tab_header_background`).
const TAB_HEIGHT: f32 = 24.0;
/// `hud::rgba` packing (ABGR): 160 grey, vanilla's red and `RealmsPopups.COLOR_INFO`.
const GREY: u32 = 0xFFA0_A0A0;
const RED: u32 = 0xFF55_55FF;
const INFO: u32 = 0xFFBE_887D;

const REALMS_INVITE_CODE: crate::clipboard::FieldId = ("realms.invite_code", 0);
const REALMS_PLAYER_NAME: crate::clipboard::FieldId = ("realms.player_name", 0);
const REALMS_SLOT_NAME: crate::clipboard::FieldId = ("realms.slot_name", 0);
const REALMS_SETTINGS_NAME: crate::clipboard::FieldId = ("realms.settings_name", 0);
const REALMS_SETTINGS_DESC: crate::clipboard::FieldId = ("realms.settings_desc", 0);

/// `RealmsSlotOptionsScreen.DIFFICULTIES` and `GAME_MODES`, in menu order (the index
/// is what the service stores).
const DIFFICULTIES: [&str; 4] = [
    "options.difficulty.peaceful",
    "options.difficulty.easy",
    "options.difficulty.normal",
    "options.difficulty.hard",
];
const GAME_MODES: [&str; 3] = [
    "selectWorld.gameMode.survival",
    "selectWorld.gameMode.creative",
    "selectWorld.gameMode.adventure",
];
const DIFFICULTY_DEFAULT: usize = 2;
const GAME_MODE_DEFAULT: usize = 0;

/// `CommonLinks.BUY_REALMS`.
const BUY_REALMS: &str = "https://aka.ms/BuyJavaRealms";
/// `CommonLinks.EXTEND_REALMS_LINK`.
const EXTEND_REALMS: &str = "https://aka.ms/ExtendJavaRealms";

/// A result future; worker lifetime is owned separately by its title service.
type Job<T> = workers::Job<T>;

fn take<T>(job: &Job<T>) -> Option<T> {
    job.try_lock().ok()?.take()
}

/// The account Realms may use: the launcher's Microsoft session (it hands one over only for
/// an account that owns the game) while its token is good.
fn session() -> Option<Arc<AuthSession>> {
    auth::current().filter(|s| !s.expired())
}

fn client(workers: &Workers) -> Option<RealmsClient> {
    session().map(|s| RealmsClient::production(&s).with_cancellation(workers.cancellation()))
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

#[derive(Clone, Debug)]
enum SettingsNext {
    Tab(ConfigureTab),
    List,
    Toggle(bool),
}
type SettingsAck = (SettingsDraft, Result<Option<bool>, RealmsError>);

/// `RealmsSettingsTab`'s fields: the name, the description and the region preference,
/// as the player is editing them.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsDraft {
    pub name: String,
    pub description: String,
    /// `RegionSelectionPreference`'s id: [`client::AUTOMATIC_PLAYER`],
    /// [`client::AUTOMATIC_OWNER`] or [`client::MANUAL`].
    pub preference: i32,
    /// The `RealmsRegion`'s `nameId`, while `preference` is `MANUAL`.
    pub region: Option<String>,
    /// The field with the caret: the name (false) or the description (true).
    pub on_description: bool,
}

impl SettingsDraft {
    /// The draft the tab opens with (`RealmsSettingsTab.updateData`): a Realm without a
    /// preference has `RegionSelectionPreferenceDto.DEFAULT`, automatic by owner.
    fn of(server: &RealmServer) -> Self {
        let (preference, region) = server.region_selection_preference.as_ref().map_or(
            (client::AUTOMATIC_OWNER, None),
            |p| {
                (
                    p.region_selection_preference,
                    p.preferred_region.clone().filter(|r| !r.is_empty()),
                )
            },
        );
        Self {
            name: server.name.clone(),
            description: server.motd.clone(),
            preference,
            region,
            on_description: false,
        }
    }
    /// The preference as `RegionSelectionPreferenceDto`: the region only for `MANUAL`.
    fn preference_dto(&self) -> RegionSelectionPreferenceDto {
        RegionSelectionPreferenceDto {
            region_selection_preference: self.preference,
            preferred_region: self
                .region
                .clone()
                .filter(|_| self.preference == client::MANUAL),
        }
    }
}

/// `RealmsSettingsTab.getTranslatableFromPreference`: a manual choice shows its region,
/// the automatic ones their own name.
fn preference_label(preference: i32, region: Option<&str>) -> String {
    let t = crate::lang::get;
    match (preference, region) {
        (client::MANUAL, Some(region)) => {
            client::region_translation_key(region).map_or_else(|| region.to_owned(), t)
        }
        (client::AUTOMATIC_PLAYER, _) => {
            t("realms.configuration.region_preference.automatic_player")
        }
        _ => t("realms.configuration.region_preference.automatic_owner"),
    }
}

/// `RealmsPreferredRegionSelectionScreen.RegionSelectionList`'s rows: automatic by player,
/// automatic by owner, then every region as a manual choice.
fn region_rows(regions: &[RegionDataDto]) -> Vec<(i32, Option<String>)> {
    [
        (client::AUTOMATIC_PLAYER, None),
        (client::AUTOMATIC_OWNER, None),
    ]
    .into_iter()
    .chain(
        regions
            .iter()
            .map(|r| (client::MANUAL, Some(r.region.clone()))),
    )
    .collect()
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

/// `RealmsWorldsTab`'s four slot buttons: the active slot is 80 square, the others 50,
/// spaced 16 and centred (`GridLayout.spacing(16)`).
fn slot_boxes(w: f32, active_slot: i32) -> [(f32, f32, i32); 4] {
    let sizes: [f32; 4] = std::array::from_fn(|i| {
        if i as i32 + 1 == active_slot {
            80.0
        } else {
            50.0
        }
    });
    let total: f32 = sizes.iter().sum::<f32>() + 16.0 * 3.0;
    let mut x = ((w - total) / 2.0).floor();
    std::array::from_fn(|i| {
        let at = (x, sizes[i], i as i32 + 1);
        x += sizes[i] + 16.0;
        at
    })
}

/// `RealmsWorldOptions.getSlotName`: the player's own name, else the empty marker, else
/// "World N" by the slot's number.
fn slot_name(options: &RealmWorldOptions, slot: i32) -> String {
    if !options.slot_name.trim().is_empty() {
        return options.slot_name.clone();
    }
    if options.empty {
        return crate::lang::get("mco.configure.world.slot.empty");
    }
    crate::lang::tr("mco.configure.world.slot", &[&slot])
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
    Delete(RealmServer, Subscription),
    /// Closing an open Realm asks first; only Continue may save and close it.
    Close(RealmServer),
    /// `RealmsPlayersTab.PlayerEntry.uninvite`'s confirm (`RealmsConfirmScreen`).
    Uninvite {
        server: RealmServer,
        player: PlayerInfo,
    },
    /// `RealmsWorldsTab.switchToFullSlot`'s question, then `RealmsClient.switchSlot`.
    SwitchSlot { server: RealmServer, slot: i32 },
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
        /// `RealmsClient.getOwnRealm` on a worker, then its answer (the slots and players).
        detail: Option<Result<RealmServer, RealmsError>>,
        detail_job: Option<Job<Result<RealmServer, RealmsError>>>,
        /// `RealmsClient.deleteRealm` on a worker, from the subscription tab's question.
        deleting: Option<Job<Result<(), RealmsError>>>,
        /// `RealmsClient.op`/`deop`/`uninvite` on a worker, from the players tab.
        member_job: Option<Job<Result<(), RealmsError>>>,
        /// The settings tab's editable fields, once the tab has been opened.
        settings: Option<SettingsDraft>,
        /// `RealmsClient.preferredRegions` for the region preference screen.
        regions: Option<Result<Vec<RegionDataDto>, RealmsError>>,
        regions_job: Option<Job<Result<Vec<RegionDataDto>, RealmsError>>>,
        /// `RealmsClient.updateConfiguration` on a worker.
        saving: Option<Job<Result<SettingsAck, RealmsError>>>,
        error: Option<String>,
        /// `RealmsClient.open`/`close` on a worker.
        job: Option<Job<Result<(), RealmsError>>>,
    },
    /// `RealmsSlotOptionsScreen`: the active slot's name and world options.
    SlotOptions {
        server: RealmServer,
        slot: RealmSlot,
        name: String,
        difficulty: usize,
        game_mode: usize,
        force_game_mode: bool,
        spawn_protection: i32,
        /// `RealmsClient.updateSlot` on a worker.
        job: Option<Job<Result<(), RealmsError>>>,
        error: Option<String>,
    },
    /// `RealmsPreferredRegionSelectionScreen`: the region preference list.
    RegionPreference {
        server: RealmServer,
        /// The settings tab's draft, which Done writes the choice back to.
        settings: SettingsDraft,
        regions: Vec<RegionDataDto>,
        /// The highlighted row: `(preference, region)`.
        selected: Option<(i32, Option<String>)>,
    },
    /// `RealmsInviteScreen`: a name to invite to the Realm.
    Invite {
        server: RealmServer,
        /// The invited players as the tab had them (the duplicate check).
        players: Vec<PlayerInfo>,
        name: String,
        /// `RealmsClient.invite` on a worker, then the players.
        job: Option<Job<Result<Vec<PlayerInfo>, RealmsError>>>,
        error: Option<String>,
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

/// The identity and operation last laid out for a visible player row. A
/// roster refresh must not redirect a click to whoever now has its old index.
#[derive(Clone, Debug)]
struct PlayerRow {
    button: u8,
    realm_id: i64,
    player: PlayerInfo,
}

#[derive(Debug)]
struct SlotIntent {
    aborted: Arc<AtomicBool>,
    previous_error: Option<String>,
}
impl Drop for SlotIntent {
    fn drop(&mut self) {
        // Slot retries belong to their cancellable task screen. Ordinary
        // confirmed invites and permission operations keep their own lifetime.
        self.aborted.store(true, Ordering::Release);
    }
}

#[derive(Clone, Debug)]
pub struct Screen {
    pub state: FetchState,
    pub subscreen: SubScreen,
    pub selected: Option<usize>,
    pub joining: Option<Joining>,
    /// Leaving a Realm (`RealmsMainScreen.leaveServer`): the list reloads when it's done.
    leaving: Option<Job<Result<(), RealmsError>>>,
    workers: Workers,
    /// The configure screen behind an invite or confirmation, including its
    /// loaded subscription, players and in-flight jobs (`Screen.onClose`).
    return_subscreen: Option<Box<SubScreen>>,
    slot_switch: Option<Arc<SlotIntent>>,
    settings_next: Option<SettingsNext>,
    region_scroll: f32,
    players_scroll: f32,
    player_rows: RefCell<Vec<PlayerRow>>,
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
        if client.cancelled() {
            return Err(RealmsError::Cancelled);
        }
        if aborted.load(Ordering::Relaxed) {
            return Err(RealmsError::Timeout);
        }
        match client.join_world(realm_id) {
            Err(RealmsError::Retry(seconds)) => {
                let until = Instant::now() + Duration::from_secs(seconds.min(60));
                while Instant::now() < until
                    && !aborted.load(Ordering::Relaxed)
                    && !client.cancelled()
                {
                    thread::sleep(Duration::from_millis(100));
                }
            }
            other => return other,
        }
    }
    Err(RealmsError::Timeout)
}

/// `SwitchSlotTask.run`: false means not switched yet, never completion. The
/// native task makes at most 25 attempts and waits after RetryCallException.
fn switch_slot_task(
    client: &RealmsClient,
    realm: i64,
    slot: i32,
    aborted: &AtomicBool,
    pause: impl FnMut(Duration) -> Result<(), RealmsError>,
) -> Result<(), RealmsError> {
    boolean_realm_task(client, aborted, || client.switch_slot(realm, slot), pause)
}

fn set_realm_open_task(client: &RealmsClient, realm: i64, open: bool) -> Result<(), RealmsError> {
    let aborted = AtomicBool::new(false);
    boolean_realm_task(
        client,
        &aborted,
        || {
            if open {
                client.open_realm(realm)
            } else {
                client.close_realm(realm)
            }
        },
        |delay| wait_for_retry(client, &aborted, delay),
    )
}

fn boolean_realm_task(
    client: &RealmsClient,
    aborted: &AtomicBool,
    mut request: impl FnMut() -> Result<bool, RealmsError>,
    mut pause: impl FnMut(Duration) -> Result<(), RealmsError>,
) -> Result<(), RealmsError> {
    for _ in 0..25 {
        if client.cancelled() || aborted.load(Ordering::Acquire) {
            return Err(RealmsError::Cancelled);
        }
        let answer = request();
        if client.cancelled() || aborted.load(Ordering::Acquire) {
            return Err(RealmsError::Cancelled);
        }
        match answer {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(RealmsError::Retry(seconds)) => {
                // RetryCallException accepts 0..=120, otherwise its delay is 5.
                let seconds = if seconds <= 120 { seconds } else { 5 };
                pause(Duration::from_secs(seconds))?;
            }
            Err(error) => return Err(error),
        }
    }
    Err(RealmsError::Timeout)
}

fn wait_for_retry(
    client: &RealmsClient,
    aborted: &AtomicBool,
    delay: Duration,
) -> Result<(), RealmsError> {
    let until = Instant::now() + delay;
    loop {
        if client.cancelled() || aborted.load(Ordering::Acquire) {
            return Err(RealmsError::Cancelled);
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        thread::sleep(left.min(Duration::from_millis(25)));
    }
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
        Self::with_workers(Workers::new())
    }

    pub(crate) fn with_workers(workers: Workers) -> Self {
        let mut screen = Self {
            state: FetchState::Unauthenticated,
            subscreen: SubScreen::Main,
            selected: None,
            joining: None,
            leaving: None,
            workers,
            return_subscreen: None,
            slot_switch: None,
            settings_next: None,
            region_scroll: 0.0,
            players_scroll: 0.0,
            player_rows: RefCell::new(Vec::new()),
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
        self.state = match client(&self.workers) {
            Some(client) => FetchState::Loading(self.workers.spawn(move || client.list_worlds())),
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
        let mut refetch_detail = false;
        let mut slot_options_saved = false;
        let mut open_region_screen = false;
        let mut settings_done = None;
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
                detail,
                detail_job,
                deleting,
                member_job,
                regions,
                regions_job,
                saving,
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
                if let Some(result) = detail_job.as_ref().and_then(take) {
                    *detail_job = None;
                    *detail = Some(result);
                }
                if let Some(result) = member_job.as_ref().and_then(take) {
                    *member_job = None;
                    self.slot_switch = None;
                    match result {
                        // `updateOps`: the list reads again with the new operators.
                        Ok(()) => {
                            *error = None;
                            refetch_detail = true;
                        }
                        Err(e) => *error = Some(e.text()),
                    }
                }
                if let Some(result) = regions_job.as_ref().and_then(take) {
                    *regions_job = None;
                    match result {
                        Ok(list) => {
                            *regions = Some(Ok(list));
                            open_region_screen = true;
                        }
                        Err(e) => *error = Some(e.text()),
                    }
                }
                if let Some(result) = saving.as_ref().and_then(take) {
                    *saving = None;
                    match result {
                        Ok((saved, opened)) => {
                            // A metadata read started before this save cannot
                            // replace its acknowledged values on a later frame.
                            *detail_job = None;
                            let open = opened.as_ref().ok().copied().flatten();
                            server.name = saved.name.clone();
                            server.motd = saved.description.clone();
                            server.region_selection_preference = Some(saved.preference_dto());
                            if let Some(open) = open {
                                server.state = if open { "OPEN" } else { "CLOSED" }.into();
                            }
                            if let Some(Ok(detail)) = detail {
                                detail.name = server.name.clone();
                                detail.motd = server.motd.clone();
                                detail.region_selection_preference =
                                    server.region_selection_preference.clone();
                                if open.is_some() {
                                    detail.state = server.state.clone();
                                }
                            }
                            let next = match opened {
                                Ok(_) => self.settings_next.take(),
                                Err(e) => {
                                    self.settings_next = None;
                                    *error = Some(e.text());
                                    None
                                }
                            };
                            settings_done = Some((server.clone(), next));
                        }
                        Err(e) => {
                            self.settings_next = None;
                            *error = Some(e.text());
                        }
                    }
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
            SubScreen::SlotOptions { job, error, .. } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        // `saveSlotSettings`'s `setScreen(this)`.
                        Ok(()) => slot_options_saved = true,
                        Err(e) => *error = Some(e.text()),
                    }
                }
            }
            SubScreen::Invite {
                server, job, error, ..
            } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        Ok(_players) => {
                            // `invitePlayer` succeeded: back to the players tab, which
                            // reads the list again.
                            let server = server.clone();
                            self.return_subscreen = None;
                            self.subscreen = SubScreen::Configure {
                                server,
                                tab: ConfigureTab::Players,
                                subscription: None,
                                subscription_job: None,
                                detail: None,
                                detail_job: None,
                                deleting: None,
                                member_job: None,
                                settings: None,
                                regions: None,
                                regions_job: None,
                                saving: None,
                                error: None,
                                job: None,
                            };
                            self.start_configure_subscription();
                            self.start_configure_detail();
                        }
                        Err(e) => *error = Some(e.text()),
                    }
                }
            }
            SubScreen::Connecting { .. } => return self.poll_join(),
            _ => {}
        }
        if let Some((saved, next)) = settings_done {
            if let FetchState::Loaded(list) = &mut self.state
                && let Some(realm) = list.iter_mut().find(|r| r.id == saved.id)
            {
                realm.name = saved.name.clone();
                realm.motd = saved.motd.clone();
                realm.region_selection_preference = saved.region_selection_preference.clone();
                realm.state = saved.state.clone();
            }
            match next {
                Some(SettingsNext::Tab(tab)) => self.apply_configure_tab(tab),
                Some(SettingsNext::List) => self.back_to_list_now(),
                _ => {}
            }
        }
        self.prepare_settings_draft();
        if open_region_screen {
            self.open_region_preference();
        }
        if slot_options_saved {
            // Success reloads updated settings; cancellation alone restores
            // the unchanged parent and its existing jobs.
            self.return_subscreen = None;
            self.back_to_worlds();
        }
        if refetch_detail {
            // Any previous detail request predates the accepted mutation and
            // must not suppress or overwrite its completion refresh.
            if let SubScreen::Configure { detail_job, .. } = &mut self.subscreen {
                *detail_job = None;
            }
            self.start_configure_detail();
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
        match &self.subscreen {
            SubScreen::JoinCode { .. } => Some([(w / 2.0).floor() - 100.0, 64.0, 200.0, 20.0]),
            SubScreen::Invite { .. } => Some([(w / 2.0).floor() - 100.0, 62.0, 200.0, 20.0]),
            SubScreen::SlotOptions { .. } => Some([(w / 2.0).floor() - 154.0, 62.0, 152.0, 20.0]),
            SubScreen::Configure {
                tab: ConfigureTab::Settings,
                settings: Some(draft),
                saving: None,
                ..
            } => {
                let left = (w / 2.0).floor() - 106.0;
                Some(if draft.on_description {
                    [left, 86.0, 212.0, 20.0]
                } else {
                    [left, 46.0, 212.0, 20.0]
                })
            }
            _ => None,
        }
    }

    pub fn focused_text(&mut self) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
        match &mut self.subscreen {
            SubScreen::JoinCode { code, .. } => Some((REALMS_INVITE_CODE, code, 128)),
            SubScreen::Invite { name, .. } => Some((REALMS_PLAYER_NAME, name, 16)),
            SubScreen::SlotOptions { name, .. } => Some((REALMS_SLOT_NAME, name, 16)),
            SubScreen::Configure {
                tab: ConfigureTab::Settings,
                settings: Some(draft),
                saving: None,
                ..
            } => Some(if draft.on_description {
                (REALMS_SETTINGS_DESC, &mut draft.description, 32)
            } else {
                (REALMS_SETTINGS_NAME, &mut draft.name, 32)
            }),
            _ => None,
        }
    }

    pub fn tab(&mut self) {
        if let SubScreen::Configure {
            tab: ConfigureTab::Settings,
            settings: Some(draft),
            saving: None,
            ..
        } = &mut self.subscreen
        {
            draft.on_description = !draft.on_description;
        }
    }

    /// Escape returns configure children to their parent; other sub-screens go
    /// to the list (cancelling a join), and the list returns to the title (true).
    pub fn escape(&mut self) -> bool {
        if self.settings_next.is_some() {
            return false;
        }
        if let SubScreen::RegionPreference {
            server, settings, ..
        } = &self.subscreen
        {
            self.back_to_settings(server.clone(), settings.clone());
            return false;
        }
        if self.cancel_slot_switch() {
            return false;
        }
        if matches!(self.subscreen, SubScreen::Main) {
            return true;
        }
        if matches!(self.subscreen, SubScreen::Invite { .. }) {
            self.back_to_players();
        } else if matches!(self.subscreen, SubScreen::SlotOptions { .. }) {
            self.back_to_worlds();
        } else if !self.cancel_configure_popup() {
            self.back_to_list();
        }
        false
    }

    fn enter_configure_child(&mut self, child: SubScreen) {
        self.return_subscreen = Some(Box::new(std::mem::replace(&mut self.subscreen, child)));
    }

    fn restore_configure_parent(&mut self) -> bool {
        if let Some(parent) = self.return_subscreen.take() {
            self.subscreen = *parent;
            true
        } else {
            false
        }
    }

    /// PopupScreen.onClose and a negative RealmsConfirmScreen answer restore
    /// the same configure screen, including its loaded data and pending work.
    fn cancel_configure_popup(&mut self) -> bool {
        if matches!(
            self.subscreen,
            SubScreen::Popup(
                Popup::Delete(..)
                    | Popup::Close(_)
                    | Popup::Uninvite { .. }
                    | Popup::SwitchSlot { .. }
            )
        ) && self.restore_configure_parent()
        {
            return true;
        }
        let SubScreen::Popup(Popup::Delete(server, subscription)) = &self.subscreen else {
            return false;
        };
        self.subscreen = SubScreen::Configure {
            server: server.clone(),
            tab: ConfigureTab::Subscription,
            subscription: Some(Ok(subscription.clone())),
            subscription_job: None,
            settings: None,
            regions: None,
            regions_job: None,
            saving: None,
            detail: None,
            detail_job: None,
            deleting: None,
            member_job: None,
            error: None,
            job: None,
        };
        true
    }

    fn back_to_list(&mut self) {
        if self.begin_settings_transition(SettingsNext::List) {
            return;
        }
        self.back_to_list_now();
    }

    fn back_to_list_now(&mut self) {
        self.cancel_slot_switch();
        if let Some((_, aborted)) = self.joining.take() {
            aborted.store(true, Ordering::Relaxed);
        }
        let reload = matches!(self.subscreen, SubScreen::PendingInvites { .. });
        self.return_subscreen = None;
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
        let Some(client) = client(&self.workers) else {
            return Outcome::None;
        };
        let aborted = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&aborted);
        let id = server.id;
        let job = self
            .workers
            .spawn(move || fetch_address(&client, id, &flag));
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
        if let Some(client) = client(&self.workers) {
            let id = server.id;
            *subscription_job = Some(self.workers.spawn(move || client.subscription_for(id)));
        }
    }

    /// `RealmsConfigureWorldScreen.init`'s `fetchServerData`: the slots and players, on a
    /// worker.
    fn start_configure_detail(&mut self) {
        let SubScreen::Configure {
            server, detail_job, ..
        } = &mut self.subscreen
        else {
            return;
        };
        if detail_job.is_some() {
            return;
        }
        if let Some(client) = client(&self.workers) {
            let id = server.id;
            *detail_job = Some(self.workers.spawn(move || client.get_own_realm(id)));
        }
    }

    /// `RealmsInviteScreen.onInvite`: a blank name or one already invited is refused
    /// before any request; the rest goes to `RealmsClient.invite` on a worker.
    fn invite_clicked(&mut self) {
        let (name, id, already) = match &self.subscreen {
            SubScreen::Invite {
                server,
                players,
                name,
                job,
                ..
            } if job.is_none() => (
                name.trim().to_owned(),
                server.id,
                players
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case(name.trim())),
            ),
            _ => return,
        };
        let refusal = if name.is_empty() {
            Some("mco.configure.world.players.error")
        } else if already {
            Some("mco.configure.world.players.invite.duplicate")
        } else {
            None
        };
        if let SubScreen::Invite { job, error, .. } = &mut self.subscreen {
            if let Some(key) = refusal {
                *error = Some(crate::lang::get(key));
                return;
            }
            if let Some(client) = client(&self.workers) {
                *error = None;
                *job = Some(self.workers.spawn(move || client.invite_player(id, &name)));
            }
        }
    }

    /// A chosen region preference: back to the settings tab with it.
    fn back_to_settings(&mut self, server: RealmServer, settings: SettingsDraft) {
        if self.restore_configure_parent() {
            if let SubScreen::Configure {
                settings: draft, ..
            } = &mut self.subscreen
            {
                *draft = Some(settings);
            }
            return;
        }
        self.subscreen = SubScreen::Configure {
            server,
            tab: ConfigureTab::Settings,
            subscription: None,
            subscription_job: None,
            detail: None,
            detail_job: None,
            deleting: None,
            member_job: None,
            settings: Some(settings),
            regions: None,
            regions_job: None,
            saving: None,
            error: None,
            job: None,
        };
        self.start_configure_subscription();
        self.start_configure_detail();
    }

    /// `RealmsSettingsTab.onDeselected`: the edited name, description and region preference,
    /// saved through `RealmsClient.updateConfiguration` when any of them changed.
    fn begin_settings_transition(&mut self, next: SettingsNext) -> bool {
        if !matches!(
            self.subscreen,
            SubScreen::Configure {
                tab: ConfigureTab::Settings,
                ..
            }
        ) {
            return false;
        }
        if self.settings_next.is_some() {
            return true;
        }
        match client(&self.workers) {
            Some(client) => self.begin_settings_using(client, next),
            None => false,
        }
    }

    fn begin_settings_using(&mut self, client: RealmsClient, next: SettingsNext) -> bool {
        let (realm, snapshot, slot, changed) = match &self.subscreen {
            SubScreen::Configure {
                server,
                detail: Some(Ok(detail)),
                settings: Some(draft),
                saving: None,
                ..
            } => {
                let Some(slot) = detail.slots.get(&detail.active_slot).cloned() else {
                    return false;
                };
                let mut snapshot = draft.clone();
                snapshot.name = snapshot.name.trim().to_owned();
                if snapshot.description.trim().is_empty() {
                    snapshot.description.clear();
                }
                let wanted = snapshot.preference_dto();
                let before = server
                    .region_selection_preference
                    .clone()
                    .unwrap_or_default();
                let changed = snapshot.name != server.name
                    || snapshot.description != server.motd
                    || wanted != before;
                (server.id, snapshot, slot, changed)
            }
            SubScreen::Configure {
                saving: Some(_), ..
            } => return true,
            _ => return false,
        };
        let toggle = match next {
            SettingsNext::Toggle(open) => Some(open),
            _ => None,
        };
        if !changed && toggle.is_none() {
            return false;
        }
        let client = client.with_cancellation(self.workers.cancellation());
        let work = self.workers.spawn(move || {
            if changed {
                client.update_configuration(
                    realm,
                    &snapshot.name,
                    &snapshot.description,
                    &snapshot.preference_dto(),
                    &slot,
                )?;
            }
            let opened = match toggle {
                Some(open) => set_realm_open_task(&client, realm, open).map(|()| Some(open)),
                None => Ok(None),
            };
            Ok((snapshot, opened))
        });
        if let SubScreen::Configure { saving, error, .. } = &mut self.subscreen {
            *saving = Some(work);
            *error = None;
        }
        self.settings_next = Some(next);
        true
    }

    fn apply_configure_tab(&mut self, wanted: ConfigureTab) {
        if let SubScreen::Configure { tab, .. } = &mut self.subscreen {
            *tab = wanted;
        }
        self.prepare_settings_draft();
    }

    fn select_configure_tab(&mut self, wanted: ConfigureTab) {
        let SubScreen::Configure { server, tab, .. } = &self.subscreen else {
            return;
        };
        if !wanted.enabled(server.expired) || *tab == wanted {
            return;
        }
        if *tab == ConfigureTab::Settings
            && self.begin_settings_transition(SettingsNext::Tab(wanted))
        {
            return;
        }
        self.apply_configure_tab(wanted);
    }

    fn prepare_settings_draft(&mut self) {
        if let SubScreen::Configure {
            server,
            tab: ConfigureTab::Settings,
            detail: Some(Ok(detail)),
            settings,
            ..
        } = &mut self.subscreen
            && settings.is_none()
        {
            server.name = detail.name.clone();
            server.motd = detail.motd.clone();
            server.region_selection_preference = detail.region_selection_preference.clone();
            *settings = Some(SettingsDraft::of(detail));
        }
    }

    /// `RealmsSettingsTab.openPreferenceSelector`, once the region list is there.
    fn open_region_preference(&mut self) {
        if self.settings_next.is_some() {
            return;
        }
        let SubScreen::Configure {
            server,
            tab: ConfigureTab::Settings,
            settings: Some(draft),
            regions: Some(Ok(list)),
            ..
        } = &self.subscreen
        else {
            return;
        };
        let (server, settings, regions) = (server.clone(), draft.clone(), list.clone());
        let selected = Some((settings.preference, settings.region.clone()));
        self.region_scroll = selected
            .as_ref()
            .and_then(|wanted| region_rows(&regions).iter().position(|row| row == wanted))
            .unwrap_or(0) as f32;
        self.enter_configure_child(SubScreen::RegionPreference {
            server,
            settings,
            regions,
            selected,
        });
    }

    /// `RealmsSlotOptionsScreen.onClose`: back to the worlds tab.
    fn back_to_worlds(&mut self) {
        let SubScreen::SlotOptions { server, .. } = &self.subscreen else {
            return;
        };
        let server = server.clone();
        if self.restore_configure_parent() {
            return;
        }
        self.subscreen = SubScreen::Configure {
            server,
            tab: ConfigureTab::Worlds,
            subscription: None,
            subscription_job: None,
            detail: None,
            detail_job: None,
            deleting: None,
            member_job: None,
            settings: None,
            regions: None,
            regions_job: None,
            saving: None,
            error: None,
            job: None,
        };
        self.start_configure_subscription();
        self.start_configure_detail();
    }

    /// `RealmsSlotOptionsScreen.saveSettings`: the edited slot, saved on a worker. A world
    /// that is not NORMAL keeps its stored spawn protection and force-game-mode, as
    /// vanilla does.
    fn save_slot_options(&mut self) {
        let SubScreen::SlotOptions {
            server,
            slot,
            name,
            difficulty,
            game_mode,
            force_game_mode,
            spawn_protection,
            job,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        if job.is_some() {
            return;
        }
        let normal = server.world_type == "NORMAL";
        // `setWorldName`: the default name is stored as empty.
        let default_name = crate::lang::tr(
            "mco.configure.world.slot",
            &[&slot.slot_id as &dyn std::fmt::Display],
        );
        let trimmed = name.trim();
        let slot_name = if trimmed == default_name {
            String::new()
        } else {
            trimmed.to_owned()
        };
        let updated = RealmSlot {
            slot_id: slot.slot_id,
            options: RealmWorldOptions {
                spawn_protection: if normal {
                    *spawn_protection
                } else {
                    slot.options.spawn_protection
                },
                force_game_mode: if normal {
                    *force_game_mode
                } else {
                    slot.options.force_game_mode
                },
                difficulty: *difficulty as i32,
                game_mode: *game_mode as i32,
                slot_name,
                version: slot.options.version.clone(),
                compatibility: slot.options.compatibility.clone(),
                template_id: slot.options.template_id,
                template_image: slot.options.template_image.clone(),
                empty: slot.options.empty,
            },
            settings: slot.settings.clone(),
        };
        if let Some(client) = client(&self.workers) {
            let id = server.id;
            *job = Some(self.workers.spawn(move || client.update_slot(id, &updated)));
        }
    }

    /// `RealmsInviteScreen.onClose`: back to the players tab.
    fn back_to_players(&mut self) {
        let SubScreen::Invite { server, .. } = &self.subscreen else {
            return;
        };
        let server = server.clone();
        if self.restore_configure_parent() {
            return;
        }
        self.subscreen = SubScreen::Configure {
            server,
            tab: ConfigureTab::Players,
            subscription: None,
            subscription_job: None,
            detail: None,
            detail_job: None,
            deleting: None,
            member_job: None,
            settings: None,
            regions: None,
            regions_job: None,
            saving: None,
            error: None,
            job: None,
        };
        self.start_configure_subscription();
        self.start_configure_detail();
    }

    pub fn click(&mut self, hit: Option<u8>, x: f32, y: f32, (w, h): (f32, f32)) -> Outcome {
        if self.settings_next.is_some() {
            return Outcome::None;
        }
        if self.slot_switch.is_some() && hit != Some(32) {
            return Outcome::None;
        }
        match hit {
            Some(1) => self.play_selected(),
            Some(2) => Outcome::Back,
            Some(10) => {
                if let Some(server) = self.selected_server().cloned() {
                    self.players_scroll = 0.0;
                    self.player_rows.get_mut().clear();
                    self.subscreen = SubScreen::Configure {
                        server,
                        tab: ConfigureTab::Worlds,
                        subscription: None,
                        subscription_job: None,
                        detail: None,
                        detail_job: None,
                        deleting: None,
                        member_job: None,
                        settings: None,
                        regions: None,
                        regions_job: None,
                        saving: None,
                        error: None,
                        job: None,
                    };
                    self.start_configure_subscription();
                    self.start_configure_detail();
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
                let job =
                    client(&self.workers).map(|c| self.workers.spawn(move || c.pending_invites()));
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
                    match client(&self.workers) {
                        Some(client) => {
                            let code = code.trim().to_owned();
                            *error = None;
                            *job =
                                Some(self.workers.spawn(move || client.redeem_invite_code(&code)));
                        }
                        None => *error = Some(crate::lang::get("horizon.realms.not_signed_in")),
                    }
                }
                Outcome::None
            }
            Some(51) => {
                if !self.cancel_configure_popup() {
                    self.back_to_list();
                }
                Outcome::None
            }
            Some(32) => {
                if !self.cancel_slot_switch() {
                    self.back_to_list();
                }
                Outcome::None
            }
            Some(21) | Some(22) | Some(41) | Some(52) => {
                self.back_to_list();
                Outcome::None
            }
            Some(30) => {
                if let SubScreen::Configure {
                    server,
                    tab: ConfigureTab::Settings,
                    saving: None,
                    ..
                } = &self.subscreen
                {
                    if server.is_open() {
                        let popup = Popup::Close(server.clone());
                        self.enter_configure_child(SubScreen::Popup(popup));
                    } else {
                        self.begin_settings_transition(SettingsNext::Toggle(true));
                    }
                }
                Outcome::None
            }
            // The configuration screen's tabs (60..=63) and its footer: Play (64).
            Some(sel @ 60..=63) => {
                if let Some(wanted) = ConfigureTab::ALL.get(usize::from(sel - 60)) {
                    self.select_configure_tab(*wanted);
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
                    server,
                    subscription: Some(Ok(subscription)),
                    deleting,
                    ..
                } = &self.subscreen
                    && server.expired
                    && deleting.is_none()
                {
                    let popup = Popup::Delete(server.clone(), subscription.clone());
                    self.enter_configure_child(SubScreen::Popup(popup));
                }
                Outcome::None
            }
            Some(67) => {
                if let SubScreen::Configure { server, tab, .. } = &self.subscreen
                    && *tab == ConfigureTab::Players
                {
                    let players = match &self.subscreen {
                        SubScreen::Configure {
                            detail: Some(Ok(detail)),
                            ..
                        } => detail.players.clone(),
                        _ => Vec::new(),
                    };
                    let invite = SubScreen::Invite {
                        server: server.clone(),
                        players,
                        name: String::new(),
                        job: None,
                        error: None,
                    };
                    self.enter_configure_child(invite);
                }
                Outcome::None
            }
            // RealmsSlotOptionsScreen: the active slot's options.
            Some(94) => {
                if let SubScreen::Configure {
                    server,
                    detail: Some(Ok(detail)),
                    member_job: None,
                    job: None,
                    ..
                } = &self.subscreen
                    && let Some(slot) = detail.slots.get(&detail.active_slot)
                {
                    let normal = server.world_type == "NORMAL";
                    let options = SubScreen::SlotOptions {
                        server: server.clone(),
                        slot: slot.clone(),
                        name: slot.options.slot_name.clone(),
                        difficulty: usize::try_from(slot.options.difficulty)
                            .ok()
                            .filter(|d| *d < DIFFICULTIES.len())
                            .unwrap_or(DIFFICULTY_DEFAULT),
                        game_mode: usize::try_from(slot.options.game_mode)
                            .ok()
                            .filter(|m| *m < GAME_MODES.len())
                            .unwrap_or(GAME_MODE_DEFAULT),
                        force_game_mode: normal && slot.options.force_game_mode,
                        spawn_protection: if normal {
                            slot.options.spawn_protection
                        } else {
                            0
                        },
                        job: None,
                        error: None,
                    };
                    self.enter_configure_child(options);
                }
                Outcome::None
            }
            // RealmsPreferredRegionSelectionScreen.onDone / onClose.
            Some(87) => {
                if let SubScreen::RegionPreference {
                    server,
                    settings,
                    selected,
                    ..
                } = &self.subscreen
                    && let Some((preference, region)) = selected
                {
                    let server = server.clone();
                    let mut settings = settings.clone();
                    settings.preference = *preference;
                    settings.region = region.clone();
                    self.back_to_settings(server, settings);
                }
                Outcome::None
            }
            Some(86) => {
                if let SubScreen::RegionPreference {
                    server, settings, ..
                } = &self.subscreen
                {
                    let server = server.clone();
                    let settings = settings.clone();
                    self.back_to_settings(server, settings);
                }
                Outcome::None
            }
            // RealmsSettingsTab.openPreferenceSelector: the region list, then the screen.
            Some(89) => {
                if matches!(
                    self.subscreen,
                    SubScreen::Configure {
                        regions: Some(Ok(_)),
                        ..
                    }
                ) {
                    self.open_region_preference();
                } else if let SubScreen::Configure { regions_job, .. } = &mut self.subscreen
                    && regions_job.is_none()
                    && let Some(client) = client(&self.workers)
                {
                    *regions_job = Some(self.workers.spawn(move || client.preferred_regions()));
                }
                Outcome::None
            }

            // RealmsSlotOptionsScreen: cycle, toggle, Done and Cancel.
            Some(95) => {
                if let SubScreen::SlotOptions {
                    slot, difficulty, ..
                } = &mut self.subscreen
                    && !slot.is_hardcore()
                {
                    *difficulty = (*difficulty + 1) % DIFFICULTIES.len();
                }
                Outcome::None
            }
            Some(96) => {
                if let SubScreen::SlotOptions {
                    slot, game_mode, ..
                } = &mut self.subscreen
                    && !slot.is_hardcore()
                {
                    *game_mode = (*game_mode + 1) % GAME_MODES.len();
                }
                Outcome::None
            }
            Some(97) => {
                if let SubScreen::SlotOptions {
                    server,
                    slot,
                    force_game_mode,
                    ..
                } = &mut self.subscreen
                    && server.world_type == "NORMAL"
                    && !slot.is_hardcore()
                {
                    *force_game_mode = !*force_game_mode;
                }
                Outcome::None
            }
            Some(98) => {
                self.save_slot_options();
                Outcome::None
            }
            Some(99) => {
                self.back_to_worlds();
                Outcome::None
            }
            // RealmsInviteScreen: Invite (80) and Back (81).
            Some(80) => {
                self.invite_clicked();
                Outcome::None
            }
            Some(81) => {
                self.back_to_players();
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
            Some(btn @ 100..=255) => {
                // The button's meaning: a pending invite's answer, or a player row's
                // Op/Deop/Remove (`RealmsPlayersTab.PlayerEntry`).
                enum Row {
                    Nothing,
                    Pending(bool, String),
                    Member(bool, i64, String),
                    Uninvite(RealmServer, PlayerInfo),
                }
                let row = match &self.subscreen {
                    SubScreen::PendingInvites { invites, job } if job.is_none() && btn <= 200 => {
                        match invites.as_ref().and_then(|i| i.as_ref().ok()) {
                            Some(list) => match list.get(usize::from((btn - 100) / 2)) {
                                Some(invite) => {
                                    Row::Pending((btn - 100) % 2 == 0, invite.invitation_id.clone())
                                }
                                None => Row::Nothing,
                            },
                            None => Row::Nothing,
                        }
                    }
                    SubScreen::Configure {
                        server,
                        tab,
                        detail,
                        member_job,
                        ..
                    } if *tab == ConfigureTab::Players && member_job.is_none() => {
                        let hit = self
                            .player_rows
                            .borrow()
                            .iter()
                            .find(|row| btn == row.button || btn == row.button + 1)
                            .cloned();
                        match hit.filter(|row| row.realm_id == server.id) {
                            Some(hit) => {
                                let uuid = hit.player.uuid.as_deref().and_then(auth::parse_uuid);
                                let current = uuid.and_then(|uuid| {
                                    detail
                                        .as_ref()
                                        .and_then(|d| d.as_ref().ok())?
                                        .players
                                        .iter()
                                        .find(|player| {
                                            player.uuid.as_deref().and_then(auth::parse_uuid)
                                                == Some(uuid)
                                        })
                                });
                                match current {
                                    Some(player) if btn == hit.button => Row::Member(
                                        !hit.player.operator,
                                        server.id,
                                        player.uuid.clone().unwrap(),
                                    ),
                                    Some(player) => Row::Uninvite(server.clone(), player.clone()),
                                    None => Row::Nothing,
                                }
                            }
                            None => Row::Nothing,
                        }
                    }
                    _ => Row::Nothing,
                };
                match row {
                    Row::Nothing => {}
                    Row::Pending(accept, id) => {
                        if let SubScreen::PendingInvites { job, .. } = &mut self.subscreen
                            && job.is_none()
                            && let Some(client) = client(&self.workers)
                        {
                            *job = Some(self.workers.spawn(move || {
                                let done = if accept {
                                    client.accept_invite(&id)
                                } else {
                                    client.reject_invite(&id)
                                };
                                done.and_then(|()| client.pending_invites())
                            }));
                        }
                    }
                    Row::Member(op, realm_id, uuid) => {
                        if let SubScreen::Configure {
                            member_job, error, ..
                        } = &mut self.subscreen
                            && member_job.is_none()
                            && let Some(client) = client(&self.workers)
                        {
                            *error = None;
                            *member_job = Some(self.workers.spawn(move || {
                                if op {
                                    client.op_player(realm_id, &uuid).map(drop)
                                } else {
                                    client.deop_player(realm_id, &uuid).map(drop)
                                }
                            }));
                        }
                    }
                    Row::Uninvite(server, player) => {
                        self.enter_configure_child(SubScreen::Popup(Popup::Uninvite {
                            server,
                            player,
                        }));
                    }
                }
                Outcome::None
            }
            // The tab bar draws its own tabs (`MenuTabBar`), so a click there carries no
            // button id: the tab bar's geometry decides.
            None if matches!(
                self.subscreen,
                SubScreen::Configure { .. } | SubScreen::RegionPreference { .. }
            ) =>
            {
                let (bar_x, tab_w) = tab_geometry(w);
                if matches!(self.subscreen, SubScreen::Configure { .. })
                    && y < TAB_HEIGHT
                    && x >= bar_x
                {
                    let index = ((x - bar_x) / tab_w).floor() as usize;
                    if let Some(wanted) = ConfigureTab::ALL.get(index) {
                        self.select_configure_tab(*wanted);
                    }
                    return Outcome::None;
                }
                // RealmsSettingsTab's two fields: the click moves the caret between them.
                if let SubScreen::Configure {
                    tab: ConfigureTab::Settings,
                    settings: Some(draft),
                    ..
                } = &mut self.subscreen
                {
                    let left = (w / 2.0).floor() - 106.0;
                    if (left..left + 212.0).contains(&x) {
                        if (46.0..66.0).contains(&y) {
                            draft.on_description = false;
                            return Outcome::None;
                        }
                        if (86.0..106.0).contains(&y) {
                            draft.on_description = true;
                            return Outcome::None;
                        }
                    }
                }
                // RealmsPreferredRegionSelectionScreen: a row is the preference (the region
                // rows are MANUAL + that region).
                if let SubScreen::RegionPreference {
                    regions, selected, ..
                } = &mut self.subscreen
                {
                    let rows = region_rows(regions);
                    let top = 40.0;
                    let range = Self::region_range_at(self.region_scroll, rows.len(), h);
                    let row = range.start + ((y - top) / 16.0).floor().max(0.0) as usize;
                    if x >= ((w.ceil() - 300.0) / 2.0).floor()
                        && x < ((w.ceil() - 300.0) / 2.0).floor() + 300.0
                        && y >= top
                        && y < h.ceil() - 48.0
                        && row < range.end
                        && let Some(pick) = rows.get(row)
                    {
                        *selected = Some(pick.clone());
                    }
                    return Outcome::None;
                }
                // RealmsSlotOptionsScreen's spawn-protection slider: a click on the track
                // sets the value (there is no drag event on this screen yet).
                if let SubScreen::SlotOptions {
                    server,
                    slot,
                    spawn_protection,
                    ..
                } = &mut self.subscreen
                    && server.world_type == "NORMAL"
                    && !slot.is_hardcore()
                {
                    let (sx, sw) = (((w / 2.0).floor()) + 2.0, 152.0);
                    if (118.0..138.0).contains(&y) && (sx..sx + sw).contains(&x) {
                        let value = crate::ui::slider_value(sx, sw, x);
                        *spawn_protection = (value * 16.0).round() as i32;
                        return Outcome::None;
                    }
                }
                // RealmsWorldsTab's slot buttons: a different slot asks before switching.
                let switch = match &self.subscreen {
                    SubScreen::Configure {
                        server,
                        tab: ConfigureTab::Worlds,
                        detail: Some(Ok(detail)),
                        member_job: None,
                        job: None,
                        ..
                    } => slot_boxes(w, detail.active_slot)
                        .into_iter()
                        .find(|(bx, size, slot)| {
                            x >= *bx
                                && x < bx + size
                                && y >= 44.0
                                && y < 44.0 + size
                                && *slot != detail.active_slot
                                && detail
                                    .slots
                                    .get(slot)
                                    .is_some_and(|entry| !entry.options.empty)
                        })
                        .map(|(_, _, slot)| (server.clone(), slot)),
                    _ => None,
                };
                if let Some((server, slot)) = switch {
                    self.enter_configure_child(SubScreen::Popup(Popup::SwitchSlot {
                        server,
                        slot,
                    }));
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

    fn cancel_slot_switch(&mut self) -> bool {
        let Some(intent) = self.slot_switch.take() else {
            return false;
        };
        intent.aborted.store(true, Ordering::Release);
        if let SubScreen::Configure {
            member_job, error, ..
        } = &mut self.subscreen
        {
            *member_job = None;
            *error = intent.previous_error.clone();
        }
        true
    }

    fn start_slot_switch(
        &mut self,
        client: RealmsClient,
        server: RealmServer,
        slot: i32,
        parent: Option<Box<SubScreen>>,
    ) {
        self.start_slot_switch_with_pause(client, server, slot, parent, wait_for_retry);
    }

    fn start_slot_switch_with_pause(
        &mut self,
        client: RealmsClient,
        server: RealmServer,
        slot: i32,
        parent: Option<Box<SubScreen>>,
        mut pause: impl FnMut(&RealmsClient, &AtomicBool, Duration) -> Result<(), RealmsError>
        + Send
        + 'static,
    ) {
        let realm = server.id;
        self.subscreen = match parent {
            Some(parent) if matches!(&*parent, SubScreen::Configure { server, .. } if server.id == realm) => {
                *parent
            }
            _ => SubScreen::Configure {
                server,
                tab: ConfigureTab::Worlds,
                subscription: None,
                subscription_job: None,
                settings: None,
                regions: None,
                regions_job: None,
                saving: None,
                detail: None,
                detail_job: None,
                deleting: None,
                member_job: None,
                error: None,
                job: None,
            },
        };
        let previous_error = match &mut self.subscreen {
            SubScreen::Configure { error, .. } => error.take(),
            _ => None,
        };
        let aborted = Arc::new(AtomicBool::new(false));
        self.slot_switch = Some(Arc::new(SlotIntent {
            aborted: aborted.clone(),
            previous_error,
        }));
        let client = client.with_cancellation(self.workers.cancellation());
        let work = self.workers.spawn(move || {
            switch_slot_task(&client, realm, slot, &aborted, |delay| {
                pause(&client, &aborted, delay)
            })
        });
        if let SubScreen::Configure { member_job, .. } = &mut self.subscreen {
            *member_job = Some(work);
        }
    }

    /// Continue in a popup.
    fn popup_continue(&mut self) -> Outcome {
        let SubScreen::Popup(popup) = &self.subscreen else {
            return Outcome::None;
        };
        let popup = popup.clone();
        let parent = self.return_subscreen.take();
        self.subscreen = SubScreen::Main;
        match popup {
            Popup::Leave(server) => {
                // leaveServer: on a thread, then the list reloads.
                if let Some(client) = client(&self.workers) {
                    let id = server.id;
                    self.leaving = Some(self.workers.spawn(move || client.uninvite_myself(id)));
                    self.selected = None;
                }
                Outcome::None
            }
            Popup::Close(server) => {
                if let Some(parent) = parent {
                    self.subscreen = *parent;
                } else {
                    self.back_to_settings(server.clone(), SettingsDraft::of(&server));
                }
                self.begin_settings_transition(SettingsNext::Toggle(false));
                Outcome::None
            }
            Popup::ResourcePack { pack } => {
                let address = pack.address.clone();
                if let Ok(mut slot) = PENDING_PACK.lock() {
                    *slot = Some(pack);
                }
                Outcome::Connect(address)
            }
            // RealmsWorldsTab.switchToFullSlot, answered: `RealmsClient.switchSlot` on a
            // worker, then the detail reads again.
            Popup::SwitchSlot { server, slot } => {
                let Some(client) = client(&self.workers) else {
                    return Outcome::None;
                };
                self.start_slot_switch(client, server, slot, parent);
                Outcome::None
            }
            // RealmsPlayersTab.PlayerEntry.uninvite, answered: the player goes and the
            // list reads again.
            Popup::Uninvite { server, player } => {
                let Some(client) = client(&self.workers) else {
                    return Outcome::None;
                };
                let (id, uuid) = (server.id, player.uuid.clone().unwrap_or_default());
                self.subscreen = SubScreen::Configure {
                    server,
                    tab: ConfigureTab::Players,
                    subscription: None,
                    subscription_job: None,
                    detail: None,
                    detail_job: None,
                    deleting: None,
                    member_job: Some(
                        self.workers
                            .spawn(move || client.uninvite_player(id, &uuid)),
                    ),
                    settings: None,
                    regions: None,
                    regions_job: None,
                    saving: None,
                    error: None,
                    job: None,
                };
                self.start_configure_subscription();
                self.start_configure_detail();
                Outcome::None
            }
            // RealmsSubscriptionTab.deleteRealm: the question was asked over the
            // configuration screen, so it answers there; the list returns once the
            // Realm is gone.
            Popup::Delete(server, _) => {
                let Some(client) = client(&self.workers) else {
                    return Outcome::None;
                };
                let id = server.id;
                self.subscreen = SubScreen::Configure {
                    server,
                    tab: ConfigureTab::Subscription,
                    subscription: None,
                    subscription_job: None,
                    detail: None,
                    detail_job: None,
                    deleting: Some(self.workers.spawn(move || client.delete_realm(id))),
                    member_job: None,
                    settings: None,
                    regions: None,
                    regions_job: None,
                    saving: None,
                    error: None,
                    job: None,
                };
                // A failed delete stays here and the tab reads again.
                self.start_configure_subscription();
                self.start_configure_detail();
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

    /// Bind controls to the roster actually laid out for a drawn frame. Queries
    /// that recompute hit rectangles must not replace those UUIDs mid-frame.
    fn remember_player_rows(&self, size: (f32, f32)) {
        let mut rows = self.player_rows.borrow_mut();
        rows.clear();
        let SubScreen::Configure {
            server,
            tab: ConfigureTab::Players,
            detail: Some(Ok(detail)),
            ..
        } = &self.subscreen
        else {
            return;
        };
        for (visible, index) in self.player_range(detail.players.len(), size.1).enumerate() {
            rows.push(PlayerRow {
                button: 100 + visible as u8 * 2,
                realm_id: server.id,
                player: detail.players[index].clone(),
            });
        }
    }

    fn region_range(&self, len: usize, height: f32) -> std::ops::Range<usize> {
        Self::region_range_at(self.region_scroll, len, height)
    }

    fn region_range_at(scroll: f32, len: usize, height: f32) -> std::ops::Range<usize> {
        let rows = ((height.ceil() - 48.0 - 40.0) / 16.0).floor().max(0.0) as usize;
        let start = scroll.clamp(0.0, len.saturating_sub(rows) as f32).floor() as usize;
        start..(start + rows).min(len)
    }

    fn player_range(&self, len: usize, height: f32) -> std::ops::Range<usize> {
        let rows = (((height.ceil() - PLAYERS_BOTTOM - PLAYERS_TOP) / ROW_HEIGHT)
            .floor()
            .max(0.0) as usize)
            .min(MAX_PLAYER_ROWS);
        let start = self
            .players_scroll
            .clamp(0.0, len.saturating_sub(rows) as f32)
            .floor() as usize;
        start..(start + rows).min(len)
    }

    fn player_button_rects(width: f32, visible: usize) -> [[f32; 4]; 2] {
        let left = ((width.ceil() - ROW_WIDTH) / 2.0).floor();
        let action_width = 132.0 / 2.0;
        let x = left + ROW_WIDTH - 132.0;
        let y = PLAYERS_TOP + visible as f32 * ROW_HEIGHT + 8.0;
        [
            [x, y, action_width - 2.0, 20.0],
            [x + action_width, y, action_width - 2.0, 20.0],
        ]
    }

    /// Touch action identity is recorded only by draw, using the exact same
    /// rectangles as the visible buttons. It is never a caption or row index.
    fn register_player_bindings(&self, size: (f32, f32)) {
        if !matches!(
            self.subscreen,
            SubScreen::Configure {
                tab: ConfigureTab::Players,
                member_job: None,
                ..
            }
        ) {
            return;
        }
        for (visible, row) in self.player_rows.borrow().iter().enumerate() {
            let Some(uuid) = row.player.uuid.as_deref().and_then(auth::parse_uuid) else {
                continue;
            };
            let permission = if row.player.operator { "deop" } else { "op" };
            for (rect, operation) in Self::player_button_rects(size.0, visible)
                .into_iter()
                .zip([permission, "remove"])
            {
                crate::widgets::pointer_binding(
                    rect,
                    format!(
                        "realms:{}:{uuid:032x}:{operation}:{}",
                        row.realm_id, row.player.operator,
                    ),
                );
            }
        }
    }

    /// Mouse, trackpad, controller and touch-drag wheel events take the same
    /// path through Title. Fractional deltas accumulate instead of being lost.
    pub fn scroll(&mut self, delta: f32, cursor: (f32, f32), size: (f32, f32)) -> bool {
        if let SubScreen::RegionPreference { regions, .. } = &self.subscreen {
            let left = ((size.0.ceil() - 300.0) / 2.0).floor();
            if !delta.is_finite()
                || cursor.0 < left
                || cursor.0 >= left + 300.0
                || cursor.1 < 40.0
                || cursor.1 >= size.1.ceil() - 48.0
            {
                return false;
            }
            let len = region_rows(regions).len();
            let max = len.saturating_sub(self.region_range(len, size.1).len()) as f32;
            self.region_scroll = (self.region_scroll.clamp(0.0, max) - delta).clamp(0.0, max);
            return true;
        }
        let SubScreen::Configure {
            tab: ConfigureTab::Players,
            detail: Some(Ok(detail)),
            ..
        } = &self.subscreen
        else {
            return false;
        };
        let left = ((size.0.ceil() - ROW_WIDTH) / 2.0).floor();
        if !delta.is_finite()
            || cursor.0 < left
            || cursor.0 >= left + ROW_WIDTH
            || cursor.1 < PLAYERS_TOP
            || cursor.1 >= size.1.ceil() - PLAYERS_BOTTOM
        {
            return false;
        }
        let rows = self.player_range(detail.players.len(), size.1).len();
        let max = detail.players.len().saturating_sub(rows) as f32;
        self.players_scroll = (self.players_scroll.clamp(0.0, max) - delta).clamp(0.0, max);
        true
    }

    pub fn buttons(
        &self,
        w: f32,
        h: f32,
    ) -> Vec<(f32, f32, f32, String, bool, u8, Option<(&'static str, f32)>)> {
        let (w, h) = (w.ceil(), h.ceil());
        let cx = (w / 2.0).floor();
        let left = cx - (CONTENT_WIDTH / 2.0).floor();
        let mid = (h / 2.0).floor();
        let t = crate::lang::get;
        let mut buttons = Vec::new();
        // Bootstrap a newly opened screen before its first draw. Later hit
        // queries leave frame bindings alone; draw refreshes them explicitly.
        let unbound = self.player_rows.borrow().is_empty();
        if unbound {
            self.remember_player_rows((w, h));
        }

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
                detail,
                deleting,
                member_job,
                saving,
                job,
                ..
            } => {
                // The footer: Play, the open/close toggle (vanilla keeps it on the settings
                // tab; it stays here until that tab lands) and Back.
                buttons.push((
                    cx - 102.0,
                    h - 26.0,
                    100.0,
                    t("mco.selectServer.play"),
                    server.is_open() && self.slot_switch.is_none() && self.settings_next.is_none(),
                    64,
                    None,
                ));
                buttons.push((
                    cx + 2.0,
                    h - 26.0,
                    100.0,
                    t(if self.slot_switch.is_some() {
                        "gui.cancel"
                    } else {
                        "gui.back"
                    }),
                    self.settings_next.is_none(),
                    32,
                    None,
                ));
                // RealmsSettingsTab: the region preference and the open/close toggle
                // (vanilla keeps the toggle on this tab).
                if *tab == ConfigureTab::Settings {
                    buttons.push((
                        cx - 106.0,
                        146.0,
                        212.0,
                        t("mco.configure.world.buttons.region_preference"),
                        saving.is_none() && job.is_none() && member_job.is_none(),
                        89,
                        None,
                    ));
                    let toggle = if server.is_open() {
                        t("mco.configure.world.buttons.close")
                    } else {
                        t("mco.configure.world.buttons.open")
                    };
                    buttons.push((
                        cx - 106.0,
                        174.0,
                        212.0,
                        toggle,
                        job.is_none() && member_job.is_none() && saving.is_none(),
                        30,
                        None,
                    ));
                }
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
                // RealmsWorldsTab: World Options (Backup and Reset follow in their PRs).
                if *tab == ConfigureTab::Worlds
                    && let Some(Ok(detail)) = detail
                    && let Some(slot) = detail.slots.get(&detail.active_slot)
                    && !slot.options.empty
                {
                    buttons.push((
                        cx - 76.0,
                        h - 96.0,
                        152.0,
                        t("mco.configure.world.buttons.options"),
                        !server.expired && member_job.is_none() && job.is_none(),
                        94,
                        None,
                    ));
                }
                // RealmsPlayersTab: a row's Op/Deop/Remove, then Invite Player.
                if *tab == ConfigureTab::Players
                    && let Some(Ok(detail)) = detail
                {
                    let range = self.player_range(detail.players.len(), h);
                    for (visible, index) in range.enumerate() {
                        let player = &detail.players[index];
                        let id = 100 + visible as u8 * 2;
                        let permission = if player.operator {
                            "mco.configure.world.invites.normal.tooltip"
                        } else {
                            "mco.configure.world.invites.ops.tooltip"
                        };
                        // Native PlayerEntry exposes one active permission
                        // button (Op or Deop), plus Remove.
                        for (n, (rect, key)) in Self::player_button_rects(w, visible)
                            .into_iter()
                            .zip([permission, "mco.configure.world.invites.remove.tooltip"])
                            .enumerate()
                        {
                            buttons.push((
                                rect[0],
                                rect[1],
                                rect[2],
                                t(key),
                                player.uuid.as_deref().and_then(auth::parse_uuid).is_some()
                                    && member_job.is_none(),
                                id + n as u8,
                                None,
                            ));
                        }
                    }
                    buttons.push((
                        cx - 100.0,
                        h - 60.0,
                        200.0,
                        t("mco.configure.world.buttons.invite"),
                        true,
                        67,
                        None,
                    ));
                }
            }
            SubScreen::RegionPreference { .. } => {
                buttons.push((cx - 102.0, h - 36.0, 100.0, t("gui.done"), true, 87, None));
                buttons.push((cx + 2.0, h - 36.0, 100.0, t("gui.cancel"), true, 86, None));
            }
            SubScreen::SlotOptions {
                server,
                slot,
                difficulty,
                game_mode,
                force_game_mode,
                job,
                ..
            } => {
                // `RealmsSlotOptionsScreen`: the two cycle buttons, the on/off and the
                // footer (the name field and the spawn-protection slider are drawn).
                let normal = server.world_type == "NORMAL";
                let hardcore = slot.is_hardcore();
                let on_off = |on: bool| {
                    if on {
                        t("options.on")
                    } else {
                        t("options.off")
                    }
                };
                buttons.push((
                    cx - 154.0,
                    90.0,
                    152.0,
                    crate::lang::tr(
                        "options.generic_value",
                        &[&t("options.difficulty"), &t(DIFFICULTIES[*difficulty])],
                    ),
                    !hardcore,
                    95,
                    None,
                ));
                buttons.push((
                    cx + 2.0,
                    90.0,
                    152.0,
                    crate::lang::tr(
                        "options.generic_value",
                        &[&t("selectWorld.gameMode"), &t(GAME_MODES[*game_mode])],
                    ),
                    !hardcore,
                    96,
                    None,
                ));
                buttons.push((
                    cx - 154.0,
                    118.0,
                    152.0,
                    crate::lang::tr(
                        "options.generic_value",
                        &[
                            &t("mco.configure.world.forceGameMode"),
                            &on_off(*force_game_mode),
                        ],
                    ),
                    normal && !hardcore,
                    97,
                    None,
                ));
                buttons.push((
                    cx - 102.0,
                    h - 36.0,
                    100.0,
                    t("gui.done"),
                    job.is_none(),
                    98,
                    None,
                ));
                buttons.push((cx + 2.0, h - 36.0, 100.0, t("gui.cancel"), true, 99, None));
            }
            SubScreen::Invite { job, .. } => {
                buttons.push((
                    cx - 100.0,
                    mid + 10.0,
                    200.0,
                    t("mco.configure.world.buttons.invite"),
                    job.is_none(),
                    80,
                    None,
                ));
                buttons.push((cx - 100.0, h - 36.0, 200.0, t("gui.back"), true, 81, None));
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
        self.remember_player_rows((w, h));
        self.register_player_bindings((w, h));
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
                detail,
                settings,
                regions,
                saving,
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
                    ConfigureTab::Worlds => match detail {
                        Some(Ok(detail)) => {
                            let boxes = slot_boxes(w, detail.active_slot);
                            let top = 44.0;
                            for (bx, size, slot) in boxes {
                                let active = slot == detail.active_slot;
                                let color = if active { 0xFFB0_B0B0 } else { 0xFF70_7070 };
                                batch.fill(atlas, bx, top, size, size, color);
                                if let Some(entry) = detail.slots.get(&slot) {
                                    let name = slot_name(&entry.options, slot);
                                    let tw = HudBatch::text_width(atlas, &name);
                                    batch.text(
                                        atlas,
                                        &name,
                                        bx + ((size - tw) / 2.0).floor(),
                                        top + size - 12.0,
                                        if active { u32::MAX } else { GREY },
                                        true,
                                    );
                                }
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
                    ConfigureTab::Players => match detail {
                        Some(Ok(detail)) => {
                            let invited = crate::lang::tr(
                                "mco.configure.world.invited.number",
                                &[&detail.players.len() as &dyn std::fmt::Display],
                            );
                            centered(batch, atlas, &invited, cx, 26.0, u32::MAX);
                            let rx = ((w - ROW_WIDTH) / 2.0).floor();
                            let range = self.player_range(detail.players.len(), h);
                            let max = detail.players.len().saturating_sub(range.len()) as f32;
                            crate::widgets::scroll_area(
                                [
                                    rx,
                                    PLAYERS_TOP,
                                    ROW_WIDTH,
                                    (h - PLAYERS_BOTTOM - PLAYERS_TOP).max(0.0),
                                ],
                                self.players_scroll.min(max),
                                max,
                            );
                            for (visible, index) in range.enumerate() {
                                let player = &detail.players[index];
                                let row_y = PLAYERS_TOP + visible as f32 * ROW_HEIGHT;
                                let name = if player.operator {
                                    format!(
                                        "{} [{}]",
                                        player.name,
                                        t("mco.configure.world.invites.ops.tooltip")
                                    )
                                } else {
                                    player.name.clone()
                                };
                                batch.text(atlas, &name, rx + 4.0, row_y + 14.0, u32::MAX, true);
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
                    ConfigureTab::Settings => match settings {
                        Some(draft) => {
                            // `RealmsSettingsTab`: the name and description fields, the
                            // region preference line and the open/close button.
                            let left = cx - 106.0;
                            batch.text(
                                atlas,
                                &t("mco.configure.world.name"),
                                left,
                                34.0,
                                GREY,
                                true,
                            );
                            caret.draw(
                                batch,
                                atlas,
                                (REALMS_SETTINGS_NAME, &draft.name, 32),
                                [left, 46.0, 212.0, 20.0],
                                !draft.on_description,
                                focused_at,
                            );
                            batch.text(
                                atlas,
                                &t("mco.configure.world.description"),
                                left,
                                74.0,
                                GREY,
                                true,
                            );
                            caret.draw(
                                batch,
                                atlas,
                                (REALMS_SETTINGS_DESC, &draft.description, 32),
                                [left, 86.0, 212.0, 20.0],
                                draft.on_description,
                                focused_at,
                            );
                            batch.text(
                                atlas,
                                &t("mco.configure.world.region_preference"),
                                left,
                                114.0,
                                GREY,
                                true,
                            );
                            let current =
                                preference_label(draft.preference, draft.region.as_deref());
                            batch.text(atlas, &current, left, 126.0, GREY, true);
                            if saving.is_some() {
                                centered(
                                    batch,
                                    atlas,
                                    &t("mco.configure.world.loading"),
                                    cx,
                                    200.0,
                                    u32::MAX,
                                );
                            }
                            if regions.is_none() {
                                // The region list is loading: the button says so when clicked.
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
                }
                let status = if self.slot_switch.is_some() {
                    Some((t("mco.minigame.world.slot.screen.title"), u32::MAX))
                } else if job.is_some() {
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
            SubScreen::RegionPreference {
                server,
                regions,
                selected,
                ..
            } => {
                // `RealmsPreferredRegionSelectionScreen`: the preference rows, then the
                // MANUAL rows per region.
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.region_preference.title"),
                    cx,
                    14.0,
                    u32::MAX,
                );
                let top = 40.0;
                let rows = region_rows(regions);
                let left = ((w - 300.0) / 2.0).floor();
                let range = self.region_range(rows.len(), h);
                let max = rows.len().saturating_sub(range.len()) as f32;
                crate::widgets::scroll_area(
                    [left, 40.0, 300.0, (h - 88.0).max(0.0)],
                    self.region_scroll.min(max),
                    max,
                );
                for (visible, index) in range.enumerate() {
                    let (preference, region) = &rows[index];
                    let y = top + visible as f32 * 16.0;
                    crate::widgets::pointer_binding(
                        [left, y, 300.0, 14.0],
                        format!("realms-region:{}:{preference}:{region:?}", server.id),
                    );
                    let picked = selected
                        .as_ref()
                        .is_some_and(|(p, r)| p == preference && *r == *region);
                    let label = preference_label(*preference, region.as_deref());
                    if picked {
                        batch.fill(atlas, left, y, 300.0, 14.0, 0xFF30_3030);
                    }
                    batch.text(atlas, &label, left + 5.0, y + 2.0, u32::MAX, true);
                }
            }
            SubScreen::SlotOptions {
                server,
                slot,
                name,
                spawn_protection,
                error,
                ..
            } => {
                // `RealmsSlotOptionsScreen`: the title, the name field (labelled) and
                // the spawn-protection slider; the cycle buttons are widgets.
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.buttons.options"),
                    cx,
                    20.0,
                    u32::MAX,
                );
                batch.text(
                    atlas,
                    &t("mco.configure.world.edit.slot.name"),
                    cx - 154.0,
                    50.0,
                    GREY,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_SLOT_NAME, name, 16),
                    [cx - 154.0, 62.0, 152.0, 20.0],
                    true,
                    focused_at,
                );
                let normal = server.world_type == "NORMAL";
                let off = *spawn_protection == 0;
                let label = crate::lang::tr(
                    "options.generic_value",
                    &[
                        &t("mco.configure.world.spawnProtection"),
                        &if off {
                            t("options.off")
                        } else {
                            spawn_protection.to_string()
                        },
                    ],
                );
                let value = *spawn_protection as f32 / 16.0;
                crate::ui::slider(
                    batch,
                    atlas,
                    cx + 2.0,
                    118.0,
                    152.0,
                    &Text::literal(&label),
                    value,
                    crate::ui::hovered(cursor, cx + 2.0, 118.0, 152.0, 20.0),
                    normal && !slot.is_hardcore(),
                    false,
                );
                if let Some(e) = error {
                    for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, h - 56.0 + i as f32 * 10.0, RED);
                    }
                }
            }
            SubScreen::Invite {
                name, job, error, ..
            } => {
                // `RealmsInviteScreen`: the title, the name field and the message below
                // the Invite button.
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.buttons.invite"),
                    cx,
                    20.0,
                    u32::MAX,
                );
                batch.text(
                    atlas,
                    &t("mco.configure.world.invite.profile.name"),
                    cx - 100.0,
                    50.0,
                    GREY,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_PLAYER_NAME, name, 16),
                    [cx - 100.0, 62.0, 200.0, 20.0],
                    true,
                    focused_at,
                );
                let status = if job.is_some() {
                    Some((t("mco.configure.world.players.inviting"), u32::MAX))
                } else {
                    error.clone().map(|e| (e, RED))
                };
                if let Some((text, color)) = status {
                    for (i, line) in wrap_lines(&text, atlas, 200.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, 88.0 + i as f32 * 10.0, color);
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
                    Popup::Delete(..) => t("mco.configure.world.delete.question.line1"),
                    Popup::Close(_) => t("mco.configure.world.close.question.line1"),
                    Popup::SwitchSlot { .. } => t("mco.configure.world.slot.switch.question.line1"),
                    Popup::Uninvite { player, .. } => crate::lang::tr(
                        "mco.configure.world.uninvite.player",
                        &[&player.name as &dyn std::fmt::Display],
                    ),
                };
                let lines = wrap_lines(&message, atlas, CONTENT_WIDTH);
                let top = mid - 10.0 - 10.0 * lines.len() as f32;
                let header = match popup {
                    Popup::Uninvite { .. } => t("mco.question"),
                    Popup::Close(_) => t("mco.configure.world.close.question.title"),
                    _ => t("mco.info"),
                };
                centered(batch, atlas, &header, cx, top - 16.0, INFO);
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

/// Pure fixture construction: never starts Realms requests or reads an account.
#[cfg(test)]
pub(crate) fn players_fixture(count: usize) -> Screen {
    let server = RealmServer {
        id: 7,
        name: "Fixture".into(),
        state: "OPEN".into(),
        ..Default::default()
    };
    let mut detail = server.clone();
    detail.players = (0..count)
        .map(|index| PlayerInfo {
            name: format!("Player{index:03}"),
            uuid: Some(format!("{:032x}", index + 1)),
            operator: index % 2 == 0,
            accepted: true,
            online: false,
        })
        .collect();
    Screen {
        state: FetchState::Loaded(vec![server.clone()]),
        subscreen: SubScreen::Configure {
            server,
            tab: ConfigureTab::Players,
            subscription: Some(Ok(Subscription::default())),
            subscription_job: None,
            settings: None,
            regions: None,
            regions_job: None,
            saving: None,
            detail: Some(Ok(detail)),
            detail_job: None,
            deleting: None,
            member_job: None,
            error: None,
            job: None,
        },
        selected: Some(0),
        joining: None,
        leaving: None,
        workers: Workers::new(),
        return_subscreen: None,
        slot_switch: None,
        settings_next: None,
        region_scroll: 0.0,
        players_scroll: 0.0,
        player_rows: RefCell::new(Vec::new()),
        last_click: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Real HTTP exchanges only; the server returns without waiting forever if
    /// a regressed task stops early. Captured output excludes cookies/headers.
    fn slot_service(
        answers: Vec<(u16, Option<u64>, &'static str)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, retry, body) in answers {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(1))
                        }
                        _ => return requests,
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut bytes = [0; 4096];
                    let n = socket.read(&mut bytes).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&bytes[..n]);
                }
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    while request.len() < end + 4 + length {
                        let mut bytes = [0; 4096];
                        let n = socket.read(&mut bytes).unwrap();
                        assert_ne!(n, 0, "incomplete request body");
                        request.extend_from_slice(&bytes[..n]);
                    }
                }
                requests.push(
                    String::from_utf8_lossy(&request)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_owned(),
                );
                let retry = retry.map_or(String::new(), |s| format!("Retry-After: {s}\r\n"));
                write!(socket, "HTTP/1.1 {status} Fixture\r\n{retry}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (base, server)
    }

    #[test]
    fn region_rows_remain_clickable_and_reachable_at_phone_and_tablet_heights() {
        for height in [180.0, 300.0, 900.0] {
            let mut screen = settings_fixture();
            if let SubScreen::Configure { regions, .. } = &mut screen.subscreen {
                *regions = Some(Ok((0..29)
                    .map(|index| RegionDataDto {
                        region: format!("Region{index}"),
                        service_quality: 1,
                    })
                    .collect()));
            }
            screen.open_region_preference();
            let size = (640.0, height);
            assert!(screen.scroll(100.0, (320.0, 50.0), size));
            let mut visited = std::collections::HashSet::new();
            for _ in 0..32 {
                let range = screen.region_range(31, height);
                for (visible, index) in range.enumerate() {
                    screen.click(None, 320.0, 40.0 + visible as f32 * 16.0 + 8.0, size);
                    let SubScreen::RegionPreference {
                        selected, regions, ..
                    } = &screen.subscreen
                    else {
                        panic!()
                    };
                    assert_eq!(selected.as_ref(), Some(&region_rows(regions)[index]));
                    visited.insert(index);
                }
                screen.scroll(-1.0, (320.0, 50.0), size);
            }
            assert_eq!(visited.len(), 31);
            let SubScreen::RegionPreference { selected, .. } = &screen.subscreen else {
                panic!()
            };
            let before = selected.clone();
            screen.click(None, 0.0, 48.0, size);
            screen.click(None, 320.0, height - 47.0, size);
            let SubScreen::RegionPreference { selected, .. } = &screen.subscreen else {
                panic!()
            };
            assert_eq!(
                *selected, before,
                "off-list/footer clicks must not select rows"
            );
            finish_slot_workers(&screen.workers);
        }
    }

    fn settings_fixture() -> Screen {
        let mut screen = players_fixture(0);
        let realm: RealmServer = serde_json::from_str(
            r#"{"id":7,"name":"Original","motd":"Before","state":"OPEN","worldType":"NORMAL","activeSlot":1,"slots":[{"slotId":1,"options":"{\"slotName\":\"Spawn\"}","settings":[{"name":"hardcore","value":"false"},{"name":"viewDistance","value":"10"}]}]}"#,
        ).unwrap();
        screen.state = FetchState::Loaded(vec![realm.clone()]);
        if let SubScreen::Configure {
            server,
            detail,
            tab,
            settings,
            ..
        } = &mut screen.subscreen
        {
            *server = realm.clone();
            *detail = Some(Ok(realm.clone()));
            *tab = ConfigureTab::Settings;
            *settings = Some(SettingsDraft::of(&realm));
        }
        screen
    }

    fn wait_settings(screen: &mut Screen) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while screen.settings_next.is_some() {
            assert!(Instant::now() < deadline, "settings worker did not settle");
            screen.poll();
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn settings_save_acknowledges_the_sent_snapshot_before_switching_tabs() {
        let (base, server) = slot_service(vec![(204, None, "")]);
        let mut screen = settings_fixture();
        if let SubScreen::Configure {
            settings: Some(draft),
            ..
        } = &mut screen.subscreen
        {
            draft.name = "  Sent name  ".into();
            draft.description = "Sent description".into();
        }
        assert!(
            screen
                .begin_settings_using(slot_client(&base), SettingsNext::Tab(ConfigureTab::Players))
        );
        assert!(
            screen.focused_text().is_none(),
            "pending save must lock editable fields"
        );
        if let SubScreen::Configure {
            settings: Some(draft),
            ..
        } = &mut screen.subscreen
        {
            draft.name = "Unsent mutation".into();
        }
        wait_settings(&mut screen);
        let SubScreen::Configure {
            server: realm,
            tab: ConfigureTab::Players,
            detail: Some(Ok(detail)),
            ..
        } = &screen.subscreen
        else {
            panic!("ack must apply requested tab")
        };
        assert_eq!(realm.name, "Sent name");
        assert_eq!(detail.name, "Sent name");
        let FetchState::Loaded(realms) = &screen.state else {
            panic!()
        };
        assert_eq!(realms[0].name, "Sent name");
        assert_eq!(
            server.join().unwrap(),
            ["POST /worlds/7/configuration HTTP/1.1"]
        );
        finish_slot_workers(&screen.workers);
    }

    #[test]
    fn failed_settings_save_keeps_the_dirty_parent_and_does_not_close() {
        let (base, server) = slot_service(vec![(500, None, "unavailable")]);
        let mut screen = settings_fixture();
        if let SubScreen::Configure {
            settings: Some(draft),
            ..
        } = &mut screen.subscreen
        {
            draft.name = "Dirty name".into();
        }
        assert!(screen.begin_settings_using(slot_client(&base), SettingsNext::Toggle(false)));
        wait_settings(&mut screen);
        let SubScreen::Configure {
            server: realm,
            tab: ConfigureTab::Settings,
            settings: Some(draft),
            error: Some(_),
            ..
        } = &screen.subscreen
        else {
            panic!("failure must retain editable settings and error")
        };
        assert_eq!(realm.name, "Original");
        assert!(realm.is_open());
        assert_eq!(draft.name, "Dirty name");
        assert_eq!(server.join().unwrap().len(), 1);
        finish_slot_workers(&screen.workers);
    }

    #[test]
    fn settings_back_waits_for_the_ack_and_updates_the_cached_list() {
        let (base, server) = slot_service(vec![(204, None, "")]);
        let mut screen = settings_fixture();
        if let SubScreen::Configure {
            settings: Some(draft),
            ..
        } = &mut screen.subscreen
        {
            draft.name = "Saved".into();
        }
        assert!(screen.begin_settings_using(slot_client(&base), SettingsNext::List));
        assert!(matches!(screen.subscreen, SubScreen::Configure { .. }));
        wait_settings(&mut screen);
        assert!(matches!(screen.subscreen, SubScreen::Main));
        let FetchState::Loaded(realms) = &screen.state else {
            panic!()
        };
        assert_eq!(realms[0].name, "Saved");
        assert_eq!(server.join().unwrap().len(), 1);
        finish_slot_workers(&screen.workers);
    }

    #[test]
    fn realm_open_and_close_retry_false_until_native_true() {
        for open in [false, true] {
            let (base, server) = slot_service(vec![
                (200, None, "false"),
                (200, None, "false"),
                (200, None, "true"),
            ]);
            let client = slot_client(&base);
            let mut waits = Vec::new();
            boolean_realm_task(
                &client,
                &AtomicBool::new(false),
                || {
                    if open {
                        client.open_realm(7)
                    } else {
                        client.close_realm(7)
                    }
                },
                |wait| {
                    waits.push(wait);
                    Ok(())
                },
            )
            .unwrap();
            assert!(
                waits.is_empty(),
                "false replies retry without a native Retry-After pause"
            );
            let path = if open { "open" } else { "close" };
            assert_eq!(
                server.join().unwrap(),
                vec![format!("PUT /worlds/7/{path} HTTP/1.1"); 3]
            );
        }
    }

    #[test]
    fn realm_close_exhaustion_never_reports_success() {
        let (base, server) = slot_service(vec![(200, None, "false"); 25]);
        let client = slot_client(&base);
        assert!(
            boolean_realm_task(
                &client,
                &AtomicBool::new(false),
                || client.close_realm(7),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(server.join().unwrap().len(), 25);
    }

    #[test]
    fn cancelling_close_confirmation_preserves_the_exact_dirty_settings() {
        let mut screen = settings_fixture();
        screen.players_scroll = 12.0;
        if let SubScreen::Configure {
            settings: Some(draft),
            ..
        } = &mut screen.subscreen
        {
            draft.name = "Dirty".into();
        }
        let SubScreen::Configure { server, .. } = &screen.subscreen else {
            panic!()
        };
        screen.enter_configure_child(SubScreen::Popup(Popup::Close(server.clone())));
        screen.escape();
        let SubScreen::Configure {
            settings: Some(draft),
            subscription: Some(Ok(_)),
            detail: Some(Ok(_)),
            saving: None,
            ..
        } = &screen.subscreen
        else {
            panic!("cancel lost the original parent")
        };
        assert_eq!(draft.name, "Dirty");
        assert_eq!(screen.players_scroll, 12.0);
        assert!(screen.settings_next.is_none());
        finish_slot_workers(&screen.workers);
    }

    #[test]
    fn cancelling_region_selection_restores_the_dirty_parent_without_applying_the_choice() {
        let mut screen = settings_fixture();
        if let SubScreen::Configure {
            settings: Some(draft),
            regions,
            ..
        } = &mut screen.subscreen
        {
            draft.name = "Dirty".into();
            *regions = Some(Ok(Vec::new()));
        }
        screen.open_region_preference();
        if let SubScreen::RegionPreference { selected, .. } = &mut screen.subscreen {
            *selected = Some((client::AUTOMATIC_PLAYER, None));
        } else {
            panic!()
        }
        screen.escape();
        let SubScreen::Configure {
            settings: Some(draft),
            subscription: Some(Ok(_)),
            detail: Some(Ok(_)),
            ..
        } = &screen.subscreen
        else {
            panic!("cancel lost loaded state")
        };
        assert_eq!(draft.name, "Dirty");
        assert_eq!(draft.preference, client::AUTOMATIC_OWNER);
        screen.open_region_preference();
        if let SubScreen::RegionPreference { selected, .. } = &mut screen.subscreen {
            *selected = Some((client::MANUAL, Some("UKSouth".into())));
        }
        screen.click(Some(87), 320.0, 200.0, (640.0, 300.0));
        let SubScreen::Configure {
            settings: Some(draft),
            subscription: Some(Ok(_)),
            detail: Some(Ok(_)),
            ..
        } = &screen.subscreen
        else {
            panic!("Done lost loaded state")
        };
        assert_eq!(draft.name, "Dirty");
        assert_eq!(draft.preference, client::MANUAL);
        assert_eq!(draft.region.as_deref(), Some("UKSouth"));
        finish_slot_workers(&screen.workers);
    }

    fn slot_client(base: &str) -> RealmsClient {
        let session = AuthSession::new("Fixture", 7, "controlled-slot-token");
        RealmsClient::new(base, base, &session)
    }

    fn wait_slot_job(job: &Job<Result<(), RealmsError>>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while job.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline, "slot task did not settle");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn finish_slot_workers(workers: &Workers) {
        let handle = workers.work_handle();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !handle.shutdown() {
            assert!(Instant::now() < deadline, "slot producer did not stop");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn slot_switch_retries_false_and_completes_only_on_true() {
        let (base, server) = slot_service(vec![
            (200, None, "false"),
            (200, None, "false"),
            (200, None, "true"),
        ]);
        let aborted = AtomicBool::new(false);
        let answer = switch_slot_task(&slot_client(&base), 7, 2, &aborted, |_| {
            panic!("false does not add a native retry delay")
        });
        let requests = server.join().unwrap();
        assert_eq!(answer, Ok(()));
        assert_eq!(requests.len(), 3, "false was treated as task success");
        assert!(
            requests
                .iter()
                .all(|r| r == "PUT /worlds/7/slot/2 HTTP/1.1")
        );
    }

    #[test]
    fn slot_switch_exhaustion_keeps_the_worlds_parent_and_never_applies_success() {
        let (base, server) = slot_service(vec![(200, None, "false"); 25]);
        let mut screen = players_fixture(0);
        let pending = Arc::new(Mutex::new(None));
        if let SubScreen::Configure {
            server,
            tab,
            detail: Some(Ok(detail)),
            detail_job,
            error,
            ..
        } = &mut screen.subscreen
        {
            *tab = ConfigureTab::Worlds;
            server.active_slot = 1;
            detail.active_slot = 1;
            *detail_job = Some(pending.clone());
            *error = Some("previous status".into());
        }
        let original = screen.subscreen.clone();
        let SubScreen::Configure {
            server: realm,
            detail: Some(Ok(old_detail)),
            subscription,
            ..
        } = &original
        else {
            unreachable!()
        };
        screen.start_slot_switch(
            slot_client(&base),
            realm.clone(),
            2,
            Some(Box::new(original.clone())),
        );
        let SubScreen::Configure {
            member_job: Some(job),
            ..
        } = &screen.subscreen
        else {
            unreachable!()
        };
        let job = job.clone();
        wait_slot_job(&job);
        let _ = screen.poll();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 25);
        assert!(
            requests
                .iter()
                .all(|r| r == "PUT /worlds/7/slot/2 HTTP/1.1")
        );
        let SubScreen::Configure {
            tab: ConfigureTab::Worlds,
            detail: Some(Ok(detail)),
            detail_job: Some(still_pending),
            subscription: current_subscription,
            member_job: None,
            error: Some(error),
            ..
        } = &screen.subscreen
        else {
            panic!("the failed task applied success or lost its parent");
        };
        assert_eq!(detail, old_detail);
        assert_eq!(detail.active_slot, 1);
        assert_eq!(current_subscription, subscription);
        assert!(Arc::ptr_eq(still_pending, &pending));
        assert_eq!(error, &RealmsError::Timeout.text());
        assert!(screen.slot_switch.is_none());
        finish_slot_workers(&screen.workers);
    }

    #[test]
    fn slot_switch_honors_native_retry_after_delays_with_only_the_wait_clock_controlled() {
        let (base, server) = slot_service(vec![
            (503, Some(3), ""),
            (277, Some(120), ""),
            (503, Some(999), ""),
            (200, None, "true"),
        ]);
        let aborted = AtomicBool::new(false);
        let mut pauses = Vec::new();
        let answer = switch_slot_task(&slot_client(&base), 7, 2, &aborted, |delay| {
            pauses.push(delay);
            Ok(())
        });
        assert_eq!(answer, Ok(()));
        assert_eq!(
            pauses,
            [
                Duration::from_secs(3),
                Duration::from_secs(120),
                Duration::from_secs(5)
            ]
        );
        assert_eq!(server.join().unwrap().len(), 4);
        aborted.store(true, Ordering::Release);
        assert_eq!(
            switch_slot_task(
                &slot_client("http://127.0.0.1:9"),
                7,
                2,
                &aborted,
                |_| unreachable!()
            ),
            Err(RealmsError::Cancelled)
        );
    }

    #[test]
    fn cancelling_a_pending_slot_retry_keeps_worlds_and_does_not_cancel_other_operations() {
        for escape in [true, false] {
            let (base, server) = slot_service(vec![(503, Some(30), "")]);
            let mut screen = players_fixture(0);
            if let SubScreen::Configure { tab, error, .. } = &mut screen.subscreen {
                *tab = ConfigureTab::Worlds;
                *error = Some("retained status".into());
            }
            let parent = screen.subscreen.clone();
            let SubScreen::Configure { server: realm, .. } = &parent else {
                unreachable!()
            };
            let (entered, waiting) = std::sync::mpsc::channel();
            screen.start_slot_switch_with_pause(
                slot_client(&base),
                realm.clone(),
                2,
                Some(Box::new(parent)),
                move |client, aborted, delay| {
                    entered.send(delay).unwrap();
                    wait_for_retry(client, aborted, delay)
                },
            );
            let SubScreen::Configure {
                member_job: Some(job),
                ..
            } = &screen.subscreen
            else {
                unreachable!()
            };
            let job = job.clone();
            assert_eq!(
                waiting.recv_timeout(Duration::from_secs(2)).unwrap(),
                Duration::from_secs(30)
            );
            let start = Instant::now();
            if escape {
                assert!(!screen.escape());
            } else {
                let _ = screen.click(Some(32), 0.0, 0.0, (800.0, 600.0));
            }
            wait_slot_job(&job);
            assert!(start.elapsed() < Duration::from_secs(2));
            assert_eq!(
                job.lock().unwrap().take(),
                Some(Err(RealmsError::Cancelled))
            );
            assert!(
                matches!(&screen.subscreen, SubScreen::Configure { tab: ConfigureTab::Worlds, member_job: None, error: Some(e), .. } if e == "retained status")
            );
            assert!(screen.slot_switch.is_none());
            assert!(!screen.workers.cancellation().cancelled());
            assert_eq!(
                server.join().unwrap().len(),
                1,
                "cancel must prevent the next PUT"
            );
            let still_usable = screen.workers.spawn(|| Ok(11));
            let deadline = Instant::now() + Duration::from_secs(2);
            while still_usable.lock().unwrap().is_none() {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            assert_eq!(still_usable.lock().unwrap().take(), Some(Ok(11)));
            finish_slot_workers(&screen.workers);
        }
    }

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
            workers: Workers::new(),
            return_subscreen: None,
            slot_switch: None,
            settings_next: None,
            region_scroll: 0.0,
            players_scroll: 0.0,
            player_rows: RefCell::new(Vec::new()),
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

    /// `RealmsSettingsTab.updateData`: the draft starts from the Realm's own name,
    /// description and region preference; a Realm without one is automatic by owner
    /// (`RegionSelectionPreferenceDto.DEFAULT`), and only a manual choice sends its region.
    #[test]
    fn the_settings_draft_starts_from_the_realm() {
        let server: RealmServer = serde_json::from_str(
            r#"{"id":7,"name":"Mine","motd":"Hello",
                "regionSelectionPreference":{"regionSelectionPreference":2,"preferredRegion":"EastUs2"}}"#,
        )
        .unwrap();
        let mut draft = SettingsDraft::of(&server);
        assert_eq!(
            (draft.name.as_str(), draft.description.as_str()),
            ("Mine", "Hello")
        );
        assert_eq!(
            (draft.preference, draft.region.as_deref()),
            (2, Some("EastUs2"))
        );
        draft.preference = client::AUTOMATIC_PLAYER;
        assert_eq!(draft.preference_dto().preferred_region, None);
        let bare = SettingsDraft::of(&RealmServer::default());
        assert_eq!((bare.preference, bare.region), (1, None));
    }

    /// `RealmsWorldOptions.getSlotName`: the player's name, else the empty marker, else
    /// "World N" by the slot.
    #[test]
    fn a_slot_without_a_name_says_world_n_or_empty() {
        let options = RealmWorldOptions::default();
        assert_eq!(
            slot_name(&options, 3),
            crate::lang::tr("mco.configure.world.slot", &[&3])
        );
        let empty = RealmWorldOptions {
            empty: true,
            ..Default::default()
        };
        assert_eq!(
            slot_name(&empty, 2),
            crate::lang::get("mco.configure.world.slot.empty")
        );
        let named = RealmWorldOptions {
            slot_name: "Base".into(),
            ..Default::default()
        };
        assert_eq!(slot_name(&named, 1), "Base");
    }

    /// `RealmsWorldsTab`: the active slot is 80 square, the others 50, 16 apart, centred.
    #[test]
    fn the_slot_buttons_give_the_active_slot_eighty_pixels() {
        let boxes = slot_boxes(427.0, 2);
        // 80 + 50*3 + 16*3 = 278 wide, centred on 427 starts at 74.
        assert_eq!(boxes[0], (74.0, 50.0, 1));
        assert_eq!(boxes[1].0, 140.0);
        assert_eq!(boxes[1].1, 80.0);
        assert_eq!(boxes[1].2, 2);
        assert_eq!(boxes[3].1, 50.0);
    }

    /// `RealmsInviteScreen.onInvite`: a blank name and one already invited are refused
    /// before any request.
    #[test]
    fn inviting_an_empty_or_duplicate_name_asks_nothing() {
        let mut screen = Screen {
            state: FetchState::Loaded(vec![server(7, None)]),
            subscreen: SubScreen::Invite {
                server: server(7, None),
                players: vec![PlayerInfo {
                    name: "Alex".into(),
                    uuid: Some("01234567-89ab-cdef-0123-456789abcdef".into()),
                    ..Default::default()
                }],
                name: "  ".into(),
                job: None,
                error: None,
            },
            selected: Some(0),
            joining: None,
            leaving: None,
            workers: Workers::new(),
            return_subscreen: None,
            slot_switch: None,
            settings_next: None,
            region_scroll: 0.0,
            players_scroll: 0.0,
            player_rows: RefCell::new(Vec::new()),
            last_click: None,
        };
        screen.invite_clicked();
        let SubScreen::Invite { error, job, .. } = &screen.subscreen else {
            panic!("still the invite screen");
        };
        assert!(job.is_none(), "a blank name asks the service nothing");
        assert_eq!(
            error.as_deref(),
            Some(crate::lang::get("mco.configure.world.players.error").as_str())
        );

        if let SubScreen::Invite { name, error, .. } = &mut screen.subscreen {
            *name = "ALEX".into();
            *error = None;
        }
        screen.invite_clicked();
        let SubScreen::Invite { error, job, .. } = &screen.subscreen else {
            panic!("still the invite screen");
        };
        assert!(job.is_none(), "a duplicate name asks the service nothing");
        assert_eq!(
            error.as_deref(),
            Some(crate::lang::get("mco.configure.world.players.invite.duplicate").as_str())
        );
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

    /// RealmsSubscriptionTab opens its warning over the configure screen;
    /// PopupScreen.onClose restores that screen, including its selected tab.
    #[test]
    fn cancelling_realm_deletion_restores_the_subscription_tab() {
        let mut realm = server(7, None);
        realm.expired = true;
        let subscription = Subscription {
            start_date: 1_700_000_000_000,
            days_left: -1,
            kind: "NORMAL".into(),
        };
        for escape in [false, true] {
            let mut screen = listed(vec![realm.clone()]);
            screen.subscreen = SubScreen::Configure {
                server: realm.clone(),
                tab: ConfigureTab::Subscription,
                subscription: Some(Ok(subscription.clone())),
                subscription_job: None,
                settings: None,
                regions: None,
                regions_job: None,
                saving: None,
                detail: None,
                detail_job: None,
                deleting: None,
                member_job: None,
                error: None,
                job: None,
            };
            let _ = screen.click(Some(65), 0.0, 0.0, (800.0, 600.0));
            assert!(matches!(screen.subscreen, SubScreen::Popup(_)));
            if escape {
                assert!(!screen.escape());
            } else {
                let _ = screen.click(Some(51), 0.0, 0.0, (800.0, 600.0));
            }
            assert!(
                matches!(
                    &screen.subscreen,
                    SubScreen::Configure {
                        server,
                        tab: ConfigureTab::Subscription,
                        subscription: Some(Ok(data)),
                        subscription_job: None,
                        settings: None,
                        regions: None,
                        regions_job: None,
                        saving: None,
                        detail: None,
                        detail_job: None,
                        deleting: None,
                        member_job: None,
                        job: None,
                        ..
                    } if server == &realm && data == &subscription
                ),
                "cancel must restore the loaded subscription without starting a service request"
            );
            assert_eq!(screen.selected, Some(0));
            assert!(screen.joining.is_none());
            assert!(screen.leaving.is_none());
        }
    }

    /// The native invite screen and removal confirmation return to the same
    /// configure screen. Keeping that object also preserves its pending fetches.
    #[test]
    fn cancelling_configure_children_preserves_loaded_data_and_pending_jobs() {
        for action in [65, 67, 101] {
            for escape in [false, true] {
                let mut realm = server(7, None);
                realm.expired = action == 65;
                let player = PlayerInfo {
                    name: "Alex".into(),
                    uuid: Some("0123456789abcdef0123456789abcdef".into()),
                    ..Default::default()
                };
                let mut detail = realm.clone();
                detail.players = vec![player];
                let tab = if action == 65 {
                    ConfigureTab::Subscription
                } else {
                    ConfigureTab::Players
                };
                let subscription = Subscription {
                    start_date: 123,
                    days_left: 12,
                    kind: "NORMAL".into(),
                };
                let detail_job = Arc::new(Mutex::new(None));
                let mut screen = listed(vec![realm.clone()]);
                screen.subscreen = SubScreen::Configure {
                    server: realm.clone(),
                    tab,
                    subscription: Some(Ok(subscription.clone())),
                    subscription_job: None,
                    settings: None,
                    regions: None,
                    regions_job: None,
                    saving: None,
                    detail: Some(Ok(detail.clone())),
                    detail_job: Some(detail_job.clone()),
                    deleting: None,
                    member_job: None,
                    error: Some("retained status".into()),
                    job: None,
                };
                let _ = screen.buttons(800.0, 600.0);
                let _ = screen.click(Some(action), 0.0, 0.0, (800.0, 600.0));
                assert!(matches!(
                    screen.subscreen,
                    SubScreen::Invite { .. } | SubScreen::Popup(_)
                ));
                if escape {
                    assert!(!screen.escape());
                } else {
                    let _ = screen.click(
                        Some(if action == 67 { 81 } else { 51 }),
                        0.0,
                        0.0,
                        (800.0, 600.0),
                    );
                }
                let SubScreen::Configure {
                    server,
                    tab: restored_tab,
                    subscription: Some(Ok(data)),
                    detail: Some(Ok(roster)),
                    detail_job: Some(pending),
                    error,
                    member_job: None,
                    ..
                } = &screen.subscreen
                else {
                    panic!("configure parent was lost for {action}");
                };
                assert_eq!(server, &realm);
                assert_eq!(*restored_tab, tab);
                assert_eq!(data, &subscription);
                assert_eq!(roster, &detail);
                assert_eq!(error.as_deref(), Some("retained status"));
                assert!(Arc::ptr_eq(pending, &detail_job));
                assert!(screen.return_subscreen.is_none());
                // The pending detail fetch still reaches the restored screen.
                let mut updated = detail.clone();
                updated.players[0].online = true;
                *detail_job.lock().unwrap() = Some(Ok(updated.clone()));
                let _ = screen.poll();
                assert!(
                    matches!(&screen.subscreen, SubScreen::Configure { detail: Some(Ok(got)), .. } if got == &updated)
                );
            }
        }
    }

    #[test]
    fn large_player_rosters_have_safe_unique_widgets_and_every_player_is_reachable() {
        for height in [240.0, 4096.0] {
            let mut screen = players_fixture(100);
            let size = (800.0, height);
            let cursor = (400.0, PLAYERS_TOP + 1.0);
            let mut reached = std::collections::HashSet::new();
            for _ in 0..102 {
                // The same CPU layout binding draw records for a new frame.
                screen.remember_player_rows(size);
                let buttons = screen.buttons(size.0, size.1);
                let ids: std::collections::HashSet<_> = buttons.iter().map(|b| b.5).collect();
                assert_eq!(ids.len(), buttons.len(), "overlapping u8 ids at {height}");
                let rows = screen.player_rows.borrow().clone();
                assert!(!rows.is_empty());
                for row in &rows {
                    let remove = buttons.iter().find(|b| b.5 == row.button + 1).unwrap();
                    assert!(remove.4);
                    assert!(remove.1 >= PLAYERS_TOP && remove.1 + 20.0 <= height - PLAYERS_BOTTOM);
                    let before = screen.players_scroll;
                    let _ = screen.click(Some(remove.5), 0.0, 0.0, size);
                    assert!(
                        matches!(&screen.subscreen, SubScreen::Popup(Popup::Uninvite { player, .. }) if player.uuid == row.player.uuid)
                    );
                    let _ = screen.click(Some(51), 0.0, 0.0, size);
                    assert_eq!(screen.players_scroll, before);
                    reached.insert(row.player.uuid.clone().unwrap());
                }
                let before = screen.player_range(100, height).start;
                assert!(screen.scroll(-(rows.len() as f32), cursor, size));
                if screen.player_range(100, height).start == before {
                    break;
                }
            }
            assert_eq!(reached.len(), 100, "roster was truncated at {height}");
        }
    }

    #[test]
    fn player_actions_remember_uuid_across_refresh_scroll_and_cancel() {
        let mut screen = players_fixture(100);
        let size = (800.0, 240.0);
        assert!(screen.scroll(-50.0, (400.0, 61.0), size));
        let _ = screen.buttons(size.0, size.1);
        let painted = screen.player_rows.borrow()[0].clone();
        if let SubScreen::Configure {
            detail: Some(Ok(detail)),
            ..
        } = &mut screen.subscreen
        {
            detail.players.swap(50, 51);
        }
        // A refresh happened after layout, before the click. Target the UUID
        // actually painted, not the player now occupying its old list index.
        let _ = screen.click(Some(painted.button + 1), 0.0, 0.0, size);
        assert!(
            matches!(&screen.subscreen, SubScreen::Popup(Popup::Uninvite { player, .. }) if player.uuid == painted.player.uuid)
        );
        assert!(!screen.escape());
        assert_eq!(screen.players_scroll, 50.0);
        screen.remember_player_rows(size);
        let _ = screen.buttons(size.0, size.1);
        let repainted = screen.player_rows.borrow()[0].clone();
        assert_ne!(painted.player.uuid, repainted.player.uuid);
        if let SubScreen::Configure {
            detail: Some(Ok(detail)),
            ..
        } = &mut screen.subscreen
        {
            detail.players.retain(|p| p.uuid != repainted.player.uuid);
        }
        let _ = screen.click(Some(repainted.button + 1), 0.0, 0.0, size);
        assert!(
            matches!(screen.subscreen, SubScreen::Configure { .. }),
            "a removed UUID must not redirect the action to a neighbor"
        );
        let _ = screen.buttons(size.0, size.1);
        let _ = screen.click(Some(67), 0.0, 0.0, size);
        assert!(matches!(screen.subscreen, SubScreen::Invite { .. }));
        let _ = screen.click(Some(81), 0.0, 0.0, size);
        assert_eq!(screen.players_scroll, 50.0);
        assert!(matches!(
            screen.subscreen,
            SubScreen::Configure {
                tab: ConfigureTab::Players,
                ..
            }
        ));
    }

    #[test]
    fn player_scroll_accumulates_fractional_input_and_clamps_after_resize() {
        let mut screen = players_fixture(100);
        let size = (800.0, 240.0);
        let cursor = (400.0, 61.0);
        for _ in 0..10 {
            assert!(screen.scroll(-0.25, cursor, size));
        }
        assert_eq!(screen.player_range(100, size.1).start, 2);
        assert!(!screen.scroll(f32::NAN, cursor, size));
        assert!(!screen.scroll(-1.0, (0.0, 0.0), size));
        assert_eq!(screen.player_range(100, size.1).start, 2);
        assert!(screen.scroll(-f32::MAX, cursor, size));
        assert_eq!(screen.player_range(100, size.1).end, 100);
        let _ = screen.buttons(800.0, 4096.0);
        assert_eq!(screen.player_rows.borrow().len(), MAX_PLAYER_ROWS);
        assert!(screen.scroll(1.0, cursor, (800.0, 4096.0)));
        assert_eq!(screen.player_range(100, 4096.0).start, 21);
        assert!(screen.scroll(f32::MAX, cursor, (800.0, 4096.0)));
        assert_eq!(screen.player_range(100, 4096.0).start, 0);
    }

    #[test]
    fn delayed_player_taps_keep_uuid_realm_and_operation_identity_across_paints() {
        let saved = crate::widgets::take();
        let mut screen = players_fixture(2);
        let size = (800.0, 240.0);
        let rects = Screen::player_button_rects(size.0, 0);
        let point = |rect: [f32; 4]| (rect[0] + rect[2] / 2.0, rect[1] + 10.0);
        let paint = |screen: &Screen, frame| {
            crate::widgets::set_action_screen(7);
            screen.remember_player_rows(size);
            screen.register_player_bindings(size);
            for (x, y, w, label, enabled, ..) in screen.buttons(size.0, size.1) {
                crate::widgets::record(&label, [x, y, w, 20.0], enabled);
            }
            crate::widgets::end_frame(frame);
        };
        paint(&screen, 1);
        let remove = crate::widgets::capture_pointer(point(rects[1]));
        paint(&screen, 2);
        assert_eq!(crate::widgets::capture_pointer(point(rects[1])), remove);
        if let SubScreen::Configure {
            detail: Some(Ok(detail)),
            ..
        } = &mut screen.subscreen
        {
            detail.players.swap(0, 1);
        }
        paint(&screen, 3);
        assert_ne!(
            crate::widgets::capture_pointer(point(rects[1])),
            remove,
            "a repaint to another UUID must cancel the delayed tap"
        );
        let permission = crate::widgets::capture_pointer(point(rects[0]));
        if let SubScreen::Configure {
            detail: Some(Ok(detail)),
            ..
        } = &mut screen.subscreen
        {
            detail.players[0].operator = !detail.players[0].operator;
        }
        paint(&screen, 4);
        assert_ne!(crate::widgets::capture_pointer(point(rects[0])), permission);
        let realm = crate::widgets::capture_pointer(point(rects[1]));
        if let SubScreen::Configure { server, .. } = &mut screen.subscreen {
            server.id += 1;
        }
        paint(&screen, 5);
        assert_ne!(crate::widgets::capture_pointer(point(rects[1])), realm);
        crate::widgets::put(saved);
    }

    #[test]
    fn cancelling_slot_options_and_switching_keeps_the_exact_worlds_parent() {
        let detail: RealmServer = serde_json::from_str(r#"{"id":7,"name":"Fixture","worldType":"NORMAL","activeSlot":1,"slots":[{"slotId":1,"options":"{}"},{"slotId":2,"options":"{}"}]}"#).unwrap();
        for options in [true, false] {
            for escape in [true, false] {
                let mut screen = players_fixture(1);
                let pending = Arc::new(Mutex::new(None));
                let subscription = Subscription {
                    days_left: 12,
                    ..Default::default()
                };
                screen.subscreen = SubScreen::Configure {
                    server: detail.clone(),
                    tab: ConfigureTab::Worlds,
                    subscription: Some(Ok(subscription.clone())),
                    subscription_job: None,
                    settings: None,
                    regions: None,
                    regions_job: None,
                    saving: None,
                    detail: Some(Ok(detail.clone())),
                    detail_job: Some(pending.clone()),
                    deleting: None,
                    member_job: None,
                    error: Some("retained status".into()),
                    job: None,
                };
                if options {
                    let _ = screen.click(Some(94), 0.0, 0.0, (800.0, 600.0));
                    assert!(matches!(screen.subscreen, SubScreen::SlotOptions { .. }));
                } else {
                    let (x, size, _) = slot_boxes(800.0, 1)[1];
                    let _ = screen.click(None, x + size / 2.0, 45.0, (800.0, 600.0));
                    assert!(matches!(
                        screen.subscreen,
                        SubScreen::Popup(Popup::SwitchSlot { slot: 2, .. })
                    ));
                }
                if escape {
                    assert!(!screen.escape());
                } else {
                    let _ = screen.click(
                        Some(if options { 99 } else { 51 }),
                        0.0,
                        0.0,
                        (800.0, 600.0),
                    );
                }
                let SubScreen::Configure {
                    tab: ConfigureTab::Worlds,
                    subscription: Some(Ok(data)),
                    detail: Some(Ok(world)),
                    detail_job: Some(job),
                    error,
                    ..
                } = &screen.subscreen
                else {
                    panic!("the worlds parent was lost");
                };
                assert_eq!(data, &subscription);
                assert_eq!(world, &detail);
                assert!(Arc::ptr_eq(job, &pending));
                assert_eq!(error.as_deref(), Some("retained status"));
            }
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
