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
    PendingInviteDto, PlayerInfo, RealmAddress, RealmServer, RealmSlot, RealmTierConfiguration,
    RealmTierRange, RealmWorldOptions, RealmsClient, RealmsError, RegionDataDto,
    RegionSelectionPreferenceDto, Subscription,
};
use horizon_client_render::auth::{self, AuthSession};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER_HEIGHT: f32 = 44.0;
const CONTENT_WIDTH: f32 = 308.0;
/// `RealmsPdpScreen`'s list: the row width and the text's width beside the picture.
const PDP_ROW_WIDTH: f32 = 290.0;
const PDP_TEXT_WIDTH: f32 = 182.0;

const ROW_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 36.0;
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
    /// The picked distances, once the tier configuration arrived
    /// (`RealmsSettingsTab`'s two `OptionInstance`s; none while it hasn't).
    pub render_distance: Option<i32>,
    pub simulation_distance: Option<i32>,
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
            render_distance: None,
            simulation_distance: None,
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

/// `RealmsSettingsTab.save`'s `updatedRenderDistance`/`updatedSimulationDistance`: a distance
/// is sent only when the player changed it and the tier answer lets the service configure it.
fn changed_distance(picked: Option<i32>, range: Option<&RealmTierRange>) -> Option<i32> {
    picked.filter(|picked| range.is_some_and(|range| *picked != range.current_or_default()))
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
    Delete(RealmServer),
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
        /// `RealmsClient.getRealmTierConfiguration`, then its answer: the settings tab's
        /// distance sliders (`tierConfigurationFetched` keeps it to one fetch).
        tier: Option<Result<RealmTierConfiguration, RealmsError>>,
        tier_job: Option<Job<Result<RealmTierConfiguration, RealmsError>>>,
        /// `RealmsClient.preferredRegions` for the region preference screen.
        regions: Option<Result<Vec<RegionDataDto>, RealmsError>>,
        regions_job: Option<Job<Result<Vec<RegionDataDto>, RealmsError>>>,
        /// `RealmsClient.updateConfiguration` on a worker.
        saving: Option<Job<Result<(), RealmsError>>>,
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
    /// `RealmsPdpScreen`: the product page behind Add Realm, its picture rows and scroll.
    AddRealm {
        scroll: crate::ui::Scroll,
        /// The list's drawn height, measured in [`Screen::draw`] and read by the wheel.
        content: std::cell::Cell<f32>,
    },
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

/// `RealmsPdpScreen`'s list: the trial header (no picture) and the three features, each
/// with its 80x45 picture and the text beside it. The header has no subtitle: Horizon does
/// not ask `trialAvailabilityTask`, and vanilla shows the trial description only when a
/// trial is available (its footer then offers the trial instead of Buy).
fn pdp_entries() -> [(&'static str, &'static str, &'static [&'static str]); 4] {
    [
        ("", "mco.pdp.trial.title", &[]),
        (
            "gui/realms/friends",
            "mco.pdp.friends.title",
            &["mco.pdp.friends.description"],
        ),
        (
            "gui/realms/minigames",
            "mco.pdp.minigames.title",
            &["mco.pdp.minigames.description"],
        ),
        (
            "gui/realms/private_server",
            "mco.pdp.private.title",
            &[
                "mco.pdp.private.backups",
                "mco.pdp.private.easyToManage",
                "mco.pdp.private.worldSlots",
                "mco.pdp.private.secure",
            ],
        ),
    ]
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
        let mut refetch_detail = false;
        let mut slot_options_saved = false;
        let mut open_region_screen = false;
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
                settings,
                tier,
                tier_job,
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
                if let Some(result) = tier_job.as_ref().and_then(take) {
                    *tier_job = None;
                    // The tab's sliders take the ranges as they arrived; a draft opened
                    // before the answer gets its initial values now.
                    if let Ok(config) = result.as_ref()
                        && let Some(draft) = settings.as_mut()
                        && draft.render_distance.is_none()
                        && draft.simulation_distance.is_none()
                    {
                        draft.render_distance = config
                            .render_distance
                            .as_ref()
                            .map(RealmTierRange::current_or_default);
                        draft.simulation_distance = config
                            .sim_distance
                            .as_ref()
                            .map(RealmTierRange::current_or_default);
                    }
                    *tier = Some(result);
                }
                if let Some(result) = member_job.as_ref().and_then(take) {
                    *member_job = None;
                    match result {
                        // `updateOps`: the list reads again with the new operators.
                        Ok(()) => refetch_detail = true,
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
                        // `saveSettings` updates the screen's own copy and reads again; the
                        // saved distances are the tier's new current ones.
                        Ok(()) => {
                            if let Some(draft) = settings.as_ref() {
                                server.name = draft.name.trim().to_owned();
                                server.motd = draft.description.clone();
                                server.region_selection_preference = Some(draft.preference_dto());
                                if let Some(Ok(config)) = tier.as_mut() {
                                    let update = |range: &mut Option<RealmTierRange>,
                                                  picked: Option<i32>| {
                                        if let (Some(range), Some(picked)) =
                                            (range.as_mut(), picked)
                                        {
                                            range.current = Some(picked);
                                        }
                                    };
                                    update(&mut config.render_distance, draft.render_distance);
                                    update(&mut config.sim_distance, draft.simulation_distance);
                                }
                            }
                            refetch_detail = true;
                        }
                        Err(e) => *error = Some(e.text()),
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
                                tier: None,
                                tier_job: None,
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
        if open_region_screen {
            self.open_region_preference();
        }
        if slot_options_saved {
            self.back_to_worlds();
        }
        if refetch_detail {
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
                ..
            } => Some(if draft.on_description {
                (REALMS_SETTINGS_DESC, &mut draft.description, 32)
            } else {
                (REALMS_SETTINGS_NAME, &mut draft.name, 32)
            }),
            _ => None,
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
        self.save_settings_tab();
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
        if let Some(client) = client() {
            let id = server.id;
            *detail_job = Some(spawn(move || client.get_own_realm(id)));
        }
        self.start_configure_tier();
    }

    /// `RealmsConfigureWorldScreen.init`'s `fetchTierConfiguration`: asked again on every
    /// entry until it answered (`tierConfigurationFetched`).
    fn start_configure_tier(&mut self) {
        let SubScreen::Configure {
            server,
            tier,
            tier_job,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        if tier.is_some() || tier_job.is_some() {
            return;
        }
        if let Some(client) = client() {
            let id = server.id;
            *tier_job = Some(spawn(move || client.get_tier_configuration(id)));
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
            if let Some(client) = client() {
                *error = None;
                *job = Some(spawn(move || client.invite_player(id, &name)));
            }
        }
    }

    /// A chosen region preference: back to the settings tab with it.
    fn back_to_settings(&mut self, server: RealmServer, settings: SettingsDraft) {
        self.subscreen = SubScreen::Configure {
            server,
            tab: ConfigureTab::Settings,
            subscription: None,
            subscription_job: None,
            detail: None,
            detail_job: None,
            deleting: None,
            member_job: None,
            tier: None,
            tier_job: None,
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

    /// `RealmsSettingsTab.onDeselected` → `RealmsConfigureWorldScreen.saveSettings`: the name,
    /// description and region preference through `RealmsClient.updateConfiguration`, and a
    /// changed distance through its own PUT — each only when it differs from what the service
    /// has.
    fn save_settings_tab(&mut self) {
        let SubScreen::Configure {
            tab: ConfigureTab::Settings,
            server,
            detail,
            settings: Some(draft),
            tier,
            saving,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        if saving.is_some() {
            return;
        }
        let wanted = draft.preference_dto();
        let region_changed = server.region_selection_preference.as_ref().map_or(
            wanted.region_selection_preference != client::AUTOMATIC_OWNER,
            |p| *p != wanted,
        );
        let configuration_changed =
            draft.name.trim() != server.name || draft.description != server.motd || region_changed;
        let (render_distance, sim_distance) = match tier.as_ref() {
            Some(Ok(config)) => (
                changed_distance(draft.render_distance, config.render_distance.as_ref()),
                changed_distance(draft.simulation_distance, config.sim_distance.as_ref()),
            ),
            _ => (None, None),
        };
        if !configuration_changed && render_distance.is_none() && sim_distance.is_none() {
            return;
        }
        let Some(Ok(detail)) = detail else {
            return;
        };
        let Some(slot) = detail.slots.get(&detail.active_slot).cloned() else {
            return;
        };
        if let Some(client) = client() {
            let (id, name, description) = (
                server.id,
                draft.name.trim().to_owned(),
                draft.description.clone(),
            );
            *saving = Some(spawn(move || {
                if configuration_changed {
                    client.update_configuration(id, &name, &description, &wanted, &slot)?;
                }
                if let Some(render_distance) = render_distance {
                    client.update_render_distance(id, render_distance)?;
                }
                if let Some(simulation_distance) = sim_distance {
                    client.update_simulation_distance(id, simulation_distance)?;
                }
                Ok(())
            }));
        }
    }

    /// `RealmsSettingsTab.openPreferenceSelector`, once the region list is there.
    fn open_region_preference(&mut self) {
        let SubScreen::Configure {
            server,
            settings: Some(draft),
            regions: Some(Ok(list)),
            ..
        } = &self.subscreen
        else {
            return;
        };
        let (server, settings, regions) = (server.clone(), draft.clone(), list.clone());
        let selected = Some((settings.preference, settings.region.clone()));
        self.subscreen = SubScreen::RegionPreference {
            server,
            settings,
            regions,
            selected,
        };
    }

    /// `RealmsSlotOptionsScreen.onClose`: back to the worlds tab.
    fn back_to_worlds(&mut self) {
        let SubScreen::SlotOptions { server, .. } = &self.subscreen else {
            return;
        };
        let server = server.clone();
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
            tier: None,
            tier_job: None,
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
        if let Some(client) = client() {
            let id = server.id;
            *job = Some(spawn(move || client.update_slot(id, &updated)));
        }
    }

    /// `RealmsInviteScreen.onClose`: back to the players tab.
    fn back_to_players(&mut self) {
        let SubScreen::Invite { server, .. } = &self.subscreen else {
            return;
        };
        let server = server.clone();
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
            tier: None,
            tier_job: None,
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
                        detail: None,
                        detail_job: None,
                        deleting: None,
                        member_job: None,
                        settings: None,
                        tier: None,
                        tier_job: None,
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
                self.subscreen = SubScreen::AddRealm {
                    scroll: crate::ui::Scroll::default(),
                    content: std::cell::Cell::new(0.0),
                };
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
                    self.subscreen = SubScreen::Invite {
                        server: server.clone(),
                        players,
                        name: String::new(),
                        job: None,
                        error: None,
                    };
                }
                Outcome::None
            }
            // RealmsSlotOptionsScreen: the active slot's options.
            Some(94) => {
                if let SubScreen::Configure {
                    server,
                    detail: Some(Ok(detail)),
                    ..
                } = &self.subscreen
                    && let Some(slot) = detail.slots.get(&detail.active_slot)
                {
                    let normal = server.world_type == "NORMAL";
                    self.subscreen = SubScreen::SlotOptions {
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
                if let SubScreen::Configure {
                    server,
                    settings: Some(draft),
                    regions,
                    ..
                } = &self.subscreen
                {
                    let draft = draft.clone();
                    let server = server.clone();
                    let loaded = matches!(regions, Some(Ok(_)));
                    if loaded {
                        if let SubScreen::Configure {
                            regions: Some(Ok(regions)),
                            ..
                        } = &self.subscreen
                        {
                            let regions = regions.clone();
                            let selected = Some((draft.preference, draft.region.clone()));
                            self.subscreen = SubScreen::RegionPreference {
                                server,
                                settings: draft,
                                regions,
                                selected,
                            };
                        }
                    } else if let SubScreen::Configure { regions_job, .. } = &mut self.subscreen
                        && regions_job.is_none()
                        && let Some(client) = client()
                    {
                        *regions_job = Some(spawn(move || client.preferred_regions()));
                    }
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
            Some(btn @ 100..=200) => {
                // The button's meaning: a pending invite's answer, or a player row's
                // Op/Deop/Remove (`RealmsPlayersTab.PlayerEntry`).
                enum Row {
                    Nothing,
                    Pending(bool, String),
                    Member(bool, i64, String),
                    Uninvite(RealmServer, PlayerInfo),
                }
                let row = match &self.subscreen {
                    SubScreen::PendingInvites { invites, job } if job.is_none() => {
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
                        let index = usize::from((btn - 100) / 3);
                        let which = (btn - 100) % 3;
                        let player = match detail {
                            Some(Ok(detail)) => detail.players.get(index).cloned(),
                            _ => None,
                        };
                        match player {
                            Some(player) if which < 2 => match &player.uuid {
                                Some(uuid) if !uuid.is_empty() => {
                                    Row::Member(which == 0, server.id, uuid.clone())
                                }
                                _ => Row::Nothing,
                            },
                            Some(player) => Row::Uninvite(server.clone(), player),
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
                            && let Some(client) = client()
                        {
                            *job = Some(spawn(move || {
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
                        if let SubScreen::Configure { member_job, .. } = &mut self.subscreen
                            && member_job.is_none()
                            && let Some(client) = client()
                        {
                            *member_job = Some(spawn(move || {
                                if op {
                                    client.op_player(realm_id, &uuid).map(drop)
                                } else {
                                    client.deop_player(realm_id, &uuid).map(drop)
                                }
                            }));
                        }
                    }
                    Row::Uninvite(server, player) => {
                        self.subscreen = SubScreen::Popup(Popup::Uninvite { server, player });
                    }
                }
                Outcome::None
            }
            // The tab bar draws its own tabs (`MenuTabBar`), so a click there carries no
            // button id: the tab bar's geometry decides.
            None if matches!(self.subscreen, SubScreen::Configure { .. }) => {
                let (bar_x, tab_w) = tab_geometry(w);
                if y < TAB_HEIGHT && x >= bar_x {
                    let index = ((x - bar_x) / tab_w).floor() as usize;
                    let current = match &self.subscreen {
                        SubScreen::Configure { tab, .. } => Some(*tab),
                        _ => None,
                    };
                    if let (Some(current), Some(wanted)) = (current, ConfigureTab::ALL.get(index))
                        && *wanted != current
                        && current == ConfigureTab::Settings
                    {
                        self.save_settings_tab();
                    }
                    if let SubScreen::Configure {
                        server,
                        tab,
                        settings,
                        tier,
                        ..
                    } = &mut self.subscreen
                        && let Some(wanted) = ConfigureTab::ALL.get(index)
                        && wanted.enabled(server.expired)
                    {
                        if *wanted == ConfigureTab::Settings && settings.is_none() {
                            // `RealmsSettingsTab.updateData`: the sliders start at the tier's
                            // current values when its answer is already here.
                            let mut draft = SettingsDraft::of(server);
                            if let Some(Ok(config)) = tier.as_ref() {
                                draft.render_distance = config
                                    .render_distance
                                    .as_ref()
                                    .map(RealmTierRange::current_or_default);
                                draft.simulation_distance = config
                                    .sim_distance
                                    .as_ref()
                                    .map(RealmTierRange::current_or_default);
                            }
                            *settings = Some(draft);
                        }
                        *tab = *wanted;
                    }
                    return Outcome::None;
                }
                // RealmsSettingsTab's two fields: the click moves the caret between them.
                // The two distance sliders: a click on the track sets the value, as the
                // spawn-protection slider does (no drag event on this screen yet).
                if let SubScreen::Configure {
                    tab: ConfigureTab::Settings,
                    settings: Some(draft),
                    tier,
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
                    if let Some(Ok(config)) = tier.as_ref() {
                        // `OptionInstance`'s slider: the value snaps into the range.
                        let bands = [
                            (
                                config.render_distance.as_ref(),
                                126.0,
                                &mut draft.render_distance,
                            ),
                            (
                                config.sim_distance.as_ref(),
                                166.0,
                                &mut draft.simulation_distance,
                            ),
                        ];
                        for (range, slider_y, picked) in bands {
                            let (Some(range), Some(picked)) = (range, picked) else {
                                continue;
                            };
                            if (slider_y..slider_y + 20.0).contains(&y)
                                && (left..left + 212.0).contains(&x)
                            {
                                *picked =
                                    range.from_slider(crate::ui::slider_value(left, 212.0, x));
                                return Outcome::None;
                            }
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
                    let row = ((y - top) / 16.0).floor() as usize;
                    if y >= top
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
                    self.subscreen = SubScreen::Popup(Popup::SwitchSlot { server, slot });
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
            // RealmsWorldsTab.switchToFullSlot, answered: `RealmsClient.switchSlot` on a
            // worker, then the detail reads again.
            Popup::SwitchSlot { server, slot } => {
                let Some(client) = client() else {
                    return Outcome::None;
                };
                let id = server.id;
                self.subscreen = SubScreen::Configure {
                    server,
                    tab: ConfigureTab::Worlds,
                    subscription: None,
                    subscription_job: None,
                    detail: None,
                    detail_job: None,
                    deleting: None,
                    member_job: Some(spawn(move || client.switch_slot(id, slot).map(drop))),
                    settings: None,
                    tier: None,
                    tier_job: None,
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
            // RealmsPlayersTab.PlayerEntry.uninvite, answered: the player goes and the
            // list reads again.
            Popup::Uninvite { server, player } => {
                let Some(client) = client() else {
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
                    member_job: Some(spawn(move || client.uninvite_player(id, &uuid))),
                    settings: None,
                    tier: None,
                    tier_job: None,
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
                    detail: None,
                    detail_job: None,
                    deleting: Some(spawn(move || client.delete_realm(id))),
                    member_job: None,
                    settings: None,
                    tier: None,
                    tier_job: None,
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

    /// `AbstractScrollArea.mouseScrolled` over the product page's list.
    pub fn wheel(&mut self, delta: f32, cursor: (f32, f32), (_, h): (f32, f32)) {
        let SubScreen::AddRealm { scroll, content } = &mut self.subscreen else {
            return;
        };
        let (top, bottom) = (44.0, h - 40.0);
        if cursor.1 < top || cursor.1 >= bottom {
            return;
        }
        scroll.wheel(delta, 10.0, top, bottom, content.get());
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
                detail,
                tier,
                deleting,
                member_job,
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
                    server.is_open(),
                    64,
                    None,
                ));
                buttons.push((cx + 2.0, h - 26.0, 100.0, t("gui.back"), true, 32, None));
                // RealmsSettingsTab: the region preference and the open/close toggle
                // (vanilla keeps the toggle on this tab). The two distance sliders push
                // them down once the tier answer arrived.
                if *tab == ConfigureTab::Settings {
                    let shift = if matches!(tier, Some(Ok(_))) {
                        80.0
                    } else {
                        0.0
                    };
                    buttons.push((
                        cx - 106.0,
                        146.0 + shift,
                        212.0,
                        t("mco.configure.world.buttons.region_preference"),
                        true,
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
                        174.0 + shift,
                        212.0,
                        toggle,
                        job.is_none(),
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
                        !server.expired,
                        94,
                        None,
                    ));
                }
                // RealmsPlayersTab: a row's Op/Deop/Remove, then Invite Player.
                if *tab == ConfigureTab::Players
                    && let Some(Ok(detail)) = detail
                {
                    let rx = ((w - ROW_WIDTH) / 2.0).floor();
                    for (i, player) in detail.players.iter().enumerate() {
                        let row_y = 36.0 + 22.0 + 2.0 + i as f32 * ROW_HEIGHT;
                        if row_y + ROW_HEIGHT > h - 48.0 {
                            break;
                        }
                        let id = 100 + i as u8 * 3;
                        let width = (ROW_WIDTH - 140.0).max(40.0) / 3.0;
                        // The vanilla buttons carry the sprite; these say what they do in
                        // the same three translations (`invites.*.tooltip`).
                        for (n, key) in [
                            (0u8, "mco.configure.world.invites.ops.tooltip"),
                            (1, "mco.configure.world.invites.normal.tooltip"),
                            (2, "mco.configure.world.invites.remove.tooltip"),
                        ] {
                            buttons.push((
                                rx + ROW_WIDTH - 132.0 + n as f32 * width,
                                row_y + 8.0,
                                width - 2.0,
                                t(key),
                                player.uuid.is_some() && member_job.is_none(),
                                id + n,
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
            SubScreen::AddRealm { .. } => {
                // `RealmsPdpScreen`'s footer: the buy button carries the PDP's own title.
                buttons.push((
                    cx - 102.0,
                    h - 26.0,
                    100.0,
                    t("mco.selectServer.purchase"),
                    true,
                    40,
                    None,
                ));
                buttons.push((cx + 2.0, h - 26.0, 100.0, t("gui.back"), true, 41, None));
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
                detail,
                settings,
                tier,
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
                            for (i, player) in detail.players.iter().enumerate() {
                                let row_y = 36.0 + 22.0 + 2.0 + i as f32 * ROW_HEIGHT;
                                if row_y + ROW_HEIGHT > h - 48.0 {
                                    break;
                                }
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
                            // `RealmsSettingsTab`: the name and description fields, the two
                            // distance sliders once the tier answer arrived, the region
                            // preference line and the open/close button.
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
                            let mut shift = 0.0;
                            if let Some(Ok(config)) = tier.as_ref() {
                                // `createDistanceOption`: a slider over the range, its value
                                // as `options.chunks`.
                                for (range, picked, tooltip, y) in [
                                    (
                                        config.render_distance.as_ref(),
                                        draft.render_distance,
                                        "mco.configure.world.renderDistance.tooltip",
                                        126.0,
                                    ),
                                    (
                                        config.sim_distance.as_ref(),
                                        draft.simulation_distance,
                                        "mco.configure.world.simulationDistance.tooltip",
                                        166.0,
                                    ),
                                ] {
                                    let Some((range, picked)) = range.zip(picked) else {
                                        continue;
                                    };
                                    let value = (picked - range.min) as f32
                                        / (range.max - range.min) as f32;
                                    // `CommonLayouts.labeledElement`: the caption above, the
                                    // button showing the value alone (`options.chunks`).
                                    let text = crate::lang::tr(
                                        "options.chunks",
                                        &[&picked as &dyn std::fmt::Display],
                                    );
                                    // `createDistanceOption`: the button's
                                    // `cachedConstantTooltip`.
                                    let hover = crate::ui::hovered(cursor, left, y, 212.0, 20.0);
                                    crate::ui::slider(
                                        batch,
                                        atlas,
                                        left,
                                        y,
                                        212.0,
                                        &Text::literal(&text),
                                        value.clamp(0.0, 1.0),
                                        hover,
                                        saving.is_none(),
                                        false,
                                    );
                                    if hover {
                                        crate::ui::tooltip(
                                            batch,
                                            atlas,
                                            &[Text::translate(tooltip, &[])],
                                            cursor,
                                        );
                                    }
                                }
                                shift = 80.0;
                                batch.text(
                                    atlas,
                                    &t("mco.configure.world.renderDistance"),
                                    left,
                                    114.0,
                                    GREY,
                                    true,
                                );
                                batch.text(
                                    atlas,
                                    &t("mco.configure.world.simulationDistance"),
                                    left,
                                    154.0,
                                    GREY,
                                    true,
                                );
                            }
                            batch.text(
                                atlas,
                                &t("mco.configure.world.region_preference"),
                                left,
                                114.0 + shift,
                                GREY,
                                true,
                            );
                            let current =
                                preference_label(draft.preference, draft.region.as_deref());
                            batch.text(atlas, &current, left, 126.0 + shift, GREY, true);
                            if saving.is_some() {
                                centered(
                                    batch,
                                    atlas,
                                    &t("mco.configure.world.loading"),
                                    cx,
                                    200.0 + shift,
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
            SubScreen::RegionPreference {
                regions, selected, ..
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
                for (i, (preference, region)) in rows.iter().enumerate() {
                    let y = top + i as f32 * 16.0;
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
            SubScreen::AddRealm { scroll, content } => {
                // `RealmsPdpScreen`: the Realms header's logo, then the trial header and
                // the three feature rows (an 80x45 picture and 182-wide text), scrolling
                // between the header and the footer.
                if atlas.sprites.contains_key("gui/title/realms") {
                    // The header's logo, as `draw_main` draws it.
                    batch.sprite_region(
                        atlas,
                        "gui/title/realms",
                        [cx - 64.0, 5.0, 128.0, 34.0],
                        128.0,
                        64.0,
                    );
                } else {
                    centered(
                        batch,
                        atlas,
                        &t("mco.selectServer.purchase"),
                        cx,
                        14.0,
                        u32::MAX,
                    );
                }
                let (top, bottom) = (44.0, h - 40.0);
                let rx = ((w - PDP_ROW_WIDTH) / 2.0).floor();
                // `PdpFeatureEntry.entryHeight`: the picture's 45 or the text, plus 16.
                let mut rows: Vec<(Option<&'static str>, Vec<String>, bool, f32)> = Vec::new();
                for (image, title, descriptions) in pdp_entries() {
                    let mut lines = wrap_lines(&t(title), atlas, PDP_TEXT_WIDTH);
                    let mut height = lines.len() as f32 * 10.0 + 4.0;
                    for description in descriptions {
                        lines.push(String::new());
                        height += 2.0;
                        let wrapped = wrap_lines(&t(description), atlas, PDP_TEXT_WIDTH);
                        height += wrapped.len() as f32 * 10.0;
                        lines.extend(wrapped);
                    }
                    let header = image.is_empty();
                    let row = if header {
                        height + 16.0
                    } else {
                        height.max(45.0) + 16.0
                    };
                    let image = (!image.is_empty()).then_some(image);
                    rows.push((image, lines, header, row));
                }
                let total: f32 = rows.iter().map(|row| row.3).sum();
                content.set(total);
                crate::ui::scrollbar(
                    batch,
                    atlas,
                    rx + PDP_ROW_WIDTH + 2.0,
                    top,
                    bottom,
                    total,
                    scroll.amount,
                );
                let mut y = top - scroll.amount;
                for (image, lines, header, height) in &rows {
                    if y + height >= top && y <= bottom {
                        if let Some(image) = image {
                            crate::ui::blit_sprite(
                                batch,
                                atlas,
                                image,
                                rx + 4.0,
                                y + 4.0,
                                80.0,
                                45.0,
                                u32::MAX,
                            );
                        }
                        let mut text_y = y + 4.0;
                        for line in lines {
                            if line.is_empty() {
                                text_y += 2.0;
                                continue;
                            }
                            let x = if *header {
                                cx - (HudBatch::text_width(atlas, line) / 2.0).floor()
                            } else {
                                rx + 100.0
                            };
                            batch.text(atlas, line, x, text_y, u32::MAX, true);
                            text_y += 10.0;
                        }
                    }
                    y += height;
                }
            }
            SubScreen::Popup(popup) => {
                let message = match popup {
                    Popup::Leave(_) => t("mco.configure.world.leave.question.line1"),
                    Popup::ResourcePack { .. } => t("mco.configure.world.resourcepack.question"),
                    Popup::Delete(_) => t("mco.configure.world.delete.question.line1"),
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

    /// `MenuTabBar.builder(this, width)`: at most 400 wide, centred, four even tabs.
    #[test]
    fn the_tab_bar_is_centred_and_four_tabs_wide() {
        // 854x480 at GUI scale 2 is 427x240: 372 inner width, 28 from the left, 94 a tab.
        assert_eq!(tab_geometry(427.0), (28.0, 94.0));
        // A window narrower than 400 keeps its own width less the bar's margins:
        // 272 inner width over four tabs, 14 from the left.
        assert_eq!(tab_geometry(300.0), (14.0, 68.0));
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
    /// `RealmsSettingsTab.save`: only a distance the player changed, and only when the tier
    /// answer names the range, is sent.
    #[test]
    fn a_distance_is_saved_only_when_it_differs_from_the_service() {
        let range = RealmTierRange {
            min: 2,
            max: 16,
            default_value: 8,
            current: Some(10),
        };
        assert_eq!(changed_distance(Some(10), Some(&range)), None);
        assert_eq!(changed_distance(Some(12), Some(&range)), Some(12));
        // No tier answer: the slider was never usable, so nothing goes out.
        assert_eq!(changed_distance(Some(12), None), None);
        assert_eq!(changed_distance(None, Some(&range)), None);
        // A range without `current` compares against its default (`currentOrDefault`).
        let default = RealmTierRange {
            current: None,
            ..range
        };
        assert_eq!(changed_distance(Some(8), Some(&default)), None);
        assert_eq!(changed_distance(Some(9), Some(&default)), Some(9));
    }
}
