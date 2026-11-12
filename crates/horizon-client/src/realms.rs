//! Minecraft Realms screens: `RealmsMainScreen` with its `RealmsHeader`, the join-code and
//! pending-invite screens, the owner's open/close, `RealmsPopups.infoPopupScreen` for the
//! questions, `GetServerDetailsTask` under `RealmsLongRunningMcoTaskScreen` for Play, and
//! `RealmsResetWorldScreen` with its template, local-world and upload screens.
//! Layout from the vanilla screens' behaviour and the game's own sprites.
//!
//! Realms is used only with the signed-in player's own Microsoft account: the session the
//! launcher hands over after its owner check (`auth::current`). Without one (offline play, an
//! expired token) the screen says so and makes no request. Every request runs on a worker
//! thread; the title polls the results once a frame (`poll`).
pub mod availability;
pub mod client;
pub mod download;
pub mod upload;

use crate::hud::{HudAtlas, HudBatch, rgba};
use crate::text::Text;
use client::{
    Backup, InviteCode, PendingInviteDto, PlayerInfo, RealmAddress, RealmServer, RealmSetting,
    RealmSlot, RealmTierConfiguration, RealmTierRange, RealmWorldOptions, RealmsClient,
    RealmsError, RealmsNotification, RegionDataDto, RegionSelectionPreferenceDto, Subscription,
    WorldDownload, WorldTemplate, WorldTemplateList,
};
use horizon_client_render::auth::{self, AuthSession};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER_HEIGHT: f32 = 44.0;
const CONTENT_WIDTH: f32 = 308.0;
/// `CommonLinks.REALMS_TERMS`: the Realms terms page the terms screen's link opens.
const REALMS_TERMS_URL: &str = "https://aka.ms/MinecraftRealmsTerms";
/// `RealmsPdpScreen`'s list: the row width and the text's width beside the picture.
const PDP_ROW_WIDTH: f32 = 290.0;
const PDP_TEXT_WIDTH: f32 = 182.0;

const ROW_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 36.0;
/// `RealmsSelectWorldTemplateScreen`'s rows (`ObjectSelectionList`'s 46).
const TEMPLATE_ROW: f32 = 46.0;
/// `RealmsConfigureWorldScreen`'s `MenuTabBar` height (`gui/tab_header_background`).
const TAB_HEIGHT: f32 = 24.0;
/// `hud::rgba` packing (ABGR): 160 grey, vanilla's red and `RealmsPopups.COLOR_INFO`.
const GREY: u32 = 0xFFA0_A0A0;
/// `-65536` — vanilla's invalid-input and warning red.
const RED: u32 = 0xFF00_00FF;
/// `RealmsPopups.COLOR_INFO` (8226750).
const INFO: u32 = 0xFFBE_877D;

const REALMS_INVITE_CODE: crate::clipboard::FieldId = ("realms.invite_code", 0);
const REALMS_PLAYER_NAME: crate::clipboard::FieldId = ("realms.player_name", 0);
const REALMS_UPLOAD_SEARCH: crate::clipboard::FieldId = ("realms.upload_search", 0);
const REALMS_SLOT_NAME: crate::clipboard::FieldId = ("realms.slot_name", 0);
const REALMS_CREATE_NAME: crate::clipboard::FieldId = ("realms.create_name", 0);
const REALMS_CREATE_DESC: crate::clipboard::FieldId = ("realms.create_desc", 0);
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
/// `InviteCodeExpiration`: the seven choices, the last one never expiring.
const EXPIRATIONS: [(&str, Option<i64>); 7] = [
    (
        "mco.configure.world.invite_codes.expiration.30_minutes",
        Some(30 * 60),
    ),
    (
        "mco.configure.world.invite_codes.expiration.1_hour",
        Some(60 * 60),
    ),
    (
        "mco.configure.world.invite_codes.expiration.6_hours",
        Some(6 * 60 * 60),
    ),
    (
        "mco.configure.world.invite_codes.expiration.12_hours",
        Some(12 * 60 * 60),
    ),
    (
        "mco.configure.world.invite_codes.expiration.1_day",
        Some(24 * 60 * 60),
    ),
    (
        "mco.configure.world.invite_codes.expiration.7_days",
        Some(7 * 24 * 60 * 60),
    ),
    ("mco.configure.world.invite_codes.expiration.never", None),
];

/// `CommonLinks.BUY_REALMS`.
const BUY_REALMS: &str = "https://aka.ms/BuyJavaRealms";
/// `CommonLinks.EXTEND_REALMS_LINK`.
const EXTEND_REALMS: &str = "https://aka.ms/ExtendJavaRealms";
/// `CommonLinks.REALMS_CONTENT_CREATION`.
const REALMS_CONTENT_CREATION: &str = "https://aka.ms/MinecraftRealmsContentCreator";

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

/// `RealmsLongRunningMcoTaskScreen`'s running task: the worker's job, the title the
/// screen shows while it runs (`LongRunningTask.getTitle`, updated as the next task
/// starts), and the flag `cancel()` sets (`LongRunningTask.abortTask`, checked
/// between the steps).
#[derive(Clone, Debug)]
pub struct RunningTask {
    job: Job<Result<(), RealmsError>>,
    title: Arc<Mutex<String>>,
    abort: Arc<AtomicBool>,
}

/// `DownloadTask`: the download-info request on a worker (`DownloadTask.run`'s retry loop)
/// and the flag its cancel sets (`LongRunningTask.aborted`).
#[derive(Clone, Debug)]
pub struct DownloadTask {
    job: Job<Result<WorldDownload, RealmsError>>,
    abort: Arc<AtomicBool>,
}

/// `RestoreTask`: `RealmsClient.restoreWorld` on a worker, retried while the service says
/// to wait, and the flag its cancel sets (`LongRunningTask.aborted`).
#[derive(Clone, Debug)]
pub struct RestoreTask {
    job: Job<Result<(), RealmsError>>,
    abort: Arc<AtomicBool>,
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
/// `CommonLinks.extendRealms`: the extend page with the subscription and profile, and the
/// `ref` only for the expired-Realm paths (the subscription tab passes NONE).
fn extend_link(server: &RealmServer, profile: u128, expired: bool) -> String {
    match &server.remote_subscription_id {
        None => EXTEND_REALMS.to_owned(),
        Some(sub) => {
            let base = format!("{EXTEND_REALMS}?subscriptionId={sub}&profileId={profile:032x}");
            if expired {
                let reference = if server.expired_trial {
                    "expiredTrial"
                } else {
                    "expiredRealm"
                };
                format!("{base}&ref={reference}")
            } else {
                base
            }
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

/// `RealmsServer.shouldPlayButtonBeActive`: open and unexpired, and this client can play
/// it — the compatibility says compatible or an upgrade, or the player owns the Realm.
fn can_play(server: &RealmServer, me: Option<u128>) -> bool {
    server.is_open()
        && (server.compatibility == "COMPATIBLE"
            || server.compatibility == "NEEDS_UPGRADE"
            || me.is_some_and(|me| server.owned_by(me)))
}

/// `RealmsServer.McoServerComparator` through `RealmsServerList.updateServersList`:
/// uninitialized Realms first, then expired trials, then the player's own Realm, then
/// unexpired before expired, open before closed, and finally the id. (Snapshot realms are
/// not modelled: 26.3 release never lists them.)
fn sort_servers(servers: &mut [RealmServer], owner: Option<&str>) {
    servers.sort_by(|a, b| {
        (b.state == "UNINITIALIZED")
            .cmp(&(a.state == "UNINITIALIZED"))
            .then(b.expired_trial.cmp(&a.expired_trial))
            .then((b.owner.as_deref() == owner).cmp(&(a.owner.as_deref() == owner)))
            .then(a.expired.cmp(&b.expired))
            .then((b.state == "OPEN").cmp(&(a.state == "OPEN")))
            .then(a.id.cmp(&b.id))
    });
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

/// `RealmsSettingsTab`'s column: its `GridLayout` gives every cell `padding(8)`
/// (`defaultCellSetting().padding(8)`), and each `CommonLayouts.labeledElement` is a
/// 9-tall label, 4 of spacing, then its 20-tall widget. The region cell's widget is the
/// 9-tall preference line over the 20-tall button; the toggle cell is the button alone.
const SETTINGS_CELL_PADDING: f32 = 8.0;
const SETTINGS_LABEL_HEIGHT: f32 = 9.0;
const SETTINGS_LABEL_SPACING: f32 = 4.0;
const SETTINGS_WIDGET_HEIGHT: f32 = 20.0;
/// A plain `labeledElement` cell: (9 + 4 + 20) + padding top and bottom.
const SETTINGS_ROW_HEIGHT: f32 = SETTINGS_LABEL_HEIGHT
    + SETTINGS_LABEL_SPACING
    + SETTINGS_WIDGET_HEIGHT
    + 2.0 * SETTINGS_CELL_PADDING;
/// The region cell: (label 9 + spacing 4 + line 9 + button 20) + padding.
const SETTINGS_REGION_ROW_HEIGHT: f32 = SETTINGS_LABEL_HEIGHT
    + SETTINGS_LABEL_SPACING
    + SETTINGS_LABEL_HEIGHT
    + SETTINGS_WIDGET_HEIGHT
    + 2.0 * SETTINGS_CELL_PADDING;

/// `RealmsSettingsTab`'s row tops relative to the column's own top — name, description,
/// render distance, simulation distance, region, toggle — and the column's height. The
/// two distance rows exist only once the tier answer is there (`createDistanceOption`).
fn settings_rows(with_tier: bool) -> ([f32; 6], f32) {
    let mut rows = [0.0; 6];
    rows[1] = SETTINGS_ROW_HEIGHT;
    if with_tier {
        rows[2] = 2.0 * SETTINGS_ROW_HEIGHT;
        rows[3] = 3.0 * SETTINGS_ROW_HEIGHT;
        rows[4] = 4.0 * SETTINGS_ROW_HEIGHT;
        rows[5] = rows[4] + SETTINGS_REGION_ROW_HEIGHT;
    } else {
        rows[4] = 2.0 * SETTINGS_ROW_HEIGHT;
        rows[5] = rows[4] + SETTINGS_REGION_ROW_HEIGHT;
    }
    (
        rows,
        rows[5] + SETTINGS_WIDGET_HEIGHT + 2.0 * SETTINGS_CELL_PADDING,
    )
}

/// `GridLayoutTab.doLayout`'s `FrameLayout.alignInRectangle(..., 0.5F, 1/6F)` inside the
/// tab area below the 24-tall `MenuTabBar` and above the 33-tall footer. Java's `int`
/// cast truncates toward zero, which matters on short windows.
fn settings_column_top(h: f32, content_height: f32) -> f32 {
    24.0 + ((h - 24.0 - 33.0 - content_height) / 6.0).trunc()
}

/// A configuration screen with no data yet, for an error screen's `nextScreen`
/// (`RealmsConfigureWorldScreen.createErrorScreen` returns the list or the tab it came
/// from); the fetch starters run when it is restored.
fn configure_screen(server: RealmServer, tab: ConfigureTab) -> SubScreen {
    SubScreen::Configure {
        server,
        tab,
        subscription: None,
        subscription_job: None,
        detail: None,
        detail_job: None,
        deleting: None,
        member_job: None,
        settings: None,
        players_scroll: crate::ui::Scroll::default(),
        tier: None,
        tier_job: None,
        regions: None,
        regions_job: None,
        saving: None,
        error: None,
        job: None,
    }
}

/// `RealmsSettingsTab.isRealmNameValid`: a name that trims to something and has no
/// leading or trailing whitespace.
fn realm_name_valid(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && name.len() == trimmed.len()
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

/// `WorldSelectionList.filterAccepts`: the level name or its folder contains the filter,
/// case-insensitively (`String.toLowerCase(Locale.ROOT)`).
fn upload_matches(world: &crate::title::WorldEntry, query: &str) -> bool {
    let query = query.to_lowercase();
    query.is_empty()
        || world.name.to_lowercase().contains(&query)
        || world.folder.to_lowercase().contains(&query)
}

/// `RealmsTextureManager.getTexture`: the base64 PNG decoded and resampled to the 64x64
/// `horizon/world-iconN` slot (nearest texel, like `skins::resample_64`).
fn template_image(encoded: &str) -> Option<Vec<u8>> {
    let bytes = crate::skins::base64_decode(encoded)?;
    let (width, height, rgba) = crate::hud::decode_image_bytes(&bytes).ok()?;
    resample_icon(width, height, &rgba)
}

/// A cheap identity for a base64 picture, so two images of the same length never share
/// a cache entry.
fn encoded_key(encoded: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    encoded.hash(&mut hasher);
    hasher.finish()
}

/// `RealmsWorldSlotButton.State`'s picture, in `extractContents`' order: a minigame's
/// image (or `presets/isles.png` when it has none), the empty frame, a template's own
/// image, or the slot's title panorama.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotPicture<'a> {
    Image(&'a str),
    Preset,
    Empty,
    Panorama(&'static str),
}

/// `RealmsWorldSlotButton.State`: `image` is the minigame's `minigameImage` for slot 4
/// and the slot's `templateImage` otherwise (only when its `templateId` is not -1).
fn slot_picture(slot: i32, empty: bool, image: Option<&str>) -> SlotPicture<'_> {
    if slot == 4 {
        return image.map_or(SlotPicture::Preset, SlotPicture::Image);
    }
    if empty {
        return SlotPicture::Empty;
    }
    if let Some(image) = image {
        return SlotPicture::Image(image);
    }
    match slot {
        1 => SlotPicture::Panorama("panorama_0"),
        2 => SlotPicture::Panorama("panorama_2"),
        3 => SlotPicture::Panorama("panorama_3"),
        _ => SlotPicture::Empty,
    }
}

/// `TextRenderingUtils.decompose(I18n.get("mco.template.select.none", "%link"), link)`:
/// the translated message's lines, the one holding the `%s` argument split into
/// `before`/`link`/`after`; every other line is whole (`link` is None).
fn template_none_lines(text: &str, link: &str) -> Vec<(String, Option<String>, String)> {
    text.replacen("%s", "%link", 1)
        .split('\n')
        .map(|line| match line.split_once("%link") {
            Some((before, after)) => (before.to_owned(), Some(link.to_owned()), after.to_owned()),
            None => (line.to_owned(), None, String::new()),
        })
        .collect()
}

fn resample_icon(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let mut out = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let (sx, sy) = (x * w / 64, y * h / 64);
            let source = (sy * w + sx) * 4;
            let target = (y * 64 + x) * 4;
            out[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }
    Some(out)
}

/// `PlayerFaceExtractor`/`RealmsUtil.extractPlayerFace`: the head's front 8x8 texels at
/// (8, 8) with the hat layer (40, 8) over them, scaled to the 64x64 slot.
fn face_from_skin(skin: &[u8]) -> Option<Vec<u8>> {
    if skin.len() < 64 * 64 * 4 {
        return None;
    }
    let mut out = vec![0u8; 64 * 64 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let base = ((8 + y) * 64 + 8 + x) * 4;
            let hat = ((8 + y) * 64 + 40 + x) * 4;
            let mut texel = [skin[base], skin[base + 1], skin[base + 2], skin[base + 3]];
            let hat_alpha = skin[hat + 3] as u16;
            if hat_alpha > 0 {
                for channel in 0..3 {
                    texel[channel] = ((skin[hat + channel] as u16 * hat_alpha
                        + texel[channel] as u16 * (255 - hat_alpha))
                        / 255) as u8;
                }
                texel[3] = 255;
            }
            for dy in 0..8 {
                for dx in 0..8 {
                    let target = ((y * 8 + dy) * 64 + x * 8 + dx) * 4;
                    out[target..target + 4].copy_from_slice(&texel);
                }
            }
        }
    }
    Some(out)
}

/// Upload a decoded 64x64 picture into a `horizon/world-iconN` atlas slot (the slots the
/// title world list uses) and name its sprite. `None` when the atlas has no such slot.
fn icon_sprite(batch: &mut HudBatch, atlas: &HudAtlas, slot: usize, rgba: &[u8]) -> Option<String> {
    let sprite = format!("horizon/world-icon{}", slot % crate::hud::WORLD_ICON_SLOTS);
    let rect = atlas.sprites.get(&sprite)?;
    batch.patches.push(crate::font::Patch {
        x: (rect.u0 * atlas.width as f32).round() as u32,
        y: (rect.v0 * atlas.height as f32).round() as u32,
        width: 64,
        height: 64,
        rgba: rgba.to_vec(),
    });
    Some(sprite)
}

/// `InviteCodeExpiration.closestTo`: the choice nearest the time left on the code.
fn closest_expiration(expiration_millis: i64) -> usize {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let left = (expiration_millis - now).max(0) / 1000;
    let mut best = EXPIRATIONS.len() - 1;
    let mut best_delta = i64::MAX;
    for (i, (_, seconds)) in EXPIRATIONS.iter().enumerate() {
        let Some(seconds) = seconds else { continue };
        let delta = (left - seconds).abs();
        if delta < best_delta {
            best_delta = delta;
            best = i;
        }
    }
    best
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

/// `RealmsUtil.convertToAgePresentationFromInstant`: "now", seconds, minutes, hours or days
/// ago, from an epoch-millisecond stamp. The clock is the wall clock, not the animation
/// clock `hud_overlays::now_ms` (that one counts from the process start).
fn age_presentation(epoch_millis: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff = now - epoch_millis;
    if diff < 0 {
        return crate::lang::get("mco.time.now");
    }
    let seconds = diff / 1000;
    let key = if seconds < 60 {
        ("mco.time.secondsAgo", seconds)
    } else if seconds < 3600 {
        ("mco.time.minutesAgo", seconds / 60)
    } else if seconds < 86400 {
        ("mco.time.hoursAgo", seconds / 3600)
    } else {
        ("mco.time.daysAgo", seconds / 86400)
    };
    crate::lang::tr(key.0, &[&key.1 as &dyn std::fmt::Display])
}

/// `RealmsBackupScreen.Entry.populateChangeList`: each backup's metadata keys that differ
/// from the older backup (or are missing there); the last backup has none. An `uploaded`
/// key's value is the backup's short date.
fn backup_changes(backups: &[Backup]) -> Vec<std::collections::HashMap<String, String>> {
    let mut out = Vec::with_capacity(backups.len());
    for (index, backup) in backups.iter().enumerate() {
        let mut changes = std::collections::HashMap::new();
        if index + 1 < backups.len() {
            let older = &backups[index + 1];
            for (key, value) in &backup.metadata {
                if let Some(old) = older.metadata.get(key)
                    && !key.contains("uploaded")
                    && old == value
                {
                    continue;
                }
                if key.contains("uploaded") {
                    changes.insert(key.clone(), short_date_text(backup.last_modified));
                } else {
                    changes.insert(key.clone(), value.clone());
                }
            }
        }
        out.push(changes);
    }
    out
}

/// `RealmsBackupInfoScreen.BackupInfoListEntry.translateKey`.
fn backup_info_key(key: &str) -> String {
    let t = crate::lang::get;
    let candidate = match key {
        "template_name" => "mco.backup.entry.templateName",
        "game_difficulty" => "mco.backup.entry.gameDifficulty",
        "name" => "mco.backup.entry.name",
        "game_server_version" => "mco.backup.entry.gameServerVersion",
        "uploaded" => "mco.backup.entry.uploaded",
        "enabled_packs" => "mco.backup.entry.enabledPack",
        "description" => "mco.backup.entry.description",
        "game_mode" => "mco.backup.entry.gameMode",
        "seed" => "mco.backup.entry.seed",
        "world_type" => "mco.backup.entry.worldType",
        _ => "mco.backup.entry.undefined",
    };
    t(candidate)
}

/// `RealmsBackupInfoScreen.checkForSpecificMetadata`: game modes and difficulties by the
/// index the service stored, world types by their name; everything else literal.
fn backup_info_value(key: &str, value: &str) -> String {
    let t = crate::lang::get;
    let lower = key.to_lowercase();
    if lower.contains("game") && lower.contains("mode") {
        return value
            .parse::<usize>()
            .ok()
            .and_then(|index| GAME_MODES.get(index))
            .map(|key| t(key))
            .unwrap_or_else(|| t("mco.backup.unknown"));
    }
    if lower.contains("game") && lower.contains("difficulty") {
        return value
            .parse::<usize>()
            .ok()
            .and_then(|index| DIFFICULTIES.get(index))
            .map(|key| t(key))
            .unwrap_or_else(|| t("mco.backup.unknown"));
    }
    if key == "world_type" {
        let name = match value.to_uppercase().as_str() {
            "NORMAL" => "normal",
            "MINIGAME" => "minigame",
            "ADVENTUREMAP" => "adventureMap",
            "EXPERIENCE" => "experience",
            "INSPIRATION" => "inspiration",
            _ => return t("mco.backup.entry.worldType.unknown"),
        };
        return t(&format!("mco.backup.entry.worldType.{name}"));
    }
    value.to_owned()
}

/// `RealmsSubscriptionTab.localPresentation`: the start date in the player's own local
/// time. The process has no locale to format with, so the shape is fixed (`%Y-%m-%d`)
/// where vanilla asks for the locale's medium date.
/// `Util.localizedDateFormatter(FormatStyle.MEDIUM)` — `DateTimeFormatter
/// .ofLocalizedDateTime`: each locale's medium date-time shape. The port has no
/// locale database, so the language's shape comes from this table (zh's medium
/// date-time is `2026年11月6日 12:16:04`), with an ISO fallback.
fn start_date_text(epoch_millis: i64) -> String {
    let Some(time) = chrono::DateTime::from_timestamp_millis(epoch_millis) else {
        return String::new();
    };
    let time = time.with_timezone(&chrono::Local);
    let format = match crate::lang::code().as_str() {
        "zh_cn" | "zh_tw" | "lzh" => "%Y年%-m月%-d日 %H:%M:%S",
        "ja_jp" => "%Y/%-m/%-d %H:%M:%S",
        _ => "%Y-%m-%d %H:%M:%S",
    };
    time.format(format).to_string()
}

/// `RealmsEditInviteCodeScreen.DATE_FORMAT`: `Util.localizedDateFormatter(SHORT)`. The
/// port has no locale database, so each language's short shape is spelled out with an
/// ISO fallback (vanilla itself formats per locale).
fn short_date_text(epoch_millis: i64) -> String {
    let Some(time) = chrono::DateTime::from_timestamp_millis(epoch_millis) else {
        return String::new();
    };
    let time = time.with_timezone(&chrono::Local);
    let format = match crate::lang::code().as_str() {
        "zh_cn" | "zh_tw" | "lzh" | "ja_jp" => "%Y/%-m/%-d %H:%M",
        "en_us" => "%-m/%-d/%y %-I:%M %p",
        _ => "%Y-%m-%d %H:%M",
    };
    time.format(format).to_string()
}

/// One 200-wide column line of the subscription tab.
fn subscription_label(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    x: f32,
    y: f32,
    text: &str,
    color: u32,
) {
    batch.text(atlas, text, x, y, color, true);
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
#[derive(Clone, Debug)]
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
    /// `RealmsWorldsTab.switchToEmptySlot`'s question — the same text as the full slot's —
    /// then `RealmsResetWorldScreen.forEmptySlot`.
    SwitchEmpty { server: RealmServer, slot: i32 },
    /// `RealmsBackupScreen.Entry.restoreClicked`: the warning popup over the backup list,
    /// then `RealmsClient.restoreWorld`.
    RestoreBackup {
        server: RealmServer,
        slot_id: i32,
        backup: Backup,
        /// The list as it was, so the popup can draw it behind (the background screen).
        list: Vec<Backup>,
        selected: Option<usize>,
    },
    /// `RealmsBackupScreen.downloadClicked`: the info popup over the backup list, then
    /// `DownloadTask`.
    DownloadLatest {
        server: RealmServer,
        slot_id: i32,
        /// The list as it was, so the popup can draw it behind (the background screen).
        list: Vec<Backup>,
        selected: Option<usize>,
    },
    /// `checkDownloadSize`'s `warningAcknowledgePopupScreen`: the world is over 5 GiB, OK
    /// starts the download anyway (Escape leaves the download screen idle, as vanilla's
    /// `checked` flag does).
    DownloadOversized {
        server: RealmServer,
        slot_id: i32,
        download: WorldDownload,
        world_name: String,
        /// The download screen under the popup, so its dots keep blinking.
        started: Instant,
    },
    /// `RealmsSettingsTab`'s close question (`RealmsPopups.customPopupScreen` over the
    /// configuration screen): Continue saves and closes, Cancel returns to the tab.
    CloseWorld {
        server: RealmServer,
        settings: SettingsDraft,
        detail: Option<Result<RealmServer, RealmsError>>,
        tier: Option<Result<RealmTierConfiguration, RealmsError>>,
    },
    /// `ConfirmLinkScreen.confirmLink`: a website the screen would open, asked first. The
    /// parent is the screen the answer returns to (`ConfirmLinkScreen`'s `parentScreen`).
    OpenLink {
        url: String,
        parent: Box<SubScreen>,
        /// Where a yes goes; the popup itself when its URL button asked.
        open_parent: Option<Box<SubScreen>>,
    },
    /// `RealmsNotification.InfoPopup.buildScreen`: a `PopupScreen` over the list.
    Notice {
        uuid: String,
        title: String,
        message: String,
        image: Option<String>,
        url: Option<(String, String)>,
    },
}

/// `RealmsResetWorldScreen`'s entry points (`forResetSlot`, `forEmptySlot`, `forNewRealm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetKind {
    /// `forResetSlot`: the world is replaced after a warning; the active slot's.
    Reset,
    /// `forEmptySlot`: the slot has no world yet.
    Empty,
    /// `forNewRealm`: the Realm itself does not exist yet; the reset creates it first
    /// (`RealmCreationTask`).
    NewRealm,
}

impl ResetKind {
    /// The screen's title (`RealmsResetWorldScreen`'s constants).
    fn title(self) -> &'static str {
        match self {
            Self::Reset => "mco.reset.world.title",
            Self::Empty => "mco.configure.world.switch.slot",
            Self::NewRealm => "mco.selectServer.create",
        }
    }
    /// The subtitle, and whether it is the red warning.
    fn subtitle(self) -> (&'static str, bool) {
        match self {
            Self::Reset => ("mco.reset.world.warning", true),
            Self::Empty => ("mco.configure.world.switch.slot.subtitle", false),
            Self::NewRealm => ("mco.selectServer.create.subtitle", false),
        }
    }
    /// `RESET_WORLD_RESET_TASK_TITLE` and `CREATE_WORLD_RESET_TASK_TITLE`.
    fn task_title(self) -> &'static str {
        match self {
            Self::Reset => "mco.reset.world.resetting.screen.title",
            Self::Empty | Self::NewRealm => "mco.create.world.reset.title",
        }
    }
}

/// The four template groups of `RealmsResetWorldScreen`'s grid, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateGroup {
    Normal,
    Adventure,
    Experience,
    Inspiration,
}

/// `RealmsCreateWorldFlow.createWorld`'s target: the reset screen's Generate New World
/// tile asks the title for the Create New World screen, whose settings upload into
/// `slot`.
#[derive(Clone, Debug)]
pub struct CreateWorldRequest {
    pub server: RealmServer,
    pub slot: i32,
    pub kind: ResetKind,
    /// `RealmCreationTask`'s name and description, for `ResetKind::NewRealm`.
    pub creation: Option<(String, String)>,
}

/// What the Create New World screen decided, for the temporary world
/// `RealmsCreateWorldFlow.createTemporaryWorldFolder` writes before the upload.
#[derive(Clone, Debug)]
pub struct CreatedWorld {
    /// The world's level data, its name, game type, difficulty, game rules and features.
    pub level: crate::save::Level,
    pub world_gen: crate::save::WorldGen,
    pub flat_settings: Option<horizon_world::nbt::Tag>,
    pub single_biome: Option<String>,
    /// level.dat's `DataPacks`: enabled and disabled ids.
    pub data_packs: (Vec<String>, Vec<String>),
    /// The Create World screen's temporary pack folder, moved into the world
    /// (`Files.move(tempDataPackDir, worldFolder.resolve("datapacks"))`).
    pub pack_dir: Option<PathBuf>,
}

impl TemplateGroup {
    const ALL: [Self; 4] = [
        Self::Normal,
        Self::Adventure,
        Self::Experience,
        Self::Inspiration,
    ];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|g| *g == self)
            .expect("a group is in ALL")
    }
    /// `RealmsServer.WorldType`'s name: what `fetchWorldTemplates` puts in the path.
    fn world_type(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Adventure => "ADVENTUREMAP",
            Self::Experience => "EXPERIENCE",
            Self::Inspiration => "INSPIRATION",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Normal => "mco.reset.world.template",
            Self::Adventure => "mco.reset.world.adventure",
            Self::Experience => "mco.reset.world.experience",
            Self::Inspiration => "mco.reset.world.inspiration",
        }
    }
}

/// `RealmsSelectWorldTemplateScreen`'s list: one page fetched on a worker, the number of
/// the page after it, and whether it was the last (the list shows each page as it lands).
type TemplatePages = Job<Result<(Vec<WorldTemplate>, i32, bool), RealmsError>>;

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
        /// The list's scroll (`ObjectSelectionList`): the port drew what fit before.
        scroll: crate::ui::Scroll,
        /// The clicked row (`ContainerObjectSelectionList`'s selection).
        selected: Option<usize>,
    },
    /// `RealmsBackupScreen`: the Realm's automatic backups.
    Backups {
        server: RealmServer,
        slot_id: i32,
        backups: Option<Result<Vec<Backup>, RealmsError>>,
        list_job: Option<Job<Result<Vec<Backup>, RealmsError>>>,
        /// `RestoreTask` while the restore runs.
        restore_job: Option<RestoreTask>,
        /// `DownloadTask` while it asks the service for the latest world's links.
        download_task: Option<DownloadTask>,
        /// The clicked row (`ContainerObjectSelectionList`'s selection).
        selected: Option<usize>,
        /// The list's scroll (`ContainerObjectSelectionList`).
        scroll: crate::ui::Scroll,
    },
    /// `RealmsDownloadLatestWorldScreen`: the latest world's archive coming down into the
    /// saves folder, with vanilla's bar, percentage, speed and Cancel.
    DownloadLatest {
        server: RealmServer,
        slot_id: i32,
        /// `WorldDownload`: where the world and its resource pack come from.
        download: WorldDownload,
        /// `downloadName`: the name the unpacked world takes (before `FileDownload`'s
        /// clean-up).
        world_name: String,
        /// `checkDownloadSize`'s HEAD: the link's `Content-Length`, run once.
        size_job: Option<Job<u64>>,
        progress: Arc<download::DownloadProgress>,
        job: Option<Job<Result<(), download::DownloadError>>>,
        error: Option<download::DownloadError>,
        /// The worker answered (done or failed): Cancel gives way to Done.
        done: bool,
        /// When the screen opened: the dots' blink (`tickCount / 10` at 20 ticks a second).
        started: Instant,
        /// `previousWrittenBytes`/`previousTimeSnapshot`/`bytesPersSecond`: the written
        /// bytes, when they were sampled, and the last rate, sampled every 20 ticks.
        speed: std::cell::Cell<(u64, Instant, u64)>,
    },
    /// `RealmsBackupInfoScreen`: one backup's changed metadata.
    BackupInfo {
        server: RealmServer,
        slot_id: i32,
        backup: Backup,
        /// The list it came from, so Back returns to it as it was (`lastScreen`).
        list: Vec<Backup>,
        selected: Option<usize>,
        list_scroll: crate::ui::Scroll,
        scroll: crate::ui::Scroll,
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
        /// The players tab's list scroll (`InvitedObjectSelectionList`).
        players_scroll: crate::ui::Scroll,
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
        /// The list's scroll (`ObjectSelectionList`): the rows can outgrow the screen.
        scroll: crate::ui::Scroll,
        /// The settings tab's data, carried back so its save can run at once.
        detail: Option<Result<RealmServer, RealmsError>>,
        tier: Option<Result<RealmTierConfiguration, RealmsError>>,
    },
    /// `RealmsInviteCodesScreen`: the Realm's shareable codes.
    InviteCodes {
        server: RealmServer,
        codes: Option<Result<Vec<InviteCode>, RealmsError>>,
        list_job: Option<Job<Result<Vec<InviteCode>, RealmsError>>>,
        /// `createInviteCode` on a worker.
        job: Option<Job<Result<(), RealmsError>>>,
        error: Option<String>,
        /// The highlighted row (`InviteCodeSelectionList`).
        selected: Option<usize>,
    },
    /// `RealmsEditInviteCodeScreen`: one code's active flag and expiration.
    EditCode {
        server: RealmServer,
        code: InviteCode,
        active: bool,
        /// `EXPIRATIONS`' index.
        expiration: usize,
        job: Option<Job<Result<(), RealmsError>>>,
        error: Option<String>,
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
    /// `RealmsResetWorldScreen`: the 3x2 tiles that replace the slot's world.
    ResetWorld {
        server: RealmServer,
        slot: i32,
        kind: ResetKind,
        /// `RealmCreationTask`'s name and description, for `ResetKind::NewRealm`.
        creation: Option<(String, String)>,
        /// `init`'s fetcher: page 1 of each group's templates, in `TemplateGroup::ALL`'s order.
        templates: Option<[Result<WorldTemplateList, RealmsError>; 4]>,
        job: Option<Job<[Result<WorldTemplateList, RealmsError>; 4]>>,
        /// The template reset (`ResettingTemplateWorldTask`) while it runs, then its answer.
        reset: Option<RunningTask>,
        error: Option<String>,
    },
    /// `RealmsSelectWorldTemplateScreen`: a world type's templates, a page at a time.
    TemplateList {
        server: RealmServer,
        slot: i32,
        kind: ResetKind,
        /// `RealmCreationTask`'s name and description, for `ResetKind::NewRealm`.
        creation: Option<(String, String)>,
        /// `None` for the minigame list, which switches the Realm instead of resetting it.
        group: Option<TemplateGroup>,
        templates: Vec<WorldTemplate>,
        selected: Option<usize>,
        scroll: crate::ui::Scroll,
        job: Option<TemplatePages>,
        /// `SwitchMinigameTask` on a worker, from the minigame list's Select.
        select: Option<RunningTask>,
        /// `Entry.websiteButton`/`trailerButton` as drawn: (row, rect, trailer) — the
        /// click needs the row's version width, which only the draw measures.
        icons: std::cell::RefCell<Vec<(usize, [f32; 4], bool)>>,
        error: Option<String>,
    },
    /// `RealmsCreateRealmScreen`: the name and description of a Realm the service made
    /// but has not been set up yet (`RealmsMainScreen.ServerEntry.createUnitializedRealm`).
    CreateRealm {
        server: RealmServer,
        name: String,
        description: String,
        /// The field with the caret: the name (false) or the description (true).
        on_description: bool,
        error: Option<String>,
    },
    /// `RealmsSelectFileToUploadScreen`: the local worlds an upload can pick.
    UploadSelect {
        server: RealmServer,
        slot: i32,
        kind: ResetKind,
        /// `RealmCreationTask`'s name and description, for `ResetKind::NewRealm`.
        creation: Option<(String, String)>,
        worlds: Vec<crate::title::WorldEntry>,
        selected: Option<usize>,
        scroll: crate::ui::Scroll,
        /// The search box's text (`WorldSelectionList.updateFilter`).
        query: String,
    },
    /// `RealmsUploadScreen`: the pack and the body on a worker, its numbers and its answer.
    Uploading {
        server: RealmServer,
        slot: i32,
        world: crate::title::WorldEntry,
        progress: Arc<upload::UploadProgress>,
        job: Option<Job<Result<(), upload::UploadError>>>,
        error: Option<upload::UploadError>,
        /// The worker answered (done or failed): Cancel gives way to Done.
        done: bool,
        /// When the screen opened: the dots' blink (`tickCount / 10` at 20 ticks a second).
        started: Instant,
    },
    /// `RealmsCreateWorldFlow.createWorld`: the temporary world the Create New World
    /// screen described is packed and uploaded, under `AlertScreen`
    /// (`mco.create.world.reset.title`) with Cancel.
    CreateWorldUpload {
        request: CreateWorldRequest,
        progress: Arc<upload::UploadProgress>,
        job: Option<Job<Result<(), upload::UploadError>>>,
    },
    /// `RealmsPdpScreen`: the product page behind Add Realm, its picture rows and scroll.
    AddRealm {
        scroll: crate::ui::Scroll,
        /// The list's drawn height, measured in [`Screen::draw`] and read by the wheel.
        content: std::cell::Cell<f32>,
        /// The rows as last drawn, in scroll order: the click needs the heights the
        /// wrapped text gave them.
        rows: std::cell::RefCell<Vec<[f32; 4]>>,
        /// `RealmsClient.trialAvailable` (`RealmsDataFetcher.trialAvailabilityTask`): the
        /// header's trial note is shown only when this says yes.
        trial: bool,
        trial_job: Option<Job<Result<bool, RealmsError>>>,
    },
    Popup(Popup),
    /// `RealmsLongRunningMcoTaskScreen` running `GetServerDetailsTask`: the join request is on
    /// a worker (`joining`); an error stays on screen with a Back button.
    Connecting {
        server: RealmServer,
        error: Option<String>,
        /// `GetServerDetailsTask.lastScreen`: where a 6002 answer returns
        /// (`RealmsTermsScreen`'s Disagree and Escape).
        last: Box<SubScreen>,
    },
    /// `RealmsTermsScreen`: a new account must agree to the Realms terms before its first
    /// join (the service answers 6002).
    Terms {
        server: RealmServer,
        last: Box<SubScreen>,
        /// `RealmsClient.agreeToTos` on a worker; the join runs again when it succeeds.
        agree: Option<Job<Result<(), RealmsError>>>,
    },
    /// `RealmsGenericErrorScreen`: a service failure replaces the screen; OK and Escape
    /// return to `next` (`nextScreen`).
    Error {
        /// `mco.errorMessage.realmsService.realmsError` with the code, or the title itself.
        title: String,
        /// The error's message, drawn in the -2142128 blue.
        detail: String,
        next: Box<SubScreen>,
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
    /// The game's `saves` folder: `RealmsSelectFileToUploadScreen` lists it
    /// (`minecraft.gameDirectory`'s `saves`).
    saves: Option<PathBuf>,
    /// `RealmsNewsManager`: the page the header's news button opens, and whether it has
    /// not been read yet (vanilla keeps the read link in `realms_persistence.json`).
    news_link: Option<String>,
    news_job: Option<Job<Result<Option<String>, RealmsError>>>,
    news_unread: bool,
    /// `RealmsHeader`'s pending-invite count for the invite button's badge
    /// (`numberOfPendingInvites`).
    pending_invites: u32,
    pending_invites_job: Option<Job<Result<Vec<PendingInviteDto>, RealmsError>>>,
    /// `RealmsHeader.createLayout`'s join button width, measured in the draw (text plus 16,
    /// at most half of the 308 content the logo leaves).
    join_width: std::cell::Cell<f32>,
    /// The product page's selected row (`ObjectSelectionList.getSelected`).
    pub(crate) pdp_row: Option<usize>,
    /// The last click took a product-page row (`ObjectSelectionList.mouseClicked`): the
    /// keyboard focus moves to the list.
    list_clicked: bool,
    /// `ConfirmLinkScreen`'s three buttons as last drawn (`Choice::new` lays them out from
    /// the message's width).
    link_buttons: std::cell::Cell<[(f32, f32); 3]>,
    /// `RealmsTermsScreen`'s terms link as last drawn: its hit box follows the text's
    /// rendered width (`extractRenderState`'s `onLink`).
    terms_link: std::cell::Cell<[f32; 4]>,
    /// `RealmsMainScreen.notifications`: the service's messages; the first `visitUrl`
    /// becomes the list's top entry (`NotificationMessageEntry`).
    notifications: Vec<RealmsNotification>,
    notifications_job: Option<Job<Result<Vec<RealmsNotification>, RealmsError>>>,
    /// `handledSeenNotifications`: the ones `notifications/seen` answered for.
    handled_seen: Vec<String>,
    /// `notifications/dismiss` on a worker.
    dismiss_job: Option<Job<Result<(), RealmsError>>>,
    /// `NotificationMessageEntry`'s open button, then its dismiss cross, as last drawn; and
    /// the entry's height. The click needs the wrapped text's height.
    notice_rects: std::cell::Cell<[[f32; 4]; 2]>,
    notice_height: std::cell::Cell<f32>,
    /// `PopupScreen`'s two buttons as last drawn: OK, then the URL button.
    notice_buttons: std::cell::Cell<[[f32; 4]; 2]>,
    /// `PopupScreen`'s Continue and Cancel as last drawn (`buildButtonRow`'s geometry,
    /// which the message's wrapped height decides).
    popup_buttons: std::cell::Cell<[[f32; 4]; 2]>,
    /// The main list's scroll (`ObjectSelectionList`): the port drew what fit before.
    main_scroll: crate::ui::Scroll,
    /// The subscription tab's Extend and Delete button tops, as the draw laid the
    /// column out (`RealmsSubscriptionTab`'s centred stack).
    subscription_buttons: std::cell::Cell<(f32, f32)>,
    /// The players tab's per-row buttons as last drawn: the rect and its id (the op
    /// toggle's width follows its label, which only the draw measures).
    players_button_rects: std::cell::RefCell<Vec<([f32; 4], u8)>>,
    /// `RealmsBackupScreen.Entry`'s Changes/Restore buttons as last drawn.
    backup_row_buttons: std::cell::RefCell<Vec<([f32; 4], u8)>>,
    /// The settings tab's `ScrollableLayout`: the column is taller than the tab area on
    /// short windows (vanilla scrolls it).
    settings_scroll: crate::ui::Scroll,
    /// The settings tab's held distance slider (`AbstractSliderButton.dragging`): 0 for
    /// render distance, 1 for simulation distance.
    settings_slider: Option<usize>,
    /// The other held sliders: 1 the slot options' spawn protection, 2 the edit-code
    /// expiration.
    other_slider: Option<u8>,
    /// A button press that has to wait for the settings tab's save (`Play`, the open/close
    /// toggle): `RealmsConfigureWorldScreen` saves an active tab before either runs.
    after_save: Option<AfterSave>,
    /// `RealmsMainScreen.redirectToPdpIfNoRealms`: opened from the title screen, the first
    /// empty server list opens the product page.
    redirect_to_pdp: bool,
    /// `mco.template.select.none`'s link segment as last drawn: the empty template list's
    /// link to the content-creation page.
    template_link_rect: std::cell::Cell<[f32; 4]>,
    /// `RealmsMainScreen.trialAvailable`: the Add Realm button reads "free trial" and the
    /// product page offers "Try for free".
    trial: bool,
    trial_job: Option<Job<Result<bool, RealmsError>>>,
    /// `RealmsDataFetcher.liveStatsTask`: each Realm's online players (`getLiveStats`).
    live_players: std::collections::HashMap<i64, Vec<String>>,
    live_job: Option<Job<Result<std::collections::HashMap<i64, Vec<String>>, RealmsError>>>,
    /// `RealmsDataFetcher`'s periods: the server list every 60 s, pending invites every
    /// 10 s, news and notifications every 300 s.
    last_list_refresh: Instant,
    last_pending_refresh: Instant,
    last_news_refresh: Instant,
    /// The region list answered with the region button's press waiting: open the
    /// preference screen then (`RealmsConfigureWorldScreen.init` fetches it up front).
    region_screen_requested: bool,
    /// `RealmsTextureManager`'s world-template pictures, decoded to the 64x64
    /// `horizon/world-iconN` slots, by `(template id, image length)`.
    template_images: std::cell::RefCell<std::collections::HashMap<String, Option<Vec<u8>>>>,
    /// `RealmsSelectFileToUploadScreen`'s world icons: `icon.png` decoded, cached by the
    /// file's modified time (the same cache the title world list uses).
    world_icons: std::cell::RefCell<
        std::collections::HashMap<std::path::PathBuf, (SystemTime, Option<Vec<u8>>)>,
    >,
    /// `RealmsUtil.extractPlayerFace`: the profile resolver and skin downloader the
    /// title screen otherwise has none of (`Game` keeps its own for the tab list).
    skin_workers: SkinWorkers,
    /// The 64x64 face of each downloaded skin, by its skin location.
    skin_faces: std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>>,
}

/// The title screen's own skin resolution workers (§`RealmsUtil.extractPlayerFace`).
/// Shared through `Arc` because `Screen` is cloned; `Debug` is a placeholder because the
/// resolver and downloader hold threads and files, not printable state.
#[derive(Clone)]
struct SkinWorkers {
    profiles: std::sync::Arc<std::sync::Mutex<crate::skins::Resolver>>,
    downloads: std::sync::Arc<std::sync::Mutex<crate::skins::Downloader>>,
}

impl std::fmt::Debug for SkinWorkers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkinWorkers").finish_non_exhaustive()
    }
}

/// `RealmsSettingsTab.save`'s callers: the settings are written before the action runs.
#[derive(Clone, Debug)]
enum AfterSave {
    /// `RealmsConfigureWorldScreen`'s Play: `onClose()` saves, then joins.
    Play(RealmServer),
    /// The open/close toggle: save, then `openTheWorld(false)`/`closeTheWorld()`.
    Close(i64),
    Open(i64),
}

pub enum Outcome {
    None,
    Back,
    Connect(String),
    OpenUri(String),
    /// `KeyboardHandler.setClipboard` for the title's own clipboard.
    Copy(String),
    /// `RealmsResetWorldScreen`'s Generate New World tile: `RealmsCreateWorldFlow`
    /// opens Create World and uploads the world it describes.
    CreateWorld(CreateWorldRequest),
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

/// `RealmsCreateWorldFlow.createWorld`'s worker: `createTemporaryWorldFolder`, then
/// `RealmCreationTask` → `SwitchSlotTask` → `packAndUpload`, as an upload's worker does.
fn upload_created_world(
    client: &RealmsClient,
    server: &RealmServer,
    slot: i32,
    creation: Option<&(String, String)>,
    world: &CreatedWorld,
    progress: &upload::UploadProgress,
) -> Result<(), upload::UploadError> {
    let dir = create_temporary_world(world).map_err(upload::UploadError::Failed)?;
    let result = (|| {
        if let Some((name, description)) = creation {
            client
                .initialize_realm(server.id, name, description)
                .map_err(|e| upload::UploadError::Failed(e.text()))?;
        }
        if slot != server.active_slot {
            client
                .switch_slot(server.id, slot)
                .map_err(|e| upload::UploadError::Failed(e.text()))?;
        }
        // `RealmsWorldOptions.createFromSettings`: the world's own settings, the game's
        // version as the world version.
        let options = RealmWorldOptions {
            spawn_protection: 0,
            force_game_mode: false,
            difficulty: i32::from(world.level.difficulty_id()),
            game_mode: if world.level.creative { 1 } else { 0 },
            slot_name: world.level.name.clone(),
            version: crate::save::VERSION_NAME.into(),
            compatibility: "UNVERIFIABLE".into(),
            template_id: -1,
            template_image: None,
            empty: false,
        };
        let realm_slot = RealmSlot {
            slot_id: slot,
            options,
            settings: vec![RealmSetting {
                name: "hardcore".into(),
                value: world.level.hardcore.to_string(),
            }],
        };
        upload::upload_world(client, server.id, &realm_slot, &dir, progress)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// `RealmsCreateWorldFlow.createTemporaryWorldFolder`: level.dat as
/// `WorldData.createTag(null)` writes it (no `singleplayer_uuid`), the world generation
/// and game rules, and the Create World screen's data packs moved into the world. No
/// chunks: the Realm generates them when the world first opens.
fn create_temporary_world(world: &CreatedWorld) -> std::result::Result<PathBuf, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "horizon-realms-world-upload-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let result = (|| {
        let mut level = world.level.clone();
        level.data_packs = Some(world.data_packs.clone());
        level.write_upload(&dir).map_err(|e| e.to_string())?;
        crate::save::write_world_gen_settings_with(
            &dir,
            level.seed,
            world.world_gen,
            world.flat_settings.clone(),
            world.single_biome.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        if let Some(pack_dir) = &world.pack_dir {
            // `Files.move(tempDataPackDir, worldFolder.resolve("datapacks"))`; the
            // Create World screen's temporary folder is gone afterwards.
            crate::datapack::install(pack_dir, &dir, &world.data_packs.0)?;
            let _ = std::fs::remove_dir_all(pack_dir);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(dir),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// `RealmsPdpScreen`'s list: the trial header (no picture) and the three features, each
/// with its 80x45 picture and the text beside it. The header's subtitle
/// (`mco.pdp.trial.description`, with `icon/trial_available`) is shown only when the service
/// says a trial is available (`trialAvailabilityTask`).
fn pdp_entries() -> [(&'static str, &'static str, &'static [&'static str]); 4] {
    [
        ("", "mco.pdp.trial.title", &["mco.pdp.trial.description"]),
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

/// `RealmsPdpScreen.AbstractPdpEntry.getNarration`'s lines: the title and its descriptions,
/// a line each (`CommonComponents.joinLines`), the trial's only with a trial available.
fn pdp_row_lines(row: usize, trial: bool) -> Vec<String> {
    let (_, title, descriptions) = pdp_entries()[row];
    let mut lines = vec![crate::lang::get(title)];
    // `PdpList`'s header carries the trial description only with a trial available.
    if title != "mco.pdp.trial.title" || trial {
        lines.extend(descriptions.iter().map(|key| crate::lang::get(key)));
    }
    lines
}

/// `RealmsPdpScreen.AbstractPdpEntry.getNarration`: `narrator.select` of those lines.
fn pdp_row_narration(row: usize, trial: bool) -> String {
    crate::screen_narrator::translate("narrator.select", &[&pdp_row_lines(row, trial).join("\n")])
}

/// `RealmsSelectWorldTemplateScreen.fetchTemplates`: one page at a time; the list shows it
/// as it arrives and the screen chains the next page until the last or an empty one.
fn fetch_template_page(
    client: &RealmsClient,
    world_type: &str,
    page: i32,
    page_size: i32,
) -> Result<(Vec<WorldTemplate>, i32, bool), RealmsError> {
    let answer = client.fetch_world_templates(page, page_size, world_type)?;
    let empty = answer.templates.is_empty();
    Ok((
        answer.templates,
        page + 1,
        empty || last_template_page(answer.page, answer.size, answer.total),
    ))
}

/// `WorldTemplatePaginatedList.isLastPage`.
fn last_template_page(page: i32, size: i32, total: i32) -> bool {
    page > 0 && size > 0 && total > 0 && page * size >= total
}

fn wrap_lines(text: &str, atlas: &HudAtlas, width: f32) -> Vec<String> {
    crate::multiplayer::wrap(text, width, atlas)
}
fn centered(batch: &mut HudBatch, atlas: &HudAtlas, text: &str, cx: f32, y: f32, color: u32) {
    let w = HudBatch::text_width(atlas, text);
    batch.text(atlas, text, cx - (w / 2.0).floor(), y, color, true);
}

/// `RealmsResetWorldScreen.init`'s grid: three 60-wide tiles with 16 padding a side, and a
/// 16-tall spacer between the two rows (`FrameButton` is 60 wide and 69 tall: the button
/// plus its 9-tall label). The contents frame starts at `min(63, h - 187)`.
fn reset_content_y(h: f32) -> f32 {
    63.0_f32.min(h - 187.0)
}

fn reset_tile(cx: f32, top: f32, i: usize) -> (f32, f32) {
    let column = (i % 3) as f32;
    let row = (i / 3) as f32;
    (cx - 122.0 + column * 92.0, top + row * 85.0)
}

/// `Unit.humanReadable`: binary units, a tenth at GB (`mco.upload.failed.too_big`).
fn human_readable(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == UNITS.len() - 1 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// The row `y` falls on in a list that starts at `top` and has scrolled `offset` pixels.
fn row_at(
    x: f32,
    y: f32,
    w: f32,
    top: f32,
    bottom: f32,
    height: f32,
    offset: f32,
) -> Option<usize> {
    let rx = ((w - ROW_WIDTH) / 2.0).floor();
    if y < top || y >= bottom || x < rx || x > rx + ROW_WIDTH {
        return None;
    }
    let row = (y - top - 2.0 + offset) / height;
    (row >= 0.0).then(|| row.floor() as usize)
}

impl Screen {
    pub fn new() -> Self {
        Self::with_saves(None)
    }

    /// The screen with the game's `saves` folder, which `RealmsSelectFileToUploadScreen`
    /// lists (`new RealmsSelectFileToUploadScreen` reads the level list).
    pub fn with_saves(saves: Option<PathBuf>) -> Self {
        // `SkinManager`'s cache under the game directory (`assets/skins`).
        let skin_root = saves
            .as_ref()
            .and_then(|saves| saves.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("assets/skins");
        let mut screen = Self {
            state: FetchState::Unauthenticated,
            subscreen: SubScreen::Main,
            selected: None,
            joining: None,
            leaving: None,
            last_click: None,
            saves,
            news_link: None,
            news_job: None,
            news_unread: false,
            pending_invites: 0,
            pending_invites_job: None,
            join_width: std::cell::Cell::new(90.0),
            pdp_row: None,
            list_clicked: false,
            link_buttons: std::cell::Cell::new([(0.0, 0.0); 3]),
            terms_link: std::cell::Cell::new([0.0; 4]),
            notifications: Vec::new(),
            notifications_job: None,
            handled_seen: Vec::new(),
            dismiss_job: None,
            notice_rects: std::cell::Cell::new([[0.0; 4]; 2]),
            notice_height: std::cell::Cell::new(0.0),
            notice_buttons: std::cell::Cell::new([[0.0; 4]; 2]),
            popup_buttons: std::cell::Cell::new([[0.0; 4]; 2]),
            main_scroll: crate::ui::Scroll::default(),
            subscription_buttons: std::cell::Cell::new((0.0, 0.0)),
            players_button_rects: std::cell::RefCell::new(Vec::new()),
            backup_row_buttons: std::cell::RefCell::new(Vec::new()),
            settings_scroll: crate::ui::Scroll::default(),
            settings_slider: None,
            other_slider: None,
            after_save: None,
            redirect_to_pdp: false,
            template_link_rect: std::cell::Cell::new([0.0; 4]),
            trial: false,
            trial_job: None,
            live_players: std::collections::HashMap::new(),
            live_job: None,
            last_list_refresh: Instant::now(),
            last_pending_refresh: Instant::now(),
            last_news_refresh: Instant::now(),
            region_screen_requested: false,
            template_images: std::cell::RefCell::new(std::collections::HashMap::new()),
            world_icons: std::cell::RefCell::new(std::collections::HashMap::new()),
            skin_workers: SkinWorkers {
                profiles: std::sync::Arc::new(std::sync::Mutex::new(crate::skins::Resolver::new())),
                downloads: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::skins::Downloader::new(skin_root),
                )),
            },
            skin_faces: std::cell::RefCell::new(std::collections::HashMap::new()),
        };
        // `RealmsNewsManager`'s constructor: the stored link and read state start the
        // session (`RealmsPersistence.readFile` of the game directory's file).
        let (stored_link, stored_unread) = screen.read_news_persistence();
        screen.news_link = stored_link;
        screen.news_unread = stored_unread;
        screen.refresh();
        screen
    }

    /// `new RealmsMainScreen(lastScreen, ...)` from the title screen: vanilla sets
    /// `redirectToPdpIfNoRealms`, so a first empty list opens the product page.
    pub fn from_title(saves: Option<PathBuf>) -> Self {
        let mut screen = Self::with_saves(saves);
        screen.redirect_to_pdp = true;
        screen
    }

    /// `RealmsPersistence.getPathToData`: `realms_persistence.json` in the game
    /// directory — the saves folder's parent.
    fn news_persistence_path(&self) -> Option<PathBuf> {
        self.saves
            .as_ref()?
            .parent()
            .map(|dir| dir.join("realms_persistence.json"))
    }

    /// `RealmsPersistence.readFile`: the stored news link and its read flag; a missing
    /// or unreadable file is the fresh state.
    fn read_news_persistence(&self) -> (Option<String>, bool) {
        let Some(path) = self.news_persistence_path() else {
            return (None, false);
        };
        match std::fs::read_to_string(&path) {
            Ok(json) => {
                let data: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
                (
                    data["newsLink"].as_str().map(str::to_owned),
                    data["hasUnreadNews"].as_bool().unwrap_or(false),
                )
            }
            Err(_) => (None, false),
        }
    }

    /// `RealmsPersistence.writeFile`.
    fn write_news_persistence(&self, link: Option<&str>, unread: bool) {
        let Some(path) = self.news_persistence_path() else {
            return;
        };
        let json = serde_json::json!({ "newsLink": link, "hasUnreadNews": unread });
        let _ = std::fs::write(&path, json.to_string());
    }

    pub fn layout_state(&self) -> LayoutState {
        match &self.state {
            FetchState::Loading(_) => LayoutState::Loading,
            // A notification entry alone makes the list (`RealmsMainScreen`'s `LIST`).
            FetchState::Loaded(servers)
                if !servers.is_empty() || !self.notifications.is_empty() =>
            {
                LayoutState::List
            }
            _ => LayoutState::NoRealms,
        }
    }

    /// `AbstractSelectionList.updateNarration` over the selected server
    /// (`ServerEntry.getNarration`): the list narrates its selection as a widget line.
    pub fn narration(&self) -> Option<String> {
        if !matches!(self.subscreen, SubScreen::Main) {
            return None;
        }
        let server = self.selected_server()?;
        if server.state == "UNINITIALIZED" {
            return Some(crate::screen_narrator::translate(
                "gui.narrate.button",
                &[&crate::lang::get("mco.selectServer.uninitialized")],
            ));
        }
        Some(crate::screen_narrator::translate(
            "narrator.select",
            &[&server.name],
        ))
    }

    /// `RealmsUtil.extractPlayerFace`: a player's head in a `horizon/world-iconN` slot,
    /// resolved and downloaded on the title screen's own workers. `None` until both
    /// answers are there (or when the player has no profile or skin).
    fn face_sprite(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        uuid: Option<&str>,
        slot: usize,
    ) -> Option<String> {
        let id = uuid.and_then(horizon_client_render::auth::parse_uuid)?;
        let (value, signature) = {
            let mut profiles = self.skin_workers.profiles.lock().ok()?;
            let crate::skins::Resolve::Done(Some(profile)) =
                profiles.state(&crate::skins::NameOrId::Id(id))
            else {
                return None;
            };
            profile.textures()?
        };
        let textures = crate::skins::unpack(&value, signature, crate::skins::texture_uris());
        let url = textures.skin?;
        let location = crate::skins::skin_location(&crate::skins::texture_hash(&url)?);
        let mut downloads = self.skin_workers.downloads.lock().ok()?;
        if downloads.state(crate::skins::Kind::Skin, &url) != crate::skins::Download::Ready {
            return None;
        }
        // The ready list is the game's hand-off; drain it so it does not grow, then
        // read the processed 64x64 skin from the downloader's own cache.
        let _ = downloads.take_ready();
        let cached = self.skin_faces.borrow().get(&location).cloned();
        let face = match cached {
            Some(face) => face,
            None => {
                let skin = downloads.image(&location)?;
                let face = face_from_skin(skin)?;
                self.skin_faces
                    .borrow_mut()
                    .insert(location.clone(), face.clone());
                face
            }
        };
        drop(downloads);
        icon_sprite(batch, atlas, slot, &face)
    }

    /// `RealmsMainScreen.createAddRealmButton`: with a trial available the button reads
    /// "free trial" instead of "buy".
    fn add_realm_label(&self) -> String {
        if self.trial {
            crate::lang::get("mco.selectServer.freeTrial")
        } else {
            crate::lang::get("mco.selectServer.purchase")
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

    /// `NotificationMessageEntry`'s notification: the first `visitUrl` one, as
    /// `refreshEntries` adds an entry for it and stops.
    fn notice(&self) -> Option<&RealmsNotification> {
        self.notifications
            .iter()
            .find(|notification| notification.kind == "visitUrl" && notification.url.is_some())
    }

    /// `NotificationMessageEntry.textWidth`: the row is 300 wide, 40 a side.
    fn notice_lines(&self, atlas: &HudAtlas) -> Vec<String> {
        match self.notice() {
            Some(notification) => {
                wrap_lines(&notification.visit_message(), atlas, ROW_WIDTH - 80.0)
            }
            None => Vec::new(),
        }
    }

    /// `RealmsMainScreen.markNotificationsAsSeen`: the list's entry is answered as seen
    /// once (`handledSeenNotifications` holds the ones already sent).
    fn mark_notifications_seen(&mut self) {
        let Some(uuid) = self
            .notice()
            .filter(|notification| {
                !notification.seen && !self.handled_seen.contains(&notification.uuid)
            })
            .map(|notification| notification.uuid.clone())
        else {
            return;
        };
        self.handled_seen.push(uuid.clone());
        if let Some(client) = client() {
            spawn(move || client.notifications_seen(std::slice::from_ref(&uuid)));
        }
    }

    /// `RealmsMainScreen.dismissNotification`: the service is told and the entry leaves.
    fn dismiss_notification(&mut self, uuid: &str) {
        let Some(notification) = self.notifications.iter().find(|n| n.uuid == uuid) else {
            return;
        };
        if !notification.dismissable {
            return;
        }
        let uuid = notification.uuid.clone();
        self.notifications.retain(|n| n.uuid != uuid);
        if let Some(client) = client() {
            let uuids = vec![uuid];
            self.dismiss_job = Some(spawn(move || client.notifications_dismiss(&uuids)));
        }
    }

    /// `RealmsHeader.NotificationButton.extractNotificationCounter`: the badge sits 8x8
    /// at the button's top right corner (x + width - 5, y - 3) — drawn on top of the
    /// button, so [`Self::draw`] cannot have drawn it: the buttons render after. Unread
    /// news shows notification/more; the invite button counts the service's pending
    /// invites, notification/1..5 then more. The main screen only (`draw_main`'s header).
    pub fn draw_header_badges(&self, atlas: &HudAtlas, batch: &mut HudBatch, w: f32) {
        // `RealmsHeader` belongs to the main list and the product page alike: both draw
        // the pending-invite and news badges on their buttons.
        if !matches!(self.subscreen, SubScreen::Main | SubScreen::AddRealm { .. }) {
            return;
        }
        let cx = (w / 2.0).floor();
        let badge = |count: u32| match count {
            0 => None,
            1 => Some("gui/sprites/notification/1"),
            2 => Some("gui/sprites/notification/2"),
            3 => Some("gui/sprites/notification/3"),
            4 => Some("gui/sprites/notification/4"),
            5 => Some("gui/sprites/notification/5"),
            _ => Some("gui/sprites/notification/more"),
        };
        if self.news_unread
            && let Some(sprite) = badge(u32::MAX)
            && atlas.sprites.contains_key(sprite)
        {
            crate::ui::blit_sprite(
                batch,
                atlas,
                sprite,
                cx + 154.0 - 20.0 + 15.0,
                12.0 - 3.0,
                8.0,
                8.0,
                u32::MAX,
            );
        }
        if self.pending_invites > 0
            && let Some(sprite) = badge(self.pending_invites)
            && atlas.sprites.contains_key(sprite)
        {
            crate::ui::blit_sprite(
                batch,
                atlas,
                sprite,
                cx + 154.0 - 44.0 + 15.0,
                12.0 - 3.0,
                8.0,
                8.0,
                u32::MAX,
            );
        }
    }

    /// `RealmsMainScreen.refreshServerList`.
    pub fn refresh(&mut self) {
        self.selected = None;
        let now = Instant::now();
        self.last_list_refresh = now;
        self.last_pending_refresh = now;
        self.last_news_refresh = now;
        self.news_job = client().map(|client| spawn(move || client.news_link()));
        self.notifications_job = client().map(|client| spawn(move || client.notifications()));
        // `RealmsHeader`'s invite badge counts the service's pending invites
        // (`numberOfPendingInvites`).
        self.pending_invites_job = client().map(|client| spawn(move || client.pending_invites()));
        // `RealmsDataFetcher.trialAvailabilityTask`: the footer's free-trial button.
        self.trial_job = client().map(|client| spawn(move || client.trial_available()));
        // `RealmsDataFetcher.liveStatsTask`: the rows' online faces.
        self.live_job = client().map(|client| spawn(move || client.live_players()));
        self.state = match client() {
            Some(client) => FetchState::Loading(spawn(move || client.list_worlds())),
            None => FetchState::Unauthenticated,
        };
    }

    /// `RealmsGenericErrorScreen.onClose`: the `nextScreen` returns; a bare configure
    /// screen starts its fetches again (`RealmsConfigureWorldScreen.init`).
    fn restore_after_error(&mut self, next: SubScreen) {
        self.subscreen = next;
        if matches!(
            self.subscreen,
            SubScreen::Configure {
                subscription: None,
                detail: None,
                ..
            }
        ) {
            self.start_configure_subscription();
            self.start_configure_detail();
        }
        // The download task's error returns to the backup list, which reads again.
        if matches!(self.subscreen, SubScreen::Backups { backups: None, .. }) {
            self.start_backups();
        }
    }

    /// `LongRunningTask.error`: a task's failure replaces the screen with
    /// `RealmsGenericErrorScreen(new Component(message), nextScreen)` — the generic title
    /// and the message as its detail; OK returns to the list.
    fn show_generic_error(&mut self, detail: String) {
        self.subscreen = SubScreen::Error {
            title: crate::lang::get("mco.errorMessage.generic"),
            detail,
            next: Box::new(SubScreen::Main),
        };
    }

    /// `RealmsMainScreen.openRealmsPdp`: the product page, and its `trialAvailable` check.
    fn open_pdp(&mut self) {
        let trial_job = client().map(|client| spawn(move || client.trial_available()));
        self.subscreen = SubScreen::AddRealm {
            scroll: crate::ui::Scroll::default(),
            content: std::cell::Cell::new(0.0),
            rows: std::cell::RefCell::new(Vec::new()),
            trial: false,
            trial_job,
        };
    }

    /// Picks up what the workers finished; called once a frame.
    pub fn poll(&mut self) -> Outcome {
        // `RealmsDataFetcher`'s `DataFetcher.Subscription.tick`: the main list polls its
        // tasks on their own periods while it is the screen showing.
        if matches!(self.subscreen, SubScreen::Main) {
            let now = Instant::now();
            if !matches!(self.state, FetchState::Loading(_))
                && now.duration_since(self.last_list_refresh) >= Duration::from_secs(60)
            {
                self.last_list_refresh = now;
                self.state = match client() {
                    Some(client) => FetchState::Loading(spawn(move || client.list_worlds())),
                    None => FetchState::Unauthenticated,
                };
            }
            if self.pending_invites_job.is_none()
                && now.duration_since(self.last_pending_refresh) >= Duration::from_secs(10)
            {
                self.last_pending_refresh = now;
                self.pending_invites_job =
                    client().map(|client| spawn(move || client.pending_invites()));
                self.live_job = client().map(|client| spawn(move || client.live_players()));
            }
            if self.news_job.is_none()
                && self.notifications_job.is_none()
                && now.duration_since(self.last_news_refresh) >= Duration::from_secs(300)
            {
                self.last_news_refresh = now;
                self.news_job = client().map(|client| spawn(move || client.news_link()));
                self.notifications_job =
                    client().map(|client| spawn(move || client.notifications()));
            }
        }
        let mut open_pdp_if_no_realms = false;
        if let FetchState::Loading(job) = &self.state
            && let Some(result) = take(job)
        {
            self.state = match result {
                // `RealmsServerList.updateServersList` sorts with
                // `McoServerComparator`; `openPdpIfNoRealms` then runs once.
                Ok(mut servers) => {
                    let owner = session().map(|s| s.name.clone());
                    sort_servers(&mut servers, owner.as_deref());
                    if self.redirect_to_pdp {
                        self.redirect_to_pdp = false;
                        open_pdp_if_no_realms =
                            servers.is_empty() && matches!(self.subscreen, SubScreen::Main);
                    }
                    FetchState::Loaded(servers)
                }
                Err(e) => FetchState::Error(e),
            };
        }
        if open_pdp_if_no_realms {
            self.open_pdp();
        }
        if let Some(job) = &self.news_job
            && let Some(result) = take(job)
        {
            self.news_job = None;
            if let Ok(link) = result {
                // `RealmsNewsManager.updateNewsStorage`: a link different from the stored
                // one saves itself as unread; the stored state stays otherwise.
                if link.is_some() && link != self.news_link {
                    self.write_news_persistence(link.as_deref(), true);
                    self.news_link = link;
                    self.news_unread = true;
                }
            }
        }
        if let Some(job) = &self.pending_invites_job
            && let Some(result) = take(job)
        {
            self.pending_invites_job = None;
            if let Ok(list) = result {
                self.pending_invites = list.len() as u32;
            }
        }
        if let Some(job) = &self.trial_job
            && let Some(result) = take(job)
        {
            self.trial_job = None;
            // `RealmsMainScreen.setTrialAvailable`: a failure keeps the last answer.
            if let Ok(available) = result {
                self.trial = available;
            }
        }
        if let Some(job) = &self.live_job
            && let Some(result) = take(job)
        {
            self.live_job = None;
            // `RealmsDataFetcher.liveStatsTask`: a failure keeps the last list.
            if let Ok(players) = result {
                self.live_players = players;
            }
        }
        if let Some(job) = &self.notifications_job
            && let Some(result) = take(job)
        {
            self.notifications_job = None;
            if let Ok(notifications) = result {
                self.notifications = notifications;
                self.mark_notifications_seen();
                self.show_info_popup();
            }
        }
        if let Some(job) = &self.dismiss_job
            && take(job).is_some()
        {
            self.dismiss_job = None;
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
        let mut saved_code = false;
        let mut refetch_codes = false;
        let mut back_to_worlds: Option<RealmServer> = None;
        let mut created = false;
        // The settings save finished: a Play or the close toggle continues.
        let mut settings_saved = false;
        // An invite-code fetch failure: `RealmsGenericErrorScreen` over the players tab.
        let mut codes_error: Option<(RealmServer, RealmsError)> = None;
        // `RestoreTask`'s failure: the error screen over the configuration screen.
        let mut restore_error: Option<(RealmServer, RealmsError)> = None;
        // `DownloadTask` answered with the links (`RealmsDownloadLatestWorldScreen`), or
        // with a service error (`RealmsGenericErrorScreen` over the backup list).
        let mut download_ready: Option<(RealmServer, i32, WorldDownload)> = None;
        let mut download_error: Option<(RealmServer, i32, RealmsError)> = None;
        // `checkDownloadSize`'s answer: the oversized warning, or the download itself.
        let mut oversized_warning: Option<(RealmServer, i32, WorldDownload, String, Instant)> =
            None;
        let mut start_download: Option<(WorldDownload, String, Arc<download::DownloadProgress>)> =
            None;
        // `LongRunningTask.error`: a reset or minigame-switch task's failure replaces the
        // screen with the generic error screen.
        let mut task_error: Option<String> = None;
        // `RealmsEditInviteCodeScreen.handleFailure` keeps the edit screen as its next.
        let mut edit_error: Option<(RealmServer, InviteCode, bool, usize, RealmsError)> = None;
        // `RealmsInviteCodesScreen.handleFailure` returns to the loaded codes list.
        let mut codes_action_error: Option<(
            RealmServer,
            Option<Result<Vec<InviteCode>, RealmsError>>,
            Option<usize>,
            RealmsError,
        )> = None;
        // A configure screen's fetch failed: `RealmsConfigureWorldScreen.createErrorScreen`
        // replaces the screen with the generic error screen.
        let mut fetch_error: Option<RealmsError> = None;
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
            SubScreen::PendingInvites { invites, job, .. } => {
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
                    // `fetchSubscription`: the failure replaces the screen.
                    if let Err(e) = &result {
                        fetch_error = Some(e.clone());
                    }
                    *subscription = Some(result);
                }
                if let Some(result) = detail_job.as_ref().and_then(take) {
                    *detail_job = None;
                    // `RealmsConfigureWorldScreen.fetchServerData`: the detail *is* the
                    // screen's server copy (its region preference is what the settings
                    // draft starts from; the list's copy does not carry it); the failure
                    // replaces the screen.
                    if let Err(e) = &result {
                        fetch_error = Some(e.clone());
                    }
                    if let Ok(fetched) = &result {
                        *server = fetched.clone();
                    }
                    *detail = Some(result);
                }
                if let Some(result) = tier_job.as_ref().and_then(take) {
                    *tier_job = None;
                    // The tab's sliders take the ranges as they arrived; a draft opened
                    // before the answer gets its initial values now. `fetchTierConfiguration`'s
                    // failure replaces the screen.
                    if let Err(e) = &result {
                        fetch_error = Some(e.clone());
                    }
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
                            // Only a press on the region button opens the list; the
                            // fetch on entry just fills the tab's data.
                            if self.region_screen_requested {
                                self.region_screen_requested = false;
                                open_region_screen = true;
                            }
                        }
                        // `fetchRegionData`: the failure replaces the screen.
                        Err(e) => fetch_error = Some(e),
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
                            settings_saved = true;
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
                                players_scroll: crate::ui::Scroll::default(),
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
                        // `RealmsInviteScreen.onInvite`: any refusal reads as the fixed
                        // "no such player" text, not the service's own message.
                        Err(_) => {
                            *error = Some(crate::lang::get("mco.configure.world.players.error"))
                        }
                    }
                }
            }
            SubScreen::Backups {
                server,
                slot_id,
                backups,
                list_job,
                restore_job,
                download_task,
                ..
            } => {
                if let Some(result) = list_job.as_ref().and_then(take) {
                    *list_job = None;
                    // `fetchRealmsBackups`: a failure only logs; the list stays empty.
                    *backups = Some(result);
                }
                if let Some(task) = restore_job.as_ref()
                    && let Some(result) = take(&task.job)
                {
                    *restore_job = None;
                    match result {
                        // `RestoreTask`: the configuration screen again.
                        Ok(()) => back_to_worlds = Some(server.clone()),
                        Err(e) => restore_error = Some((server.clone(), e)),
                    }
                }
                if let Some(task) = download_task.as_ref()
                    && let Some(result) = take(&task.job)
                {
                    *download_task = None;
                    match result {
                        // `DownloadTask.run`: the download screen replaces the task screen.
                        Ok(download) => download_ready = Some((server.clone(), *slot_id, download)),
                        // A service error: `RealmsGenericErrorScreen(e, lastScreen)`.
                        Err(e) => download_error = Some((server.clone(), *slot_id, e)),
                    }
                }
            }
            SubScreen::DownloadLatest {
                server,
                slot_id,
                download,
                world_name,
                size_job,
                progress,
                job,
                error,
                done,
                started,
                ..
            } => {
                // `checkDownloadSize`: the HEAD runs once; an answer over the limit asks
                // (`warningAcknowledgePopupScreen`), any other starts `downloadSave`.
                if let Some(size) = size_job.as_ref().and_then(take) {
                    *size_job = None;
                    if size >= download::SIZE_LIMIT {
                        oversized_warning = Some((
                            server.clone(),
                            *slot_id,
                            download.clone(),
                            world_name.clone(),
                            *started,
                        ));
                    } else {
                        start_download =
                            Some((download.clone(), world_name.clone(), Arc::clone(progress)));
                    }
                }
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    *done = true;
                    if let Err(e) = result {
                        *error = Some(e);
                    }
                }
            }
            SubScreen::InviteCodes {
                server,
                codes,
                selected,
                list_job,
                job,
                ..
            } => {
                if let Some(result) = list_job.as_ref().and_then(take) {
                    *list_job = None;
                    match result {
                        Ok(list) => *codes = Some(Ok(list)),
                        // `handleFailure`: the failure replaced the screen with the generic
                        // error screen, whose next screen is the configuration screen.
                        Err(e) => codes_error = Some((server.clone(), e)),
                    }
                }
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        // A create or delete: the list reads again.
                        Ok(()) => refetch_codes = true,
                        // `handleFailure`: the generic error screen, returning to the
                        // codes list it was over.
                        Err(e) => {
                            codes_action_error =
                                Some((server.clone(), codes.clone(), *selected, e));
                        }
                    }
                }
            }
            SubScreen::ResetWorld {
                server,
                kind,
                templates,
                job,
                reset,
                ..
            } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    *templates = Some(result);
                }
                if let Some(result) = reset.as_ref().map(|task| &task.job).and_then(take) {
                    *reset = None;
                    match result {
                        // `resetWorldRunnable`: the configuration screen again, the list
                        // reloaded (`RealmsMainScreen.refreshServerList`); a new Realm goes
                        // back to the list it was created from.
                        Ok(()) => match kind {
                            ResetKind::NewRealm => {
                                created = true;
                            }
                            _ => {
                                refresh = true;
                                back_to_worlds = Some(server.clone());
                            }
                        },
                        Err(e) => task_error = Some(e.text()),
                    }
                }
            }
            SubScreen::TemplateList {
                server,
                templates,
                job,
                group,
                select,
                error,
                ..
            } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        // `fetchTemplatesAsync`: each page joins the list as it arrives;
                        // the next one is asked for while the answer is not the last page.
                        Ok((page, next_page, last)) => {
                            templates.extend(page);
                            if !last && let Some(client) = client() {
                                let world_type = group
                                    .map_or("MINIGAME", TemplateGroup::world_type)
                                    .to_owned();
                                *job = Some(spawn(move || {
                                    fetch_template_page(&client, &world_type, next_page, 10)
                                }));
                            }
                        }
                        // `fetchTemplatesAsync`: a page failure keeps the list and shows
                        // `mco.template.select.failure` when nothing arrived.
                        Err(e) => *error = Some(e.text()),
                    }
                }
                if let Some(result) = select.as_ref().map(|task| &task.job).and_then(take) {
                    *select = None;
                    match result {
                        // `SwitchMinigameTask`: the configuration screen again.
                        Ok(()) => back_to_worlds = Some(server.clone()),
                        Err(e) => task_error = Some(e.text()),
                    }
                }
            }
            SubScreen::AddRealm {
                trial, trial_job, ..
            } => {
                if let Some(result) = trial_job.as_ref().and_then(take) {
                    *trial_job = None;
                    // A failed ask leaves the note off, as `trialAvailable` staying false does.
                    *trial = result.unwrap_or(false);
                }
            }
            SubScreen::Uploading {
                job, error, done, ..
            } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    *done = true;
                    if let Err(e) = result {
                        *error = Some(e);
                    }
                }
            }
            SubScreen::CreateWorldUpload { request, job, .. } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    let request = request.clone();
                    *job = None;
                    match result {
                        // `RealmsCreateWorldFlow.createWorld`'s success: the return screen
                        // (the configuration screen) — or, for a Realm just created, it is
                        // played (`RealmsMainScreen.play(server, returnScreen, true)`).
                        Ok(()) => {
                            if request.creation.is_some() {
                                return self.start_join_after_create(request.server);
                            }
                            self.back_to_configure_worlds(request.server);
                        }
                        // `RealmsUploadCanceledException`: the reset screen it started from.
                        Err(upload::UploadError::Cancelled) => {
                            self.subscreen = Self::reset_screen(request);
                        }
                        // `RealmsGenericErrorScreen(mco.create.world.failed, lastScreen)`:
                        // the generic title over that message.
                        Err(_) => {
                            self.subscreen = SubScreen::Error {
                                title: crate::lang::get("mco.errorMessage.generic"),
                                detail: crate::lang::get("mco.create.world.failed"),
                                next: Box::new(Self::reset_screen(request)),
                            };
                        }
                    }
                }
            }
            SubScreen::EditCode {
                server,
                code,
                active,
                expiration,
                job,
                ..
            } => {
                if let Some(result) = job.as_ref().and_then(take) {
                    *job = None;
                    match result {
                        // The update or delete is saved: back to the list, which reads again.
                        Ok(()) => saved_code = true,
                        // `handleFailure`: the generic error screen, whose next is this
                        // edit screen again.
                        Err(e) => {
                            edit_error =
                                Some((server.clone(), code.clone(), *active, *expiration, e));
                        }
                    }
                }
            }
            SubScreen::Connecting { .. } => return self.poll_join(),
            SubScreen::Terms { .. } => return self.poll_terms(),
            _ => {}
        }
        if let Some(detail) = task_error {
            self.show_generic_error(detail);
        }
        if let Some((server, e)) = restore_error {
            let code = e.code();
            let detail = e.text();
            self.subscreen = SubScreen::Error {
                title: crate::lang::tr(
                    "mco.errorMessage.realmsService.realmsError",
                    &[&code as &dyn std::fmt::Display],
                ),
                detail,
                next: Box::new(configure_screen(server, ConfigureTab::Worlds)),
            };
        }
        if let Some((server, slot_id, e)) = download_error {
            // `DownloadTask.run`'s `RealmsServiceException`: the generic error screen's
            // next screen is the backup list it started from.
            let code = e.code();
            self.subscreen = SubScreen::Error {
                title: crate::lang::tr(
                    "mco.errorMessage.realmsService.realmsError",
                    &[&code as &dyn std::fmt::Display],
                ),
                detail: e.text(),
                next: Box::new(SubScreen::Backups {
                    server,
                    slot_id,
                    backups: None,
                    list_job: None,
                    restore_job: None,
                    download_task: None,
                    selected: None,
                    scroll: crate::ui::Scroll::default(),
                }),
            };
        }
        if let Some((server, slot_id, download, world_name, started)) = oversized_warning {
            // `warningAcknowledgePopupScreen(this, ...)` over the download screen.
            self.subscreen = SubScreen::Popup(Popup::DownloadOversized {
                server,
                slot_id,
                download,
                world_name,
                started,
            });
        }
        if let Some((download, world_name, progress)) = start_download {
            self.start_download(download, world_name, progress);
        }
        if let Some((server, slot_id, download)) = download_ready {
            // `DownloadTask.run`: `setScreen(new RealmsDownloadLatestWorldScreen(lastScreen,
            // worldDownload, downloadName, ...))`; its `init` checks the size.
            let world_name = Self::download_name(&server);
            let size_job = {
                let link = download.download_link.clone();
                spawn(move || download::content_length(&link))
            };
            self.subscreen = SubScreen::DownloadLatest {
                server,
                slot_id,
                download,
                world_name,
                size_job: Some(size_job),
                progress: Arc::new(download::DownloadProgress::default()),
                job: None,
                error: None,
                done: false,
                started: Instant::now(),
                speed: std::cell::Cell::new((0, Instant::now(), 0)),
            };
        }
        if let Some((server, e)) = codes_error {
            let code = e.code();
            let detail = e.text();
            self.subscreen = SubScreen::Error {
                title: crate::lang::tr(
                    "mco.errorMessage.realmsService.realmsError",
                    &[&code as &dyn std::fmt::Display],
                ),
                detail,
                next: Box::new(configure_screen(server, ConfigureTab::Players)),
            };
        }
        if let Some((server, code, active, expiration, e)) = edit_error {
            let error_code = e.code();
            let detail = e.text();
            self.subscreen = SubScreen::Error {
                title: crate::lang::tr(
                    "mco.errorMessage.realmsService.realmsError",
                    &[&error_code as &dyn std::fmt::Display],
                ),
                detail,
                next: Box::new(SubScreen::EditCode {
                    server,
                    code,
                    active,
                    expiration,
                    job: None,
                    error: None,
                }),
            };
        }
        if let Some((server, codes, selected, e)) = codes_action_error {
            let error_code = e.code();
            let detail = e.text();
            self.subscreen = SubScreen::Error {
                title: crate::lang::tr(
                    "mco.errorMessage.realmsService.realmsError",
                    &[&error_code as &dyn std::fmt::Display],
                ),
                detail,
                next: Box::new(SubScreen::InviteCodes {
                    server,
                    codes,
                    list_job: None,
                    job: None,
                    error: None,
                    selected,
                }),
            };
        }
        if settings_saved && let Some(action) = self.after_save.take() {
            let _ = self.run_after_save(action);
        }
        if refetch_codes {
            self.start_invite_codes();
        }
        if saved_code {
            self.back_to_codes();
        }
        if open_region_screen {
            self.open_region_preference();
        }
        if created {
            // `RealmsCreateRealmScreen.showResetWorldScreen`'s callback: the list again, read
            // from the service (`RealmsMainScreen.refreshServerList`).
            self.subscreen = SubScreen::Main;
            self.selected = None;
            self.refresh();
        }
        if let Some(e) = fetch_error {
            // `RealmsConfigureWorldScreen.createErrorScreen`: the fetch failure replaces
            // the screen; the list it was opened from returns.
            let code = e.code();
            self.subscreen = SubScreen::Error {
                title: crate::lang::tr(
                    "mco.errorMessage.realmsService.realmsError",
                    &[&code as &dyn std::fmt::Display],
                ),
                detail: e.text(),
                next: Box::new(SubScreen::Main),
            };
        }
        if let Some(server) = back_to_worlds {
            self.back_to_configure_worlds(server);
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
        let SubScreen::Connecting { server, last, .. } = self.subscreen.clone() else {
            return Outcome::None;
        };
        let address = match result {
            Ok(address) => address,
            Err(e) => {
                // 6002: a new account has not agreed to the Realms terms yet; vanilla
                // shows `RealmsTermsScreen` over the screen that started the join.
                if e.code() == 6002 {
                    self.subscreen = SubScreen::Terms {
                        server,
                        last,
                        agree: None,
                    };
                    return Outcome::None;
                }
                // 6006: the world is out of date; only its owner can reset it.
                let owner = session().is_some_and(|s| server.owned_by(s.uuid));
                let detail = if e.code() == 6006 && !owner {
                    format!(
                        "{} {}",
                        crate::lang::get("mco.brokenworld.nonowner.title"),
                        crate::lang::get("mco.brokenworld.nonowner.error")
                    )
                } else {
                    e.text()
                };
                // `LongRunningTask.error`: `RealmsGenericErrorScreen` replaces the screen.
                self.show_generic_error(detail);
                return Outcome::None;
            }
        };
        let Some(target) = address.address.as_deref().and_then(connect_address) else {
            log::warn!("Horizon: Realms gave no usable address to join");
            // `GetServerDetailsTask`: the failure replaces the connecting screen.
            self.show_generic_error(crate::lang::get("mco.errorMessage.connectionFailure"));
            return Outcome::None;
        };
        match (address.resource_pack_url, address.resource_pack_hash) {
            // `new URL(..)` fails in scheduleResourcePackDownload: the pack can't download.
            (Some(url), Some(_)) if !crate::server_packs::valid_url(&url) => {
                self.show_generic_error(crate::lang::get("mco.download.resourcePack.fail"));
                Outcome::None
            }
            (Some(url), Some(hash)) => {
                let pack = RealmsPack {
                    address: target,
                    id: pack_id(&server),
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

    /// `RealmsTermsScreen.agreedToTos`: the service's answer, then the join again.
    fn poll_terms(&mut self) -> Outcome {
        let result = {
            let SubScreen::Terms { agree, .. } = &self.subscreen else {
                return Outcome::None;
            };
            let Some(job) = agree else {
                return Outcome::None;
            };
            take(job)
        };
        let Some(result) = result else {
            return Outcome::None;
        };
        let SubScreen::Terms { server, last, .. } = self.subscreen.clone() else {
            return Outcome::None;
        };
        match result {
            // `RealmsTermsScreen.agreedToTos`: a new `GetServerDetailsTask` with the same
            // `lastScreen`.
            Ok(()) => self.start_join_from(server, last),
            // Vanilla logs the failure and stays on the terms screen.
            Err(e) => {
                log::warn!("Horizon: couldn't agree to the Realms terms: {}", e.text());
                self.subscreen = SubScreen::Terms {
                    server,
                    last,
                    agree: None,
                };
                Outcome::None
            }
        }
    }

    pub fn focused_box(&self, (w, h): (f32, f32)) -> Option<[f32; 4]> {
        match &self.subscreen {
            SubScreen::JoinCode { .. } => {
                // The field sits at the draw's content y + 13 (`AbstractRealmsCodeScreen`'s
                // contentY, min(63, H - 94)); a hard 76 diverged on short windows.
                let content_y = (h - 94.0).min(63.0);
                Some([(w / 2.0).floor() - 100.0, content_y + 13.0, 200.0, 20.0])
            }
            SubScreen::Invite { .. } => Some([(w / 2.0).floor() - 100.0, 76.0, 200.0, 20.0]),
            // `RealmsSelectFileToUploadScreen`: the 200-wide search box at header + 13.
            SubScreen::UploadSelect { .. } => Some([(w / 2.0).floor() - 100.0, 13.0, 200.0, 20.0]),
            SubScreen::CreateRealm { on_description, .. } => {
                let left = (w / 2.0).floor() - 105.0;
                let content_y = (h - 107.0).min(63.0);
                Some(if *on_description {
                    [left, content_y + 54.0, 210.0, 20.0]
                } else {
                    [left, content_y + 13.0, 210.0, 20.0]
                })
            }
            SubScreen::SlotOptions { server, .. } => {
                // The labelled 150-wide name field at content top + 13.
                let warning = matches!(
                    server.world_type.as_str(),
                    "ADVENTUREMAP" | "INSPIRATION" | "EXPERIENCE"
                );
                let content_top = if warning { 88.0 } else { 63.0 };
                Some([(w / 2.0).floor() - 75.0, content_top + 13.0, 150.0, 20.0])
            }
            SubScreen::Configure {
                tab: ConfigureTab::Settings,
                settings: Some(draft),
                tier,
                ..
            } => {
                let left = (w / 2.0).floor() - 106.0;
                let (rows, content_h) = settings_rows(matches!(tier, Some(Ok(_))));
                let top = settings_column_top(h, content_h);
                let max_scroll = (content_h - (h - 24.0 - 33.0)).max(0.0);
                let scroll = self.settings_scroll.amount.min(max_scroll);
                let row = usize::from(draft.on_description);
                Some([
                    left,
                    top + rows[row]
                        + SETTINGS_CELL_PADDING
                        + SETTINGS_LABEL_HEIGHT
                        + SETTINGS_LABEL_SPACING
                        - scroll,
                    212.0,
                    20.0,
                ])
            }
            _ => None,
        }
    }

    pub fn focused_text(&mut self) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
        match &mut self.subscreen {
            // `RealmsJoinRealmWithCodeScreen`: `setEditable(false)` while validating.
            SubScreen::JoinCode { code, job, .. } => {
                job.is_none().then_some((REALMS_INVITE_CODE, code, 32))
            }
            // `profileName.setEditable(false)` while the invite is in flight.
            SubScreen::Invite { name, job, .. } => {
                job.is_none().then_some((REALMS_PLAYER_NAME, name, 32))
            }
            SubScreen::CreateRealm {
                name,
                description,
                on_description,
                ..
            } => Some(if *on_description {
                (REALMS_CREATE_DESC, description, 32)
            } else {
                (REALMS_CREATE_NAME, name, 32)
            }),
            SubScreen::SlotOptions { name, .. } => Some((REALMS_SLOT_NAME, name, 32)),
            // `RealmsSelectFileToUploadScreen`'s search box (`EditBox` default 32).
            SubScreen::UploadSelect { query, .. } => Some((REALMS_UPLOAD_SEARCH, query, 32)),
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

    /// `TabNavigationBar.keyPressed` over the configuration screen's tab bar: with Ctrl
    /// held, a digit picks that tab (clamped to the last, as `Mth.clamp` does) and Tab
    /// steps to the next enabled one, or the previous with Shift. `None` when the key is
    /// not the bar's; otherwise whether a tab changed (the click sound).
    pub fn tab_key(
        &mut self,
        key: winit::keyboard::KeyCode,
        ctrl: bool,
        shift: bool,
    ) -> Option<bool> {
        use winit::keyboard::KeyCode;
        if !ctrl {
            return None;
        }
        let current = match &self.subscreen {
            SubScreen::Configure { tab, .. } => *tab,
            _ => return None,
        };
        let next = match key {
            KeyCode::Digit1 | KeyCode::Numpad1 => Some(0),
            KeyCode::Digit2 | KeyCode::Numpad2 => Some(1),
            KeyCode::Digit3 | KeyCode::Numpad3 => Some(2),
            KeyCode::Digit4 | KeyCode::Numpad4 => Some(3),
            KeyCode::Digit5 | KeyCode::Numpad5 => Some(4),
            KeyCode::Digit6 | KeyCode::Numpad6 => Some(5),
            KeyCode::Digit7 | KeyCode::Numpad7 => Some(6),
            KeyCode::Digit8 | KeyCode::Numpad8 => Some(7),
            KeyCode::Digit9 | KeyCode::Numpad9 => Some(8),
            KeyCode::Digit0 | KeyCode::Numpad0 => Some(9),
            KeyCode::Tab => {
                // `getNextTabIndex`'s cycle: skip the disabled tabs (an expired
                // Realm's settings tab).
                let step = if shift { -1 } else { 1 };
                let expired = match &self.subscreen {
                    SubScreen::Configure { server, .. } => server.expired,
                    _ => false,
                };
                let mut index = ConfigureTab::ALL
                    .iter()
                    .position(|tab| *tab == current)
                    .unwrap_or(0) as isize;
                for _ in 0..ConfigureTab::ALL.len() {
                    index = (index + step).rem_euclid(ConfigureTab::ALL.len() as isize);
                    if ConfigureTab::ALL[index as usize].enabled(expired) {
                        break;
                    }
                }
                Some(index as usize)
            }
            _ => None,
        };
        // `Mth.clamp(tabIndex, 0, this.tabs.size() - 1)`.
        let index = next?.min(ConfigureTab::ALL.len() - 1);
        let wanted = ConfigureTab::ALL[index];
        let expired = match &self.subscreen {
            SubScreen::Configure { server, .. } => server.expired,
            _ => false,
        };
        if !wanted.enabled(expired) || wanted == current {
            return Some(false);
        }
        // `onTabDeselected` saves an active settings tab before the new one opens.
        if current == ConfigureTab::Settings {
            self.save_settings_tab();
        }
        if let SubScreen::Configure {
            server,
            tab,
            settings,
            tier,
            ..
        } = &mut self.subscreen
            && wanted.enabled(server.expired)
        {
            // `RealmsConfigurationTab.onSelected` → `RealmsSettingsTab.updateData`: every
            // selection refills the draft from the service's Realm, not the stale one.
            if wanted == ConfigureTab::Settings {
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
            *tab = wanted;
            return Some(true);
        }
        Some(false)
    }

    /// `AbstractSliderButton.onDrag`: the settings tab's held distance slider follows the
    /// pointer while the button stays down.
    pub fn drag(&mut self, cursor: (f32, f32), (w, h): (f32, f32)) {
        // `ScrollableLayout`'s held scrollbar (the settings tab's column).
        if self.settings_scroll.held()
            && let SubScreen::Configure {
                tab: ConfigureTab::Settings,
                tier,
                ..
            } = &self.subscreen
        {
            let content = settings_rows(matches!(tier, Some(Ok(_)))).1;
            self.settings_scroll.drag(cursor.1, 24.0, h - 33.0, content);
            return;
        }
        if let Some(slider) = self.other_slider {
            match slider {
                // `RealmsSlotOptionsScreen.SettingsSlider`: 0..16 with step 1.
                1 => {
                    if let SubScreen::SlotOptions {
                        server,
                        spawn_protection,
                        ..
                    } = &mut self.subscreen
                        && server.world_type == "NORMAL"
                    {
                        let x = (w / 2.0).floor() + 4.0;
                        *spawn_protection =
                            (crate::ui::slider_value(x, 150.0, cursor.0) * 16.0).round() as i32;
                    }
                }
                // `RealmsEditInviteCodeScreen.ExpirationSlider`: one choice per step.
                2 => {
                    if let SubScreen::EditCode {
                        active: true,
                        job: None,
                        expiration,
                        ..
                    } = &mut self.subscreen
                    {
                        let left = (w / 2.0).floor() - 154.0;
                        let value = crate::ui::slider_value(left, 308.0, cursor.0);
                        *expiration = (value * (EXPIRATIONS.len() - 1) as f32).round() as usize;
                    }
                }
                _ => {}
            }
            return;
        }
        let Some(which) = self.settings_slider else {
            return;
        };
        let SubScreen::Configure {
            tab: ConfigureTab::Settings,
            settings: Some(draft),
            tier,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        let Some(Ok(config)) = tier.as_ref() else {
            return;
        };
        let range = match which {
            0 => config.render_distance.as_ref(),
            _ => config.sim_distance.as_ref(),
        };
        let Some(range) = range else {
            return;
        };
        let left = (w / 2.0).floor() - 106.0;
        let value = range.from_slider(crate::ui::slider_value(left, 212.0, cursor.0));
        if which == 0 {
            draft.render_distance = Some(value);
        } else {
            draft.simulation_distance = Some(value);
        }
    }

    /// `AbstractSliderButton.onRelease`.
    pub fn release(&mut self) {
        self.settings_slider = None;
        self.other_slider = None;
        self.settings_scroll.release();
    }

    /// Escape: a sub-screen goes back to the list (cancelling a join); the list goes back to
    /// the title (true). The reset screens keep vanilla's parents: the configuration screen
    /// for the reset screen, the reset screen for the two lists.
    pub fn escape(&mut self) -> bool {
        // `RealmsTermsScreen.keyPressed`: Escape is Disagree, the screen that asked returns.
        if let SubScreen::Terms { last, .. } = &self.subscreen {
            self.subscreen = *last.clone();
            return false;
        }
        // `ConfirmLinkScreen`'s no (Escape too) returns to the screen that asked.
        if let SubScreen::Popup(Popup::OpenLink { parent, .. }) = &self.subscreen {
            self.subscreen = *parent.clone();
            return false;
        }
        // `RealmsGenericErrorScreen.onClose`: the screen the error interrupted returns.
        if let SubScreen::Error { next, .. } = &self.subscreen {
            let next = *next.clone();
            self.restore_after_error(next);
            return false;
        }
        // `PopupScreen.onClose`: the notice is dismissed, the list behind it shows.
        if let SubScreen::Popup(Popup::Notice { uuid, .. }) = &self.subscreen {
            let uuid = uuid.clone();
            self.dismiss_notification(&uuid);
            self.subscreen = SubScreen::Main;
            return false;
        }
        // The close question's Escape is its Cancel: back to the settings tab with the
        // draft still in hand.
        if let SubScreen::Popup(Popup::CloseWorld {
            server,
            settings,
            detail,
            tier,
        }) = &self.subscreen
        {
            let (server, settings, detail, tier) = (
                server.clone(),
                settings.clone(),
                detail.clone(),
                tier.clone(),
            );
            self.back_to_settings(server, settings, detail, tier);
            return false;
        }
        // The restore question's Escape is its Cancel: the list reads again.
        if let SubScreen::Popup(Popup::RestoreBackup {
            server, slot_id, ..
        }) = &self.subscreen
        {
            let (server, slot_id) = (server.clone(), *slot_id);
            self.subscreen = SubScreen::Backups {
                server,
                slot_id,
                backups: None,
                list_job: None,
                restore_job: None,
                download_task: None,
                selected: None,
                scroll: crate::ui::Scroll::default(),
            };
            self.start_backups();
            return false;
        }
        // The download question's Escape is its Cancel: the list reads again.
        if let SubScreen::Popup(Popup::DownloadLatest {
            server, slot_id, ..
        }) = &self.subscreen
        {
            let (server, slot_id) = (server.clone(), *slot_id);
            self.back_to_backups(server, slot_id);
            return false;
        }
        // `warningAcknowledgePopupScreen`'s Escape closes back to the download screen,
        // whose `checked` flag then keeps it from downloading (vanilla's own stalemate).
        if let SubScreen::Popup(Popup::DownloadOversized {
            server,
            slot_id,
            download,
            world_name,
            started,
        }) = &self.subscreen
        {
            self.subscreen = SubScreen::DownloadLatest {
                server: server.clone(),
                slot_id: *slot_id,
                download: download.clone(),
                world_name: world_name.clone(),
                size_job: None,
                progress: Arc::new(download::DownloadProgress::default()),
                job: None,
                error: None,
                done: false,
                started: *started,
                speed: std::cell::Cell::new((0, Instant::now(), 0)),
            };
            return false;
        }
        // `RealmsDownloadLatestWorldScreen.onClose`: Escape cancels the download and the
        // backup list returns.
        if let SubScreen::DownloadLatest {
            server,
            slot_id,
            progress,
            ..
        } = &self.subscreen
        {
            progress.cancel();
            let (server, slot_id) = (server.clone(), *slot_id);
            self.back_to_backups(server, slot_id);
            return false;
        }
        // `RealmsBackupScreen.onClose` / `RealmsBackupInfoScreen.onClose`.
        if matches!(
            self.subscreen,
            SubScreen::Backups { .. } | SubScreen::BackupInfo { .. }
        ) {
            match &self.subscreen {
                SubScreen::Backups {
                    restore_job: Some(task),
                    ..
                } => task.abort.store(true, Ordering::Relaxed),
                SubScreen::Backups {
                    download_task: Some(task),
                    ..
                } => task.abort.store(true, Ordering::Relaxed),
                _ => {}
            }
            self.back_to_worlds_tab();
            return false;
        }
        // `RealmsEditInviteCodeScreen.onClose`: Back and Escape return to the codes list
        // while the back button is active (no request in flight).
        if matches!(self.subscreen, SubScreen::EditCode { job: None, .. }) {
            self.back_to_codes();
            return false;
        }
        // `PopupScreen.onClose`: the background screen returns. `RealmsPopups` builds the
        // configure screen's questions over it, so they go back there; the leave question
        // (`RealmsMainScreen.leaveClicked`) and the resource pack's (`GetServerDetailsTask`)
        // keep the list they were started from.
        if let SubScreen::Popup(popup) = &self.subscreen {
            let server = match popup {
                Popup::Delete(server) => Some(server.clone()),
                Popup::Uninvite { server, .. } => Some(server.clone()),
                Popup::SwitchSlot { server, .. } | Popup::SwitchEmpty { server, .. } => {
                    Some(server.clone())
                }
                _ => None,
            };
            if let Some(server) = server {
                self.back_to_configure_worlds(server);
                return false;
            }
        }
        enum Escape {
            Title,
            List,
            /// The reset screen rebuilt like `RealmsSelectWorldTemplateScreen.onClose`'s
            /// `callback.accept(null)`: `lastScreen` keeps its `realmCreationTask` field, so
            /// the creation rides along.
            Reset(RealmServer, i32, ResetKind, Option<(String, String)>),
            Configure(RealmServer),
            /// `RealmsLongRunningMcoTaskScreen.cancel`: the tasks abort, the screen the
            /// tasks were started from returns.
            CancelTask(RealmServer),
        }
        let action = match &self.subscreen {
            SubScreen::Main => Escape::Title,
            SubScreen::ResetWorld { server, reset, .. } => {
                if let Some(task) = reset {
                    task.abort.store(true, Ordering::Relaxed);
                    Escape::CancelTask(server.clone())
                } else {
                    Escape::Configure(server.clone())
                }
            }
            SubScreen::TemplateList {
                server,
                slot,
                kind,
                creation,
                group,
                select,
                ..
            } => {
                if let Some(task) = select {
                    // The minigame switch runs in the task screen: Escape cancels it.
                    task.abort.store(true, Ordering::Relaxed);
                    Escape::CancelTask(server.clone())
                } else if group.is_none() {
                    // `RealmsWorldsTab.switchToMinigame`'s callback: the list belongs to
                    // the configuration screen, not to a reset screen.
                    Escape::Configure(server.clone())
                } else {
                    Escape::Reset(server.clone(), *slot, *kind, creation.clone())
                }
            }
            SubScreen::UploadSelect {
                server,
                slot,
                kind,
                creation,
                ..
            } => Escape::Reset(server.clone(), *slot, *kind, creation.clone()),
            SubScreen::Uploading {
                server,
                progress,
                done,
                ..
            } => {
                if *done {
                    Escape::Configure(server.clone())
                } else {
                    // `RealmsUploadScreen.keyPressed`: Escape cancels the body.
                    progress.cancel();
                    return false;
                }
            }
            // `AlertScreen.shouldCloseOnEsc` is false: Escape does nothing while the
            // generated world uploads.
            SubScreen::CreateWorldUpload { .. } => return false,
            _ => Escape::List,
        };
        match action {
            Escape::Title => return true,
            Escape::List => self.back_to_list(),
            Escape::Reset(server, slot, kind, creation) => {
                self.subscreen = SubScreen::ResetWorld {
                    server,
                    slot,
                    kind,
                    creation,
                    templates: None,
                    job: None,
                    reset: None,
                    error: None,
                };
            }
            Escape::Configure(server) => self.back_to_configure_worlds(server),
            Escape::CancelTask(server) => self.back_to_configure_worlds(server),
        }
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
        // `RealmsMainScreen.play(server, cancelScreen)`: the screen that asked is the
        // one `RealmsTermsScreen` returns to.
        let last = Box::new(self.subscreen.clone());
        self.start_join_from(server, last)
    }

    /// `GetServerDetailsTask` from `last`, the screen its failures and the terms screen
    /// return to.
    fn start_join_from(&mut self, server: RealmServer, last: Box<SubScreen>) -> Outcome {
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
            last,
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
        self.start_configure_regions();
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

    /// `RealmsConfigureWorldScreen.init`'s `fetchRegionData`: the settings tab's region
    /// list, fetched on entry (not only when its button is pressed).
    fn start_configure_regions(&mut self) {
        let SubScreen::Configure {
            regions,
            regions_job,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        if regions.is_some() || regions_job.is_some() {
            return;
        }
        if let Some(client) = client() {
            *regions_job = Some(spawn(move || client.preferred_regions()));
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
    fn back_to_settings(
        &mut self,
        server: RealmServer,
        settings: SettingsDraft,
        detail: Option<Result<RealmServer, RealmsError>>,
        tier: Option<Result<RealmTierConfiguration, RealmsError>>,
    ) {
        self.subscreen = SubScreen::Configure {
            server,
            tab: ConfigureTab::Settings,
            subscription: None,
            subscription_job: None,
            detail,
            detail_job: None,
            deleting: None,
            member_job: None,
            tier,
            tier_job: None,
            settings: Some(settings),
            players_scroll: crate::ui::Scroll::default(),
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
    /// Whether a save request went out: the buttons that must wait for it
    /// (`Play`, the open/close toggle) continue immediately when nothing changed.
    fn save_settings_tab(&mut self) -> bool {
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
            return false;
        };
        if saving.is_some() {
            return false;
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
            return false;
        }
        let Some(Ok(detail)) = detail else {
            return false;
        };
        let Some(slot) = detail.slots.get(&detail.active_slot).cloned() else {
            return false;
        };
        let Some(client) = client() else {
            return false;
        };
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
        true
    }

    /// `RealmsSettingsTab.save`'s callers: run `action` after the save request, or at once
    /// when there is nothing to save.
    fn save_then(&mut self, action: AfterSave) -> Outcome {
        if self.save_settings_tab() {
            self.after_save = Some(action);
            Outcome::None
        } else {
            self.run_after_save(action)
        }
    }

    /// The action a completed save unlocks (`RealmsConfigureWorldScreen.closeTheWorld`,
    /// `openTheWorld` and Play).
    fn run_after_save(&mut self, action: AfterSave) -> Outcome {
        match action {
            AfterSave::Play(server) => self.start_join(server),
            AfterSave::Close(id) => {
                if let SubScreen::Configure { error, job, .. } = &mut self.subscreen
                    && job.is_none()
                    && let Some(client) = client()
                {
                    *error = None;
                    *job = Some(spawn(move || client.close_realm(id)));
                }
                Outcome::None
            }
            AfterSave::Open(id) => {
                if let SubScreen::Configure { error, job, .. } = &mut self.subscreen
                    && job.is_none()
                    && let Some(client) = client()
                {
                    *error = None;
                    *job = Some(spawn(move || client.open_realm(id)));
                }
                Outcome::None
            }
        }
    }

    /// `RealmsSettingsTab.openPreferenceSelector`, once the region list is there.
    fn open_region_preference(&mut self) {
        let SubScreen::Configure {
            server,
            settings: Some(draft),
            regions: Some(Ok(list)),
            detail,
            tier,
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
            scroll: crate::ui::Scroll::default(),
            detail: detail.clone(),
            tier: tier.clone(),
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
            players_scroll: crate::ui::Scroll::default(),
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
        // `saveSettings`: ADVENTUREMAP, EXPERIENCE and INSPIRATION keep the slot's own
        // spawn protection and force toggle (the screen built theirs as 0 and false for
        // everything else, MINIGAME included); the name saves verbatim — the default
        // name stores as empty (`setWorldName`), no trim.
        let keeps_own = matches!(
            server.world_type.as_str(),
            "ADVENTUREMAP" | "EXPERIENCE" | "INSPIRATION"
        );
        let default_name = crate::lang::tr(
            "mco.configure.world.slot",
            &[&slot.slot_id as &dyn std::fmt::Display],
        );
        let slot_name = if name == &default_name {
            String::new()
        } else {
            name.clone()
        };
        let updated = RealmSlot {
            slot_id: slot.slot_id,
            options: RealmWorldOptions {
                spawn_protection: if keeps_own {
                    slot.options.spawn_protection
                } else {
                    *spawn_protection
                },
                force_game_mode: if keeps_own {
                    slot.options.force_game_mode
                } else {
                    *force_game_mode
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

    /// `RealmsInviteCodesScreen.refreshInviteCodes`: the list reads again.
    fn start_invite_codes(&mut self) {
        let SubScreen::InviteCodes {
            server,
            list_job,
            selected,
            ..
        } = &mut self.subscreen
        else {
            return;
        };
        if list_job.is_some() {
            return;
        }
        // `refreshInviteCodes` clears the selection before the list reads again.
        *selected = None;
        if let Some(client) = client() {
            let id = server.id;
            *list_job = Some(spawn(move || client.invite_codes(id)));
        }
    }

    /// `RealmsEditInviteCodeScreen`'s save or delete done: back to the codes list, which
    /// reads again.
    fn back_to_codes(&mut self) {
        let SubScreen::EditCode { server, .. } = &self.subscreen else {
            return;
        };
        let server = server.clone();
        self.subscreen = SubScreen::InviteCodes {
            server,
            codes: None,
            list_job: None,
            job: None,
            error: None,
            selected: None,
        };
        self.start_invite_codes();
    }

    /// `RealmsBackupScreen.fetchRealmsBackups`: the list reads again.
    fn start_backups(&mut self) {
        let SubScreen::Backups {
            server, list_job, ..
        } = &mut self.subscreen
        else {
            return;
        };
        if list_job.is_some() {
            return;
        }
        if let Some(client) = client() {
            let id = server.id;
            *list_job = Some(spawn(move || client.backups_for(id)));
        }
    }

    /// `RestoreTask`: the restore on a worker; the Worlds tab returns when it is done.
    fn start_restore(&mut self, server: RealmServer, slot_id: i32, backup: Backup) {
        let Some(client) = client() else {
            return;
        };
        let abort = Arc::new(AtomicBool::new(false));
        let job = {
            let abort = Arc::clone(&abort);
            let realm_id = server.id;
            let backup_id = backup.backup_id;
            spawn(move || {
                // `RestoreTask.run`: up to 25 tries, the service's `Retry-After` between
                // them, `pause(1L)` before the screen the restore started from returns.
                for _ in 0..25 {
                    if abort.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    match client.restore_world(realm_id, &backup_id) {
                        Ok(()) => {
                            thread::sleep(Duration::from_secs(1));
                            return Ok(());
                        }
                        Err(RealmsError::Retry(seconds)) => {
                            thread::sleep(Duration::from_secs(seconds));
                        }
                        Err(e) => return Err(e),
                    }
                }
                // Vanilla's loop just ends here; report the busy service instead of
                // leaving the task screen up forever.
                Err(RealmsError::Timeout)
            })
        };
        self.subscreen = SubScreen::Backups {
            server,
            slot_id,
            backups: None,
            list_job: None,
            restore_job: Some(RestoreTask { job, abort }),
            download_task: None,
            selected: None,
            scroll: crate::ui::Scroll::default(),
        };
    }

    /// `RealmsBackupScreen.onClose`: back to the Worlds tab.
    fn back_to_worlds_tab(&mut self) {
        let server = match &self.subscreen {
            SubScreen::Backups { server, .. } | SubScreen::BackupInfo { server, .. } => {
                server.clone()
            }
            _ => return,
        };
        self.back_to_configure_worlds(server);
    }

    /// `RealmsBackupScreen.downloadClicked`'s `downloadName`: the Realm's name and its
    /// active slot's name (`Objects.requireNonNullElse(name, "") + " (" + getSlotName + ")"`).
    fn download_name(server: &RealmServer) -> String {
        let slot = server.active_slot;
        match server.slots.get(&slot) {
            Some(slot_data) => format!("{} ({})", server.name, slot_name(&slot_data.options, slot)),
            // The detail answer carries the slots; without it only the Realm's name.
            None => server.name.clone(),
        }
    }

    /// `DownloadTask`: `RealmsClient.requestDownloadInfo` up to 25 times, waiting the
    /// service's `Retry-After` between tries (`LongRunningTask.pause`); the task screen
    /// shows `mco.download.preparing` until the links are in.
    fn start_download_task(&mut self, server: RealmServer, slot_id: i32) {
        let Some(client) = client() else {
            return;
        };
        let abort = Arc::new(AtomicBool::new(false));
        let job = {
            let abort = Arc::clone(&abort);
            let realm_id = server.id;
            spawn(move || {
                for _ in 0..25 {
                    if abort.load(Ordering::Relaxed) {
                        return Ok(WorldDownload::default());
                    }
                    match client.request_download_info(realm_id, slot_id) {
                        Ok(download) => {
                            // `DownloadTask.run`'s `pause(1L)` before the screen change.
                            thread::sleep(Duration::from_secs(1));
                            if abort.load(Ordering::Relaxed) {
                                return Ok(WorldDownload::default());
                            }
                            return Ok(download);
                        }
                        Err(RealmsError::Retry(seconds)) => {
                            thread::sleep(Duration::from_secs(seconds.min(60)));
                        }
                        Err(e) => return Err(e),
                    }
                }
                // Vanilla's loop just ends here; the task screen would stay up forever.
                Err(RealmsError::Timeout)
            })
        };
        if let SubScreen::Backups { download_task, .. } = &mut self.subscreen {
            *download_task = Some(DownloadTask { job, abort });
        }
    }

    /// `RealmsDownloadLatestWorldScreen.downloadSave`: `FileDownload` on a worker.
    fn start_download(
        &mut self,
        download: WorldDownload,
        world_name: String,
        progress: Arc<download::DownloadProgress>,
    ) {
        let Some(saves) = self.saves.clone() else {
            if let SubScreen::DownloadLatest { error, done, .. } = &mut self.subscreen {
                *error = Some(download::DownloadError::Failed(
                    "the game has no saves folder".into(),
                ));
                *done = true;
            }
            return;
        };
        let job =
            spawn(move || download::download_world(&download, &world_name, &saves, &progress));
        if let SubScreen::DownloadLatest { job: slot, .. } = &mut self.subscreen {
            *slot = Some(job);
        }
    }

    /// `RealmsBackupScreen` again: every download screen's `lastScreen`, re-initialised so
    /// the list reads again (`Gui.setScreen` calls `init`).
    fn back_to_backups(&mut self, server: RealmServer, slot_id: i32) {
        self.subscreen = SubScreen::Backups {
            server,
            slot_id,
            backups: None,
            list_job: None,
            restore_job: None,
            download_task: None,
            selected: None,
            scroll: crate::ui::Scroll::default(),
        };
        self.start_backups();
    }

    /// `warningAcknowledgePopupScreen`'s OK: `setScreen(this)`, then `downloadSave`.
    fn download_oversized_ok(&mut self) {
        let SubScreen::Popup(Popup::DownloadOversized {
            server,
            slot_id,
            download,
            world_name,
            started,
        }) = &self.subscreen
        else {
            return;
        };
        let (server, slot_id, download, world_name, started) = (
            server.clone(),
            *slot_id,
            download.clone(),
            world_name.clone(),
            *started,
        );
        let progress = Arc::new(download::DownloadProgress::default());
        self.subscreen = SubScreen::DownloadLatest {
            server,
            slot_id,
            download: download.clone(),
            world_name: world_name.clone(),
            size_job: None,
            progress: Arc::clone(&progress),
            job: None,
            error: None,
            done: false,
            started,
            speed: std::cell::Cell::new((0, Instant::now(), 0)),
        };
        self.start_download(download, world_name, progress);
    }

    /// Back to the players tab from the codes screens.
    fn back_to_players_tab(&mut self) {
        let server = match &self.subscreen {
            SubScreen::InviteCodes { server, .. } | SubScreen::EditCode { server, .. } => {
                server.clone()
            }
            _ => return,
        };
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
            players_scroll: crate::ui::Scroll::default(),
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
            players_scroll: crate::ui::Scroll::default(),
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

    /// `RealmsConfigureWorldScreen` with the worlds tab, its data read again: the screen a
    /// finished reset (or upload) leaves (`resetWorldRunnable`, `getNewScreen`).
    fn back_to_configure_worlds(&mut self, server: RealmServer) {
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
            players_scroll: crate::ui::Scroll::default(),
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

    /// `RealmsWorldsTab.resetButtonPressed`: a minigame Realm picks another template, any other
    /// Realm replaces the active slot's world.
    fn reset_button(&mut self) {
        let SubScreen::Configure {
            server,
            tab: ConfigureTab::Worlds,
            detail: Some(Ok(detail)),
            ..
        } = &self.subscreen
        else {
            return;
        };
        let server = server.clone();
        let slot = server.active_slot;
        if server.world_type == "MINIGAME" {
            self.open_template_list(server, slot, ResetKind::Reset, None, None);
            return;
        }
        let empty = detail
            .slots
            .get(&detail.active_slot)
            .is_none_or(|slot| slot.options.empty);
        let kind = if empty {
            ResetKind::Empty
        } else {
            ResetKind::Reset
        };
        self.open_reset_world(server, slot, kind);
    }

    /// `new RealmsResetWorldScreen`: `init` asks the service for page 1 of each group.
    fn open_reset_world(&mut self, server: RealmServer, slot: i32, kind: ResetKind) {
        let job = client().map(|client| {
            spawn(move || {
                [
                    client.fetch_world_templates(1, 10, "NORMAL"),
                    client.fetch_world_templates(1, 10, "ADVENTUREMAP"),
                    client.fetch_world_templates(1, 10, "EXPERIENCE"),
                    client.fetch_world_templates(1, 10, "INSPIRATION"),
                ]
            })
        });
        self.subscreen = SubScreen::ResetWorld {
            server,
            slot,
            kind,
            creation: None,
            templates: None,
            job,
            reset: None,
            error: None,
        };
    }

    /// `new RealmsSelectWorldTemplateScreen`: its first page (the reset screen's when it has
    /// one) and the rest of the pages, then the list.
    fn open_template_list(
        &mut self,
        server: RealmServer,
        slot: i32,
        kind: ResetKind,
        creation: Option<(String, String)>,
        group: Option<TemplateGroup>,
    ) {
        let prefetched = match (&self.subscreen, group) {
            (
                SubScreen::ResetWorld {
                    templates: Some(lists),
                    ..
                },
                Some(wanted),
            ) => lists[wanted.index()].clone().ok(),
            _ => None,
        };
        let (templates, next_page, page_size) = match prefetched {
            Some(page) => (page.templates, page.page + 1, page.size.max(10)),
            // `new WorldTemplatePaginatedList(10)`: page 0 of ten, so page 1 is next.
            None => (Vec::new(), 1, 10),
        };
        let job = client().map(|client| {
            let world_type = group
                .map_or("MINIGAME", TemplateGroup::world_type)
                .to_owned();
            spawn(move || fetch_template_page(&client, world_type.as_str(), next_page, page_size))
        });
        self.subscreen = SubScreen::TemplateList {
            server,
            slot,
            kind,
            creation,
            group,
            templates,
            selected: None,
            scroll: crate::ui::Scroll::default(),
            job,
            select: None,
            icons: std::cell::RefCell::new(Vec::new()),
            error: None,
        };
    }

    /// `RealmsSelectWorldTemplateScreen.selectTemplate`: the minigame task, or the reset.
    /// `RealmsSelectFileToUploadScreen.upload`: the selected row of the filtered list
    /// opens the upload screen (the button and a row's Enter both run it).
    fn upload_selected(&mut self) {
        let SubScreen::UploadSelect {
            server,
            slot,
            creation,
            worlds,
            selected,
            query,
            ..
        } = &self.subscreen
        else {
            return;
        };
        let Some(world) = selected
            .and_then(|i| {
                worlds
                    .iter()
                    .filter(|world| upload_matches(world, query))
                    .nth(i)
            })
            .cloned()
        else {
            return;
        };
        let (server, slot) = (server.clone(), *slot);
        let creation = creation.clone();
        self.start_upload(server, slot, world, creation);
    }

    fn select_template(&mut self) {
        let t = crate::lang::get;
        let SubScreen::TemplateList {
            server,
            slot,
            kind,
            creation,
            group,
            templates,
            selected,
            ..
        } = &self.subscreen
        else {
            return;
        };
        let Some(template) = selected.and_then(|i| templates.get(i)).cloned() else {
            return;
        };
        let (server, slot, kind, group) = (server.clone(), *slot, *kind, *group);
        let creation = creation.clone();
        if group.is_none() {
            // `SwitchMinigameTask`: `putIntoMinigameMode`, then the configuration screen; the
            // task asks again when the service says no (`for i = 0; i < 25`).
            let task = client().map(|client| {
                let (realm_id, minigame) = (server.id, template.id.clone());
                let title = Arc::new(Mutex::new(t("mco.minigame.world.starting.screen.title")));
                let abort = Arc::new(AtomicBool::new(false));
                let job = {
                    let abort = Arc::clone(&abort);
                    spawn(move || {
                        for _ in 0..25 {
                            if abort.load(Ordering::Relaxed) {
                                return Ok(());
                            }
                            if client.put_into_minigame_mode(realm_id, &minigame)? {
                                break;
                            }
                        }
                        Ok(())
                    })
                };
                RunningTask { job, title, abort }
            });
            if let SubScreen::TemplateList { select, .. } = &mut self.subscreen {
                *select = task;
            }
            return;
        }
        // `ResettingTemplateWorldTask`: the reset screen again, `runResetTasks`' task list
        // running in a `RealmsLongRunningMcoTaskScreen` whose title follows each task.
        let first = creation
            .as_ref()
            .map_or_else(|| t(kind.task_title()), |_| t("mco.create.world.wait"));
        let task = client().map(|client| {
            let (realm_id, active, template_id) = (server.id, server.active_slot, template.id);
            let title = Arc::new(Mutex::new(first));
            let abort = Arc::new(AtomicBool::new(false));
            let job = {
                let (title, abort) = (Arc::clone(&title), Arc::clone(&abort));
                let creation = creation.clone();
                spawn(move || {
                    // `runResetTasks`: `RealmCreationTask` runs before the slot switch and
                    // the reset, then `SwitchSlotTask` when the slot is not the active one.
                    if let Some((name, description)) = creation.as_ref() {
                        client.initialize_realm(realm_id, name, description)?;
                    }
                    if abort.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    if slot != active {
                        *title.lock().expect("a task title") =
                            t("mco.minigame.world.slot.screen.title");
                        client.switch_slot(realm_id, slot)?;
                        if abort.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                    }
                    *title.lock().expect("a task title") = t(kind.task_title());
                    let template_id = template_id.parse::<i64>().map_err(|_| RealmsError::Parse)?;
                    client
                        .reset_world_with_template(realm_id, template_id)
                        .map(drop)
                })
            };
            RunningTask { job, title, abort }
        });
        self.subscreen = SubScreen::ResetWorld {
            server,
            slot,
            kind,
            creation: creation.clone(),
            templates: None,
            job: None,
            reset: task,
            error: None,
        };
    }

    /// `RealmsUploadScreen`: the local world's folder on a worker, packed and posted.
    fn start_upload(
        &mut self,
        server: RealmServer,
        slot: i32,
        world: crate::title::WorldEntry,
        creation: Option<(String, String)>,
    ) {
        let Some(saves) = self.saves.clone() else {
            return;
        };
        let progress = Arc::new(upload::UploadProgress::default());
        let job = client().map(|client| {
            let progress = Arc::clone(&progress);
            let active = server.active_slot;
            let options =
                RealmWorldOptions {
                    // `RealmsWorldOptions.createFromSettings`: the world's own settings, its saved
                    // version as the world version.
                    spawn_protection: 0,
                    force_game_mode: false,
                    difficulty: crate::save::Level::read_summary(&saves.join(&world.folder))
                        .map_or(2, |level| {
                            crate::title::DIFFICULTIES
                                .iter()
                                .position(|d| *d == level.difficulty)
                                .unwrap_or(2) as i32
                        }),
                    game_mode: world.game_mode,
                    slot_name: world.name.clone(),
                    version: world.version.clone(),
                    compatibility: "UNVERIFIABLE".into(),
                    template_id: -1,
                    template_image: None,
                    empty: false,
                };
            let realm_slot = RealmSlot {
                slot_id: slot,
                options,
                settings: vec![RealmSetting {
                    name: "hardcore".into(),
                    value: world.hardcore.to_string(),
                }],
            };
            let world_dir = saves.join(&world.folder);
            spawn(move || {
                // `RealmsUploadScreen.init`'s tasks: `RealmCreationTask` (initializeRealm) runs
                // first, then the slot switch, then the upload itself.
                if let Some((name, description)) = creation.as_ref() {
                    client
                        .initialize_realm(server.id, name, description)
                        .map_err(|e| upload::UploadError::Failed(e.text()))?;
                }
                // `RealmsUploadScreen.init`: `SwitchSlotTask` first, when the slot isn't active.
                if slot != active {
                    client
                        .switch_slot(server.id, slot)
                        .map_err(|e| upload::UploadError::Failed(e.text()))?;
                }
                upload::upload_world(&client, server.id, &realm_slot, &world_dir, &progress)
            })
        });
        self.subscreen = SubScreen::Uploading {
            server,
            slot,
            world,
            progress,
            job,
            error: None,
            done: false,
            started: Instant::now(),
        };
    }

    /// `RealmsCreateWorldFlow.createWorld`: the Create New World screen's settings become
    /// a temporary world (`createTemporaryWorldFolder`) that is packed and uploaded into
    /// the slot; `AlertScreen` (`mco.create.world.reset.title`) shows while it runs.
    pub fn start_create_world_upload(&mut self, request: CreateWorldRequest, world: CreatedWorld) {
        let progress = Arc::new(upload::UploadProgress::default());
        let job = client().map(|client| {
            let progress = Arc::clone(&progress);
            let server = request.server.clone();
            let slot = request.slot;
            let creation = request.creation.clone();
            spawn(move || {
                upload_created_world(&client, &server, slot, creation.as_ref(), &world, &progress)
            })
        });
        if job.is_none() {
            // The upload cannot run without the account's session: the same failure the
            // service's refusal would show, back to the reset screen.
            self.subscreen = SubScreen::Error {
                title: crate::lang::get("mco.errorMessage.generic"),
                detail: crate::lang::get("mco.create.world.failed"),
                next: Box::new(Self::reset_screen(request)),
            };
            return;
        }
        self.subscreen = SubScreen::CreateWorldUpload {
            request,
            progress,
            job,
        };
    }

    /// The Create New World screen's Back for the Generate New World tile
    /// (`CreateWorldScreen.openFresh`'s cancel): the screen the reset screen was opened
    /// from — the configuration screen, or the list for a Realm being created.
    pub fn cancel_create_world(&mut self, request: CreateWorldRequest) {
        match request.kind {
            ResetKind::NewRealm => {
                self.subscreen = SubScreen::Main;
                self.selected = None;
                self.refresh();
            }
            ResetKind::Reset | ResetKind::Empty => self.back_to_configure_worlds(request.server),
        }
    }

    /// `RealmsResetWorldScreen` again, as the reset's own tasks return to it.
    fn reset_screen(request: CreateWorldRequest) -> SubScreen {
        SubScreen::ResetWorld {
            server: request.server,
            slot: request.slot,
            kind: request.kind,
            creation: request.creation,
            templates: None,
            job: None,
            reset: None,
            error: None,
        }
    }

    /// `RealmsMainScreen.play(server, returnScreen, true)` for a Realm that was just
    /// initialized: the address request without the list's Play gate (the list has not
    /// reloaded the Realm yet).
    fn start_join_after_create(&mut self, server: RealmServer) -> Outcome {
        let last = Box::new(self.subscreen.clone());
        self.start_join_from(server, last)
    }

    /// `new RealmsSelectFileToUploadScreen`: the game's own worlds (`WorldSelectionList`), as the
    /// title screen lists them.
    fn open_upload_select(
        &mut self,
        server: RealmServer,
        slot: i32,
        kind: ResetKind,
        creation: Option<(String, String)>,
    ) {
        let worlds = self
            .saves
            .as_deref()
            .map(crate::title::list_worlds)
            .unwrap_or_default();
        self.subscreen = SubScreen::UploadSelect {
            server,
            slot,
            kind,
            creation,
            worlds,
            selected: None,
            scroll: crate::ui::Scroll::default(),
            query: String::new(),
        };
    }

    pub fn click(&mut self, hit: Option<u8>, x: f32, y: f32, (w, h): (f32, f32)) -> Outcome {
        self.list_clicked = false;
        // `RealmsTermsScreen.mouseClicked`: a click on the underlined link opens
        // `CommonLinks.REALMS_TERMS` (the draw recorded the text's box).
        if hit.is_none() && matches!(self.subscreen, SubScreen::Terms { .. }) {
            let [x1, y1, x2, y2] = self.terms_link.get();
            if x1 <= x && x <= x2 && y1 <= y && y <= y2 {
                return Outcome::OpenUri(REALMS_TERMS_URL.to_owned());
            }
        }
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
                        players_scroll: crate::ui::Scroll::default(),
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
            Some(11) => {
                // RealmsMainScreen.onRenew: `ConfirmLinkScreen.confirmLink`.
                if let (Some(server), Some(s)) = (self.selected_server().cloned(), session()) {
                    self.open_link(extend_link(&server, s.uuid, true));
                }
                Outcome::None
            }
            Some(12) => {
                self.leave_clicked(session().map(|s| s.uuid));
                Outcome::None
            }
            Some(13) => {
                self.open_pdp();
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
                self.subscreen = SubScreen::PendingInvites {
                    invites: None,
                    job,
                    scroll: crate::ui::Scroll::default(),
                    selected: None,
                };
                Outcome::None
            }
            Some(16) => {
                // `RealmsHeader.openNews`: the link, then the read state saved.
                if let Some(link) = self.news_link.clone() {
                    self.news_unread = false;
                    self.write_news_persistence(self.news_link.as_deref(), false);
                    self.open_link(link);
                }
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
            // `PopupScreen.onClose`'s Cancel: the background screen returns — the
            // configure screen for the questions `RealmsPopups` builds over it, the list
            // for the leave and resource pack questions.
            Some(51) => {
                // The restore question's Cancel returns to the list it asked over.
                let restore = match &self.subscreen {
                    SubScreen::Popup(Popup::RestoreBackup {
                        server, slot_id, ..
                    }) => Some((server.clone(), *slot_id)),
                    _ => None,
                };
                if let Some((server, slot_id)) = restore {
                    self.subscreen = SubScreen::Backups {
                        server,
                        slot_id,
                        backups: None,
                        list_job: None,
                        restore_job: None,
                        download_task: None,
                        selected: None,
                        scroll: crate::ui::Scroll::default(),
                    };
                    self.start_backups();
                    return Outcome::None;
                }
                // The download question's Cancel returns to the list it asked over.
                let download = match &self.subscreen {
                    SubScreen::Popup(Popup::DownloadLatest {
                        server, slot_id, ..
                    }) => Some((server.clone(), *slot_id)),
                    _ => None,
                };
                if let Some((server, slot_id)) = download {
                    self.back_to_backups(server, slot_id);
                    return Outcome::None;
                }
                // The close question returns to the tab with its draft, not to a
                // rebuilt tab: the settings are still the player's unsaved edits.
                let close_world = match &self.subscreen {
                    SubScreen::Popup(Popup::CloseWorld {
                        server,
                        settings,
                        detail,
                        tier,
                    }) => Some((
                        server.clone(),
                        settings.clone(),
                        detail.clone(),
                        tier.clone(),
                    )),
                    _ => None,
                };
                if let Some((server, settings, detail, tier)) = close_world {
                    self.back_to_settings(server, settings, detail, tier);
                    return Outcome::None;
                }
                let server = match &self.subscreen {
                    SubScreen::Popup(Popup::Delete(server)) => Some(server.clone()),
                    SubScreen::Popup(Popup::Uninvite { server, .. }) => Some(server.clone()),
                    SubScreen::Popup(Popup::SwitchSlot { server, .. })
                    | SubScreen::Popup(Popup::SwitchEmpty { server, .. }) => Some(server.clone()),
                    _ => None,
                };
                if let Some(server) = server {
                    self.back_to_configure_worlds(server);
                } else {
                    self.back_to_list();
                }
                Outcome::None
            }
            Some(21) | Some(22) | Some(32) | Some(41) | Some(52) => {
                self.back_to_list();
                Outcome::None
            }
            Some(30) => {
                // `RealmsSettingsTab.closeOpenButton`: a close asks first (`RealmsPopups`
                // custom popup), and both answers save the tab before the request.
                let state = match &self.subscreen {
                    SubScreen::Configure {
                        server,
                        tab: ConfigureTab::Settings,
                        settings: Some(draft),
                        detail,
                        tier,
                        ..
                    } => Some((server.clone(), draft.clone(), detail.clone(), tier.clone())),
                    _ => None,
                };
                let Some((server, settings, detail, tier)) = state else {
                    return Outcome::None;
                };
                if server.is_open() {
                    self.subscreen = SubScreen::Popup(Popup::CloseWorld {
                        server,
                        settings,
                        detail,
                        tier,
                    });
                } else {
                    return self.save_then(AfterSave::Open(server.id));
                }
                Outcome::None
            }
            // The configuration screen's tabs (60..=63) and its footer: Play (64).
            // RealmsWorldsTab's Backup button: the backup list.
            Some(170) => {
                if let SubScreen::Configure {
                    server,
                    detail: Some(Ok(detail)),
                    ..
                } = &self.subscreen
                    && let Some(client) = client()
                {
                    let (server, slot_id, id) = (detail.clone(), detail.active_slot, server.id);
                    self.subscreen = SubScreen::Backups {
                        server,
                        slot_id,
                        backups: None,
                        list_job: Some(spawn(move || client.backups_for(id))),
                        restore_job: None,
                        download_task: None,
                        selected: None,
                        scroll: crate::ui::Scroll::default(),
                    };
                }
                Outcome::None
            }
            // RealmsBackupScreen: a row's Changes (210 + 2i) and Restore (211 + 2i).
            Some(btn @ 210..=249) => {
                let picked = match &self.subscreen {
                    SubScreen::Backups {
                        server,
                        slot_id,
                        backups: Some(Ok(list)),
                        restore_job,
                        selected,
                        scroll,
                        ..
                    } => {
                        let row = usize::from((btn - 210) / 2);
                        let restore = (btn - 210) % 2 == 1;
                        list.get(row).cloned().map(|backup| {
                            (
                                server.clone(),
                                *slot_id,
                                backup,
                                restore && restore_job.is_none(),
                                list.clone(),
                                *selected,
                                *scroll,
                            )
                        })
                    }
                    _ => None,
                };
                let Some((server, slot_id, backup, restore, list, selected, list_scroll)) = picked
                else {
                    return Outcome::None;
                };
                if restore && !server.expired {
                    // `restoreClicked`: `RealmsPopups.warningPopupScreen` first.
                    self.subscreen = SubScreen::Popup(Popup::RestoreBackup {
                        server,
                        slot_id,
                        backup,
                        list,
                        selected,
                    });
                } else if !restore {
                    // `Entry.populateChangeList` filled the backup's change list when the
                    // list was built; the info screen reads that, not the metadata.
                    let changes = backup_changes(&list);
                    let row = usize::from((btn - 210) / 2);
                    let mut backup = backup;
                    if let Some(changes) = changes.get(row) {
                        backup.change_list = changes.clone();
                        backup.uploaded_version = changes.contains_key("uploaded");
                    }
                    self.subscreen = SubScreen::BackupInfo {
                        server,
                        slot_id,
                        backup,
                        list,
                        selected,
                        list_scroll,
                        scroll: crate::ui::Scroll::default(),
                    };
                }
                Outcome::None
            }
            Some(190) => {
                self.back_to_worlds_tab();
                Outcome::None
            }
            // `RealmsBackupScreen.downloadClicked`: the info popup, over the list.
            Some(192) => {
                let picked = match &self.subscreen {
                    SubScreen::Backups {
                        server,
                        slot_id,
                        backups: Some(Ok(list)),
                        selected,
                        ..
                    } => Some((server.clone(), *slot_id, list.clone(), *selected)),
                    _ => None,
                };
                if let Some((server, slot_id, list, selected)) = picked {
                    self.subscreen = SubScreen::Popup(Popup::DownloadLatest {
                        server,
                        slot_id,
                        list,
                        selected,
                    });
                }
                Outcome::None
            }
            // `RealmsLongRunningMcoTaskScreen`'s Cancel while a restore or the download's
            // prepare request runs: the task aborts, the Worlds tab returns.
            Some(193) => {
                match &self.subscreen {
                    SubScreen::Backups {
                        restore_job: Some(task),
                        ..
                    } => task.abort.store(true, Ordering::Relaxed),
                    SubScreen::Backups {
                        download_task: Some(task),
                        ..
                    } => task.abort.store(true, Ordering::Relaxed),
                    _ => {}
                }
                self.back_to_worlds_tab();
                Outcome::None
            }
            // `warningAcknowledgePopupScreen`'s OK: the warning was accepted.
            Some(194) => {
                self.download_oversized_ok();
                Outcome::None
            }
            // `RealmsDownloadLatestWorldScreen.onClose`: Cancel stops the download (or Done
            // closes the finished screen); the backup list returns and reads again.
            Some(195) => {
                if let SubScreen::DownloadLatest {
                    server,
                    slot_id,
                    progress,
                    ..
                } = &self.subscreen
                {
                    progress.cancel();
                    let (server, slot_id) = (server.clone(), *slot_id);
                    self.back_to_backups(server, slot_id);
                }
                Outcome::None
            }
            Some(191) => {
                // `RealmsBackupInfoScreen.onClose`: `lastScreen` is the same list, with
                // its rows, selection and scroll.
                if let SubScreen::BackupInfo {
                    server,
                    slot_id,
                    list,
                    selected,
                    list_scroll,
                    ..
                } = &self.subscreen
                {
                    let (server, slot_id, list, selected, list_scroll) = (
                        server.clone(),
                        *slot_id,
                        list.clone(),
                        *selected,
                        *list_scroll,
                    );
                    self.subscreen = SubScreen::Backups {
                        server,
                        slot_id,
                        backups: Some(Ok(list)),
                        list_job: None,
                        restore_job: None,
                        download_task: None,
                        selected,
                        scroll: list_scroll,
                    };
                }
                Outcome::None
            }
            // RealmsWorldsTab's reset/new-world button.
            Some(90) => {
                self.reset_button();
                Outcome::None
            }
            // `RealmsResetWorldScreen.init`'s tiles: Generate New World (70), Upload (71)
            // and the four template groups (72..=75).
            Some(tile @ 70..=75) => {
                if let SubScreen::ResetWorld {
                    server,
                    slot,
                    kind,
                    creation,
                    reset: None,
                    ..
                } = &self.subscreen
                {
                    let (server, slot, kind) = (server.clone(), *slot, *kind);
                    let creation = creation.clone();
                    if tile == 70 {
                        // `RealmsCreateWorldFlow.createWorld`: the title opens its Create
                        // New World screen; the settings come back for the upload.
                        return Outcome::CreateWorld(CreateWorldRequest {
                            server,
                            slot,
                            kind,
                            creation,
                        });
                    }
                    if tile == 71 {
                        self.open_upload_select(server, slot, kind, creation);
                    } else if let Some(group) = TemplateGroup::ALL.get(usize::from(tile - 72)) {
                        self.open_template_list(server, slot, kind, creation, Some(*group));
                    }
                }
                Outcome::None
            }
            // `RealmsTermsScreen.init`: Agree posts the terms, then the join runs again.
            Some(23) => {
                if let SubScreen::Terms { agree, .. } = &mut self.subscreen
                    && agree.is_none()
                    && let Some(client) = client()
                {
                    *agree = Some(spawn(move || client.agree_to_tos()));
                }
                Outcome::None
            }
            // `RealmsTermsScreen.init`: Disagree is the screen that asked.
            Some(24) => {
                if let SubScreen::Terms { last, .. } = &self.subscreen {
                    self.subscreen = *last.clone();
                }
                Outcome::None
            }
            // `RealmsGenericErrorScreen.onClose`: the screen the error interrupted returns.
            Some(91) => {
                if let SubScreen::Error { next, .. } = &self.subscreen {
                    let next = *next.clone();
                    self.restore_after_error(next);
                }
                Outcome::None
            }
            // `RealmsResetWorldScreen.onClose`: the configuration screen.
            Some(76) => {
                if let SubScreen::ResetWorld { server, .. } = &self.subscreen {
                    let server = server.clone();
                    self.back_to_configure_worlds(server);
                }
                Outcome::None
            }
            // `RealmsLongRunningMcoTaskScreen.cancel`: the tasks abort, the screen the
            // tasks were started from returns.
            Some(88) => {
                let server = match &self.subscreen {
                    SubScreen::ResetWorld { server, reset, .. } => {
                        if let Some(task) = reset {
                            task.abort.store(true, Ordering::Relaxed);
                        }
                        Some(server.clone())
                    }
                    SubScreen::TemplateList { server, select, .. } => {
                        if let Some(task) = select {
                            task.abort.store(true, Ordering::Relaxed);
                        }
                        Some(server.clone())
                    }
                    _ => None,
                };
                if let Some(server) = server {
                    self.back_to_configure_worlds(server);
                }
                Outcome::None
            }
            Some(77) => {
                self.select_template();
                Outcome::None
            }
            // `RealmsSelectWorldTemplateScreen.onClose`: `callback.accept(null)`, the reset
            // screen — whose `realmCreationTask` field survives the round trip.
            Some(78) => {
                if let SubScreen::TemplateList {
                    server,
                    slot,
                    kind,
                    creation,
                    group,
                    ..
                } = &self.subscreen
                {
                    let (server, slot, kind, group) = (server.clone(), *slot, *kind, *group);
                    let creation = creation.clone();
                    // `RealmsWorldsTab.switchToMinigame`'s callback sends a cancelled
                    // minigame list back to the configuration screen; the reset screen's
                    // own list returns to the reset screen (`realmCreationTask` kept).
                    if group.is_none() {
                        self.back_to_configure_worlds(server);
                    } else {
                        self.subscreen = SubScreen::ResetWorld {
                            server,
                            slot,
                            kind,
                            creation,
                            templates: None,
                            job: None,
                            reset: None,
                            error: None,
                        };
                    }
                }
                Outcome::None
            }
            // The selected template's trailer and publisher page (`ConfirmLinkScreen`).
            Some(which @ (79 | 92)) => {
                if let SubScreen::TemplateList {
                    templates,
                    selected,
                    ..
                } = &self.subscreen
                    && let Some(template) = selected.and_then(|i| templates.get(i))
                {
                    let link = if which == 79 {
                        template.trailer.clone()
                    } else {
                        template.link.clone()
                    };
                    if let Some(link) = link {
                        self.open_link(link);
                    }
                }
                Outcome::None
            }
            // `RealmsCreateRealmScreen`: Continue goes to the reset screen, whose tasks
            // create the Realm (`showResetWorldScreen`); Back returns to the list.
            Some(42) => {
                if let SubScreen::CreateRealm {
                    server,
                    name,
                    description,
                    ..
                } = &self.subscreen
                    && !name.trim().is_empty()
                {
                    let (server, name, description) = (
                        server.clone(),
                        name.trim().to_owned(),
                        description.trim().to_owned(),
                    );
                    let slot = server.active_slot;
                    let job = client().map(|client| {
                        spawn(move || {
                            [
                                client.fetch_world_templates(1, 10, "NORMAL"),
                                client.fetch_world_templates(1, 10, "ADVENTUREMAP"),
                                client.fetch_world_templates(1, 10, "EXPERIENCE"),
                                client.fetch_world_templates(1, 10, "INSPIRATION"),
                            ]
                        })
                    });
                    self.subscreen = SubScreen::ResetWorld {
                        server,
                        slot,
                        kind: ResetKind::NewRealm,
                        creation: Some((name, description)),
                        templates: None,
                        job,
                        reset: None,
                        error: None,
                    };
                }
                Outcome::None
            }
            Some(43) => {
                self.subscreen = SubScreen::Main;
                Outcome::None
            }
            // `RealmsSelectFileToUploadScreen`'s Upload and Back.
            Some(82) => {
                self.upload_selected();
                Outcome::None
            }
            Some(83) => {
                if let SubScreen::UploadSelect {
                    server,
                    slot,
                    kind,
                    creation,
                    ..
                } = &self.subscreen
                {
                    let (server, slot, kind) = (server.clone(), *slot, *kind);
                    let creation = creation.clone();
                    self.subscreen = SubScreen::ResetWorld {
                        server,
                        slot,
                        kind,
                        creation,
                        templates: None,
                        job: None,
                        reset: None,
                        error: None,
                    };
                }
                Outcome::None
            }
            // `RealmsUploadScreen.onCancel`: the body's reader stops.
            Some(84) => {
                if let SubScreen::Uploading { progress, done, .. } = &self.subscreen
                    && !*done
                {
                    progress.cancel();
                }
                Outcome::None
            }
            // `RealmsUploadScreen.onBack`: the configuration screen.
            Some(85) => {
                if let SubScreen::Uploading { server, .. } = &self.subscreen {
                    let server = server.clone();
                    self.back_to_configure_worlds(server);
                }
                Outcome::None
            }
            // `RealmsCreateWorldFlow`'s `AlertScreen` Cancel: `realmsWorldUpload.cancel()`.
            // The worker's cancelled answer returns to the reset screen (`poll`).
            Some(93) => {
                if let SubScreen::CreateWorldUpload { progress, .. } = &self.subscreen {
                    progress.cancel();
                }
                Outcome::None
            }
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
                // `RealmsConfigureWorldScreen`'s Play: `onClose()` saves the active tab
                // first, then the join starts.
                SubScreen::Configure { server, .. } => {
                    self.save_then(AfterSave::Play(server.clone()))
                }
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
                    // `RealmsWorldOptions.getSlotName`: an empty name reads the slot's
                    // default (or empty for an empty slot).
                    let name = if slot.options.slot_name.is_empty() {
                        if slot.options.empty {
                            crate::lang::get("mco.configure.world.slot.empty")
                        } else {
                            crate::lang::tr(
                                "mco.configure.world.slot",
                                &[&slot.slot_id as &dyn std::fmt::Display],
                            )
                        }
                    } else {
                        slot.options.slot_name.clone()
                    };
                    self.subscreen = SubScreen::SlotOptions {
                        server: server.clone(),
                        slot: slot.clone(),
                        name,
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
                    detail,
                    tier,
                    ..
                } = &self.subscreen
                    && let Some((preference, region)) = selected
                {
                    let server = server.clone();
                    let mut settings = settings.clone();
                    settings.preference = *preference;
                    settings.region = region.clone();
                    self.back_to_settings(server, settings, detail.clone(), tier.clone());
                }
                Outcome::None
            }
            Some(86) => {
                if let SubScreen::RegionPreference {
                    server,
                    settings,
                    detail,
                    tier,
                    ..
                } = &self.subscreen
                {
                    let server = server.clone();
                    let settings = settings.clone();
                    self.back_to_settings(server, settings, detail.clone(), tier.clone());
                }
                Outcome::None
            }
            // RealmsSettingsTab.openPreferenceSelector: the region list, then the screen.
            Some(89) => {
                if let SubScreen::Configure {
                    server,
                    settings: Some(draft),
                    regions,
                    detail,
                    tier,
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
                                scroll: crate::ui::Scroll::default(),
                                detail: detail.clone(),
                                tier: tier.clone(),
                            };
                        }
                    } else if matches!(self.subscreen, SubScreen::Configure { .. }) {
                        // `openPreferenceSelector` while the entry fetch is still running:
                        // the answer opens the list.
                        self.region_screen_requested = true;
                        self.start_configure_regions();
                    }
                }
                Outcome::None
            }
            // RealmsInviteCodesScreen: the Players tab's second button.
            Some(68) => {
                if let SubScreen::Configure { server, .. } = &self.subscreen
                    && let Some(client) = client()
                {
                    let id = server.id;
                    self.subscreen = SubScreen::InviteCodes {
                        server: server.clone(),
                        codes: None,
                        list_job: Some(spawn(move || client.invite_codes(id))),
                        job: None,
                        error: None,
                        selected: None,
                    };
                }
                Outcome::None
            }
            // RealmsInviteCodesScreen: Create (160) and Back (161).
            Some(160) => {
                if let SubScreen::InviteCodes { server, job, .. } = &mut self.subscreen
                    && job.is_none()
                    && let Some(client) = client()
                {
                    let id = server.id;
                    *job = Some(spawn(move || client.create_invite_code(id, true, None)));
                }
                Outcome::None
            }
            Some(161) => {
                self.back_to_players_tab();
                Outcome::None
            }
            // RealmsEditInviteCodeScreen: Active (162), Copy (163), Delete (164), Done
            // (165) and Back (166).
            Some(162) => {
                if let SubScreen::EditCode { active, .. } = &mut self.subscreen {
                    *active = !*active;
                }
                Outcome::None
            }
            Some(163) => {
                if let SubScreen::EditCode { code, .. } = &self.subscreen {
                    return Outcome::Copy(code.code.clone());
                }
                Outcome::None
            }
            Some(164) => {
                if let SubScreen::EditCode { job, code, .. } = &mut self.subscreen
                    && job.is_none()
                    && let Some(client) = client()
                {
                    let code = code.code.clone();
                    *job = Some(spawn(move || client.delete_invite_code(&code)));
                }
                Outcome::None
            }
            Some(165) => {
                // `onSave`: unchanged values just close; a changed expiration is resolved
                // against the current date, while an unchanged one keeps the stored date.
                let unchanged = match &self.subscreen {
                    SubScreen::EditCode {
                        code,
                        active,
                        expiration,
                        ..
                    } => {
                        *active == code.enabled
                            && *expiration
                                == code
                                    .expiration_date
                                    .map(closest_expiration)
                                    .unwrap_or(EXPIRATIONS.len() - 1)
                    }
                    _ => false,
                };
                if unchanged {
                    self.back_to_codes();
                    return Outcome::None;
                }
                if let SubScreen::EditCode {
                    server,
                    code,
                    active,
                    expiration,
                    job,
                    ..
                } = &mut self.subscreen
                    && job.is_none()
                    && let Some(client) = client()
                {
                    let (id, code_text) = (server.id, code.code.clone());
                    let (enabled, index, stored) = (*active, *expiration, code.expiration_date);
                    let changed = index
                        != stored
                            .map(closest_expiration)
                            .unwrap_or(EXPIRATIONS.len() - 1);
                    *job = Some(spawn(move || {
                        // `resolveExpirationDate`: a changed choice becomes now + its
                        // duration (epoch millis, `Instant.toEpochMilli`); otherwise the
                        // code keeps the date it had.
                        let expiration = if changed {
                            EXPIRATIONS[index.min(EXPIRATIONS.len() - 1)]
                                .1
                                .map(|seconds| {
                                    use std::time::{SystemTime, UNIX_EPOCH};
                                    let now = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|d| d.as_millis() as i64)
                                        .unwrap_or(0);
                                    now + seconds * 1000
                                })
                        } else {
                            stored
                        };
                        client.update_invite_code(id, &code_text, enabled, expiration)
                    }));
                }
                Outcome::None
            }
            Some(166) => {
                // `RealmsEditInviteCodeScreen.onClose`: `parentScreen` is the codes list.
                self.back_to_codes();
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
            Some(66) => {
                // RealmsSubscriptionTab's extend: `ConfirmLinkScreen.confirmLinkNow`.
                let link = match (&self.subscreen, session()) {
                    (SubScreen::Configure { server, .. }, Some(s)) => {
                        Some(extend_link(server, s.uuid, false))
                    }
                    _ => None,
                };
                if let Some(link) = link {
                    self.open_link(link);
                }
                Outcome::None
            }
            Some(40) => {
                // RealmsPdpScreen's footer: Try for Free (START_REALMS_TRIAL) when a
                // trial is available, else Buy (BUY_REALMS).
                let on_trial = matches!(&self.subscreen, SubScreen::AddRealm { trial: true, .. });
                self.open_link(if on_trial {
                    "https://aka.ms/startjavarealmstrial".to_owned()
                } else {
                    BUY_REALMS.to_owned()
                });
                Outcome::None
            }
            Some(50) => self.popup_continue(),
            // `ConfirmLinkScreen.addButtons`: Open in Browser, Copy to Clipboard, Cancel.
            Some(53) => self.popup_continue(),
            Some(54) => self.popup_copy(),
            Some(55) => self.popup_cancel(),
            // `PopupScreen`: OK and the InfoPopup's URL button.
            Some(56) => self.notice_ok(),
            Some(57) => self.notice_url(),
            // RealmsInviteCodesScreen: Copy, Delete and Edit act on the selected row.
            Some(180) => {
                if let SubScreen::InviteCodes {
                    codes: Some(Ok(list)),
                    selected,
                    ..
                } = &self.subscreen
                    && let Some(code) = selected.and_then(|i| list.get(i))
                {
                    return Outcome::Copy(code.code.clone());
                }
                Outcome::None
            }
            Some(181) => {
                if let SubScreen::InviteCodes {
                    codes: Some(Ok(list)),
                    selected,
                    job,
                    ..
                } = &mut self.subscreen
                    && job.is_none()
                    && let Some(code) = selected.and_then(|i| list.get(i))
                    && let Some(client) = client()
                {
                    let code = code.code.clone();
                    *job = Some(spawn(move || client.delete_invite_code(&code)));
                }
                Outcome::None
            }
            Some(182) => {
                if let SubScreen::InviteCodes {
                    codes: Some(Ok(list)),
                    selected,
                    ..
                } = &self.subscreen
                    && let Some(code) = selected.and_then(|i| list.get(i).cloned())
                {
                    let expiration = code
                        .expiration_date
                        .map(closest_expiration)
                        .unwrap_or(EXPIRATIONS.len() - 1);
                    let active = code.enabled;
                    if let SubScreen::InviteCodes { server, .. } = &self.subscreen {
                        let server = server.clone();
                        self.subscreen = SubScreen::EditCode {
                            server,
                            code,
                            active,
                            expiration,
                            job: None,
                            error: None,
                        };
                    }
                }
                Outcome::None
            }
            Some(btn @ 100..=200) => {
                // The button's meaning: a pending invite's answer, or a player row's
                // Op/Deop/Remove (`RealmsPlayersTab.PlayerEntry`).
                enum Row {
                    Nothing,
                    Pending(bool, String),
                    Member(bool, i64, String),
                    Uninvite(RealmServer, PlayerInfo),
                    Copy(String),
                    Edit(InviteCode),
                    DeleteCode(String),
                }
                let row = match &self.subscreen {
                    SubScreen::PendingInvites { invites, job, .. } if job.is_none() => {
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
                    SubScreen::InviteCodes {
                        codes: Some(Ok(list)),
                        job,
                        ..
                    } => {
                        let index = usize::from((btn - 130) / 3);
                        let which = (btn - 130) % 3;
                        match list.get(index) {
                            Some(code) if which == 0 => Row::Copy(code.code.clone()),
                            Some(code) if which == 1 => Row::Edit(code.clone()),
                            Some(code) if job.is_none() => Row::DeleteCode(code.code.clone()),
                            _ => Row::Nothing,
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
                    Row::Copy(code) => return Outcome::Copy(code),
                    Row::Edit(code) => {
                        if let SubScreen::InviteCodes { server, .. } = &self.subscreen {
                            let server = server.clone();
                            let expiration = code
                                .expiration_date
                                .map(closest_expiration)
                                .unwrap_or(EXPIRATIONS.len() - 1);
                            let active = code.enabled;
                            self.subscreen = SubScreen::EditCode {
                                server,
                                code,
                                active,
                                expiration,
                                job: None,
                                error: None,
                            };
                        }
                    }
                    Row::DeleteCode(code) => {
                        if let SubScreen::InviteCodes { job, .. } = &mut self.subscreen
                            && job.is_none()
                            && let Some(client) = client()
                        {
                            *job = Some(spawn(move || client.delete_invite_code(&code)));
                        }
                    }
                }
                Outcome::None
            }
            // The tab bar draws its own tabs (`MenuTabBar`), so a click there carries no
            // button id: the tab bar's geometry decides.
            // Clicks that miss every button: the tab bar, the screens' drawn fields and
            // lists (the Configure guard covers its tab bar; the others their rows).
            None if matches!(
                self.subscreen,
                SubScreen::Configure { .. }
                    | SubScreen::RegionPreference { .. }
                    | SubScreen::EditCode { .. }
                    | SubScreen::SlotOptions { .. }
                    | SubScreen::CreateRealm { .. }
                    | SubScreen::InviteCodes { .. }
            ) =>
            {
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
                        // `onSelected` → `updateData`: the tab refills its fields from
                        // the Realm each time it is opened, discarding a stale draft.
                        if *wanted == ConfigureTab::Settings {
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
                // The two distance sliders: a press on the track starts
                // `AbstractSliderButton`'s drag and snaps the value into the range.
                if let SubScreen::Configure {
                    tab: ConfigureTab::Settings,
                    settings: Some(draft),
                    tier,
                    ..
                } = &mut self.subscreen
                {
                    let (rows, content_h) = settings_rows(matches!(tier, Some(Ok(_))));
                    let top = settings_column_top(h, content_h);
                    let max_scroll = (content_h - (h - 24.0 - 33.0)).max(0.0);
                    let scroll = self.settings_scroll.amount.min(max_scroll);
                    let left = (w / 2.0).floor() - 106.0;
                    // `ScrollableLayout`'s scrollbar (4 past the 212-wide column, 6 wide).
                    if self
                        .settings_scroll
                        .press((x, y), left + 224.0, 24.0, h - 33.0, content_h)
                    {
                        return Outcome::None;
                    }
                    let widget_y = |row: usize| {
                        top + rows[row]
                            + SETTINGS_CELL_PADDING
                            + SETTINGS_LABEL_HEIGHT
                            + SETTINGS_LABEL_SPACING
                            - scroll
                    };
                    if (left..left + 212.0).contains(&x) {
                        if (widget_y(0)..widget_y(0) + 20.0).contains(&y) {
                            draft.on_description = false;
                            return Outcome::None;
                        }
                        if (widget_y(1)..widget_y(1) + 20.0).contains(&y) {
                            draft.on_description = true;
                            return Outcome::None;
                        }
                    }
                    if let Some(Ok(config)) = tier.as_ref() {
                        // `OptionInstance`'s slider: the value snaps into the range.
                        let bands = [
                            (
                                config.render_distance.as_ref(),
                                widget_y(2),
                                &mut draft.render_distance,
                            ),
                            (
                                config.sim_distance.as_ref(),
                                widget_y(3),
                                &mut draft.simulation_distance,
                            ),
                        ];
                        for (which, (range, slider_y, picked)) in bands.into_iter().enumerate() {
                            let (Some(range), Some(picked)) = (range, picked) else {
                                continue;
                            };
                            if (slider_y..slider_y + 20.0).contains(&y)
                                && (left..left + 212.0).contains(&x)
                            {
                                *picked =
                                    range.from_slider(crate::ui::slider_value(left, 212.0, x));
                                self.settings_slider = Some(which);
                                return Outcome::None;
                            }
                        }
                    }
                }
                // `RealmsInviteCodesScreen`: a row takes the selection (`Entry
                // .mouseClicked`), 28 high from the contents' top.
                if let SubScreen::InviteCodes {
                    codes, selected, ..
                } = &mut self.subscreen
                    && let Some(Ok(list)) = codes
                {
                    let top = 33.0;
                    let rx = ((w - 308.0) / 2.0).floor();
                    if x >= rx && x < rx + 308.0 && y >= top {
                        let row = ((y - top - 2.0) / 28.0).floor() as usize;
                        *selected = (row < list.len()).then_some(row);
                    }
                    return Outcome::None;
                }
                // RealmsCreateRealmScreen's two fields: the click moves the caret.
                if let SubScreen::CreateRealm { on_description, .. } = &mut self.subscreen {
                    let left = (w / 2.0).floor() - 105.0;
                    let content_y = (h - 107.0).min(63.0);
                    if (left..left + 210.0).contains(&x) {
                        if (content_y + 13.0..content_y + 33.0).contains(&y) {
                            *on_description = false;
                            return Outcome::None;
                        }
                        if (content_y + 54.0..content_y + 74.0).contains(&y) {
                            *on_description = true;
                            return Outcome::None;
                        }
                    }
                }
                // `RealmsPreferredRegionSelectionScreen`: a row is the preference (the
                // region rows are MANUAL + that region); a double click plays it down
                // (`Entry.mouseClicked`'s `playDownSound` + `onDone`).
                if let SubScreen::RegionPreference {
                    regions,
                    selected,
                    scroll,
                    ..
                } = &mut self.subscreen
                {
                    let rows = region_rows(regions);
                    // The list is 33..h-33, its first entry at 35.
                    let top = 35.0;
                    let row = ((y - top + scroll.amount) / 16.0).floor() as usize;
                    let mut double = false;
                    if y >= top
                        && let Some(pick) = rows.get(row)
                    {
                        let now = SystemTime::now();
                        double = matches!(self.last_click, Some((last, at)) if last == row
                            && now.duration_since(at).is_ok_and(|d| d.as_millis() < 250));
                        self.last_click = Some((row, now));
                        *selected = Some(pick.clone());
                    }
                    if double {
                        return self.click(Some(87), x, y, (w, h));
                    }
                    return Outcome::None;
                }
                if let SubScreen::EditCode {
                    active,
                    expiration,
                    job,
                    ..
                } = &mut self.subscreen
                    && *active
                    && job.is_none()
                {
                    let left = (w / 2.0).floor() - 154.0;
                    if (93.0..113.0).contains(&y) && (left..left + 308.0).contains(&x) {
                        let value = crate::ui::slider_value(left, 308.0, x);
                        *expiration = (value * (EXPIRATIONS.len() - 1) as f32).round() as usize;
                        self.other_slider = Some(2);
                        return Outcome::None;
                    }
                }
                // RealmsSlotOptionsScreen's spawn-protection slider: a click on the track
                // sets the value (there is no drag event on this screen yet).
                if let SubScreen::SlotOptions {
                    server,
                    spawn_protection,
                    ..
                } = &mut self.subscreen
                    && server.world_type == "NORMAL"
                {
                    // The 150-wide slider in the right column at content top + 69
                    // (58-tall header with the warning, 33 without).
                    let warning = matches!(
                        server.world_type.as_str(),
                        "ADVENTUREMAP" | "INSPIRATION" | "EXPERIENCE"
                    );
                    let top = if warning { 88.0 } else { 63.0 } + 69.0;
                    let (sx, sw) = (((w / 2.0).floor()) + 4.0, 150.0);
                    if (top..top + 20.0).contains(&y) && (sx..sx + sw).contains(&x) {
                        let value = crate::ui::slider_value(sx, sw, x);
                        *spawn_protection = (value * 16.0).round() as i32;
                        self.other_slider = Some(1);
                        return Outcome::None;
                    }
                }
                // RealmsWorldsTab's slot buttons (`createSlotButton`): `getAction` leaves the
                // active slot and, on an expired Realm, an empty one inert.
                let switch = match &self.subscreen {
                    SubScreen::Configure {
                        server,
                        tab: ConfigureTab::Worlds,
                        detail: Some(Ok(detail)),
                        ..
                    } => {
                        // The same 80/50 geometry the draw uses.
                        let x0 = ((w - 278.0) / 2.0).floor();
                        let y0 = 24.0 + ((h - 57.0 - 176.0) / 6.0).trunc();
                        let big = if detail.world_type == "MINIGAME" {
                            4
                        } else {
                            detail.active_slot
                        };
                        let mut bx = x0;
                        let mut hit = None;
                        for slot in 1..=4 {
                            let active = slot == big;
                            let size = if active { 80.0 } else { 50.0 };
                            let by = if active { y0 } else { y0 + 30.0 };
                            // `RealmsWorldSlotButton.State`: slot 4 is the minigame, empty
                            // while the Realm has none; the other slots read their options.
                            let (empty, minigame) = if slot == 4 {
                                (server.minigame_id.is_none_or(|id| id == -1), true)
                            } else {
                                (
                                    detail
                                        .slots
                                        .get(&slot)
                                        .is_none_or(|entry| entry.options.empty),
                                    false,
                                )
                            };
                            if !active
                                && !(empty && server.expired)
                                && (bx..bx + size).contains(&x)
                                && (by..by + size).contains(&y)
                            {
                                hit = Some((server.clone(), slot, minigame, empty));
                            }
                            bx += size + 16.0;
                        }
                        hit
                    }
                    _ => None,
                };
                if let Some((server, slot, minigame, empty)) = switch {
                    if minigame {
                        // `RealmsWorldsTab.switchToMinigame`: the minigame template list.
                        let active = server.active_slot;
                        self.open_template_list(server, active, ResetKind::Reset, None, None);
                    } else if empty {
                        // `RealmsWorldsTab.switchToEmptySlot`'s question, then
                        // `RealmsResetWorldScreen.forEmptySlot`.
                        self.subscreen = SubScreen::Popup(Popup::SwitchEmpty { server, slot });
                    } else {
                        // `RealmsWorldsTab.switchToFullSlot`'s question, then the switch.
                        self.subscreen = SubScreen::Popup(Popup::SwitchSlot { server, slot });
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

    /// `ConfirmLinkScreen.confirmLink`'s press: the confirm, over the screen that asked.
    fn open_link(&mut self, url: String) {
        // `ConfirmLinkScreen.confirmLink`: no link, no question. An empty URI would be
        // handed to the desktop's opener, which opens the game directory.
        if url.trim().is_empty() {
            return;
        }
        let parent = self.subscreen.clone();
        self.subscreen = SubScreen::Popup(Popup::OpenLink {
            url,
            parent: Box::new(parent),
            open_parent: None,
        });
    }

    /// `ConfirmLinkScreen`'s copy button: the link to the clipboard, then the parent screen.
    fn popup_copy(&mut self) -> Outcome {
        let SubScreen::Popup(Popup::OpenLink { url, parent, .. }) = &self.subscreen else {
            return Outcome::None;
        };
        let (url, parent) = (url.clone(), parent.clone());
        self.subscreen = *parent;
        Outcome::Copy(url)
    }

    /// `ConfirmLinkScreen`'s no button: back to the screen that asked.
    fn popup_cancel(&mut self) -> Outcome {
        let SubScreen::Popup(Popup::OpenLink { parent, .. }) = &self.subscreen else {
            return Outcome::None;
        };
        self.subscreen = *parent.clone();
        Outcome::None
    }

    /// `RealmsMainScreen`'s notifications answer: the first `InfoPopup` with a translated
    /// title opens its `PopupScreen` over the list, marked seen (`buildScreen`).
    fn show_info_popup(&mut self) {
        if !matches!(self.subscreen, SubScreen::Main) {
            return;
        }
        let Some((uuid, title, message, image, url)) = self.notifications.iter().find_map(|n| {
            if n.kind != "infoPopup" {
                return None;
            }
            let title = n.info_title()?;
            Some((
                n.uuid.clone(),
                title,
                n.info_message(),
                n.image.clone(),
                n.info_url_button(),
            ))
        }) else {
            return;
        };
        if !self.handled_seen.contains(&uuid) {
            self.handled_seen.push(uuid.clone());
            if let Some(client) = client() {
                let seen = uuid.clone();
                spawn(move || client.notifications_seen(std::slice::from_ref(&seen)));
            }
        }
        self.subscreen = SubScreen::Popup(Popup::Notice {
            uuid,
            title,
            message,
            image,
            url,
        });
    }

    /// `PopupScreen`'s OK: `onClose` dismisses the notification and shows the list again.
    fn notice_ok(&mut self) -> Outcome {
        if let SubScreen::Popup(Popup::Notice { uuid, .. }) = &self.subscreen {
            let uuid = uuid.clone();
            self.dismiss_notification(&uuid);
        }
        self.subscreen = SubScreen::Main;
        Outcome::None
    }

    /// `InfoPopup`'s URL button: the notification is dismissed, the link asked about with
    /// the notice under the confirm and the list behind a yes.
    fn notice_url(&mut self) -> Outcome {
        let SubScreen::Popup(Popup::Notice {
            uuid,
            url: Some((url, _)),
            ..
        }) = &self.subscreen
        else {
            return Outcome::None;
        };
        let (uuid, url) = (uuid.clone(), url.clone());
        self.dismiss_notification(&uuid);
        let notice = self.subscreen.clone();
        self.subscreen = SubScreen::Popup(Popup::OpenLink {
            url,
            parent: Box::new(notice),
            open_parent: Some(Box::new(SubScreen::Main)),
        });
        Outcome::None
    }

    /// Continue in a popup.
    fn popup_continue(&mut self) -> Outcome {
        let SubScreen::Popup(popup) = &self.subscreen else {
            return Outcome::None;
        };
        let popup = popup.clone();
        if let Popup::OpenLink {
            url,
            parent,
            open_parent,
        } = &popup
        {
            // `ConfirmLinkScreen`: yes opens the page and returns to the screen that asked.
            self.subscreen = open_parent
                .clone()
                .unwrap_or_else(|| parent.clone())
                .as_ref()
                .clone();
            return Outcome::OpenUri(url.clone());
        }
        // `RealmsBackupScreen.Entry.restoreClicked`: Continue runs `RestoreTask`.
        if let Popup::RestoreBackup {
            server,
            slot_id,
            backup,
            ..
        } = &popup
        {
            let (server, slot_id, backup) = (server.clone(), *slot_id, backup.clone());
            self.start_restore(server, slot_id, backup);
            return Outcome::None;
        }
        // `RealmsBackupScreen.downloadClicked`: Continue runs `DownloadTask` under
        // `RealmsLongRunningMcoTaskScreen`.
        if let Popup::DownloadLatest {
            server,
            slot_id,
            list,
            selected,
        } = &popup
        {
            let (server, slot_id) = (server.clone(), *slot_id);
            self.subscreen = SubScreen::Backups {
                server: server.clone(),
                slot_id,
                backups: Some(Ok(list.clone())),
                list_job: None,
                restore_job: None,
                download_task: None,
                selected: *selected,
                scroll: crate::ui::Scroll::default(),
            };
            self.start_download_task(server, slot_id);
            return Outcome::None;
        }
        // `RealmsSettingsTab`'s close question: Continue saves the tab, then the Realm
        // closes (`if (this.save()) configurationScreen.closeTheWorld()`).
        if let Popup::CloseWorld {
            server,
            settings,
            detail,
            tier,
        } = &popup
        {
            let id = server.id;
            self.back_to_settings(
                server.clone(),
                settings.clone(),
                detail.clone(),
                tier.clone(),
            );
            return self.save_then(AfterSave::Close(id));
        }
        self.subscreen = SubScreen::Main;
        match popup {
            // Answered above; the match needs the shape.
            Popup::OpenLink { .. }
            | Popup::Notice { .. }
            | Popup::CloseWorld { .. }
            | Popup::RestoreBackup { .. }
            | Popup::DownloadLatest { .. }
            // The oversized warning has its own single OK (`download_oversized_ok`).
            | Popup::DownloadOversized { .. } => Outcome::None,
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
                    players_scroll: crate::ui::Scroll::default(),
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
            // RealmsWorldsTab.switchToEmptySlot, answered: `forEmptySlot`'s reset screen
            // replaces the empty slot's world.
            Popup::SwitchEmpty { server, slot } => {
                self.open_reset_world(server, slot, ResetKind::Empty);
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
                    players_scroll: crate::ui::Scroll::default(),
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
                    players_scroll: crate::ui::Scroll::default(),
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
        // `RealmsBackupScreen.Entry`: its Restore and Changes buttons first, then the row
        // itself opens the metadata screen.
        if matches!(self.subscreen, SubScreen::Backups { .. }) {
            let id = self
                .backup_row_buttons
                .borrow()
                .iter()
                .find(|([bx, by, bw, bh], _)| x >= *bx && x < bx + bw && y >= *by && y < by + bh)
                .map(|(_, id)| *id);
            if let Some(id) = id {
                return self.click(Some(id), x, y, (w, h));
            }
            // The row itself only takes the selection; the Changes button opens the
            // metadata screen (`ContainerObjectSelectionList.mouseClicked`).
            if let SubScreen::Backups {
                backups: Some(Ok(list)),
                scroll,
                selected,
                ..
            } = &mut self.subscreen
            {
                let rx = ((w - ROW_WIDTH) / 2.0).floor();
                if x >= rx && x < rx + ROW_WIDTH && y >= 33.0 && y < h - 33.0 {
                    let row = ((y - 33.0 - 2.0 + scroll.amount) / ROW_HEIGHT).floor();
                    if row >= 0.0 && (row as usize) < list.len() {
                        *selected = Some(row as usize);
                    }
                }
            }
            return Outcome::None;
        }
        // `RealmsPendingInvitesScreen`'s list: a press takes the row's selection
        // (`ContainerObjectSelectionList.mouseClicked`); the accept/reject buttons are
        // hit-tested before this.
        if let SubScreen::PendingInvites {
            invites: Some(Ok(list)),
            scroll,
            selected,
            ..
        } = &mut self.subscreen
        {
            let (top, bottom) = (33.0, h - 33.0);
            let rx = ((w - 280.0) / 2.0).floor();
            if x >= rx && x < rx + 280.0 && y >= top && y < bottom {
                let row = ((y - top - 2.0 + scroll.amount) / ROW_HEIGHT).floor();
                if row >= 0.0 {
                    let row = row as usize;
                    if row < list.len() {
                        *selected = Some(row);
                    }
                }
            }
            return Outcome::None;
        }
        // `RealmsSelectWorldTemplateScreen.WorldTemplateList.mouseClicked`: the empty
        // list's link asks before opening the content-creation page.
        if matches!(self.subscreen, SubScreen::TemplateList { .. }) {
            let [lx, ly, lw, lh] = self.template_link_rect.get();
            if lw > 0.0 && x >= lx && x < lx + lw && y >= ly && y < ly + lh {
                self.open_link(REALMS_CONTENT_CREATION.to_owned());
                return Outcome::None;
            }
        }
        // `ObjectSelectionList.mouseClicked`: a product-page row takes the selection (and
        // the focus); its rectangles are the ones the draw wrapped the text into.
        if let SubScreen::AddRealm { rows, .. } = &self.subscreen {
            let row = rows
                .borrow()
                .iter()
                .position(|[rx, ry, rw, rh]| x >= *rx && x < rx + rw && y >= *ry && y < ry + rh);
            if let Some(row) = row {
                self.pdp_row = Some(row);
                self.list_clicked = true;
                return Outcome::None;
            }
        }
        // `RealmsResetWorldScreen`'s tiles are drawn, not widgets: the click is a rectangle.
        if let SubScreen::ResetWorld { reset, .. } = &self.subscreen {
            if reset.is_some() {
                return Outcome::None;
            }
            let cx = (w / 2.0).floor();
            let top = reset_content_y(h);
            // `FrameButton` is 60 wide and 69 tall (its label is part of the control).
            let tile = (0..6).find(|i| {
                let (tx, ty) = reset_tile(cx, top, *i);
                x >= tx && x < tx + 60.0 && y >= ty && y < ty + 69.0
            });
            if let Some(tile) = tile {
                return self.click(Some(70 + tile as u8), x, y, (w, h));
            }
            return Outcome::None;
        }
        // `Entry.mouseClicked` checks its two link buttons before the list; the question
        // (`ConfirmLinkScreen.confirmLink`) is asked by the popup.
        let link = match &self.subscreen {
            SubScreen::TemplateList {
                templates, icons, ..
            } => icons.borrow().iter().find_map(|(row, rect, trailer)| {
                let [ix, iy, iw, ih] = *rect;
                if x < ix || x >= ix + iw || y < iy || y >= iy + ih {
                    return None;
                }
                let template = templates.get(*row)?;
                if *trailer {
                    template.trailer.clone()
                } else {
                    template.link.clone()
                }
            }),
            _ => None,
        };
        if let Some(link) = link {
            self.open_link(link);
            return Outcome::None;
        }
        // The template and upload lists pick a row; a double click selects the template at
        // once (`Entry.mouseClicked`'s `doubleClick && isFocused`). The task screen has
        // replaced the list while the minigame switch runs, so nothing is picked then.
        let picked = match &self.subscreen {
            SubScreen::TemplateList {
                group,
                templates,
                scroll,
                select,
                ..
            } if select.is_none() => {
                let top = if group.is_none() { 59.0 } else { 33.0 };
                row_at(x, y, w, top, h - 33.0, TEMPLATE_ROW, scroll.amount)
                    .filter(|row| *row < templates.len())
                    .map(|row| (row, true))
            }
            SubScreen::UploadSelect {
                worlds,
                scroll,
                query,
                ..
            } => {
                // `RealmsSelectFileToUploadScreen`: 270-wide rows from y 33 to H - 33,
                // the list filtered by the search box.
                let (top, bottom) = (33.0, h - 33.0);
                let rx = ((w - 270.0) / 2.0).floor();
                let shown = worlds
                    .iter()
                    .filter(|world| upload_matches(world, query))
                    .count();
                if y < top || y >= bottom || x < rx || x >= rx + 270.0 {
                    None
                } else {
                    let row = ((y - top - 2.0 + scroll.amount) / ROW_HEIGHT).floor();
                    (row >= 0.0)
                        .then_some(row as usize)
                        .filter(|row| *row < shown)
                        .map(|row| (row, false))
                }
            }
            _ => None,
        };
        if let Some((row, template)) = picked {
            let now = SystemTime::now();
            // `MouseHandler.DOUBLE_CLICK_THRESHOLD_MS`.
            let double = matches!(self.last_click, Some((last, at)) if last == row
                && now.duration_since(at).is_ok_and(|d| d.as_millis() < 250));
            self.last_click = Some((row, now));
            match &mut self.subscreen {
                SubScreen::TemplateList { selected, .. } => *selected = Some(row),
                SubScreen::UploadSelect { selected, .. } => *selected = Some(row),
                _ => {}
            }
            if double && template {
                self.select_template();
            } else if double
                && let SubScreen::UploadSelect {
                    server,
                    slot,
                    worlds,
                    query,
                    creation,
                    ..
                } = &self.subscreen
            {
                // `WorldListEntry.mouseClicked`'s double click runs `onEntryInteract`,
                // which opens `RealmsUploadScreen` for that level.
                let world = worlds
                    .iter()
                    .filter(|world| upload_matches(world, query))
                    .nth(row)
                    .cloned();
                if let Some(world) = world {
                    let (server, slot, creation) = (server.clone(), *slot, creation.clone());
                    self.start_upload(server, slot, world, creation);
                }
            }
            return Outcome::None;
        }
        if !matches!(self.subscreen, SubScreen::Main) {
            return Outcome::None;
        }
        let footer_h = main_footer_h(self.layout_state());
        let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
        let rx = ((w - ROW_WIDTH) / 2.0).floor();
        if y < top || y > bottom || x < rx || x > rx + ROW_WIDTH {
            return Outcome::None;
        }
        // `NotificationMessageEntry.mouseClicked`: its dismiss cross, then its open button;
        // a press anywhere on the entry stays there. The entry scrolls with the list.
        let amount = self.main_scroll.amount;
        let notice_height = self.notice_height.get();
        if y < top + 2.0 + notice_height - amount {
            let [open, dismiss] = self.notice_rects.get();
            let inside = |r: [f32; 4]| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3];
            if inside(dismiss) {
                if let Some(uuid) = self.notice().map(|notification| notification.uuid.clone()) {
                    self.dismiss_notification(&uuid);
                }
            } else if inside(open)
                && let Some(url) = self
                    .notice()
                    .and_then(|notification| notification.url.clone())
            {
                self.open_link(url);
            }
            // `AbstractSelectionList.setSelected(entry)`: the notice entry takes the
            // selection, so no server is selected and the footer's buttons go grey.
            self.selected = None;
            return Outcome::None;
        }
        let row = ((y - top - 2.0 - notice_height - amount) / ROW_HEIGHT).floor();
        let Some(row) = usize::try_from(row as i64)
            .ok()
            .filter(|row| *row < self.servers().len())
        else {
            return Outcome::None;
        };
        let Some(selected) = self.servers().get(row).cloned() else {
            return Outcome::None;
        };
        if selected.state == "UNINITIALIZED" {
            // `ServerEntry.mouseClicked`: a single click here creates rather than plays.
            // `RealmsCreateRealmScreen`'s fields start empty (no prefill from the entry).
            self.subscreen = SubScreen::CreateRealm {
                name: String::new(),
                description: String::new(),
                server: selected,
                on_description: false,
                error: None,
            };
            return Outcome::None;
        }
        let can_play = can_play(&selected, session().map(|s| s.uuid));
        let now = SystemTime::now();
        let double = matches!(self.last_click, Some((last, at)) if last == row
            && now.duration_since(at).is_ok_and(|d| d.as_millis() < 250));
        self.selected = Some(row);
        self.last_click = Some((row, now));
        if double && can_play {
            return self.play_selected();
        }
        Outcome::None
    }

    /// The product page's list is on screen: a Tab stop, and the arrows' target while no
    /// button has the keyboard (`ObjectSelectionList`).
    pub fn pdp_list(&self) -> bool {
        matches!(self.subscreen, SubScreen::AddRealm { .. })
    }

    /// A list takes the keyboard while no button has it: the server list on the main
    /// screen, the product page's rows behind Add Realm.
    pub fn list_stop(&self) -> bool {
        // Every `ObjectSelectionList` is a keyboard stop: its Enter is the row's action.
        matches!(
            self.subscreen,
            SubScreen::Main
                | SubScreen::UploadSelect { .. }
                | SubScreen::TemplateList { select: None, .. }
                | SubScreen::RegionPreference { .. }
        ) || self.pdp_list()
    }

    /// `ObjectSelectionList.nextFocusPath` over the server list: from no row an arrow takes
    /// the near end, as `nextEntry` does.
    pub fn main_arrow(&mut self, up: bool) -> bool {
        if !matches!(self.subscreen, SubScreen::Main) {
            return false;
        }
        let len = self.servers().len();
        if len == 0 {
            return false;
        }
        self.selected = Some(match self.selected {
            None if up => len - 1,
            None => 0,
            Some(row) if up => row.saturating_sub(1),
            Some(row) => (row + 1).min(len - 1),
        });
        true
    }

    /// `ServerEntry.keyPressed`'s Enter: an uninitialized entry creates a Realm, a playable
    /// one plays; any other row does nothing.
    pub fn activate_selected(&mut self) -> Outcome {
        // `ObjectSelectionList.Entry.keyPressed`'s `isSelection` for the other lists.
        match &self.subscreen {
            SubScreen::UploadSelect { .. } => {
                self.upload_selected();
                return Outcome::None;
            }
            SubScreen::TemplateList {
                select: None,
                selected: Some(_),
                ..
            } => {
                self.select_template();
                return Outcome::None;
            }
            SubScreen::RegionPreference {
                selected: Some(_), ..
            } => {
                return self.click(Some(87), 0.0, 0.0, (854.0, 480.0));
            }
            _ => {}
        }
        if !matches!(self.subscreen, SubScreen::Main) {
            return Outcome::None;
        }
        let Some(server) = self.selected_server().cloned() else {
            return Outcome::None;
        };
        if server.state == "UNINITIALIZED" {
            self.subscreen = SubScreen::CreateRealm {
                name: String::new(),
                description: String::new(),
                server,
                on_description: false,
                error: None,
            };
            return Outcome::None;
        }
        // `ServerEntry.keyPressed`: `isSelection` plays only when
        // `shouldPlayButtonBeActive` says so.
        if can_play(&server, session().map(|s| s.uuid)) {
            self.start_join(server)
        } else {
            Outcome::None
        }
    }

    /// `ObjectSelectionList.nextFocusPath`'s `ArrowNavigation` over the product page: with
    /// no row selected an arrow takes the near end, as `nextEntry` does.
    pub fn pdp_arrow(&mut self, up: bool) -> bool {
        if !self.pdp_list() {
            return false;
        }
        let len = pdp_entries().len();
        let row = match self.pdp_row {
            None if up => len - 1,
            None => 0,
            Some(row) if up => row.saturating_sub(1),
            Some(row) => (row + 1).min(len - 1),
        };
        self.pdp_row = Some(row);
        true
    }

    /// The click just taken was on a product-page row, so the keyboard focus moves to the
    /// list (`AbstractSelectionList.mouseClicked` → `setFocused`).
    pub fn take_list_click(&mut self) -> bool {
        std::mem::take(&mut self.list_clicked)
    }

    /// `AbstractScrollArea.mouseScrolled` over the product page's list, the template and
    /// upload lists.
    pub fn wheel(&mut self, delta: f32, cursor: (f32, f32), (_, h): (f32, f32)) {
        if let SubScreen::AddRealm {
            scroll, content, ..
        } = &mut self.subscreen
        {
            // The PdpList fills 44..h-33; `AbstractScrollArea.defaultSettings` scrolls
            // it by `entryHeight / 2` = 22.
            let (top, bottom) = (44.0, h - 33.0);
            if cursor.1 < top || cursor.1 >= bottom {
                return;
            }
            scroll.wheel(delta, 22.0, top, bottom, content.get());
            return;
        }
        // `RealmsSettingsTab`'s `ScrollableLayout`: the column scrolls when it is taller
        // than the tab area (`setMaxHeight(getContentHeight())`).
        if let SubScreen::Configure {
            tab: ConfigureTab::Settings,
            tier,
            ..
        } = &self.subscreen
        {
            let content = settings_rows(matches!(tier, Some(Ok(_)))).1;
            let (top, bottom) = (24.0, h - 33.0);
            if cursor.1 >= top && cursor.1 < bottom {
                self.settings_scroll
                    .wheel(delta, 10.0, top, bottom, content);
            }
            return;
        }
        // `ObjectSelectionList` scrolls: the server list and the pending invites too.
        let scrolled = match &self.subscreen {
            SubScreen::Main => {
                let footer_h = main_footer_h(self.layout_state());
                let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
                let notice = if self.notice().is_some() {
                    self.notice_height.get()
                } else {
                    0.0
                };
                Some((
                    top,
                    bottom,
                    notice + self.servers().len() as f32 * ROW_HEIGHT,
                    ROW_HEIGHT,
                ))
            }
            SubScreen::PendingInvites { invites, .. } => Some((
                33.0,
                h - 33.0,
                invites
                    .as_ref()
                    .and_then(|i| i.as_ref().ok())
                    .map_or(0.0, |list| list.len() as f32)
                    * ROW_HEIGHT,
                ROW_HEIGHT,
            )),
            SubScreen::RegionPreference { regions, .. } => Some((
                33.0,
                h - 33.0,
                region_rows(regions).len() as f32 * 16.0,
                16.0,
            )),
            // `RealmsBackupScreen` / `RealmsBackupInfoScreen`: 36-tall rows from 33.
            SubScreen::Backups {
                backups: Some(Ok(list)),
                ..
            } => Some((33.0, h - 33.0, list.len() as f32 * ROW_HEIGHT, ROW_HEIGHT)),
            SubScreen::BackupInfo { backup, .. } => {
                Some((33.0, h - 33.0, backup.change_list.len() as f32 * 36.0, 36.0))
            }
            // The players tab's list scrolls from the header row (RealmsPlayersTab).
            SubScreen::Configure {
                tab: ConfigureTab::Players,
                detail: Some(Ok(detail)),
                ..
            } => Some((
                25.0,
                h - 68.0,
                17.0 + detail.players.len() as f32 * 36.0,
                36.0,
            )),
            _ => None,
        };
        if let Some((top, bottom, content, height)) = scrolled
            && cursor.1 >= top
            && cursor.1 < bottom
        {
            match &mut self.subscreen {
                SubScreen::Main => {
                    self.main_scroll
                        .wheel(delta, height / 2.0, top, bottom, content);
                }
                SubScreen::PendingInvites { scroll, .. } => {
                    scroll.wheel(delta, height / 2.0, top, bottom, content);
                }
                SubScreen::RegionPreference { scroll, .. } => {
                    scroll.wheel(delta, height / 2.0, top, bottom, content);
                }
                SubScreen::Configure {
                    tab: ConfigureTab::Players,
                    players_scroll,
                    ..
                } => {
                    players_scroll.wheel(delta, height / 2.0, top, bottom, content);
                }
                _ => {}
            }
            return;
        }
        let (top, bottom, content, height) = match &self.subscreen {
            SubScreen::TemplateList {
                group, templates, ..
            } => (
                if group.is_none() { 59.0 } else { 33.0 },
                h - 33.0,
                templates.len() as f32 * TEMPLATE_ROW,
                TEMPLATE_ROW,
            ),
            SubScreen::UploadSelect { worlds, query, .. } => {
                // The draw's rows run from the 33-tall header to the 33-tall footer.
                (
                    33.0,
                    h - 33.0,
                    worlds
                        .iter()
                        .filter(|world| upload_matches(world, query))
                        .count() as f32
                        * ROW_HEIGHT,
                    ROW_HEIGHT,
                )
            }
            _ => return,
        };
        if cursor.1 < top || cursor.1 >= bottom {
            return;
        }
        match &mut self.subscreen {
            SubScreen::TemplateList { scroll, .. }
            | SubScreen::UploadSelect { scroll, .. }
            | SubScreen::Backups { scroll, .. }
            | SubScreen::BackupInfo { scroll, .. } => {
                scroll.wheel(delta, height / 2.0, top, bottom, content);
            }
            _ => {}
        }
    }

    pub fn buttons(
        &self,
        w: f32,
        h: f32,
    ) -> Vec<(f32, f32, f32, String, bool, u8, Option<(&'static str, f32)>)> {
        let cx = (w / 2.0).floor();
        let left = cx - (CONTENT_WIDTH / 2.0).floor();
        let t = crate::lang::get;
        let mut buttons = Vec::new();

        match &self.subscreen {
            SubScreen::JoinCode { code, job, .. } => {
                // `RealmsJoinRealmWithCodeScreen.init`: the Join Realm button at
                // contentY + 41, then the footer's 200-wide Back.
                let content_y = (h - 94.0).min(63.0);
                buttons.push((
                    cx - 100.0,
                    content_y + 41.0,
                    200.0,
                    t("mco.selectServer.joinCode.select"),
                    !code.trim().is_empty() && job.is_none(),
                    20,
                    None,
                ));
                // The footer is the default 33 tall: the 20-tall button centres 6 into it.
                buttons.push((cx - 100.0, h - 27.0, 200.0, t("gui.back"), true, 21, None));
            }
            SubScreen::PendingInvites {
                invites,
                job,
                scroll,
                ..
            } => {
                // The footer is the default 33 tall: Done centres 6 into it.
                buttons.push((cx - 100.0, h - 27.0, 200.0, t("gui.done"), true, 22, None));
                if let Some(Ok(list)) = invites {
                    let rx = ((w - 280.0) / 2.0).floor();
                    let (top, bottom) = (33.0, h - 33.0);
                    for (i, _) in list.iter().enumerate() {
                        let row_y = top + 2.0 + i as f32 * ROW_HEIGHT - scroll.amount;
                        if row_y + ROW_HEIGHT <= top || row_y >= bottom {
                            continue;
                        }
                        // The row id is the list index; a list past 78 rows would wrap the
                        // u8, but the visible window never shows that many.
                        let id = match u8::try_from(100 + i as u32 * 2) {
                            Ok(id) => id,
                            Err(_) => continue,
                        };
                        // `Entry`'s SpriteIconButtons: 21 square, the accept one 58 from
                        // the row's content end and the reject 29 behind it, an 18 sprite
                        // inside.
                        buttons.push((
                            rx + 220.0,
                            row_y + 8.0,
                            21.0,
                            String::new(),
                            job.is_none(),
                            id,
                            Some(("pending_invite/accept", 18.0)),
                        ));
                        buttons.push((
                            rx + 249.0,
                            row_y + 8.0,
                            21.0,
                            String::new(),
                            job.is_none(),
                            id + 1,
                            Some(("pending_invite/reject", 18.0)),
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
                job,
                ..
            } => {
                // The footer: Play and Back, two 150-wide buttons 8 apart at h - 27.
                buttons.push((
                    cx - 154.0,
                    h - 27.0,
                    150.0,
                    t("mco.selectServer.play"),
                    can_play(server, session().map(|s| s.uuid)) && job.is_none(),
                    64,
                    None,
                ));
                buttons.push((cx + 4.0, h - 27.0, 150.0, t("gui.back"), true, 32, None));
                // RealmsSettingsTab: the region preference and the open/close toggle
                // (vanilla keeps the toggle on this tab), at the row tops the tab's
                // padded column gives them. Both live inside the `ScrollableLayout`, whose
                // scissor hides them outside its view; a widget that is only partly in the
                // view is not offered either, since the button list cannot clip a sprite.
                if *tab == ConfigureTab::Settings {
                    let (rows, content_h) = settings_rows(matches!(tier, Some(Ok(_))));
                    let top = settings_column_top(h, content_h);
                    let max_scroll = (content_h - (h - 24.0 - 33.0)).max(0.0);
                    let scroll = self.settings_scroll.amount.min(max_scroll);
                    let row_y = |row: usize| top + rows[row] + SETTINGS_CELL_PADDING - scroll;
                    let (view_top, view_bottom) = (24.0, h - 33.0);
                    let visible =
                        |y: f32| y >= view_top && y + SETTINGS_WIDGET_HEIGHT <= view_bottom;
                    // The region cell's label, its 13-offset line, then the button 9
                    // under the line.
                    let region_y = row_y(4)
                        + SETTINGS_LABEL_HEIGHT
                        + SETTINGS_LABEL_SPACING
                        + SETTINGS_LABEL_HEIGHT;
                    if visible(region_y) {
                        buttons.push((
                            cx - 106.0,
                            region_y,
                            212.0,
                            t("mco.configure.world.buttons.region_preference"),
                            true,
                            89,
                            None,
                        ));
                    }
                    let toggle = if server.is_open() {
                        t("mco.configure.world.buttons.close")
                    } else {
                        t("mco.configure.world.buttons.open")
                    };
                    if visible(row_y(5)) {
                        buttons.push((
                            cx - 106.0,
                            row_y(5),
                            212.0,
                            toggle,
                            job.is_none(),
                            30,
                            None,
                        ));
                    }
                }
                // RealmsSubscriptionTab's own two buttons, where the draw centred the
                // column (Extend, then a 2px spacer over Delete).
                if *tab == ConfigureTab::Subscription && matches!(subscription, Some(Ok(_))) {
                    let (extend_y, delete_y) = self.subscription_buttons.get();
                    buttons.push((
                        cx - 100.0,
                        extend_y,
                        200.0,
                        t("mco.configure.world.subscription.extend"),
                        true,
                        66,
                        None,
                    ));
                    buttons.push((
                        cx - 100.0,
                        delete_y,
                        200.0,
                        t("mco.configure.world.delete.button"),
                        server.expired && deleting.is_none(),
                        65,
                        None,
                    ));
                }
                // RealmsWorldsTab: World Options and the Reset/New World button (Backup
                // follows in its PR); vanilla stacks the three, the port adds Reset below
                // RealmsWorldsTab: World Options, World Backups and Reset/New World, three
                // 150-wide buttons 8 apart under the slots (`GridLayout.spacing(8)`).
                if *tab == ConfigureTab::Worlds
                    && let Some(Ok(detail)) = detail
                {
                    let active = detail.slots.get(&detail.active_slot);
                    let empty = active.is_none_or(|slot| slot.options.empty);
                    let minigame = server.world_type == "MINIGAME";
                    let x0 = ((w - 278.0) / 2.0).floor();
                    let y0 = 24.0 + ((h - 57.0 - 176.0) / 6.0).trunc();
                    buttons.push((
                        x0 + 64.0,
                        y0 + 100.0,
                        150.0,
                        t("mco.configure.world.buttons.options"),
                        !server.expired && !minigame,
                        94,
                        None,
                    ));
                    // `RealmsWorldsTab`: `backupButton.active = !isMinigame()`.
                    buttons.push((
                        x0 + 64.0,
                        y0 + 128.0,
                        150.0,
                        t("mco.configure.world.backup"),
                        !minigame,
                        170,
                        None,
                    ));
                    let reset = if minigame {
                        t("mco.configure.world.buttons.switchminigame")
                    } else if empty {
                        t("mco.configure.world.buttons.newworld")
                    } else {
                        t("mco.configure.world.buttons.resetworld")
                    };
                    buttons.push((
                        x0 + 64.0,
                        y0 + 156.0,
                        150.0,
                        reset,
                        !server.expired,
                        90,
                        None,
                    ));
                }
                // RealmsPlayersTab: each row's op toggle (make or remove, by the
                // player's state) and Remove, chained from the row's right edge - each
                // button 16 wider than its label (`SpriteIconButton`), 20 tall at the
                // row's middle; then Invite Player.
                if *tab == ConfigureTab::Players && matches!(detail, Some(Ok(_))) {
                    // The rows' buttons as the draw laid them out; the sprites sit to
                    // the right of each label (the port's trailing slot).
                    for ([bx, by, bw, _bh], id) in self.players_button_rects.borrow().iter() {
                        let (label, icon) = match id % 3 {
                            0 => (
                                t("mco.configure.world.invites.normal.tooltip"),
                                "player_list/make_operator",
                            ),
                            1 => (
                                t("mco.configure.world.invites.ops.tooltip"),
                                "player_list/remove_operator",
                            ),
                            _ => (
                                t("mco.configure.world.invites.remove.tooltip"),
                                "player_list/remove_player",
                            ),
                        };
                        buttons.push((*bx, *by, *bw, label, true, *id, Some((icon, 8.0))));
                    }
                    // `init`'s actions row: Invite Player and the invite codes, two
                    // default-width buttons 8 apart, bottom-aligned under the list.
                    // The tab's actions row: 8 under the list's `contentHeight - 36`
                    // (list bottom h-68), so the buttons sit at h-60.
                    buttons.push((
                        cx - 154.0,
                        h - 60.0,
                        150.0,
                        t("mco.configure.world.buttons.invite"),
                        true,
                        67,
                        None,
                    ));
                    buttons.push((
                        cx + 4.0,
                        h - 60.0,
                        150.0,
                        t("mco.configure.world.buttons.invite_codes"),
                        true,
                        68,
                        None,
                    ));
                }
            }
            SubScreen::RegionPreference { selected, .. } => {
                // `RealmsPreferredRegionSelectionScreen`: two 150-wide buttons 8 apart
                // in the default footer; Done needs a selection.
                let x = (w - 308.0) / 2.0;
                buttons.push((
                    x,
                    h - 27.0,
                    150.0,
                    t("gui.done"),
                    selected.is_some(),
                    87,
                    None,
                ));
                buttons.push((x + 158.0, h - 27.0, 150.0, t("gui.cancel"), true, 86, None));
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
                // `RealmsSlotOptionsScreen`: two 150-wide columns at content top + 41 and
                // + 69, and the footer's Continue/Cancel centred in the 33-tall footer.
                let normal = server.world_type == "NORMAL";
                let hardcore = slot.is_hardcore();
                let warning = matches!(
                    server.world_type.as_str(),
                    "ADVENTUREMAP" | "INSPIRATION" | "EXPERIENCE"
                );
                let content_top = if warning { 88.0 } else { 63.0 } + 41.0;
                let on_off = |on: bool| {
                    if on {
                        t("options.on")
                    } else {
                        t("options.off")
                    }
                };
                buttons.push((
                    cx - 154.0,
                    content_top,
                    150.0,
                    crate::lang::tr(
                        "options.generic_value",
                        &[&t("options.difficulty"), &t(DIFFICULTIES[*difficulty])],
                    ),
                    !hardcore,
                    95,
                    None,
                ));
                buttons.push((
                    cx + 4.0,
                    content_top,
                    150.0,
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
                    content_top + 28.0,
                    150.0,
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
                    cx - 154.0,
                    h - 27.0,
                    150.0,
                    t("gui.continue"),
                    job.is_none(),
                    98,
                    None,
                ));
                buttons.push((cx + 4.0, h - 27.0, 150.0, t("gui.cancel"), true, 99, None));
            }
            SubScreen::Backups {
                backups,
                restore_job,
                download_task,
                ..
            } => {
                if restore_job.is_some() || download_task.is_some() {
                    // `RealmsLongRunningMcoTaskScreen`'s only button: Cancel.
                    let top = task_stack_top((w, h));
                    buttons.push((
                        cx - 75.0,
                        top + 102.0,
                        150.0,
                        t("gui.cancel"),
                        true,
                        193,
                        None,
                    ));
                } else {
                    // `RealmsBackupScreen.init`: Download Latest and Back, two default
                    // 150-wide buttons 8 apart at h - 27. `downloadButton.active` starts
                    // false and turns true once a non-empty list is in (`fetchRealmsBackups`).
                    let can_download = matches!(backups, Some(Ok(list)) if !list.is_empty());
                    buttons.push((
                        cx - 154.0,
                        h - 27.0,
                        150.0,
                        t("mco.backup.button.download"),
                        can_download,
                        192,
                        None,
                    ));
                    buttons.push((cx + 4.0, h - 27.0, 150.0, t("gui.back"), true, 190, None));
                }
            }
            SubScreen::DownloadLatest { done, .. } => {
                // `RealmsDownloadLatestWorldScreen.init`: a 200-wide Cancel at h - 42
                // (Done once the worker answered).
                let label = if *done {
                    t("gui.done")
                } else {
                    t("gui.cancel")
                };
                buttons.push((
                    ((w - 200.0) / 2.0).floor(),
                    h - 42.0,
                    200.0,
                    label,
                    true,
                    195,
                    None,
                ));
            }
            SubScreen::BackupInfo { .. } => {
                // `RealmsBackupInfoScreen.init`: a default 150-wide Back in the footer.
                buttons.push((cx - 75.0, h - 27.0, 150.0, t("gui.back"), true, 191, None));
            }
            SubScreen::InviteCodes {
                codes,
                list_job,
                job,
                selected,
                ..
            } => {
                // `RealmsInviteCodesScreen`: a footer of two rows, Copy/Delete/Edit 97
                // wide over Create/Back 150 wide, 8 apart (`FOOTER_HEIGHT` 64).
                let fetched = matches!(codes, Some(Ok(_)));
                let count = codes
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map_or(0, |list| list.len());
                let busy = list_job.is_some() || job.is_some();
                let idle = |enabled: bool| enabled && !busy;
                let row1 = h - 56.0;
                let x0 = ((w - 307.0) / 2.0).floor();
                buttons.push((
                    x0,
                    row1,
                    97.0,
                    t("mco.configure.world.invite_codes.copy"),
                    idle(selected.is_some()),
                    180,
                    None,
                ));
                buttons.push((
                    x0 + 105.0,
                    row1,
                    97.0,
                    t("mco.configure.world.invite_codes.delete"),
                    idle(selected.is_some()),
                    181,
                    None,
                ));
                buttons.push((
                    x0 + 210.0,
                    row1,
                    97.0,
                    t("mco.configure.world.invite_codes.edit"),
                    idle(selected.is_some()),
                    182,
                    None,
                ));
                let x = ((w - 308.0) / 2.0).floor();
                buttons.push((
                    x,
                    h - 28.0,
                    150.0,
                    t("mco.configure.world.invite_codes.create"),
                    idle(fetched && count < 5),
                    160,
                    None,
                ));
                // `onClose` only runs while the back button is active; a fetch or a
                // create/delete turns it off (`setControlsActive(false)`).
                buttons.push((x + 158.0, h - 28.0, 150.0, t("gui.back"), !busy, 161, None));
            }
            SubScreen::EditCode { active, job, .. } => {
                let on_off = if *active {
                    t("options.on")
                } else {
                    t("options.off")
                };
                // `init`'s Active cycle button: the top grid's second column, 8 past the
                // 200-wide code column, vertically centred in its 22-tall row (64).
                buttons.push((
                    cx - 154.0 + 200.0 + 8.0,
                    64.0,
                    100.0,
                    format!(
                        "{}: {on_off}",
                        t("mco.configure.world.invite_codes.edit.active")
                    ),
                    job.is_none(),
                    162,
                    None,
                ));
                // The footer's 2x2 grid of 150-wide buttons in the 64-tall footer: rows
                // centred 8 in, Copy,Delete over Done,Back.
                buttons.push((
                    cx - 154.0,
                    h - 56.0,
                    150.0,
                    t("mco.configure.world.invite_codes.copy"),
                    true,
                    163,
                    None,
                ));
                buttons.push((
                    cx + 4.0,
                    h - 56.0,
                    150.0,
                    t("mco.configure.world.invite_codes.delete"),
                    job.is_none(),
                    164,
                    None,
                ));
                buttons.push((
                    cx - 154.0,
                    h - 28.0,
                    150.0,
                    t("gui.done"),
                    job.is_none(),
                    165,
                    None,
                ));
                // `setControlsActive(false)` turns Back off while a request runs.
                buttons.push((
                    cx + 4.0,
                    h - 28.0,
                    150.0,
                    t("gui.back"),
                    job.is_none(),
                    166,
                    None,
                ));
            }
            SubScreen::Invite { job, .. } => {
                // `RealmsInviteScreen.init`: the Invite button 8 under the field (104),
                // the 200-wide Back in the 33-tall footer (h - 27).
                buttons.push((
                    cx - 100.0,
                    104.0,
                    200.0,
                    t("mco.configure.world.buttons.invite"),
                    job.is_none(),
                    80,
                    None,
                ));
                buttons.push((cx - 100.0, h - 27.0, 200.0, t("gui.back"), true, 81, None));
            }
            SubScreen::AddRealm { .. } => {
                // `RealmsHeader.createLayout`: Join Realm on the left of the 308 content,
                // the invite and news buttons 4 apart on its right.
                let signed_in = session().is_some();
                let join = t("mco.selectServer.joinRealm");
                buttons.push((
                    cx - 154.0,
                    12.0,
                    self.join_width.get().max(40.0),
                    join,
                    signed_in,
                    14,
                    None,
                ));
                buttons.push((
                    cx + 154.0 - 20.0 - 20.0 - 4.0,
                    12.0,
                    20.0,
                    String::new(),
                    signed_in,
                    15,
                    Some(("icon/invite", 14.0)),
                ));
                buttons.push((
                    cx + 154.0 - 20.0,
                    12.0,
                    20.0,
                    String::new(),
                    self.news_link.is_some(),
                    16,
                    Some(("icon/news", 14.0)),
                ));
                // `RealmsPdpScreen`'s footer: two buttons 8 apart, the buy one carrying the
                // PDP's own title.
                // `RealmsPdpScreen.init`: a trial swaps the buy button for
                // Try for Free (START_REALMS_TRIAL instead of BUY_REALMS).
                let (_, label) = match &self.subscreen {
                    SubScreen::AddRealm { trial: true, .. } => (true, t("mco.pdp.tryForFree")),
                    _ => (false, t("mco.selectServer.purchase")),
                };
                buttons.push((cx - 154.0, h - 27.0, 150.0, label, true, 40, None));
                buttons.push((cx + 4.0, h - 27.0, 150.0, t("gui.back"), true, 41, None));
            }
            // `PopupScreen.buildButtonRow`: OK and the optional URL button, centered.
            SubScreen::Popup(Popup::Notice { url, .. }) => {
                // The row the draw laid out: the URL button first, OK last
                // (`RealmsNotification.InfoPopup.buildScreen`).
                let rects = self.notice_buttons.get();
                if let Some((_, label)) = url {
                    buttons.push((
                        rects[0][0],
                        rects[0][1],
                        rects[0][2],
                        label.clone(),
                        true,
                        57,
                        None,
                    ));
                }
                buttons.push((
                    rects[1][0],
                    rects[1][1],
                    rects[1][2],
                    t("gui.ok"),
                    true,
                    56,
                    None,
                ));
            }
            // `ConfirmLinkScreen.addButtons`: Open in Browser, Copy to Clipboard and Cancel,
            // 100 wide each where its message laid them out (`Choice::new`).
            SubScreen::Popup(Popup::OpenLink { .. }) => {
                let labels = [t("chat.link.open"), t("chat.copy"), t("gui.cancel")];
                for (i, (x, y)) in self.link_buttons.get().into_iter().enumerate() {
                    buttons.push((x, y, 100.0, labels[i].clone(), true, 53 + i as u8, None));
                }
            }
            // `RealmsConfirmScreen.init`: Yes and No, 100 wide, 10 apart at row(9) —
            // w/2 - 105 and w/2 + 5.
            SubScreen::Popup(Popup::Uninvite { .. }) => {
                buttons.push((cx - 105.0, 157.0, 100.0, t("gui.yes"), true, 50, None));
                buttons.push((cx + 5.0, 157.0, 100.0, t("gui.no"), true, 51, None));
            }
            // `warningAcknowledgePopupScreen`: a single OK (`buildButtonRow` gives one
            // button the full 150).
            SubScreen::Popup(Popup::DownloadOversized { .. }) => {
                let [x, y, bw, _] = self.popup_buttons.get()[0];
                buttons.push((x, y, bw, t("gui.ok"), true, 194, None));
            }
            SubScreen::Popup(_) => {
                // `PopupScreen.buildButtonRow`'s Continue and Cancel, where the draw laid
                // the row out.
                for (i, [x, y, bw, _]) in self.popup_buttons.get().into_iter().enumerate() {
                    let label = if i == 0 {
                        t("gui.continue")
                    } else {
                        t("gui.cancel")
                    };
                    buttons.push((x, y, bw, label, true, 50 + i as u8, None));
                }
            }
            SubScreen::Connecting { .. } => {
                // `RealmsLongRunningMcoTaskScreen`: the 150-wide Cancel 102 into the stack.
                let top = task_stack_top((w, h));
                buttons.push((
                    cx - 75.0,
                    top + 102.0,
                    150.0,
                    t("gui.cancel"),
                    true,
                    52,
                    None,
                ));
            }
            SubScreen::Error { .. } => {
                // `RealmsGenericErrorScreen.init`: OK, 200 wide at h - 52.
                buttons.push((cx - 100.0, h - 52.0, 200.0, t("gui.ok"), true, 91, None));
            }
            SubScreen::Terms { .. } => {
                // `RealmsTermsScreen.init`: (width / 4 - 2)-wide Agree and Disagree at
                // (width / 4, row(12)) and (width / 2 + 4, row(12)).
                let column = (w / 4.0).floor() - 2.0;
                buttons.push((
                    (w / 4.0).floor(),
                    196.0,
                    column,
                    t("mco.terms.buttons.agree"),
                    true,
                    23,
                    None,
                ));
                buttons.push((
                    (w / 2.0).floor() + 4.0,
                    196.0,
                    column,
                    t("mco.terms.buttons.disagree"),
                    true,
                    24,
                    None,
                ));
            }
            SubScreen::ResetWorld { reset, .. } => {
                if reset.is_some() {
                    // `RealmsLongRunningMcoTaskScreen`: the tasks' screen only offers Cancel.
                    let top = task_stack_top((w, h));
                    buttons.push((
                        cx - 75.0,
                        top + 102.0,
                        150.0,
                        t("gui.cancel"),
                        true,
                        88,
                        None,
                    ));
                } else {
                    buttons.push((cx - 75.0, h - 27.0, 150.0, t("gui.back"), true, 76, None));
                }
            }
            SubScreen::TemplateList {
                templates,
                selected,
                select,
                ..
            } => {
                if select.is_some() {
                    // `RealmsLongRunningMcoTaskScreen` replaced the list while the minigame
                    // switch runs: only its Cancel.
                    let top = task_stack_top((w, h));
                    buttons.push((
                        cx - 75.0,
                        top + 102.0,
                        150.0,
                        t("gui.cancel"),
                        true,
                        88,
                        None,
                    ));
                    return buttons;
                }
                // `RealmsSelectWorldTemplateScreen`'s footer: trailer, select, cancel and the
                // publisher page, four 100-wide buttons 8 apart (424 total, or 212 either
                // side of the centre), at h - 27. Trailer and publisher are `visible =
                // false` until a template with those links is selected — their slots stay
                // reserved, they just do not draw or focus.
                let chosen = selected.and_then(|i| templates.get(i));
                let x = |i: usize| cx - 212.0 + i as f32 * 108.0;
                if chosen.is_some_and(|template| template.trailer.is_some()) {
                    buttons.push((
                        x(0),
                        h - 27.0,
                        100.0,
                        t("mco.template.button.trailer"),
                        true,
                        79,
                        None,
                    ));
                }
                buttons.push((
                    x(1),
                    h - 27.0,
                    100.0,
                    t("mco.template.button.select"),
                    chosen.is_some() && select.is_none(),
                    77,
                    None,
                ));
                buttons.push((x(2), h - 27.0, 100.0, t("gui.cancel"), true, 78, None));
                if chosen.is_some_and(|template| template.link.is_some()) {
                    buttons.push((
                        x(3),
                        h - 27.0,
                        100.0,
                        t("mco.template.button.publisher"),
                        true,
                        92,
                        None,
                    ));
                }
            }
            SubScreen::CreateRealm { name, .. } => {
                // `RealmsCreateRealmScreen.init`: Continue stays inactive while the name is
                // blank; two default 150-wide buttons 8 apart in the 33-tall footer.
                let x = (w - 308.0) / 2.0;
                buttons.push((
                    x,
                    h - 27.0,
                    150.0,
                    t("gui.continue"),
                    !name.trim().is_empty(),
                    42,
                    None,
                ));
                buttons.push((x + 158.0, h - 27.0, 150.0, t("gui.back"), true, 43, None));
            }
            SubScreen::UploadSelect {
                worlds,
                selected,
                query,
                ..
            } => {
                // `RealmsSelectFileToUploadScreen.init`: two default 150-wide buttons 8
                // apart in the 33-tall footer (h - 27); Upload needs a selected row of
                // the filtered list.
                let x = (w - 308.0) / 2.0;
                let shown = worlds
                    .iter()
                    .filter(|world| upload_matches(world, query))
                    .count();
                buttons.push((
                    x,
                    h - 27.0,
                    150.0,
                    t("mco.upload.button.name"),
                    selected.is_some_and(|i| i < shown),
                    82,
                    None,
                ));
                buttons.push((x + 158.0, h - 27.0, 150.0, t("gui.back"), true, 83, None));
            }
            SubScreen::Uploading { progress, done, .. } => {
                // `RealmsUploadScreen.init`: Done/Cancel is the default 150-wide button in
                // the 33-tall footer (h - 27), and Cancel is off while the answer verifies.
                if *done {
                    buttons.push((cx - 75.0, h - 27.0, 150.0, t("gui.done"), true, 85, None));
                } else {
                    buttons.push((
                        cx - 75.0,
                        h - 27.0,
                        150.0,
                        t("gui.cancel"),
                        !progress.completed(),
                        84,
                        None,
                    ));
                }
            }
            SubScreen::CreateWorldUpload { .. } => {
                // `AlertScreen.init`: Cancel is the 150-wide button at
                // `clamp(90 + textHeight + 12, h / 6 + 96, h - 24)`, the message empty.
                let y = (h / 6.0 + 96.0).max(102.0_f32.min(h - 24.0));
                buttons.push((cx - 75.0, y, 150.0, t("gui.cancel"), true, 93, None));
            }
            SubScreen::Main => {
                let signed_in = session().is_some();
                // `RealmsHeader.createLayout`: join on the left of the 308 content, the
                // invite and news buttons 4 apart on its right.
                buttons.push((
                    left,
                    12.0,
                    self.join_width.get().max(40.0),
                    t("mco.selectServer.joinRealm"),
                    signed_in,
                    14,
                    None,
                ));
                buttons.push((
                    left + CONTENT_WIDTH - 20.0 - 20.0 - 4.0,
                    12.0,
                    20.0,
                    String::new(),
                    signed_in,
                    15,
                    Some(("icon/invite", 14.0)),
                ));
                buttons.push((
                    left + CONTENT_WIDTH - 20.0,
                    12.0,
                    20.0,
                    String::new(),
                    self.news_link.is_some(),
                    16,
                    Some(("icon/news", 14.0)),
                ));

                match self.layout_state() {
                    LayoutState::List => {
                        let server = self.selected_server();
                        let uuid = session().map(|s| s.uuid);
                        let owned = server.zip(uuid).is_some_and(|(s, u)| s.owned_by(u));
                        // `RealmsServer.shouldPlayButtonBeActive`: unexpired, open, and
                        // compatible (or an upgrade, or the player's own Realm).
                        let can_play = server.is_some_and(|s| can_play(s, uuid));
                        let expired = server.is_some_and(|s| s.expired);
                        // `updateLayout`'s footer: the grid centred in 66 puts row 1 at
                        // h-55 and row 2 24 lower.
                        let row1 = h - 55.0;
                        let row2 = h - 31.0;
                        buttons.push((
                            left,
                            row1,
                            100.0,
                            t("mco.selectServer.play"),
                            can_play,
                            1,
                            None,
                        ));
                        // `RealmsMainScreen.updateButtonStates`: a self-owned Realm
                        // that is not uninitialized.
                        buttons.push((
                            left + 104.0,
                            row1,
                            100.0,
                            t("mco.selectServer.configure"),
                            owned && server.is_some_and(|s| s.state != "UNINITIALIZED"),
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
                            self.add_realm_label(),
                            true,
                            13,
                            None,
                        ));
                        buttons.push((left + 208.0, row2, 100.0, t("gui.back"), true, 2, None));
                    }
                    LayoutState::NoRealms | LayoutState::Loading => {
                        // `updateButtonStates`: Add Realm and Join Realm are off while
                        // the list is still LOADING (a failed fetch keeps it loading).
                        let add_enabled =
                            !matches!(self.state, FetchState::Loading(_) | FetchState::Error(_));
                        if matches!(self.state, FetchState::Error(_)) {
                            buttons.push((
                                cx - 154.0,
                                h - 31.0,
                                100.0,
                                t("mco.errorMessage.retry"),
                                true,
                                17,
                                None,
                            ));
                            buttons.push((
                                cx - 50.0,
                                h - 31.0,
                                100.0,
                                self.add_realm_label(),
                                add_enabled,
                                13,
                                None,
                            ));
                            buttons.push((
                                cx + 54.0,
                                h - 31.0,
                                100.0,
                                t("gui.back"),
                                true,
                                2,
                                None,
                            ));
                        } else {
                            buttons.push((
                                cx - 102.0,
                                h - 31.0,
                                100.0,
                                self.add_realm_label(),
                                add_enabled,
                                13,
                                None,
                            ));
                            buttons.push((cx + 2.0, h - 31.0, 100.0, t("gui.back"), true, 2, None));
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
        list_focused: bool,
    ) {
        let (w, h) = (w.ceil(), h.ceil());
        let cx = (w / 2.0).floor();
        let mid = (h / 2.0).floor();
        let t = crate::lang::get;
        crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);

        match &self.subscreen {
            SubScreen::JoinCode { code, error, job } => {
                // `AbstractRealmsCodeScreen`: the band over the default 33-tall header
                // and footer, with their separators. `RealmsJoinRealmWithCodeScreen`:
                // header title, the gray "Join Code" label, a 200-wide field, then the
                // Join Realm button 8 below it.
                crate::ui::list_background(batch, atlas, 33.0, h - 33.0, false);
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    31.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    h - 33.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                centered(
                    batch,
                    atlas,
                    &t("mco.selectServer.joinCode.title"),
                    cx,
                    12.0,
                    u32::MAX,
                );
                let content_y = (h - 94.0).min(63.0);
                batch.text(
                    atlas,
                    &t("mco.selectServer.joinCode"),
                    cx - 100.0,
                    content_y,
                    GREY,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_INVITE_CODE, code, 32),
                    [cx - 100.0, content_y + 13.0, 200.0, 20.0],
                    true,
                    focused_at,
                );
                // `EditBox.setHint` draws only while the box is not focused; this screen
                // focuses the box on open, so the hint never shows.
                if job.is_some() {
                    centered(
                        batch,
                        atlas,
                        &t("mco.selectServer.joinCode.validating"),
                        cx,
                        content_y + 69.0,
                        u32::MAX,
                    );
                } else if let Some(err) = error {
                    centered(batch, atlas, err, cx, content_y + 69.0, RED);
                }
            }
            SubScreen::PendingInvites {
                invites,
                scroll,
                selected: selected_row,
                ..
            } => {
                // `HeaderAndFooterLayout.addTitleHeader`: the title centred in the 33-tall
                // header, (33 - 9) / 2 = 12.
                centered(batch, atlas, &t("mco.invites.title"), cx, 12.0, u32::MAX);
                // `PendingInvitationSelectionList` fills the contents: y 33 to h - 33,
                // its first entry at +2.
                let (top, bottom) = (33.0, h - 33.0);
                crate::ui::list_background(batch, atlas, top, bottom, false);
                // `AbstractSelectionList.extractListSeparators`.
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    top - 2.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    bottom,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                match invites {
                    None => {}
                    // `extractRenderState`: once the answer is done, an empty list — a
                    // failed fetch included — shows the no-pending text.
                    Some(Err(_)) => {
                        centered(
                            batch,
                            atlas,
                            &t("mco.invites.nopending"),
                            cx,
                            mid - 20.0,
                            u32::MAX,
                        );
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        centered(
                            batch,
                            atlas,
                            &t("mco.invites.nopending"),
                            cx,
                            mid - 20.0,
                            u32::MAX,
                        );
                    }
                    Some(Ok(list)) => {
                        // `PendingInvitationSelectionList`: 280-wide rows of 36.
                        let rx = ((w - 280.0) / 2.0).floor();
                        let content = list.len() as f32 * ROW_HEIGHT;
                        let first = batch.vertices.len();
                        // `StringWidget.setMaxWidth(maxTextWidth)`: 280 - 32 - 32 - 42.
                        let max_text = 174.0;
                        for (i, invite) in list.iter().enumerate() {
                            let y = top + 2.0 + i as f32 * ROW_HEIGHT - scroll.amount;
                            if y + ROW_HEIGHT <= top || y >= bottom {
                                continue;
                            }
                            if *selected_row == Some(i) {
                                // `AbstractSelectionList.extractSelection`: the entry's
                                // 280x36 rect, white while the list is focused.
                                batch.fill(
                                    atlas,
                                    rx,
                                    y,
                                    280.0,
                                    ROW_HEIGHT,
                                    if list_focused {
                                        0xFFFF_FFFF
                                    } else {
                                        0xFF80_8080
                                    },
                                );
                                batch.fill(
                                    atlas,
                                    rx + 1.0,
                                    y + 1.0,
                                    278.0,
                                    ROW_HEIGHT - 2.0,
                                    0xFF00_0000,
                                );
                            }
                            // `Entry.extractContent`: the owner's 32px face at the content
                            // origin (`extractPlayerFace`), the text 40 in, at +3, +14
                            // and +26.
                            let face = self.face_sprite(
                                batch,
                                atlas,
                                invite.world_owner_uuid.as_deref(),
                                i,
                            );
                            batch.sprite_scaled(
                                atlas,
                                face.as_deref().unwrap_or("misc/unknown_server"),
                                rx + 2.0,
                                y + 2.0,
                                32.0,
                                32.0,
                                u32::MAX,
                            );
                            batch.text(
                                atlas,
                                &clamp_text(atlas, &invite.world_name, max_text),
                                rx + 40.0,
                                y + 3.0,
                                u32::MAX,
                                true,
                            );
                            batch.text(
                                atlas,
                                &clamp_text(atlas, &invite.world_owner_name, max_text),
                                rx + 40.0,
                                y + 14.0,
                                GREY,
                                true,
                            );
                            // `RealmsUtil.convertToAgePresentationFromInstant`.
                            let diff = crate::hud_overlays::now_ms() - invite.date;
                            let age = if diff < 0 {
                                t("mco.time.now")
                            } else {
                                let seconds = diff / 1000;
                                let key = if seconds < 60 {
                                    ("mco.time.secondsAgo", seconds)
                                } else if seconds < 3600 {
                                    ("mco.time.minutesAgo", seconds / 60)
                                } else if seconds < 86400 {
                                    ("mco.time.hoursAgo", seconds / 3600)
                                } else {
                                    ("mco.time.daysAgo", seconds / 86400)
                                };
                                crate::lang::tr(key.0, &[&key.1 as &dyn std::fmt::Display])
                            };
                            batch.text(
                                atlas,
                                &clamp_text(atlas, &age, max_text),
                                rx + 40.0,
                                y + 26.0,
                                GREY,
                                true,
                            );
                        }
                        batch.clip_since(first, 0.0, top, w, bottom);
                        crate::ui::scrollbar(
                            batch,
                            atlas,
                            rx + 280.0 + 8.0,
                            top,
                            bottom,
                            content,
                            scroll.amount,
                        );
                    }
                }
            }
            SubScreen::Backups {
                server,
                backups,
                restore_job,
                download_task,
                selected,
                scroll,
                ..
            } => {
                // `RestoreTask`/`DownloadTask` runs under `RealmsLongRunningMcoTaskScreen`,
                // which *replaces* the backup screen: only the logo, the task's title and
                // the dots show. Nothing of the list may leak through behind them.
                if restore_job.is_some() || download_task.is_some() {
                    crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);
                    let top = task_stack_top((w, h));
                    realms_logo(batch, atlas, cx, top);
                    let title = if download_task.is_some() {
                        t("mco.download.preparing")
                    } else {
                        t("mco.backup.restoring")
                    };
                    centered(batch, atlas, &title, cx, top + 46.0, u32::MAX);
                    centered(batch, atlas, loading_dots(), cx, top + 61.0, 0xFF80_8080);
                    return;
                }
                // `RealmsBackupScreen`: a 33-tall title header, a 36-tall row per backup
                // (content from 33 to h-33), and the two footer buttons.
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.backup"),
                    cx,
                    12.0,
                    u32::MAX,
                );
                let (top, bottom) = (33.0, h - 33.0);
                crate::ui::list_background(batch, atlas, top, bottom, false);
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    top - 2.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    bottom,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                match backups {
                    Some(Ok(list)) if list.is_empty() => {
                        // `noBackups`: centred in the list, white.
                        centered(
                            batch,
                            atlas,
                            &t("mco.backup.nobackups"),
                            cx,
                            top + (bottom - top) / 2.0 - 4.0,
                            u32::MAX,
                        );
                    }
                    Some(Ok(list)) => {
                        let changes = backup_changes(list);
                        let rx = ((w - ROW_WIDTH) / 2.0).floor();
                        let content = list.len() as f32 * ROW_HEIGHT;
                        let changes_label = t("mco.backup.changes.tooltip");
                        let restore_label = t("mco.backup.button.restore");
                        let changes_w = 8.0 + HudBatch::text_width(atlas, &changes_label);
                        // Vanilla measures the Restore button with the changes label's
                        // width too (`8 + font.width(HAS_CHANGES_TOOLTIP)`).
                        let restore_w = changes_w;
                        let first = batch.vertices.len();
                        let mut rects = Vec::new();
                        for (i, backup) in list.iter().enumerate() {
                            let y = top + 2.0 + i as f32 * ROW_HEIGHT - scroll.amount;
                            if y + ROW_HEIGHT <= top || y >= bottom {
                                continue;
                            }
                            if *selected == Some(i) {
                                batch.fill(
                                    atlas,
                                    rx,
                                    y,
                                    ROW_WIDTH,
                                    ROW_HEIGHT,
                                    if list_focused {
                                        0xFFFF_FFFF
                                    } else {
                                        0xFF80_8080
                                    },
                                );
                                batch.fill(
                                    atlas,
                                    rx + 1.0,
                                    y + 1.0,
                                    ROW_WIDTH - 2.0,
                                    ROW_HEIGHT - 2.0,
                                    0xFF00_0000,
                                );
                            }
                            // `Entry.extractContent`: the age line 7 under the row top in
                            // -8388737 for an uploaded version, then the short date in
                            // -11776948 13 lower.
                            let age = age_presentation(backup.last_modified);
                            let line = crate::lang::tr(
                                "mco.backup.entry",
                                &[&age as &dyn std::fmt::Display],
                            );
                            batch.text(
                                atlas,
                                &line,
                                rx + 2.0,
                                y + 7.0,
                                if changes[i].contains_key("uploaded") {
                                    0xFF7F_FF7F
                                } else {
                                    u32::MAX
                                },
                                true,
                            );
                            batch.text(
                                atlas,
                                &short_date_text(backup.last_modified),
                                rx + 2.0,
                                y + 20.0,
                                0xFF4C_4C4C,
                                true,
                            );
                            // The Restore button is rightmost, the Changes one 8 left of
                            // it (`iconXOffset` chains them from contentRight).
                            let mut right = rx + ROW_WIDTH - 2.0;
                            let mut row_rects = Vec::new();
                            // A row's ids are 210 + 2i and 211 + 2i, so 22 rows fit a u8.
                            if i <= 22 {
                                if !server.expired {
                                    let x = right - restore_w - 8.0;
                                    row_rects
                                        .push(([x, y + 8.0, restore_w, 20.0], 211 + 2 * i as u8));
                                    right = x - 8.0;
                                }
                                if !changes[i].is_empty() {
                                    let x = right - changes_w - 8.0;
                                    row_rects
                                        .push(([x, y + 8.0, changes_w, 20.0], 210 + 2 * i as u8));
                                }
                            }
                            for ([bx, by, bw, _], id) in &row_rects {
                                let label = if *id % 2 == 0 {
                                    &changes_label
                                } else {
                                    &restore_label
                                };
                                let hovered = crate::ui::hovered(cursor, *bx, *by, *bw, 20.0);
                                crate::ui::button(
                                    batch,
                                    atlas,
                                    *bx,
                                    *by,
                                    *bw,
                                    20.0,
                                    &Text::literal(label),
                                    true,
                                    hovered,
                                );
                            }
                            rects.extend(row_rects);
                        }
                        *self.backup_row_buttons.borrow_mut() = rects;
                        batch.clip_since(first, 0.0, top, w, bottom);
                        crate::ui::scrollbar(
                            batch,
                            atlas,
                            rx + ROW_WIDTH + 8.0,
                            top,
                            bottom,
                            content,
                            scroll.amount,
                        );
                    }
                    // `fetchRealmsBackups` only logs a failure: the list stays empty.
                    Some(Err(_)) => {}
                    None => centered(
                        batch,
                        atlas,
                        &t("mco.configure.world.loading"),
                        cx,
                        60.0,
                        u32::MAX,
                    ),
                }
            }
            SubScreen::DownloadLatest {
                progress,
                job,
                error,
                done,
                started,
                speed,
                world_name,
                ..
            } => {
                // `RealmsDownloadLatestWorldScreen.extractRenderState`: the title at 20,
                // the status at 50 with its dots, the 200-wide bar at 80-95 with the
                // percentage inside and the speed to its right, the failure at 110 in red.
                centered(batch, atlas, &t("mco.download.title"), cx, 20.0, u32::MAX);
                let status = match error {
                    Some(_) => t("mco.download.failed"),
                    None if *done => t("mco.download.done"),
                    None if progress.is_extracting() => t("mco.download.extracting"),
                    None if job.is_some() => {
                        crate::lang::tr("mco.download.downloading", &[&world_name])
                    }
                    None => t("mco.download.preparing"),
                };
                centered(batch, atlas, &status, cx, 50.0, u32::MAX);
                if !*done && error.is_none() {
                    // `extractDots`: one dot step a tick count's ten (half a second).
                    let dots = ["", ".", ". .", ". . ."];
                    let index = (started.elapsed().as_millis() / 500) as usize % dots.len();
                    let width = HudBatch::text_width(atlas, &status);
                    batch.text(
                        atlas,
                        dots[index],
                        cx + width / 2.0 + 5.0,
                        50.0,
                        u32::MAX,
                        true,
                    );
                }
                // `if (downloadStatus.bytesWritten != 0 && !cancelled)`.
                if progress.written() != 0 && !progress.is_cancelled() {
                    let percentage = progress.percentage();
                    let left = (w - 200.0) / 2.0;
                    let right = left + (200.0 * percentage as f32).round();
                    batch.fill(atlas, left - 1.0, 79.0, right - left + 2.0, 17.0, u32::MAX);
                    batch.fill(
                        atlas,
                        left,
                        80.0,
                        (right - left).max(0.0),
                        15.0,
                        0xFF80_8080,
                    );
                    let percent = format!("{:.1}", percentage * 100.0);
                    centered(
                        batch,
                        atlas,
                        &crate::lang::tr("mco.download.percent", &[&percent]),
                        cx,
                        84.0,
                        u32::MAX,
                    );
                    // `extractDownloadSpeed`: a sample every 20 ticks (a second), drawn
                    // beside the percentage while it is not 0.
                    let (last_written, last_sample, last_rate) = speed.get();
                    let now = Instant::now();
                    let rate = if now.duration_since(last_sample) >= Duration::from_secs(1) {
                        let elapsed = now.duration_since(last_sample).as_millis().max(1) as u64;
                        let rate = 1000 * progress.written().saturating_sub(last_written) / elapsed;
                        speed.set((progress.written(), now, rate));
                        rate
                    } else {
                        last_rate
                    };
                    if rate > 0 {
                        let label = crate::lang::tr("mco.download.speed", &[&human_readable(rate)]);
                        let percent_w = HudBatch::text_width(atlas, &percent);
                        batch.text(
                            atlas,
                            &label,
                            cx + (percent_w / 2.0).floor() + 15.0,
                            84.0,
                            u32::MAX,
                            true,
                        );
                    }
                }
                if error.is_some() {
                    centered(batch, atlas, &t("mco.download.failed"), cx, 110.0, RED);
                }
            }
            SubScreen::BackupInfo { backup, scroll, .. } => {
                // `RealmsBackupInfoScreen`: the changed metadata's key above its value,
                // 36-tall rows in the default 220-wide list, from 33 to h-33.
                centered(
                    batch,
                    atlas,
                    &t("mco.backup.info.title"),
                    cx,
                    12.0,
                    u32::MAX,
                );
                let (top, bottom) = (33.0, h - 33.0);
                crate::ui::list_background(batch, atlas, top, bottom, false);
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    top - 2.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    bottom,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                let rx = ((w - 220.0) / 2.0).floor();
                let entries: Vec<(&String, &String)> = backup.change_list.iter().collect();
                let content = entries.len() as f32 * 36.0;
                let first = batch.vertices.len();
                for (i, (key, value)) in entries.iter().enumerate() {
                    let y = top + 2.0 + i as f32 * 36.0 - scroll.amount;
                    if y + 36.0 <= top || y >= bottom {
                        continue;
                    }
                    batch.text(
                        atlas,
                        &backup_info_key(key.as_str()),
                        rx + 2.0,
                        y + 2.0,
                        GREY,
                        true,
                    );
                    batch.text(
                        atlas,
                        &backup_info_value(key.as_str(), value.as_str()),
                        rx + 2.0,
                        y + 14.0,
                        u32::MAX,
                        true,
                    );
                }
                batch.clip_since(first, 0.0, top, w, bottom);
                crate::ui::scrollbar(
                    batch,
                    atlas,
                    rx + 220.0 + 8.0,
                    top,
                    bottom,
                    content,
                    scroll.amount,
                );
            }
            SubScreen::Configure {
                server,
                tab,
                subscription,
                detail,
                settings,
                players_scroll,
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
                    // Painted by hand: the bumpers look for these.
                    crate::widgets::target_as(
                        [x, 0.0, tab_w, TAB_HEIGHT],
                        crate::widgets::Kind::Tab(selected),
                    );
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
                        // The underline is the label's width, at most tabWidth - 4,
                        // centred in the tab.
                        let label_w =
                            HudBatch::text_width(atlas, &t(which.title())).min(tab_w - 4.0);
                        batch.fill(
                            atlas,
                            x + ((tab_w - label_w) / 2.0).floor(),
                            22.0,
                            label_w,
                            1.0,
                            u32::MAX,
                        );
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
                    // `setTabTooltip(3, mco.configure.world.settings.expired)` is drawn by
                    // `draw_tooltip`, after every widget and the scrollbar.
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
                match tab {
                    ConfigureTab::Subscription => match subscription {
                        Some(Ok(sub)) => {
                            // `RealmsSubscriptionTab`: one 200-wide column of 9-tall rows,
                            // `rowSpacing(6)` and a 2-tall spacer before each button, in
                            // `GridLayoutTab`'s 1/6 alignment. Row tops: start 0, date 15,
                            // days label 38, days 53, extend 76, delete 110, info 144.
                            let info_lines = wrap_lines(
                                &t("mco.configure.world.subscription.recurring.info"),
                                atlas,
                                192.0,
                            );
                            let info_h = info_lines.len() as f32 * 9.0 + 8.0;
                            let rows_h = 144.0 + info_h;
                            let y0 = settings_column_top(h, rows_h);
                            subscription_label(
                                batch,
                                atlas,
                                cx - 100.0,
                                y0,
                                &t("mco.configure.world.subscription.start"),
                                u32::MAX,
                            );
                            subscription_label(
                                batch,
                                atlas,
                                cx - 100.0,
                                y0 + 15.0,
                                &start_date_text(sub.start_date),
                                GREY,
                            );
                            let days_label = if sub.kind == "RECURRING" {
                                t("mco.configure.world.subscription.recurring.daysleft")
                            } else {
                                t("mco.configure.world.subscription.timeleft")
                            };
                            subscription_label(
                                batch,
                                atlas,
                                cx - 100.0,
                                y0 + 38.0,
                                &days_label,
                                u32::MAX,
                            );
                            subscription_label(
                                batch,
                                atlas,
                                cx - 100.0,
                                y0 + 53.0,
                                &days_left_text(sub.days_left, server.expired),
                                GREY,
                            );
                            // `updateData`: the recurring info is plain white, inside its
                            // `FocusableTextWidget` — backgroundFill ALWAYS (black, 4
                            // around the text) with the 1px border it always shows
                            // (grey until focused, which this screen never is). The text
                            // is left-aligned (`setCentered(false)`).
                            let panel = (cx - 100.0, y0 + 144.0, 200.0, info_h);
                            batch.fill(
                                atlas,
                                panel.0 + 1.0,
                                panel.1,
                                panel.2 - 1.0,
                                panel.3,
                                0xFF00_0000,
                            );
                            batch.fill(atlas, panel.0, panel.1, panel.2, 1.0, GREY);
                            batch.fill(atlas, panel.0, panel.1 + panel.3 - 1.0, panel.2, 1.0, GREY);
                            batch.fill(atlas, panel.0, panel.1, 1.0, panel.3, GREY);
                            batch.fill(atlas, panel.0 + panel.2 - 1.0, panel.1, 1.0, panel.3, GREY);
                            for (i, line) in info_lines.iter().enumerate() {
                                subscription_label(
                                    batch,
                                    atlas,
                                    cx - 96.0,
                                    y0 + 148.0 + i as f32 * 9.0,
                                    line,
                                    u32::MAX,
                                );
                            }
                            self.subscription_buttons.set((y0 + 76.0, y0 + 110.0));
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
                            // `RealmsWorldsTab`: four 80/50 square slots 16 apart, the
                            // active one 80 (slot 4 on a minigame Realm), bottom-aligned.
                            let x0 = ((w - 278.0) / 2.0).floor();
                            let y0 = 24.0 + ((h - 57.0 - 176.0) / 6.0).trunc();
                            let big = if detail.world_type == "MINIGAME" {
                                4
                            } else {
                                detail.active_slot
                            };
                            let mut x = x0;
                            for slot in 1..=4 {
                                let active = slot == big;
                                let size = if active { 80.0 } else { 50.0 };
                                let y = if active { y0 } else { y0 + 30.0 };
                                let entry = detail.slots.get(&slot);
                                // `RealmsWorldSlotButton.State`: slot 4 is the minigame —
                                // its emptiness and image come from `minigameId`/
                                // `minigameImage`, and it is never hardcore.
                                let (empty, hardcore) = if slot == 4 {
                                    (server.minigame_id.is_none_or(|id| id == -1), false)
                                } else {
                                    (
                                        entry.is_none_or(|slot| slot.options.empty),
                                        entry.is_some_and(|slot| slot.is_hardcore()),
                                    )
                                };
                                // `RealmsWorldSlotButton.State`'s picture: the minigame's
                                // image or `isles.png`, the empty frame, the slot's own
                                // template image, or the slot's panorama (the atlas keeps
                                // the four slot pictures at full size).
                                let image = if slot == 4 {
                                    server.minigame_image.as_deref()
                                } else {
                                    entry.and_then(|entry| {
                                        (entry.options.template_id != -1)
                                            .then_some(entry.options.template_image.as_deref())
                                            .flatten()
                                    })
                                };
                                let tint = if active { u32::MAX } else { 0xFF8E_8E8E };
                                match slot_picture(slot, empty, image) {
                                    SlotPicture::Empty => crate::ui::blit_sprite(
                                        batch,
                                        atlas,
                                        "gui/realms/empty_frame",
                                        x + 1.0,
                                        y + 1.0,
                                        size - 2.0,
                                        size - 2.0,
                                        tint,
                                    ),
                                    SlotPicture::Image(encoded) => {
                                        let key = format!("slot-{slot}-{}", encoded_key(encoded));
                                        let pixels = {
                                            let mut cache = self.template_images.borrow_mut();
                                            cache
                                                .entry(key)
                                                .or_insert_with(|| template_image(encoded))
                                                .clone()
                                        };
                                        if let Some(sprite) = pixels.as_deref().and_then(|rgba| {
                                            icon_sprite(batch, atlas, slot as usize - 1, rgba)
                                        }) {
                                            batch.sprite_scaled(
                                                atlas,
                                                &sprite,
                                                x + 1.0,
                                                y + 1.0,
                                                size - 2.0,
                                                size - 2.0,
                                                tint,
                                            );
                                        }
                                    }
                                    SlotPicture::Preset => batch.picture(
                                        atlas,
                                        "gui/presets/isles",
                                        x + 1.0,
                                        y + 1.0,
                                        size - 2.0,
                                        size - 2.0,
                                        tint,
                                    ),
                                    SlotPicture::Panorama(panorama) => batch.picture(
                                        atlas,
                                        &format!("gui/title/background/{panorama}"),
                                        x + 1.0,
                                        y + 1.0,
                                        size - 2.0,
                                        size - 2.0,
                                        tint,
                                    ),
                                }
                                // `RealmsWorldSlotButton.extractContents`: a switchable slot
                                // shows the white frame while hovered, the active one its
                                // 0.8 grey, the rest 0.56 grey.
                                let switchable = !active && !(server.expired && empty);
                                let frame =
                                    if switchable && crate::ui::hovered(cursor, x, y, size, size) {
                                        u32::MAX
                                    } else if active {
                                        0xFFCC_CCCC
                                    } else {
                                        0xFF8E_8E8E
                                    };
                                crate::ui::blit_sprite(
                                    batch,
                                    atlas,
                                    "gui/sprites/widget/slot_frame",
                                    x,
                                    y,
                                    size,
                                    size,
                                    frame,
                                );
                                // `RealmsWorldSlotButton.extractContents`: the hardcore
                                // heart 9x8 at x + 3, y + 4.
                                if hardcore
                                    && atlas
                                        .sprites
                                        .contains_key("gui/sprites/hud/heart/hardcore_full")
                                {
                                    crate::ui::blit_sprite(
                                        batch,
                                        atlas,
                                        "gui/sprites/hud/heart/hardcore_full",
                                        x + 3.0,
                                        y + 4.0,
                                        9.0,
                                        8.0,
                                        u32::MAX,
                                    );
                                }
                                let name = if slot == 4 {
                                    t("mco.worldSlot.minigame")
                                } else {
                                    entry
                                        .map(|entry| slot_name(&entry.options, slot))
                                        .unwrap_or_else(|| t("mco.configure.world.slot.empty"))
                                };
                                // `slotNameWidget.setMaxWidth(width - (active ? 2 : 0))`:
                                // a long name never leaves its frame.
                                let name =
                                    clamp_text(atlas, &name, size - if active { 2.0 } else { 0.0 });
                                let tw = HudBatch::text_width(atlas, &name);
                                batch.text(
                                    atlas,
                                    &name,
                                    x + ((size - tw) / 2.0).floor(),
                                    y + size - 14.0,
                                    u32::MAX,
                                    true,
                                );
                                if active
                                    && let Some(entry) = entry
                                    && !entry.options.version.is_empty()
                                {
                                    let version = &entry.options.version;
                                    // `RealmsMainScreen.getVersionComponent`: grey when
                                    // COMPATIBLE, -2142128 otherwise.
                                    let color = if entry.options.compatibility == "COMPATIBLE" {
                                        GREY
                                    } else {
                                        0xFF50_50DF
                                    };
                                    let vw = HudBatch::text_width(atlas, version);
                                    batch.text(
                                        atlas,
                                        version,
                                        x + ((size - vw) / 2.0).floor(),
                                        y + size + 2.0,
                                        color,
                                        true,
                                    );
                                }
                                x += size + 16.0;
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
                            // `RealmsPlayersTab`: its `GridLayout` places the list in the
                            // tab area's first row (`contentHeight - 36` tall) with the
                            // 28-tall actions row 8 under it; the 1/6 alignment puts the
                            // list at 25, its first entry at 27, the 17-tall "Invited: N"
                            // header next and the 36-tall player rows from 44. The list
                            // draws no band or separators (both overrides are empty).
                            let rx = ((w - 300.0) / 2.0).floor();
                            let list_top = 25.0;
                            let list_bottom = list_top + (h - 93.0);
                            let top = 27.0;
                            let heading = Text::translate(
                                "mco.configure.world.invited.number",
                                &[Text::literal(detail.players.len().to_string())],
                            )
                            .underlined();
                            // `invitedWidget.getWidth()`: the formatted message's width.
                            let hw = HudBatch::text_width(
                                atlas,
                                &crate::lang::tr(
                                    "mco.configure.world.invited.number",
                                    &[&detail.players.len() as &dyn std::fmt::Display],
                                ),
                            );
                            batch.text_styled(
                                atlas,
                                &heading,
                                rx + 150.0 - (hw / 2.0).floor(),
                                top + 4.0,
                                u32::MAX,
                                true,
                            );
                            // `InvitedObjectSelectionList`: rows of 36 from the header's
                            // 17; the scroll keeps the lower players reachable.
                            // `PlayerEntry`: each row's op toggle and Remove, chained
                            // from the row's right edge - each 16 wider than its label,
                            // 20 tall at the row's middle; the op toggle swaps its
                            // sprite and label with the player's state.
                            let mut rects = Vec::new();
                            for (i, player) in detail.players.iter().enumerate() {
                                let row_y = top + 17.0 + i as f32 * 36.0 - players_scroll.amount;
                                if row_y + 36.0 <= list_top || row_y >= list_bottom {
                                    continue;
                                }
                                let id = 100 + i as u8 * 3;
                                let (op_key, op_icon) = if player.operator {
                                    (
                                        "mco.configure.world.invites.ops.tooltip",
                                        "player_list/remove_operator",
                                    )
                                } else {
                                    (
                                        "mco.configure.world.invites.normal.tooltip",
                                        "player_list/make_operator",
                                    )
                                };
                                let _ = op_icon;
                                let op_label = t(op_key);
                                let remove_label = t("mco.configure.world.invites.remove.tooltip");
                                let op_w = 16.0 + HudBatch::text_width(atlas, &op_label);
                                let remove_w = 16.0 + HudBatch::text_width(atlas, &remove_label);
                                // `getContentRight()` is the row's right edge - 2.
                                let remove_x = rx + 298.0 - remove_w;
                                let remove_y = row_y + 8.0;
                                if row_y + 36.0 > list_top {
                                    rects.push(([remove_x, remove_y, remove_w, 20.0], id + 2));
                                    rects.push((
                                        [remove_x - op_w - 8.0, remove_y, op_w, 20.0],
                                        if player.operator { id + 1 } else { id },
                                    ));
                                }
                            }
                            *self.players_button_rects.borrow_mut() = rects;
                            let first = batch.vertices.len();
                            for (i, player) in detail.players.iter().enumerate() {
                                let row_y = top + 17.0 + i as f32 * 36.0 - players_scroll.amount;
                                if row_y + 36.0 <= list_top || row_y >= list_bottom {
                                    continue;
                                }
                                // `PlayerEntry.extractContent`: the 32px face at the
                                // content origin, the name at contentX + 40 centred in
                                // the row — grey before accepting, green while online,
                                // white otherwise.
                                let face =
                                    self.face_sprite(batch, atlas, player.uuid.as_deref(), i);
                                batch.sprite_scaled(
                                    atlas,
                                    face.as_deref().unwrap_or("misc/unknown_server"),
                                    rx + 2.0,
                                    row_y + 2.0,
                                    32.0,
                                    32.0,
                                    u32::MAX,
                                );
                                let color = if !player.accepted {
                                    GREY
                                } else if player.online {
                                    0xFF00_FF00
                                } else {
                                    u32::MAX
                                };
                                let text_y = row_y + 2.0 + 16.0 - 4.0;
                                batch.text(atlas, &player.name, rx + 42.0, text_y, color, true);
                            }
                            let content = detail.players.len() as f32 * 36.0 + 17.0;
                            batch.clip_since(first, rx, list_top, rx + 300.0, list_bottom);
                            crate::ui::scrollbar(
                                batch,
                                atlas,
                                rx + 308.0,
                                list_top,
                                list_bottom,
                                content,
                                players_scroll.amount,
                            );
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
                            // `RealmsSettingsTab`: its `GridLayout` pads every cell by 8,
                            // so the rows are 49 apart (the region cell 58), and the whole
                            // column is centred at 1/6 of the tab area's leftover height
                            // inside a `ScrollableLayout` that scrolls a short window.
                            let (rows, content_h) = settings_rows(matches!(tier, Some(Ok(_))));
                            let top = settings_column_top(h, content_h);
                            let view_top = 24.0;
                            let view_bottom = h - 33.0;
                            let max_scroll = (content_h - (view_bottom - view_top)).max(0.0);
                            let scroll = self.settings_scroll.amount.min(max_scroll);
                            let left = cx - 106.0;
                            let label_y =
                                |row: usize| top + rows[row] + SETTINGS_CELL_PADDING - scroll;
                            let widget_y = |row: usize| {
                                label_y(row) + SETTINGS_LABEL_HEIGHT + SETTINGS_LABEL_SPACING
                            };
                            let first = batch.vertices.len();
                            // `isRealmNameValid`: an empty or space-padded name turns the
                            // field red and carries the validation tooltip.
                            let name_ok = realm_name_valid(&draft.name);
                            batch.text(
                                atlas,
                                &t("mco.configure.world.name"),
                                left,
                                label_y(0),
                                GREY,
                                true,
                            );
                            let name_rect = [left, widget_y(0), 212.0, 20.0];
                            caret.draw_color(
                                batch,
                                atlas,
                                (REALMS_SETTINGS_NAME, &draft.name, 32),
                                name_rect,
                                !draft.on_description,
                                focused_at,
                                if name_ok { 0xFFE0_E0E0 } else { 0xFFDF_5050 },
                            );
                            // The validation tooltip is drawn by `draw_tooltip`, after
                            // every widget and the scrollbar.
                            batch.text(
                                atlas,
                                &t("mco.configure.world.description"),
                                left,
                                label_y(1),
                                GREY,
                                true,
                            );
                            caret.draw(
                                batch,
                                atlas,
                                (REALMS_SETTINGS_DESC, &draft.description, 32),
                                [left, widget_y(1), 212.0, 20.0],
                                draft.on_description,
                                focused_at,
                            );
                            if let Some(Ok(config)) = tier.as_ref() {
                                // `createDistanceOption`: a slider over the range, its value
                                // as `options.chunks`, under its caption. The button's
                                // `cachedConstantTooltip` is drawn by `draw_tooltip`, after
                                // the scrollbar and every other widget.
                                for (row, range, picked, label_key) in [
                                    (
                                        2,
                                        config.render_distance.as_ref(),
                                        draft.render_distance,
                                        "mco.configure.world.renderDistance",
                                    ),
                                    (
                                        3,
                                        config.sim_distance.as_ref(),
                                        draft.simulation_distance,
                                        "mco.configure.world.simulationDistance",
                                    ),
                                ] {
                                    let Some((range, picked)) = range.zip(picked) else {
                                        continue;
                                    };
                                    let value = (picked - range.min) as f32
                                        / (range.max - range.min) as f32;
                                    // `CommonLayouts.labeledElement`: the caption above, the
                                    // button showing the value alone (`options.chunks`).
                                    batch.text(
                                        atlas,
                                        &t(label_key),
                                        left,
                                        label_y(row),
                                        GREY,
                                        true,
                                    );
                                    let text = crate::lang::tr(
                                        "options.chunks",
                                        &[&picked as &dyn std::fmt::Display],
                                    );
                                    let y = widget_y(row);
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
                                }
                            }
                            // `getTranslatableFromPreference`: the chosen region (or the
                            // automatic mode) in grey; a manual choice shows its
                            // `ServiceQuality` icon 10x8 at the line's end (`202` into the
                            // 212-wide cell) and only then.
                            batch.text(
                                atlas,
                                &t("mco.configure.world.region_preference"),
                                left,
                                label_y(4),
                                GREY,
                                true,
                            );
                            let current =
                                preference_label(draft.preference, draft.region.as_deref());
                            let line_y = widget_y(4);
                            batch.text(atlas, &current, left, line_y, GREY, true);
                            if draft.preference == client::MANUAL {
                                let quality = regions
                                    .as_ref()
                                    .and_then(|answer| answer.as_ref().ok())
                                    .and_then(|list| {
                                        list.iter().find(|region| {
                                            draft.region.as_deref() == Some(region.region.as_str())
                                        })
                                    })
                                    .map_or(5, |region| region.service_quality);
                                let icon = match quality {
                                    1 => "gui/sprites/icon/ping_5",
                                    2 => "gui/sprites/icon/ping_4",
                                    3 => "gui/sprites/icon/ping_3",
                                    4 => "gui/sprites/icon/ping_2",
                                    _ => "gui/sprites/icon/ping_unknown",
                                };
                                crate::ui::blit_sprite(
                                    batch,
                                    atlas,
                                    icon,
                                    left + 202.0,
                                    line_y,
                                    10.0,
                                    8.0,
                                    u32::MAX,
                                );
                            }
                            if saving.is_some() {
                                centered(
                                    batch,
                                    atlas,
                                    &t("mco.configure.world.loading"),
                                    cx,
                                    view_bottom - 12.0,
                                    u32::MAX,
                                );
                            }
                            batch.clip_since(first, 0.0, view_top, w, view_bottom);
                            // `ScrollableLayout`'s scrollbar sits 4 past the 212-wide
                            // content (the container reserves 4 + a 6-wide bar).
                            if content_h > view_bottom - view_top {
                                crate::ui::scrollbar(
                                    batch,
                                    atlas,
                                    left + 224.0,
                                    view_top,
                                    view_bottom,
                                    content_h,
                                    scroll,
                                );
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
                regions,
                selected,
                scroll,
                ..
            } => {
                // `RealmsPreferredRegionSelectionScreen`: the preference rows in the
                // 220-wide list (rows of 16 from y 40), then the MANUAL rows per region;
                // the text sits at (rowLeft + 7, rowTop + 4) with the region's
                // `ServiceQuality` icon 10x8 at rowLeft + 200.
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.region_preference.title"),
                    cx,
                    12.0,
                    u32::MAX,
                );
                // The list fills the contents (33..h-33), its first entry at +2.
                let top = 33.0;
                let bottom = h - 33.0;
                crate::ui::list_background(batch, atlas, top, bottom, false);
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    top - 2.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    bottom,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                let rows = region_rows(regions);
                let left = ((w - 220.0) / 2.0).floor();
                let content = rows.len() as f32 * 16.0;
                crate::ui::scrollbar(
                    batch,
                    atlas,
                    left + 220.0 + 8.0,
                    top,
                    bottom,
                    content,
                    scroll.amount,
                );
                // `AbstractSelectionList` scissors its items to the list: a row that
                // pokes past either separator stays inside (`extractListItems`).
                let first = batch.vertices.len();
                for (i, (preference, region)) in rows.iter().enumerate() {
                    let y = top + 2.0 + i as f32 * 16.0 - scroll.amount;
                    if y + 16.0 <= top || y >= bottom {
                        continue;
                    }
                    let picked = selected
                        .as_ref()
                        .is_some_and(|(p, r)| p == preference && *r == *region);
                    let label = preference_label(*preference, region.as_deref());
                    if picked {
                        // `AbstractSelectionList.extractSelection`: the entry black, its
                        // outline white (the list is focused).
                        batch.fill(atlas, left, y, 220.0, 16.0, 0xFFFF_FFFF);
                        batch.fill(atlas, left + 1.0, y + 1.0, 218.0, 14.0, 0xFF00_0000);
                    }
                    batch.text(atlas, &label, left + 7.0, y + 4.0, u32::MAX, true);
                    // `Entry.extractContent`: the region's `ServiceQuality` icon, 10x8 at
                    // rowLeft + 200 (`ping_5` great to `ping_2` poor, `ping_unknown`).
                    if let Some(region) = region
                        && let Some(quality) = regions
                            .iter()
                            .find(|data| &data.region == region)
                            .map(|data| data.service_quality)
                    {
                        let icon = match quality {
                            1 => "gui/sprites/icon/ping_5",
                            2 => "gui/sprites/icon/ping_4",
                            3 => "gui/sprites/icon/ping_3",
                            4 => "gui/sprites/icon/ping_2",
                            _ => "gui/sprites/icon/ping_unknown",
                        };
                        if atlas.sprites.contains_key(icon) {
                            crate::ui::blit_sprite(
                                batch,
                                atlas,
                                icon,
                                left + 200.0,
                                y + 4.0,
                                10.0,
                                8.0,
                                u32::MAX,
                            );
                        }
                    }
                }
                batch.clip_since(first, left, top, left + 220.0, bottom);
            }
            SubScreen::SlotOptions {
                server,
                name,
                spawn_protection,
                error,
                ..
            } => {
                // `RealmsSlotOptionsScreen`: the title in the 33-tall header — 58 with the
                // world-type warning under it — then the content at header + 30: the
                // labelled 150-wide name field, the cycle-button row and the force/
                // spawn-protection row 28 lower, in two 150-wide columns.
                let warning = match server.world_type.as_str() {
                    "ADVENTUREMAP" => Some("mco.configure.world.edit.subscreen.adventuremap"),
                    "INSPIRATION" => Some("mco.configure.world.edit.subscreen.inspiration"),
                    "EXPERIENCE" => Some("mco.configure.world.edit.subscreen.experience"),
                    _ => None,
                };
                let header_h = if warning.is_some() { 58.0 } else { 33.0 };
                let content_top = header_h + 30.0;
                // With the warning the 26-tall header pair centres in 58: title 16,
                // warning 33; without it the title centres in 33 at 12.
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.buttons.options"),
                    cx,
                    if warning.is_some() { 16.0 } else { 12.0 },
                    u32::MAX,
                );
                if let Some(key) = warning {
                    centered(batch, atlas, &t(key), cx, 33.0, RED);
                }
                // `CommonLayouts.labeledElement`: the label 4 over the field, 150 wide.
                batch.text(
                    atlas,
                    &t("mco.configure.world.edit.slot.name"),
                    cx - 75.0,
                    content_top,
                    u32::MAX,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_SLOT_NAME, name, 32),
                    [cx - 75.0, content_top + 13.0, 150.0, 20.0],
                    true,
                    focused_at,
                );
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
                // `spawnProtectionButton.active`: a non-NORMAL worldType keeps it off;
                // hardcore does not (only the cycles and the force toggle).
                crate::ui::slider(
                    batch,
                    atlas,
                    cx + 4.0,
                    content_top + 69.0,
                    150.0,
                    &Text::literal(&label),
                    value,
                    crate::ui::hovered(cursor, cx + 4.0, content_top + 69.0, 150.0, 20.0),
                    server.world_type == "NORMAL",
                    false,
                );
                if let Some(e) = error {
                    for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, h - 56.0 + i as f32 * 10.0, RED);
                    }
                }
            }
            SubScreen::InviteCodes {
                codes,
                error,
                selected,
                ..
            } => {
                // `RealmsInviteCodesScreen`: the count in the title once fetched — the
                // title sits 8 down when the empty list's subtitle joins the header (the
                // header grows by the subtitle and centres the pair) — then a 28-high row
                // per code whose selection is `extractSelection`'s, or the loading dots
                // at the contents' top (63).
                let loaded = matches!(codes, Some(Ok(_)));
                let count = codes
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map_or(0, |list| list.len());
                let empty = loaded && count == 0;
                let subtitle_lines = if empty {
                    let subtitle = crate::lang::tr(
                        "mco.configure.world.invite_codes.subtitle",
                        &[&5usize as &dyn std::fmt::Display],
                    );
                    wrap_lines(&subtitle, atlas, 308.0)
                } else {
                    Vec::new()
                };
                let header_h = 33.0 + subtitle_lines.len() as f32 * 9.0;
                // `shouldRenderListBackgroundAndSeparators`: only while the list is still
                // null (the loading state); a loaded list draws no band or separators.
                if !loaded {
                    crate::ui::list_background(batch, atlas, header_h, h - 64.0, false);
                    crate::ui::tiled_sized(
                        batch,
                        atlas,
                        "gui/header_separator",
                        0.0,
                        header_h - 2.0,
                        w,
                        2.0,
                        [32.0, 2.0],
                        u32::MAX,
                    );
                    crate::ui::tiled_sized(
                        batch,
                        atlas,
                        "gui/footer_separator",
                        0.0,
                        h - 64.0,
                        w,
                        2.0,
                        [32.0, 2.0],
                        u32::MAX,
                    );
                }
                let title = if loaded {
                    crate::lang::tr(
                        "mco.configure.world.invite_codes.title",
                        &[&count as &dyn std::fmt::Display, &5usize],
                    )
                } else {
                    t("mco.configure.world.buttons.invite_codes")
                };
                centered(
                    batch,
                    atlas,
                    &title,
                    cx,
                    if empty { 8.0 } else { 12.0 },
                    u32::MAX,
                );
                for (i, line) in subtitle_lines.iter().enumerate() {
                    centered(batch, atlas, line, cx, 25.0 + i as f32 * 9.0, GREY);
                }
                let top = header_h;
                let bottom = h - 64.0;
                match codes {
                    Some(Ok(list)) => {
                        let rx = ((w - 308.0) / 2.0).floor();
                        let content = list.len() as f32 * 28.0;
                        crate::ui::scrollbar(
                            batch,
                            atlas,
                            rx + 308.0 + 2.0,
                            top,
                            bottom,
                            content,
                            0.0,
                        );
                        for (i, code) in list.iter().enumerate() {
                            let y = top + 2.0 + i as f32 * 28.0;
                            if y + 28.0 > bottom {
                                break;
                            }
                            if *selected == Some(i) {
                                // `extractSelection`: the whole 308x28 entry, then its
                                // one-pixel black inner fill.
                                batch.fill(
                                    atlas,
                                    rx,
                                    y,
                                    308.0,
                                    28.0,
                                    if list_focused {
                                        0xFFFF_FFFF
                                    } else {
                                        0xFF80_8080
                                    },
                                );
                                batch.fill(atlas, rx + 1.0, y + 1.0, 306.0, 26.0, 0xFF00_0000);
                            }
                            // `Entry.extractContent`: the code alone, centred in its row.
                            batch.text(atlas, &code.code, rx + 10.0, y + 10.0, u32::MAX, true);
                        }
                    }
                    Some(Err(e)) => {
                        for (i, line) in wrap_lines(&e.text(), atlas, w - 40.0).iter().enumerate() {
                            centered(batch, atlas, line, cx, 63.0 + i as f32 * 10.0, RED);
                        }
                    }
                    // `init`'s LoadingDotsWidget: the message at the contents' top +2 and
                    // the grey dots at +17.
                    None => {
                        centered(
                            batch,
                            atlas,
                            &t("mco.configure.world.invite_codes.loading"),
                            cx,
                            65.0,
                            u32::MAX,
                        );
                        centered(batch, atlas, loading_dots(), cx, 80.0, 0xFF80_8080);
                    }
                }
                if let Some(e) = error {
                    for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, h - 76.0 + i as f32 * 10.0, RED);
                    }
                }
            }
            SubScreen::EditCode {
                code,
                active,
                expiration,
                job,
                error,
                ..
            } => {
                // `RealmsEditInviteCodeScreen`: the band over the default header and the
                // 64-tall footer, then the content stack at 63 — the code, its saved
                // expiration line 4 under it, the Active cycle button a row below that
                // and the 308-wide expiration slider under the top row.
                crate::ui::list_background(batch, atlas, 33.0, h - 64.0, false);
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    31.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    h - 64.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                centered(
                    batch,
                    atlas,
                    &t("mco.configure.world.invite_codes.edit.title"),
                    cx,
                    12.0,
                    u32::MAX,
                );
                let left = cx - 154.0;
                batch.text(atlas, &code.code, left, 63.0, u32::MAX, true);
                // `getExpiresLabel`: the code's *stored* expiration date (or Never), not
                // the slider's current choice.
                let expires = crate::lang::tr(
                    "mco.configure.world.invite_codes.edit.expires",
                    &[&match code.expiration_date {
                        Some(date) => short_date_text(date),
                        None => t("mco.configure.world.invite_codes.expiration.never"),
                    } as &dyn std::fmt::Display],
                );
                batch.text(atlas, &expires, left, 76.0, GREY, true);
                // `ExpirationSlider.updateMessage`: `optionNameValue` of its caption and
                // the current choice.
                let value = *expiration as f32 / (EXPIRATIONS.len() - 1) as f32;
                let label = crate::lang::tr(
                    "options.generic_value",
                    &[
                        &t("mco.configure.world.invite_codes.edit.expiration_date")
                            as &dyn std::fmt::Display,
                        &t(EXPIRATIONS[*expiration].0) as &dyn std::fmt::Display,
                    ],
                );
                crate::ui::slider(
                    batch,
                    atlas,
                    left,
                    93.0,
                    308.0,
                    &Text::literal(&label),
                    value,
                    crate::ui::hovered(cursor, left, 93.0, 308.0, 20.0),
                    *active && job.is_none(),
                    false,
                );
                if let Some(e) = error {
                    for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, 121.0 + i as f32 * 10.0, RED);
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
                    12.0,
                    u32::MAX,
                );
                // `RealmsInviteScreen.init`: contents at header + 30 = 63, the
                // labeledElement's label there and its 200x20 EditBox 13 below; the
                // Invite button 8 under the field (33 + 8).
                batch.text(
                    atlas,
                    &t("mco.configure.world.invite.profile.name"),
                    cx - 100.0,
                    63.0,
                    GREY,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_PLAYER_NAME, name, 32),
                    [cx - 100.0, 76.0, 200.0, 20.0],
                    true,
                    focused_at,
                );
                // `showMessage`: the inviting text is grey (-6250336), an error red.
                let status = if job.is_some() {
                    Some((t("mco.configure.world.players.inviting"), 0xFFA0_A0A0))
                } else {
                    error.clone().map(|e| (e, RED))
                };
                if let Some((text, color)) = status {
                    // `inviteButton.getY() + height + 8`: 104 + 20 + 8.
                    for (i, line) in wrap_lines(&text, atlas, 200.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, 132.0 + i as f32 * 10.0, color);
                    }
                }
            }
            SubScreen::AddRealm {
                scroll,
                content,
                rows: row_rects,
                trial,
                ..
            } => {
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
                // The footer is the default 33 tall: the list fills 44..h-33, its first
                // entry at 46 (`AbstractSelectionList.getFirstEntryY`). The band and its
                // two separators are the list's own.
                let (list_top, bottom) = (44.0, h - 33.0);
                let (top, bottom) = (46.0, bottom);
                let rx = ((w - PDP_ROW_WIDTH) / 2.0).floor();
                crate::ui::list_background(batch, atlas, list_top, bottom, false);
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/header_separator",
                    0.0,
                    list_top - 2.0,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                crate::ui::tiled_sized(
                    batch,
                    atlas,
                    "gui/footer_separator",
                    0.0,
                    bottom,
                    w,
                    2.0,
                    [32.0, 2.0],
                    u32::MAX,
                );
                // `PdpFeatureEntry.entryHeight`: the picture's 45 or the text, plus 16.
                let mut rows: Vec<(
                    Option<&'static str>,
                    &'static str,
                    usize,
                    Vec<String>,
                    bool,
                    f32,
                )> = Vec::new();
                for (image, title, descriptions) in pdp_entries() {
                    // `PdpList.addHeader`: the trial note (`mco.pdp.trial.description`, with
                    // its icon) only when the service says a trial is available.
                    let descriptions = if title == "mco.pdp.trial.title" && !*trial {
                        &[][..]
                    } else {
                        descriptions
                    };
                    // The font's lines are 9 apart (`wordWrapHeight`).
                    let title_lines = wrap_lines(&t(title), atlas, PDP_TEXT_WIDTH);
                    let mut lines = title_lines.clone();
                    let mut height = title_lines.len() as f32 * 9.0 + 4.0;
                    for description in descriptions {
                        lines.push(String::new());
                        height += 2.0;
                        let wrapped = wrap_lines(&t(description), atlas, PDP_TEXT_WIDTH);
                        height += wrapped.len() as f32 * 9.0;
                        lines.extend(wrapped);
                    }
                    let header = image.is_empty();
                    let row = if header {
                        height + 16.0
                    } else {
                        height.max(45.0) + 16.0
                    };
                    let image = (!image.is_empty()).then_some(image);
                    rows.push((image, title, title_lines.len(), lines, header, row));
                }
                let total: f32 = rows.iter().map(|row| row.5).sum();
                content.set(total);
                crate::ui::scrollbar(
                    batch,
                    atlas,
                    rx + PDP_ROW_WIDTH + 8.0,
                    list_top,
                    bottom,
                    total,
                    scroll.amount,
                );
                // `ObjectSelectionList.extractListItems`: what the list draws is clipped to
                // its rectangle, so text never leaves the border.
                let first = batch.vertices.len();
                let mut y = top - scroll.amount;
                let mut rects: Vec<[f32; 4]> = Vec::new();
                let mut hovered_row = None;
                for (row, (image, title_key, title_lines, lines, header, height)) in
                    rows.iter().enumerate()
                {
                    rects.push([rx, y, PDP_ROW_WIDTH, *height]);
                    if y + height >= top && y <= bottom {
                        let hovered = crate::ui::hovered(cursor, rx, y, PDP_ROW_WIDTH, *height);
                        if hovered {
                            hovered_row = Some(row);
                        }
                        if self.pdp_row == Some(row) {
                            // `AbstractSelectionList.extractSelection`: the row black, its
                            // outline white with the list focused and grey without.
                            batch.fill(
                                atlas,
                                rx,
                                y,
                                PDP_ROW_WIDTH,
                                *height,
                                if list_focused {
                                    0xFFFF_FFFF
                                } else {
                                    0xFF80_8080
                                },
                            );
                            batch.fill(
                                atlas,
                                rx + 1.0,
                                y + 1.0,
                                PDP_ROW_WIDTH - 2.0,
                                *height - 2.0,
                                0xFF00_0000,
                            );
                        }
                        if let Some(image) = image {
                            crate::ui::blit_sprite(
                                batch,
                                atlas,
                                image,
                                // `getContentX() + 4`, `getContentY() + 4` (the entry's
                                // content origin is its own +2).
                                rx + 6.0,
                                y + 6.0,
                                80.0,
                                45.0,
                                u32::MAX,
                            );
                        }
                        let mut text_y = y + 6.0;
                        let mut after_gap = false;
                        // `RealmsPdpScreen`'s constants: the trial title carries
                        // UNDERLINE, the feature titles BOLD.
                        let heading = if *title_key == "mco.pdp.trial.title" {
                            Text::translate(title_key, &[]).underlined()
                        } else {
                            Text::translate(title_key, &[]).bold()
                        };
                        for (line_index, line) in lines.iter().enumerate() {
                            if line.is_empty() {
                                if *header {
                                    after_gap = true;
                                }
                                text_y += 2.0;
                                continue;
                            }
                            let x = if *header {
                                cx - (HudBatch::text_width(atlas, line) / 2.0).floor()
                            } else {
                                rx + 102.0
                            };
                            // `PdpHeaderEntry`: the trial subtitle's 8x8
                            // `icon/trial_available` sits left of it.
                            if after_gap && *trial {
                                crate::ui::blit_sprite(
                                    batch,
                                    atlas,
                                    "gui/sprites/icon/trial_available",
                                    x - 12.0,
                                    text_y + 1.0,
                                    8.0,
                                    8.0,
                                    u32::MAX,
                                );
                            }
                            after_gap = false;
                            if line_index < *title_lines {
                                batch.text_styled(atlas, &heading, x, text_y, u32::MAX, true);
                            } else {
                                batch.text(atlas, line, x, text_y, u32::MAX, true);
                            }
                            text_y += 9.0;
                        }
                    }
                    y += height;
                }
                *row_rects.borrow_mut() = rects;
                // `ObjectSelectionList.updateWidgetNarration`: the list's position, then the
                // hovered row's or the selected one (`narrator.select` of its lines).
                crate::widgets::narratable(|| {
                    use crate::widgets::{ListKind, Narratable, Row};
                    let narrate = |row: usize| Row::new(row, pdp_row_narration(row, *trial));
                    Narratable::list(
                        ListKind::Object,
                        rows.len(),
                        hovered_row.map(narrate),
                        self.pdp_row.map(narrate),
                        [rx, top, PDP_ROW_WIDTH, bottom - top],
                    )
                    .focused(list_focused)
                    .key(crate::widgets::key_of(&"realms/pdp"))
                });
                batch.clip_since(first, rx, top, rx + PDP_ROW_WIDTH, bottom);
            }
            // `PopupScreen`: the list behind (`backgroundScreen`), dimmed, then the panel
            // with its 130x64 image, the 214-wide message and the button row.
            SubScreen::Popup(Popup::Notice {
                title,
                message,
                image,
                url,
                ..
            }) => {
                self.draw_main(batch, atlas, cursor, w, h, false);
                batch.fill(atlas, 0.0, 0.0, w, h, rgba(16, 16, 16, 192));
                let lines = if message.is_empty() {
                    Vec::new()
                } else {
                    wrap_lines(message, atlas, 214.0)
                };
                let image = image.as_ref().and_then(|identifier| {
                    let key = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
                    let key = key.strip_prefix("textures/").unwrap_or(key);
                    let key = key.strip_suffix(".png").unwrap_or(key);
                    atlas.sprites.contains_key(key).then(|| key.to_owned())
                });
                let n: f32 = if url.is_some() { 2.0 } else { 1.0 };
                let button_w = ((214.0 - 6.0 * (n - 1.0)) / n).min(150.0);
                let mut height = 9.0 + 12.0 + 20.0;
                if image.is_some() {
                    height += 12.0 + 64.0;
                }
                if !lines.is_empty() {
                    height += 12.0 + lines.len() as f32 * 10.0;
                }
                let top = ((h - height) / 2.0).floor();
                let left = ((w - 214.0) / 2.0).floor();
                if atlas.sprites.contains_key("gui/sprites/popup/background") {
                    crate::ui::blit_sprite(
                        batch,
                        atlas,
                        "gui/sprites/popup/background",
                        left - 18.0,
                        top - 18.0,
                        214.0 + 36.0,
                        height + 36.0,
                        u32::MAX,
                    );
                } else {
                    batch.fill(
                        atlas,
                        left - 18.0,
                        top - 18.0,
                        214.0 + 36.0,
                        height + 36.0,
                        rgba(16, 16, 16, 255),
                    );
                }
                let mut y = top;
                let heading = Text::literal(title.clone()).bold();
                let tw = HudBatch::text_width(atlas, title);
                batch.text_styled(atlas, &heading, cx - (tw / 2.0).floor(), y, u32::MAX, true);
                y += 9.0 + 12.0;
                if let Some(image) = &image {
                    crate::ui::blit_sprite(
                        batch,
                        atlas,
                        image,
                        cx - 65.0,
                        y,
                        130.0,
                        64.0,
                        u32::MAX,
                    );
                    y += 64.0 + 12.0;
                }
                for line in &lines {
                    let lw = HudBatch::text_width(atlas, line);
                    batch.text(atlas, line, cx - (lw / 2.0).floor(), y, u32::MAX, true);
                    y += 10.0;
                }
                let by = top + height - 20.0;
                let row: f32 = button_w * n + 6.0 * (n - 1.0);
                let bx = left + ((214.0 - row) / 2.0).floor();
                // `buildScreen`'s row: the URL button comes first, OK last.
                let mut rects = [[0.0; 4]; 2];
                let mut x = bx;
                if let Some((_, label)) = url {
                    let label = Text::literal(label.clone());
                    let hovered = crate::ui::hovered(cursor, x, by, button_w, 20.0);
                    crate::ui::button(batch, atlas, x, by, button_w, 20.0, &label, true, hovered);
                    rects[0] = [x, by, button_w, 20.0];
                    x += button_w + 6.0;
                }
                let ok = Text::literal(crate::lang::get("gui.ok"));
                let hovered = crate::ui::hovered(cursor, x, by, button_w, 20.0);
                crate::ui::button(batch, atlas, x, by, button_w, 20.0, &ok, true, hovered);
                rects[1] = [x, by, button_w, 20.0];
                self.notice_buttons.set(rects);
            }
            // `RealmsConfirmScreen`: the question at row(3), the player's name at row(5)
            // (`RealmsPlayersTab.uninvite`), white and centred, over the plain background.
            SubScreen::Popup(Popup::Uninvite { player, .. }) => {
                centered(batch, atlas, &t("mco.question"), cx, 79.0, u32::MAX);
                let message = crate::lang::tr(
                    "mco.configure.world.uninvite.player",
                    &[&player.name as &dyn std::fmt::Display],
                );
                centered(batch, atlas, &message, cx, 105.0, u32::MAX);
            }
            // `ConfirmLinkScreen`: chat.link.confirmTrusted as its title, the link as its
            // message, and Open in Browser / Copy to Clipboard / Cancel 100 wide each.
            SubScreen::Popup(Popup::OpenLink { url, .. }) => {
                let layout = crate::confirm_screen::Choice::new(
                    &atlas.font,
                    (w, h),
                    &Text::translate("chat.link.confirmTrusted", &[]),
                    &Text::literal(url),
                    3,
                    100.0,
                );
                for (line, x, y) in &layout.lines {
                    batch.text_styled(atlas, line, *x, *y, u32::MAX, true);
                }
                self.link_buttons
                    .set([layout.buttons[0], layout.buttons[1], layout.buttons[2]]);
            }
            SubScreen::Popup(popup) => {
                // `PopupScreen.extractRenderState` draws its `backgroundScreen` first, then
                // the transparent gradient: the leave and resource-pack questions sit over
                // the list, the close question over the settings tab with its draft.
                let underlay = match popup {
                    Popup::Leave(_) | Popup::ResourcePack { .. } => Some(SubScreen::Main),
                    Popup::CloseWorld {
                        server,
                        settings,
                        detail,
                        tier,
                    } => Some(SubScreen::Configure {
                        server: server.clone(),
                        tab: ConfigureTab::Settings,
                        subscription: None,
                        subscription_job: None,
                        detail: detail.clone(),
                        detail_job: None,
                        deleting: None,
                        member_job: None,
                        settings: Some(settings.clone()),
                        players_scroll: crate::ui::Scroll::default(),
                        tier: tier.clone(),
                        tier_job: None,
                        regions: None,
                        regions_job: None,
                        saving: None,
                        error: None,
                        job: None,
                    }),
                    // `warningPopupScreen(this, ...)`: the backup list behind it.
                    Popup::RestoreBackup {
                        server,
                        slot_id,
                        list,
                        selected,
                        ..
                    } => Some(SubScreen::Backups {
                        server: server.clone(),
                        slot_id: *slot_id,
                        backups: Some(Ok(list.clone())),
                        list_job: None,
                        restore_job: None,
                        download_task: None,
                        selected: *selected,
                        scroll: crate::ui::Scroll::default(),
                    }),
                    // `infoPopupScreen(this, ...)`: the same list behind it.
                    Popup::DownloadLatest {
                        server,
                        slot_id,
                        list,
                        selected,
                    } => Some(SubScreen::Backups {
                        server: server.clone(),
                        slot_id: *slot_id,
                        backups: Some(Ok(list.clone())),
                        list_job: None,
                        restore_job: None,
                        download_task: None,
                        selected: *selected,
                        scroll: crate::ui::Scroll::default(),
                    }),
                    // `warningAcknowledgePopupScreen(this, ...)`: the download screen,
                    // still in its `mco.download.preparing` state, behind the dim.
                    Popup::DownloadOversized {
                        server,
                        slot_id,
                        download,
                        world_name,
                        started,
                    } => Some(SubScreen::DownloadLatest {
                        server: server.clone(),
                        slot_id: *slot_id,
                        download: download.clone(),
                        world_name: world_name.clone(),
                        size_job: None,
                        progress: Arc::new(download::DownloadProgress::default()),
                        job: None,
                        error: None,
                        done: false,
                        started: *started,
                        speed: std::cell::Cell::new((0, Instant::now(), 0)),
                    }),
                    _ => None,
                };
                if let Some(subscreen) = underlay {
                    let mut parent = self.clone();
                    parent.subscreen = subscreen;
                    parent.draw(batch, atlas, caret, focused_at, cursor, w, h, list_focused);
                }
                // `PopupScreen`: the dimmed background, the `popup/background` panel — the
                // 250-wide default minus its 18 border on each side, so 214 of content —
                // the bold title, the wrapped message and `buildButtonRow`'s buttons,
                // 6 apart and at most 150 wide (`RealmsPopups` picks the title).
                // `Screen.extractTransparentBackground`: `fillGradient` from
                // 0xC0101010 to 0xD0101010 down the screen.
                batch.fill_gradient(atlas, (0.0, 0.0, w, h), 0xC010_1010, 0xD010_1010);
                let message = match popup {
                    Popup::Leave(_) => t("mco.configure.world.leave.question.line1"),
                    Popup::ResourcePack { .. } => t("mco.configure.world.resourcepack.question"),
                    Popup::Delete(_) => t("mco.configure.world.delete.question.line1"),
                    // `restoreClicked`: the backup's short date and its age.
                    Popup::RestoreBackup { backup, .. } => crate::lang::tr(
                        "mco.configure.world.restore.question.line1",
                        &[
                            &short_date_text(backup.last_modified),
                            &age_presentation(backup.last_modified),
                        ],
                    ),
                    Popup::SwitchSlot { .. } | Popup::SwitchEmpty { .. } => {
                        t("mco.configure.world.slot.switch.question.line1")
                    }
                    Popup::CloseWorld { .. } => t("mco.configure.world.close.question.line1"),
                    // `downloadClicked`: `infoPopupScreen`'s question.
                    Popup::DownloadLatest { .. } => {
                        t("mco.configure.world.restore.download.question.line1")
                    }
                    // `checkDownloadSize`: the 5 GiB warning, one line break included.
                    Popup::DownloadOversized { .. } => crate::lang::tr(
                        "mco.download.confirmation.oversized",
                        &[&human_readable(download::SIZE_LIMIT)],
                    ),
                    // Asked as `RealmsConfirmScreen`, drawn above.
                    Popup::Uninvite { .. } => String::new(),
                    // Asked as `ConfirmLinkScreen`, drawn above.
                    Popup::OpenLink { .. } => String::new(),
                    // Drawn as a `PopupScreen` above.
                    Popup::Notice { .. } => String::new(),
                };
                let (title, color) = match popup {
                    // `RealmsPopups.warningPopupScreen` asks the delete question.
                    // `warningPopupScreen` asks the delete and restore questions.
                    Popup::Delete(_) | Popup::RestoreBackup { .. } => ("mco.warning", RED),
                    // `warningAcknowledgePopupScreen`'s title is the warning too.
                    Popup::DownloadOversized { .. } => ("mco.warning", RED),
                    // `customPopupScreen` keeps the given title's own colour.
                    Popup::CloseWorld { .. } => {
                        ("mco.configure.world.close.question.title", u32::MAX)
                    }
                    _ => ("mco.info", INFO),
                };
                // `MultiLineTextWidget`: 9 per line, `buildButtonRow` 20 tall.
                let lines = wrap_lines(&message, atlas, 214.0);
                let height = 9.0 + 12.0 + lines.len() as f32 * 9.0 + 12.0 + 20.0;
                let top = ((h - height) / 2.0).floor();
                let left = ((w - 214.0) / 2.0).floor();
                if atlas.sprites.contains_key("gui/sprites/popup/background") {
                    crate::ui::blit_sprite(
                        batch,
                        atlas,
                        "gui/sprites/popup/background",
                        left - 18.0,
                        top - 18.0,
                        214.0 + 36.0,
                        height + 36.0,
                        u32::MAX,
                    );
                } else {
                    batch.fill(
                        atlas,
                        left - 18.0,
                        top - 18.0,
                        214.0 + 36.0,
                        height + 36.0,
                        rgba(16, 16, 16, 255),
                    );
                }
                let heading = Text::translate(title, &[]).bold();
                let tw = HudBatch::text_width(atlas, &t(title));
                batch.text_styled(atlas, &heading, cx - (tw / 2.0).floor(), top, color, true);
                for (i, line) in lines.iter().enumerate() {
                    centered(
                        batch,
                        atlas,
                        line,
                        cx,
                        top + 9.0 + 12.0 + i as f32 * 9.0,
                        u32::MAX,
                    );
                }
                // `buildButtonRow`'s two buttons: (214 - 6) / 2 = 104 wide, 6 apart, the
                // row as wide as the content. A single button takes `min(214, 150)`.
                let by = top + height - 20.0;
                if matches!(popup, Popup::DownloadOversized { .. }) {
                    self.popup_buttons
                        .set([[left + 32.0, by, 150.0, 20.0], [0.0; 4]]);
                } else {
                    self.popup_buttons
                        .set([[left, by, 104.0, 20.0], [left + 110.0, by, 104.0, 20.0]]);
                }
            }
            SubScreen::Connecting { .. } => {
                // `GetServerDetailsTask` runs under `RealmsLongRunningMcoTaskScreen`: the
                // same 122-tall stack as every task, its title at +46, the grey dots at
                // +61 and Cancel at +102. A failure replaces the screen with
                // `RealmsGenericErrorScreen` (`LongRunningTask.error`), so no inline text.
                let top = task_stack_top((w, h));
                realms_logo(batch, atlas, cx, top);
                centered(
                    batch,
                    atlas,
                    &t("mco.connect.connecting"),
                    cx,
                    top + 46.0,
                    u32::MAX,
                );
                centered(batch, atlas, loading_dots(), cx, top + 61.0, 0xFF80_8080);
            }
            SubScreen::Error { title, detail, .. } => {
                // `RealmsGenericErrorScreen`: the title at 80, the detail from 100 in the
                // -2142128 red, wrapped at 3/4 of the screen, 9 apart.
                centered(batch, atlas, title, cx, 80.0, u32::MAX);
                for (i, line) in wrap_lines(detail, atlas, w * 3.0 / 4.0).iter().enumerate() {
                    centered(batch, atlas, line, cx, 100.0 + i as f32 * 9.0, 0xFF50_50DF);
                }
            }
            SubScreen::Terms { .. } => {
                // `RealmsTermsScreen.extractRenderState`: the title at 17, the page's
                // first sentence at (width/2 - 120, row(5)), the underlined link after it.
                centered(batch, atlas, &t("mco.terms.title"), cx, 17.0, u32::MAX);
                let y = 105.0;
                let x = cx - 120.0;
                let first = t("mco.terms.sentence.1");
                batch.text(atlas, &first, x, y, u32::MAX, true);
                let first_width = HudBatch::text_width(atlas, &first);
                // `TERMS_LINK_TEXT`: a space, then the underlined second sentence.
                let link = format!(" {}", t("mco.terms.sentence.2"));
                let link_x = x + first_width;
                let x1 = cx - 121.0 + first_width;
                let x2 = x1 + HudBatch::text_width(atlas, &link) + 1.0;
                self.terms_link.set([x1, y, x2, y + 10.0]);
                let (mx, my) = cursor;
                let on_link = x1 <= mx && mx <= x2 && y <= my && my <= y + 10.0;
                let color = if on_link { 0xFF6C_71C4 } else { 0xFF33_66BB };
                batch.text_styled(
                    atlas,
                    &Text::literal(link).underlined(),
                    link_x,
                    y,
                    color,
                    true,
                );
            }
            SubScreen::ResetWorld {
                kind, reset, error, ..
            } => {
                if let Some(task) = reset {
                    // `RealmsLongRunningMcoTaskScreen`: the tasks' screen replaced this one —
                    // `realmsLogo`, then the `LoadingDotsWidget` (its message at widget +2,
                    // the grey dots at +17), then the Cancel 102 into the 122-tall stack.
                    let top = task_stack_top((w, h));
                    realms_logo(batch, atlas, cx, top);
                    let title = task.title.lock().expect("a task title").clone();
                    centered(batch, atlas, &title, cx, top + 46.0, u32::MAX);
                    centered(batch, atlas, loading_dots(), cx, top + 61.0, 0xFF80_8080);
                } else {
                    // `init`: the header's two 9-tall lines with 3 of padding, centred in
                    // the 33-tall frame — title 4, subtitle 19.
                    let (title, subtitle) = (kind.title(), kind.subtitle());
                    centered(batch, atlas, &t(title), cx, 4.0, u32::MAX);
                    centered(
                        batch,
                        atlas,
                        &t(subtitle.0),
                        cx,
                        19.0,
                        if subtitle.1 { RED } else { GREY },
                    );
                    if let Some(e) = error {
                        // The reset screen's own error line, under the tiles.
                        for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                            centered(batch, atlas, line, cx, h - 56.0 + i as f32 * 10.0, RED);
                        }
                    }
                    // `RealmsResetWorldScreen.init`'s tiles: Generate New World, Upload, the
                    // four groups. Generate New World opens Create New World
                    // (`RealmsCreateWorldFlow.createWorld`) and uploads what it made.
                    let tiles = [
                        ("mco.reset.world.generate", "gui/realms/new_world"),
                        ("mco.upload.select.world.title", "gui/realms/upload"),
                        ("mco.reset.world.template", "gui/realms/survival_spawn"),
                        ("mco.reset.world.adventure", "gui/realms/adventure"),
                        ("mco.reset.world.experience", "gui/realms/experience"),
                        ("mco.reset.world.inspiration", "gui/realms/inspiration"),
                    ];
                    let content_y = reset_content_y(h);
                    for (i, (label, image)) in tiles.iter().enumerate() {
                        let (x, y) = reset_tile(cx, content_y, i);
                        // The control is 60x69: the frame plus its label.
                        let hover = crate::ui::hovered(cursor, x, y, 60.0, 69.0);
                        let frame = if hover { 0xFF90_9090 } else { u32::MAX };
                        batch.fill(atlas, x + 2.0, y + 2.0, 56.0, 56.0, 0xFF20_2020);
                        // `FrameButton`: the picture inside `widget/slot_frame`.
                        crate::ui::blit_sprite(
                            batch,
                            atlas,
                            image,
                            x + 2.0,
                            y + 2.0,
                            56.0,
                            56.0,
                            frame,
                        );
                        crate::ui::blit_sprite(
                            batch,
                            atlas,
                            "gui/sprites/widget/slot_frame",
                            x,
                            y,
                            60.0,
                            60.0,
                            frame,
                        );
                        let color = if hover { GREY } else { u32::MAX };
                        let text = t(label);
                        let width = HudBatch::text_width(atlas, &text);
                        // `FrameButton.extractContents`: `centeredText(..., x + 28, y - 14)`.
                        batch.text(
                            atlas,
                            &text,
                            x + 28.0 - (width / 2.0).floor(),
                            y - 14.0,
                            color,
                            true,
                        );
                    }
                }
            }
            SubScreen::TemplateList {
                group,
                templates,
                selected,
                scroll,
                job,
                select,
                icons,
                error,
                ..
            } => {
                if let Some(task) = select {
                    // `RealmsLongRunningMcoTaskScreen` replaced the list while the switch
                    // runs: its logo, the task's title and grey dots, and Cancel.
                    let top = task_stack_top((w, h));
                    realms_logo(batch, atlas, cx, top);
                    let title = task.title.lock().expect("a task title").clone();
                    centered(batch, atlas, &title, cx, top + 46.0, u32::MAX);
                    centered(batch, atlas, loading_dots(), cx, top + 61.0, 0xFF80_8080);
                } else {
                    let title = group.map_or("mco.template.title.minigame", TemplateGroup::title);
                    centered(batch, atlas, &t(title), cx, 12.0, u32::MAX);
                    // `init`: header height 33 + 13 per subtitle line (the minigame list
                    // has two); the list starts at the header's height.
                    let top = if group.is_none() { 59.0 } else { 33.0 };
                    if group.is_none() {
                        // `mco.minigame.world.info.line1`/`line2`: the switch's two
                        // warnings, grey (-4539718), 13 apart.
                        for (i, key) in [
                            "mco.minigame.world.info.line1",
                            "mco.minigame.world.info.line2",
                        ]
                        .iter()
                        .enumerate()
                        {
                            centered(
                                batch,
                                atlas,
                                &t(key),
                                cx,
                                25.0 + i as f32 * 13.0,
                                0xFFBA_BABA,
                            );
                        }
                    }
                    let bottom = h - 33.0;
                    crate::ui::list_background(batch, atlas, top, bottom, false);
                    // `ObjectSelectionList` separators at the list's edges.
                    crate::ui::tiled_sized(
                        batch,
                        atlas,
                        "gui/header_separator",
                        0.0,
                        top - 2.0,
                        w,
                        2.0,
                        [32.0, 2.0],
                        u32::MAX,
                    );
                    crate::ui::tiled_sized(
                        batch,
                        atlas,
                        "gui/footer_separator",
                        0.0,
                        bottom,
                        w,
                        2.0,
                        [32.0, 2.0],
                        u32::MAX,
                    );
                    let rx = ((w - ROW_WIDTH) / 2.0).floor();
                    let content = templates.len() as f32 * TEMPLATE_ROW;
                    let mut icons = icons.borrow_mut();
                    icons.clear();
                    let first = batch.vertices.len();
                    for (i, template) in templates.iter().enumerate() {
                        let y = top + 2.0 + i as f32 * TEMPLATE_ROW - scroll.amount;
                        if y + TEMPLATE_ROW <= top || y >= bottom {
                            continue;
                        }
                        if *selected == Some(i) {
                            // `AbstractSelectionList.extractSelection`: the whole entry,
                            // white while the list is focused.
                            batch.fill(
                                atlas,
                                rx,
                                y,
                                ROW_WIDTH,
                                TEMPLATE_ROW,
                                if list_focused {
                                    0xFFFF_FFFF
                                } else {
                                    0xFF80_8080
                                },
                            );
                            batch.fill(
                                atlas,
                                rx + 1.0,
                                y + 1.0,
                                ROW_WIDTH - 2.0,
                                TEMPLATE_ROW - 2.0,
                                0xFF00_0000,
                            );
                        }
                        // `RealmsTextureManager.worldTemplate`: the service's base64 PNG,
                        // decoded to a 64x64 icon slot and drawn 38x38 inside the frame.
                        let pixels = template.image.as_ref().and_then(|image| {
                            let key = format!("{}:{}", template.id, image.len());
                            let mut cache = self.template_images.borrow_mut();
                            cache
                                .entry(key)
                                .or_insert_with(|| template_image(image))
                                .clone()
                        });
                        let picture = pixels
                            .as_deref()
                            .and_then(|rgba| icon_sprite(batch, atlas, i, rgba));
                        match picture {
                            Some(sprite) => {
                                batch.sprite_scaled(
                                    atlas,
                                    &sprite,
                                    rx + 3.0,
                                    y + 4.0,
                                    38.0,
                                    38.0,
                                    u32::MAX,
                                );
                            }
                            None => {
                                batch.fill(atlas, rx + 3.0, y + 4.0, 38.0, 38.0, 0xFF20_2020);
                            }
                        }
                        crate::ui::blit_sprite(
                            batch,
                            atlas,
                            "gui/sprites/widget/slot_frame",
                            rx + 2.0,
                            y + 3.0,
                            40.0,
                            40.0,
                            u32::MAX,
                        );
                        batch.text(atlas, &template.name, rx + 67.0, y + 7.0, u32::MAX, true);
                        let version = HudBatch::text_width(atlas, &template.version);
                        batch.text(
                            atlas,
                            &template.version,
                            rx + 298.0 - version - 5.0,
                            y + 7.0,
                            GREY,
                            true,
                        );
                        // `Entry`: the website and trailer buttons, left of the version.
                        for (trailer, offset) in [(false, 10.0), (true, 15.0 + 15.0)] {
                            let present = if trailer {
                                template.trailer.is_some()
                            } else {
                                template.link.is_some()
                            };
                            if !present {
                                continue;
                            }
                            // `Entry.extractContent`: `setPosition(..., getContentY())`.
                            let (x, icon_y) = (rx + 298.0 - version - 15.0 - offset, y + 2.0);
                            let hovered = crate::ui::hovered(cursor, x, icon_y, 15.0, 15.0);
                            let sprite = match (trailer, hovered) {
                                (false, false) => "gui/sprites/icon/link",
                                (false, true) => "gui/sprites/icon/link_highlighted",
                                (true, false) => "gui/sprites/icon/video_link",
                                (true, true) => "gui/sprites/icon/video_link_highlighted",
                            };
                            crate::ui::blit_sprite(
                                batch,
                                atlas,
                                sprite,
                                x,
                                icon_y,
                                15.0,
                                15.0,
                                u32::MAX,
                            );
                            icons.push((i, [x, icon_y, 15.0, 15.0], trailer));
                        }
                        batch.text(atlas, &template.author, rx + 67.0, y + 21.0, GREY, true);
                        if !template.recommended_players.is_empty() {
                            batch.text(
                                atlas,
                                &template.recommended_players,
                                rx + 67.0,
                                y + 35.0,
                                0xFF80_8080,
                                true,
                            );
                        }
                    }
                    batch.clip_since(first, 0.0, top, w, bottom);
                    crate::ui::scrollbar(
                        batch,
                        atlas,
                        rx + ROW_WIDTH + 8.0,
                        top,
                        bottom,
                        content,
                        scroll.amount,
                    );
                    if templates.is_empty() {
                        if error.is_some() {
                            // `RealmsSelectWorldTemplateScreen`: a failed fetch says so with
                            // `mco.template.select.failure`, not the error's text.
                            centered(
                                batch,
                                atlas,
                                &t("mco.template.select.failure"),
                                cx,
                                top + 10.0,
                                u32::MAX,
                            );
                        } else if job.is_some() {
                            centered(batch, atlas, loading_dots(), cx, top + 10.0, GREY);
                        } else {
                            // `RealmsSelectWorldTemplateScreen` asks
                            // `I18n.get("mco.template.select.none", "%link")` and decomposes
                            // the result (`TextRenderingUtils.decompose`): only the line with
                            // the `%s` argument carries the link (the raw translation has
                            // `%s`, not `%link`), in -13408581 (0xFF3366BB), the rest white,
                            // each line centred; hovering the link shows its URL.
                            let lines = template_none_lines(
                                &t("mco.template.select.none"),
                                &t("mco.template.select.none.linkTitle"),
                            );
                            let mut link_rect = [0.0; 4];
                            for (i, (before, link, after)) in lines.iter().enumerate() {
                                let y = 92.0 + i as f32 * 13.0;
                                let width = HudBatch::text_width(atlas, before)
                                    + link
                                        .as_deref()
                                        .map_or(0.0, |link| HudBatch::text_width(atlas, link))
                                    + HudBatch::text_width(atlas, after);
                                let mut x = cx - (width / 2.0).floor();
                                batch.text(atlas, before, x, y, u32::MAX, true);
                                x += HudBatch::text_width(atlas, before);
                                if let Some(link) = link {
                                    batch.text(atlas, link, x, y, 0xFF33_66BB, true);
                                    let link_w = HudBatch::text_width(atlas, link);
                                    if crate::ui::hovered(cursor, x, y - 3.0, link_w, 11.0) {
                                        crate::ui::tooltip(
                                            batch,
                                            atlas,
                                            &[Text::literal(REALMS_CONTENT_CREATION)],
                                            cursor,
                                        );
                                        link_rect = [x, y - 3.0, link_w, 11.0];
                                    }
                                    x += link_w;
                                }
                                batch.text(atlas, after, x, y, u32::MAX, true);
                            }
                            self.template_link_rect.set(link_rect);
                        }
                    }
                }
            }
            SubScreen::CreateRealm {
                name,
                description,
                on_description,
                error,
                ..
            } => {
                // `RealmsCreateRealmScreen`: the title, then the name and description
                // fields under their labels (210 wide, as `CommonLayouts.labeledElement`).
                centered(
                    batch,
                    atlas,
                    &t("mco.selectServer.create"),
                    cx,
                    12.0,
                    u32::MAX,
                );
                // `HeaderAndFooterLayout`: the two 33-tall labeledElements with 8 of
                // spacing make 74, so content y = min(33 + 30, H - 33 - 74).
                let content_y = (h - 107.0).min(63.0);
                let left = cx - 105.0;
                // `CommonLayouts.labeledElement`'s StringWidget keeps the default white.
                batch.text(
                    atlas,
                    &t("mco.configure.world.name"),
                    left,
                    content_y,
                    u32::MAX,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_CREATE_NAME, name, 32),
                    [left, content_y + 13.0, 210.0, 20.0],
                    !*on_description,
                    focused_at,
                );
                batch.text(
                    atlas,
                    &t("mco.configure.world.description"),
                    left,
                    content_y + 41.0,
                    u32::MAX,
                    true,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_CREATE_DESC, description, 32),
                    [left, content_y + 54.0, 210.0, 20.0],
                    *on_description,
                    focused_at,
                );
                if let Some(e) = error {
                    for (i, line) in wrap_lines(e, atlas, w - 40.0).iter().enumerate() {
                        centered(batch, atlas, line, cx, h - 56.0 + i as f32 * 10.0, RED);
                    }
                }
            }
            SubScreen::UploadSelect {
                worlds,
                selected,
                scroll,
                query,
                ..
            } => {
                // `RealmsSelectFileToUploadScreen.init`: the title and a 200-wide search
                // box 4 under it in the 33-tall header (title 0, box 13), then the list
                // from 33 to h - 33, its first entry at +2.
                centered(
                    batch,
                    atlas,
                    &t("mco.upload.select.world.title"),
                    cx,
                    0.0,
                    u32::MAX,
                );
                caret.draw(
                    batch,
                    atlas,
                    (REALMS_UPLOAD_SEARCH, query, 32),
                    [cx - 100.0, 13.0, 200.0, 20.0],
                    true,
                    focused_at,
                );
                if query.is_empty() {
                    batch.text(
                        atlas,
                        &t("selectWorld.search"),
                        cx - 96.0,
                        19.0,
                        rgba(64, 64, 64, 255),
                        true,
                    );
                }
                let shown: Vec<&crate::title::WorldEntry> = worlds
                    .iter()
                    .filter(|world| upload_matches(world, query))
                    .collect();
                let (top, bottom) = (33.0, h - 33.0);
                crate::ui::list_background(batch, atlas, top, bottom, false);
                let rx = ((w - 270.0) / 2.0).floor();
                let content = shown.len() as f32 * ROW_HEIGHT;
                let first = batch.vertices.len();
                for (i, world) in shown.iter().enumerate() {
                    let y = top + 2.0 + i as f32 * ROW_HEIGHT - scroll.amount;
                    if y + ROW_HEIGHT <= top || y >= bottom {
                        continue;
                    }
                    if *selected == Some(i) {
                        batch.fill(atlas, rx, y, 270.0, ROW_HEIGHT, 0xFF80_8080);
                        batch.fill(
                            atlas,
                            rx + 1.0,
                            y + 1.0,
                            268.0,
                            ROW_HEIGHT - 2.0,
                            0xFF00_0000,
                        );
                    }
                    // `WorldListEntry.extractContent`: the world's 32x32 `icon.png` at the
                    // content origin (a missing icon shows the unknown-server sprite),
                    // then three lines at contentY+1/+12/+21, the text at contentX + 35.
                    let pixels = self.saves.as_ref().and_then(|saves| {
                        let path = saves.join(&world.folder).join("icon.png");
                        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
                        let mut icons = self.world_icons.borrow_mut();
                        let cached = icons
                            .entry(path.clone())
                            .or_insert_with(|| (std::time::UNIX_EPOCH, None));
                        if cached.0 != modified {
                            *cached = (
                                modified,
                                crate::hud::decode(&path)
                                    .ok()
                                    .filter(|(w, h, _)| *w == 64 && *h == 64)
                                    .map(|(_, _, pixels)| pixels),
                            );
                        }
                        cached.1.clone()
                    });
                    let icon = pixels
                        .as_deref()
                        .and_then(|rgba| icon_sprite(batch, atlas, i, rgba));
                    match icon {
                        Some(sprite) => {
                            batch.sprite_scaled(
                                atlas,
                                &sprite,
                                rx + 2.0,
                                y + 2.0,
                                32.0,
                                32.0,
                                u32::MAX,
                            );
                        }
                        None => {
                            batch.sprite_scaled(
                                atlas,
                                "misc/unknown_server",
                                rx + 2.0,
                                y + 2.0,
                                32.0,
                                32.0,
                                u32::MAX,
                            );
                        }
                    }
                    let text_x = rx + 37.0;
                    batch.text(atlas, &world.name, text_x, y + 3.0, u32::MAX, true);
                    let info = format!("{} · {}", world.folder, world.version);
                    batch.text(atlas, &info, text_x, y + 14.0, 0xFF80_8080, true);
                    let mode = if world.hardcore {
                        "gameMode.hardcore"
                    } else {
                        match world.game_mode {
                            1 => "gameMode.creative",
                            2 => "gameMode.adventure",
                            3 => "gameMode.spectator",
                            _ => "gameMode.survival",
                        }
                    };
                    batch.text(
                        atlas,
                        &t(mode),
                        text_x,
                        y + 23.0,
                        if world.hardcore { RED } else { 0xFF80_8080 },
                        true,
                    );
                }
                batch.clip_since(first, 0.0, top, w, bottom);
                crate::ui::scrollbar(
                    batch,
                    atlas,
                    rx + 270.0 + 8.0,
                    top,
                    bottom,
                    content,
                    scroll.amount,
                );
                if shown.is_empty() {
                    centered(
                        batch,
                        atlas,
                        &t("mco.upload.select.world.none"),
                        cx,
                        top + 16.0,
                        u32::MAX,
                    );
                }
            }
            SubScreen::Uploading {
                world,
                progress,
                error,
                done,
                started,
                ..
            } => {
                // `RealmsUploadScreen.extractRenderState`: the status, dots, the bar and the
                // error lines; the upload speed is not shown (the port keeps no rate).
                let running = if progress.completed() {
                    t("mco.upload.verifying")
                } else if progress.started() {
                    crate::lang::tr("mco.upload.uploading", &[&world.name])
                } else {
                    t("mco.upload.preparing")
                };
                let status = match error {
                    Some(upload::UploadError::Cancelled) => t("mco.upload.cancelled"),
                    Some(upload::UploadError::WorldNotClosed) => t("mco.upload.close.failure"),
                    Some(upload::UploadError::Failed(message)) => {
                        crate::lang::tr("mco.upload.failed", &[message])
                    }
                    Some(upload::UploadError::TooLarge) => running.clone(),
                    None if *done => t("mco.upload.done"),
                    None => running,
                };
                centered(batch, atlas, &status, cx, 50.0, u32::MAX);
                if !*done {
                    let elapsed = started.elapsed().as_millis() / 500;
                    let dots = ["", ".", ". .", ". . ."][elapsed as usize % 4];
                    let width = HudBatch::text_width(atlas, &status);
                    batch.text(atlas, dots, cx + width / 2.0 + 5.0, 50.0, u32::MAX, true);
                }
                // `uploadStarted() && !cancelled`: a cancelled upload hides its bar.
                if progress.started() && !progress.is_cancelled() {
                    let percentage = progress.percentage();
                    let left = (w - 200.0) / 2.0;
                    let right = left + (200.0 * percentage as f32).round();
                    batch.fill(atlas, left - 1.0, 79.0, right - left + 2.0, 17.0, u32::MAX);
                    batch.fill(
                        atlas,
                        left,
                        80.0,
                        (right - left).max(0.0),
                        15.0,
                        0xFF80_8080,
                    );
                    let percent = format!("{:.1}", percentage * 100.0);
                    centered(
                        batch,
                        atlas,
                        &crate::lang::tr("mco.upload.percent", &[&percent]),
                        cx,
                        84.0,
                        u32::MAX,
                    );
                    // `extractUploadSpeed`: `(Unit.humanReadable(bytesPerSecond) + "/s")`
                    // just right of the percentage. The rate is the bytes over the
                    // screen's elapsed time (`UploadStatus.refreshBytesPerSecond`'s idea).
                    let elapsed = started.elapsed().as_secs_f64();
                    let rate = if elapsed > 0.0 {
                        (progress.written() as f64 / elapsed) as u64
                    } else {
                        0
                    };
                    if rate > 0 {
                        let label = format!("({}/s)", human_readable(rate));
                        let percent_w = HudBatch::text_width(atlas, &percent);
                        batch.text(
                            atlas,
                            &label,
                            cx + (percent_w / 2.0).floor() + 15.0,
                            84.0,
                            u32::MAX,
                            true,
                        );
                    }
                }
                if let Some(upload::UploadError::TooLarge) = error {
                    centered(
                        batch,
                        atlas,
                        &t("mco.upload.failed.too_big.title"),
                        cx,
                        110.0,
                        RED,
                    );
                    let size = human_readable(upload::SIZE_LIMIT);
                    centered(
                        batch,
                        atlas,
                        &crate::lang::tr("mco.upload.failed.too_big.description", &[&size]),
                        cx,
                        122.0,
                        RED,
                    );
                }
            }
            SubScreen::CreateWorldUpload { .. } => {
                // `AlertScreen.extractRenderState`: the title at 70 over an empty message;
                // the Cancel button is drawn with the screen's buttons.
                centered(
                    batch,
                    atlas,
                    &t("mco.create.world.reset.title"),
                    cx,
                    70.0,
                    u32::MAX,
                );
            }
            SubScreen::Main => self.draw_main(batch, atlas, cursor, w, h, list_focused),
        }
    }

    /// `RealmsMainScreen.NotificationMessageEntry`: the info sprite, the message centred in
    /// the 220 the 40-wide margins leave, the dismiss cross and the open-link button.
    /// Returns the entry's height, which moves the servers below it.
    fn draw_notice(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        rx: f32,
        top: f32,
        bottom: f32,
    ) -> f32 {
        let Some(notification) = self.notice() else {
            self.notice_height.set(0.0);
            return 0.0;
        };
        let lines = self.notice_lines(atlas);
        // `MultiLineTextWidget`'s line height is 9 (`wordWrapHeight`).
        let message_height = lines.len() as f32 * 9.0;
        // `addEntriesForNotification`: HEIGHT_WITHOUT_TEXT 38 plus the wrapped message.
        let height = 38.0 + message_height;
        let y = top + 2.0;
        self.notice_height.set(height);
        // The list scrolled the entry out of view: nothing draws, nothing clicks.
        if y + height <= top || y >= bottom {
            self.notice_rects.set([[0.0; 4]; 2]);
            return height;
        }
        // The grid starts at the entry's content origin: the entry's own +2.
        let content_x = rx + 2.0;
        // `ImageWidget.sprite(20, 20, icon/info)`, padded 7.
        crate::ui::blit_sprite(
            batch,
            atlas,
            "gui/sprites/icon/info",
            content_x + 7.0,
            y + 7.0,
            20.0,
            20.0,
            u32::MAX,
        );
        // Row 0's height is the icon's 27 or the text frame's 7 + message; the text is
        // centred in `textWidth` (300 - 80) from the content's 40.
        let row_height = 27.0_f32.max(7.0 + message_height);
        for (i, line) in lines.iter().enumerate() {
            let width = HudBatch::text_width(atlas, line);
            batch.text(
                atlas,
                line,
                content_x + 40.0 + ((ROW_WIDTH - 80.0 - width) / 2.0).floor(),
                y + 7.0 + i as f32 * 9.0,
                u32::MAX,
                true,
            );
        }
        // `VisitUrl.buildOpenLinkButton`: 150 wide, 4 below row 0, centred in the text
        // frame's 220.
        let button = [
            content_x + 40.0 + ((ROW_WIDTH - 80.0 - 150.0) / 2.0).floor(),
            y + row_height + 4.0,
            150.0,
            20.0,
        ];
        let label = Text::literal(notification.visit_button_text());
        let hovered = crate::ui::hovered(cursor, button[0], button[1], button[2], button[3]);
        crate::ui::button(
            batch, atlas, button[0], button[1], button[2], button[3], &label, true, hovered,
        );
        // `CrossButton`: 14x14, padded 7 from the content's right and top.
        let dismiss = [content_x + ROW_WIDTH - 7.0 - 14.0, y + 7.0, 14.0, 14.0];
        if notification.dismissable {
            let hovered =
                crate::ui::hovered(cursor, dismiss[0], dismiss[1], dismiss[2], dismiss[3]);
            let sprite = if hovered {
                "gui/sprites/widget/cross_button_highlighted"
            } else {
                "gui/sprites/widget/cross_button"
            };
            crate::ui::blit_sprite(
                batch,
                atlas,
                sprite,
                dismiss[0],
                dismiss[1],
                dismiss[2],
                dismiss[3],
                u32::MAX,
            );
        }
        self.notice_rects.set([button, dismiss]);
        self.notice_height.set(height);
        height
    }

    fn draw_main(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        w: f32,
        h: f32,
        list_focused: bool,
    ) {
        let cx = (w / 2.0).floor();
        let mid = (h / 2.0).floor();
        let t = crate::lang::get;
        // RealmsHeader: the Realms logo, and the join button's width (`text + 16`, at
        // most half of the 308 content the 128-wide logo leaves).
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
        let join = t("mco.selectServer.joinRealm");
        self.join_width
            .set((HudBatch::text_width(atlas, &join) + 16.0).min(90.0));
        let footer_h = main_footer_h(self.layout_state());
        let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
        // Only the LIST layout has an `ObjectSelectionList`: LOADING and NO_REALMS put a
        // `LoadingDotsWidget` or the empty-state content straight into the contents frame,
        // so no list band or separators.
        if self.layout_state() == LayoutState::List {
            crate::ui::list_background(batch, atlas, top, bottom, false);
            // `AbstractSelectionList.extractListSeparators`: the 2-tall lines over the
            // list's top and bottom edges.
            crate::ui::tiled_sized(
                batch,
                atlas,
                "gui/header_separator",
                0.0,
                top - 2.0,
                w,
                2.0,
                [32.0, 2.0],
                u32::MAX,
            );
            crate::ui::tiled_sized(
                batch,
                atlas,
                "gui/footer_separator",
                0.0,
                bottom,
                w,
                2.0,
                [32.0, 2.0],
                u32::MAX,
            );
        }
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
                // `layout.addToContents(new LoadingDotsWidget(...))`: the widget is 28 tall
                // (2 + 9 + 6 + 9 + 2) and sits at the contents' top — `header + 30`, or the
                // space above the footer when the window is shorter. The message is white
                // at +2, the dots grey (-8355712) at +17.
                let y = 74.0_f32.min(h - footer_h - 28.0);
                centered(
                    batch,
                    atlas,
                    &t("mco.selectServer.loading"),
                    cx,
                    y + 2.0,
                    u32::MAX,
                );
                centered(batch, atlas, loading_dots(), cx, y + 17.0, 0xFF80_8080);
            }
            FetchState::Error(e) => paragraph(batch, &e.text(), RED),
            FetchState::Loaded(servers) if servers.is_empty() && self.notice().is_some() => {
                self.draw_notice(batch, atlas, cursor, rx, top, bottom);
            }
            FetchState::Loaded(servers) if servers.is_empty() => {
                self.notice_height.set(0.0);
                // `createNoRealmsContent`: the 130x64 image, 8 of spacing, then a
                // `FocusableTextWidget` padded 4 and wrapping at 308 - 8 = 300. The
                // column sits at the contents' top (74), or above the footer when short.
                let lines = wrap_lines(&t("mco.selectServer.noRealms"), atlas, 300.0);
                let content_h = 64.0 + 8.0 + lines.len() as f32 * 9.0 + 8.0;
                let img_y = 74.0_f32.min(h - footer_h - content_h);
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
                for (i, line) in lines.iter().enumerate() {
                    centered(
                        batch,
                        atlas,
                        line,
                        cx,
                        img_y + 64.0 + 8.0 + 4.0 + i as f32 * 9.0,
                        u32::MAX,
                    );
                }
            }
            FetchState::Loaded(servers) => {
                // `NotificationMessageEntry` sits above the servers; the list scrolls
                // (`ObjectSelectionList`), so both move with the scroll and the rows
                // cull at the edges instead of stopping the list.
                let amount = self.main_scroll.amount;
                let notice = self.draw_notice(batch, atlas, cursor, rx, top - amount, bottom);
                let content = notice + servers.len() as f32 * ROW_HEIGHT;
                // `AbstractSelectionList` scissorses its entries (a half-visible row must
                // not paint over the header or the footer); the scrollbar draws outside.
                let first = batch.vertices.len();
                let mut face_slot = 0usize;
                for (i, server) in servers.iter().enumerate() {
                    let y = top + 2.0 + notice + i as f32 * ROW_HEIGHT - amount;
                    if y + ROW_HEIGHT <= top || y >= bottom {
                        continue;
                    }
                    if self.selected == Some(i) {
                        // `AbstractSelectionList.extractSelection`: the entry's own
                        // 300x36 rectangle in white with the list focused, grey without,
                        // then its black inner fill.
                        batch.fill(
                            atlas,
                            rx,
                            y,
                            ROW_WIDTH,
                            ROW_HEIGHT,
                            if list_focused {
                                0xFFFF_FFFF
                            } else {
                                0xFF80_8080
                            },
                        );
                        batch.fill(
                            atlas,
                            rx + 1.0,
                            y + 1.0,
                            ROW_WIDTH - 2.0,
                            ROW_HEIGHT - 2.0,
                            rgba(0, 0, 0, 255),
                        );
                    }
                    if server.state == "UNINITIALIZED" {
                        // `ServerEntry.extractContent`: `getContentX() - 5` is the globe
                        // (contentX is the row's +2), `getContentX() + 40 - 2` the green
                        // "click to start" line instead of the name and owner.
                        crate::ui::blit_sprite(
                            batch,
                            atlas,
                            "gui/sprites/icon/new_realm",
                            rx + 2.0 - 5.0,
                            y + ROW_HEIGHT / 2.0 - 10.0,
                            40.0,
                            20.0,
                            u32::MAX,
                        );
                        batch.text(
                            atlas,
                            &t("mco.selectServer.uninitialized"),
                            rx + 40.0,
                            y + ROW_HEIGHT / 2.0 - 4.0,
                            rgba(127, 255, 127, 255),
                            true,
                        );
                        continue;
                    }
                    // `ServerEntry.extractContent`: the owner's 32px face at the content
                    // origin (`extractPlayerFace`), the lines at +1/+12/+23 of it, the
                    // text 2 right of the face's 36.
                    let face =
                        self.face_sprite(batch, atlas, server.owner_uuid.as_deref(), face_slot);
                    face_slot += 1;
                    batch.sprite_scaled(
                        atlas,
                        face.as_deref().unwrap_or("misc/unknown_server"),
                        rx + 2.0,
                        y + 2.0,
                        32.0,
                        32.0,
                        u32::MAX,
                    );
                    let text_x = rx + 38.0;
                    let first_y = y + 1.0;
                    let minigame_active = server.world_type == "MINIGAME";
                    // `extractFirstLine`: the version right-aligned 20 in from the edge,
                    // grey; the name clamps before it (`extractClampedString`).
                    let version = if minigame_active || server.active_version.is_empty() {
                        None
                    } else {
                        Some(server.active_version.clone())
                    };
                    let version_x = version.as_ref().map(|version| {
                        rx + ROW_WIDTH - HudBatch::text_width(atlas, version) - 20.0
                    });
                    let name_max = version_x.unwrap_or(rx + ROW_WIDTH - 38.0 - 20.0);
                    let name = clamp_text(atlas, &server.name, name_max - text_x);
                    batch.text(atlas, &name, text_x, first_y, u32::MAX, true);
                    if let Some(version) = &version {
                        batch.text(
                            atlas,
                            version,
                            version_x.expect("a version x"),
                            first_y,
                            GREY,
                            true,
                        );
                    }
                    // `extractSecondLine`: a minigame Realm names it — the key's text in
                    // -171 and the name grey — any other world shows its game mode
                    // right-aligned (hardcore in red with its heart) with the description
                    // clamping before it.
                    let second_y = y + 12.0;
                    if minigame_active {
                        let template = t("mco.selectServer.minigameName");
                        let prefix = template.strip_suffix("%s").unwrap_or(&template);
                        // The whole line carries -171 (#FFFF55); the name's grey style
                        // takes over for its span.
                        batch.text(atlas, prefix, text_x, second_y, 0xFF55_FFFF, true);
                        if let Some(minigame) =
                            server.minigame_name.as_deref().filter(|m| !m.is_empty())
                        {
                            let px = text_x + HudBatch::text_width(atlas, prefix);
                            batch.text(atlas, minigame, px, second_y, GREY, true);
                        }
                    } else {
                        let game_mode = if server.game_mode == 0 {
                            Some(t("selectWorld.gameMode.survival"))
                        } else if server.game_mode == 1 {
                            Some(t("selectWorld.gameMode.creative"))
                        } else if server.game_mode == 2 {
                            Some(t("selectWorld.gameMode.adventure"))
                        } else {
                            None
                        };
                        let mode = match game_mode.as_deref() {
                            // `getGameModeComponent`: hardcore's own name in -65536 with
                            // the heart 10 left of it; the others grey.
                            Some(_) if server.is_hardcore => {
                                Some((t("gameMode.hardcore"), 0xFF00_00FF))
                            }
                            Some(mode) => Some((mode.to_owned(), GREY)),
                            None => None,
                        };
                        let mode_x = mode.as_ref().map(|(mode, _)| {
                            rx + ROW_WIDTH - HudBatch::text_width(atlas, mode) - 20.0
                        });
                        if let (Some((mode, color)), Some(mode_x)) = (&mode, mode_x) {
                            batch.text(atlas, mode, mode_x, second_y, *color, true);
                        }
                        if server.is_hardcore
                            && let Some(mode_x) = mode_x
                            && atlas
                                .sprites
                                .contains_key("gui/sprites/hud/heart/hardcore_full")
                        {
                            crate::ui::blit_sprite(
                                batch,
                                atlas,
                                "gui/sprites/hud/heart/hardcore_full",
                                mode_x - 10.0,
                                second_y,
                                8.0,
                                8.0,
                                u32::MAX,
                            );
                        }
                        // `extractClampedString`: the description ends before the mode.
                        let max = mode_x.unwrap_or(rx + ROW_WIDTH - 38.0 - 20.0);
                        let description = clamp_text(atlas, &server.motd, max - text_x);
                        batch.text(atlas, &description, text_x, second_y, GREY, true);
                    }
                    // `extractThirdLine`: other people's Realms name their owner; the
                    // owner's own expired Realm says so in -2142128.
                    let self_owned = session().is_some_and(|me| server.owned_by(me.uuid));
                    let third_y = y + 23.0;
                    if !self_owned {
                        batch.text(
                            atlas,
                            server.owner.as_deref().unwrap_or(""),
                            text_x,
                            third_y,
                            GREY,
                            true,
                        );
                    } else if server.expired {
                        let text = if server.expired_trial {
                            t("mco.selectServer.expiredTrial")
                        } else {
                            t("mco.selectServer.expiredList")
                        };
                        batch.text(atlas, &text, text_x, third_y, 0xFF50_50DF, true);
                    }
                    // `extractStatusLights`: expired, closed, the owner's Realm with under
                    // a week left, or open; their tooltips hang off the light in
                    // [`Screen::draw_tooltip`].
                    let light = if server.expired {
                        Some("gui/sprites/realm_status/expired")
                    } else if server.state == "CLOSED" {
                        Some("gui/sprites/realm_status/closed")
                    } else if self_owned && server.days_left < 7 {
                        Some("gui/sprites/realm_status/expires_soon")
                    } else if server.is_open() {
                        Some("gui/sprites/realm_status/open")
                    } else {
                        None
                    };
                    if let Some(light) = light
                        && atlas.sprites.contains_key(light)
                    {
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
                    // `ServerEntry.extractOnlinePlayers`: up to the row's end, 9x9 faces
                    // 12 apart (`9 * count + 3 * (count - 1)` wide, ending 21 in).
                    if let Some(players) = self.live_players.get(&server.id)
                        && !players.is_empty()
                    {
                        let count = players.len() as f32;
                        let mut px = rx + ROW_WIDTH - 21.0 - (9.0 * count + 3.0 * (count - 1.0));
                        for uuid in players {
                            if let Some(face) =
                                self.face_sprite(batch, atlas, Some(uuid), face_slot)
                            {
                                batch.sprite_scaled(
                                    atlas,
                                    &face,
                                    px,
                                    y + ROW_HEIGHT - 13.0,
                                    9.0,
                                    9.0,
                                    u32::MAX,
                                );
                            }
                            face_slot += 1;
                            px += 12.0;
                        }
                    }
                }
                batch.clip_since(first, 0.0, top, w, bottom);
                // `AbstractSelectionList.getScrollbarPosition`: rowRight + 6 + 2.
                crate::ui::scrollbar(
                    batch,
                    atlas,
                    rx + ROW_WIDTH + 8.0,
                    top,
                    bottom,
                    content,
                    amount,
                );
            }
        }
    }

    /// `Entry.extractRealmStatus`'s tooltip: the hovered status light names the Realm's
    /// state. Drawn after the buttons, so nothing covers it (as `MultiplayerScreen`'s).
    pub fn draw_tooltip(
        &self,
        atlas: &HudAtlas,
        batch: &mut HudBatch,
        cursor: (f32, f32),
        (w, h): (f32, f32),
    ) {
        // `RealmsHeader.setPendingInvites`/`setNews` tooltips, and
        // `RealmsMainScreen.updateButtonStates`' closed-Realm Play hint. Both screens
        // share the header; only the list has the Play button.
        if matches!(self.subscreen, SubScreen::Main | SubScreen::AddRealm { .. }) {
            let cx = (w / 2.0).floor();
            let invite = [cx + 110.0, 12.0, 20.0, 20.0];
            let news = [cx + 134.0, 12.0, 20.0, 20.0];
            if crate::ui::hovered(cursor, invite[0], invite[1], 20.0, 20.0) {
                let key = if self.pending_invites == 0 {
                    "mco.invites.nopending"
                } else {
                    "mco.invites.pending"
                };
                crate::ui::tooltip(batch, atlas, &[Text::translate(key, &[])], cursor);
            } else if crate::ui::hovered(cursor, news[0], news[1], 20.0, 20.0) {
                crate::ui::tooltip(batch, atlas, &[Text::translate("mco.news", &[])], cursor);
            }
            if matches!(self.subscreen, SubScreen::Main)
                && self.layout_state() == LayoutState::List
                && crate::ui::hovered(cursor, cx - 154.0, h - 55.0, 100.0, 20.0)
                && self
                    .selected_server()
                    .is_some_and(|server| server.state == "CLOSED" && !server.expired)
            {
                crate::ui::tooltip(
                    batch,
                    atlas,
                    &[Text::translate("mco.play.button.realm.closed", &[])],
                    cursor,
                );
            }
        }
        // `RealmsSettingsTab`'s widget tooltips (`nameEdit.setTooltip`, the distance
        // options' `cachedConstantTooltip`) and `setTabTooltip(3, ...)`: drawn last, so
        // the scrollbar and the widgets cannot cover them.
        if let SubScreen::Configure {
            server,
            tab: ConfigureTab::Settings,
            settings: Some(draft),
            tier,
            ..
        } = &self.subscreen
        {
            let (bar_x, tab_w) = tab_geometry(w);
            if server.expired
                && crate::ui::hovered(cursor, bar_x + 3.0 * tab_w, 0.0, tab_w, TAB_HEIGHT)
            {
                crate::ui::tooltip(
                    batch,
                    atlas,
                    &[Text::translate("mco.configure.world.settings.expired", &[])],
                    cursor,
                );
            }
            let (rows, content_h) = settings_rows(matches!(tier, Some(Ok(_))));
            let top = settings_column_top(h, content_h);
            let max_scroll = (content_h - (h - 24.0 - 33.0)).max(0.0);
            let scroll = self.settings_scroll.amount.min(max_scroll);
            let left = (w / 2.0).floor() - 106.0;
            let label_y = |row: usize| top + rows[row] + SETTINGS_CELL_PADDING - scroll;
            let widget_y =
                |row: usize| label_y(row) + SETTINGS_LABEL_HEIGHT + SETTINGS_LABEL_SPACING;
            // The `ScrollableLayout` scissor hides a widget outside the view.
            let (view_top, view_bottom) = (24.0, h - 33.0);
            let visible = |y: f32| y >= view_top && y + SETTINGS_WIDGET_HEIGHT <= view_bottom;
            let name_y = widget_y(0);
            if !realm_name_valid(&draft.name)
                && visible(name_y)
                && crate::ui::hovered(cursor, left, name_y, 212.0, SETTINGS_WIDGET_HEIGHT)
            {
                crate::ui::tooltip(
                    batch,
                    atlas,
                    &[Text::translate(
                        "mco.configure.world.name.validation.whitespace",
                        &[],
                    )],
                    cursor,
                );
            } else if let Some(Ok(config)) = tier.as_ref() {
                for (row, range, picked, key) in [
                    (
                        2,
                        config.render_distance.as_ref(),
                        draft.render_distance,
                        "mco.configure.world.renderDistance.tooltip",
                    ),
                    (
                        3,
                        config.sim_distance.as_ref(),
                        draft.simulation_distance,
                        "mco.configure.world.simulationDistance.tooltip",
                    ),
                ] {
                    if range.is_none() || picked.is_none() {
                        continue;
                    }
                    let y = widget_y(row);
                    if visible(y)
                        && crate::ui::hovered(cursor, left, y, 212.0, SETTINGS_WIDGET_HEIGHT)
                    {
                        crate::ui::tooltip(batch, atlas, &[Text::translate(key, &[])], cursor);
                        break;
                    }
                }
            }
        }
        // The list's own tooltips only on the list: another screen's widgets may sit over
        // the rows' coordinates.
        if !matches!(self.subscreen, SubScreen::Main) {
            return;
        }
        let servers = match &self.state {
            FetchState::Loaded(servers) if !servers.is_empty() => servers,
            _ => return,
        };
        let footer_h = main_footer_h(self.layout_state());
        let (top, bottom) = (HEADER_HEIGHT, h - footer_h);
        let rx = ((w - ROW_WIDTH) / 2.0).floor();
        let amount = self.main_scroll.amount;
        let notice = if self.notice().is_some() {
            self.notice_height.get()
        } else {
            0.0
        };
        let me = session().map(|s| s.uuid);
        for (i, server) in servers.iter().enumerate() {
            if server.state == "UNINITIALIZED" {
                continue;
            }
            let y = top + 2.0 + notice + i as f32 * ROW_HEIGHT - amount;
            if y + ROW_HEIGHT <= top || y >= bottom {
                continue;
            }
            // `extractOnlinePlayers`: the faces' strip takes the tooltip before the
            // status light does (it answers true and returns).
            if let Some(players) = self.live_players.get(&server.id)
                && !players.is_empty()
            {
                let count = players.len() as f32;
                let width = 9.0 * count + 3.0 * (count - 1.0);
                let start = rx + ROW_WIDTH - 21.0 - width;
                if cursor.0 >= start
                    && cursor.0 <= start + width
                    && cursor.1 >= y + ROW_HEIGHT - 13.0
                    && cursor.1 <= y + ROW_HEIGHT - 4.0
                {
                    crate::ui::tooltip(
                        batch,
                        atlas,
                        &[Text::translate("mco.onlinePlayers", &[])],
                        cursor,
                    );
                    break;
                }
            }
            let light = [rx + ROW_WIDTH - 17.0, y + 2.0, 10.0, 28.0];
            if cursor.0 < light[0]
                || cursor.0 >= light[0] + light[2]
                || cursor.1 < light[1]
                || cursor.1 >= light[1] + light[3]
            {
                continue;
            }
            // The light's priority order decides the text: expired, closed, the owner's
            // Realm with under a week left, or open.
            let self_owned = me.is_some_and(|me| server.owned_by(me));
            let text = if server.expired {
                Some(Text::translate("mco.selectServer.expired", &[]))
            } else if server.state == "CLOSED" {
                Some(Text::translate("mco.selectServer.closed", &[]))
            } else if self_owned && server.days_left < 7 {
                Some(if server.days_left <= 0 {
                    Text::translate("mco.selectServer.expires.soon", &[])
                } else if server.days_left == 1 {
                    Text::translate("mco.selectServer.expires.day", &[])
                } else {
                    Text::translate(
                        "mco.selectServer.expires.days",
                        &[Text::literal(server.days_left.to_string())],
                    )
                })
            } else if server.is_open() {
                Some(Text::translate("mco.selectServer.open", &[]))
            } else {
                None
            };
            if let Some(text) = text {
                crate::ui::tooltip(batch, atlas, &[text], cursor);
            }
            break;
        }
    }
}

fn loading_dots() -> &'static str {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    ["O o o", "o O o", "o o O", "o O o"][(millis / 300 % 4) as usize]
}

/// `Entry.extractClampedString`: the string cut to `max` pixels — vanilla cuts with
/// `plainSubstrByWidth` into the space `width("... ")` leaves and appends the dots.
fn clamp_text(atlas: &HudAtlas, text: &str, max: f32) -> String {
    if max <= 0.0 || HudBatch::text_width(atlas, text) <= max {
        return text.to_owned();
    }
    let room = max - HudBatch::text_width(atlas, "... ");
    let mut cut = String::new();
    for ch in text.chars() {
        let mut candidate = cut.clone();
        candidate.push(ch);
        if HudBatch::text_width(atlas, &candidate) > room {
            break;
        }
        cut = candidate;
    }
    format!("{cut}...")
}

/// `RealmsScreen.realmsLogo`: `ImageWidget.texture(128, 34, LOGO_LOCATION, 128, 64)` —
/// the top half of `textures/gui/title/realms.png` at 128×34.
fn realms_logo(batch: &mut HudBatch, atlas: &HudAtlas, cx: f32, y: f32) {
    batch.sprite_region(
        atlas,
        "gui/title/realms",
        [cx - 64.0, y, 128.0, 34.0],
        128.0,
        64.0,
    );
}

/// `RealmsMainScreen.updateLayout`'s footer height: the two-row grid (44) plus its 22
/// of padding when the list shows, the single row's 20 otherwise. `FrameLayout` centres
/// the grid, so the rows sit 11 into the area - LIST at h-55/h-31, otherwise h-31.
fn main_footer_h(state: LayoutState) -> f32 {
    if state == LayoutState::List {
        66.0
    } else {
        42.0
    }
}

/// `RealmsLongRunningMcoTaskScreen`'s centered stack (`FrameLayout.centerInRectangle`):
/// the logo 34 tall, the 28-tall `LoadingDotsWidget` with 10 above and 30 below
/// (`paddingTop(10).paddingBottom(30)`), then the 150×20 Cancel button — 122 in all.
fn task_stack_top((_, h): (f32, f32)) -> f32 {
    ((h - 122.0) / 2.0).floor()
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
            saves: None,
            news_link: None,
            news_job: None,
            news_unread: false,
            pending_invites: 0,
            pending_invites_job: None,
            join_width: std::cell::Cell::new(90.0),
            pdp_row: None,
            list_clicked: false,
            link_buttons: std::cell::Cell::new([(0.0, 0.0); 3]),
            terms_link: std::cell::Cell::new([0.0; 4]),
            notifications: Vec::new(),
            notifications_job: None,
            handled_seen: Vec::new(),
            dismiss_job: None,
            notice_rects: std::cell::Cell::new([[0.0; 4]; 2]),
            notice_height: std::cell::Cell::new(0.0),
            notice_buttons: std::cell::Cell::new([[0.0; 4]; 2]),
            popup_buttons: std::cell::Cell::new([[0.0; 4]; 2]),
            main_scroll: crate::ui::Scroll::default(),
            subscription_buttons: std::cell::Cell::new((0.0, 0.0)),
            players_button_rects: std::cell::RefCell::new(Vec::new()),
            backup_row_buttons: std::cell::RefCell::new(Vec::new()),
            settings_scroll: crate::ui::Scroll::default(),
            settings_slider: None,
            other_slider: None,
            after_save: None,
            redirect_to_pdp: false,
            template_link_rect: std::cell::Cell::new([0.0; 4]),
            trial: false,
            trial_job: None,
            live_players: std::collections::HashMap::new(),
            live_job: None,
            last_list_refresh: Instant::now(),
            last_pending_refresh: Instant::now(),
            last_news_refresh: Instant::now(),
            region_screen_requested: false,
            template_images: std::cell::RefCell::new(std::collections::HashMap::new()),
            world_icons: std::cell::RefCell::new(std::collections::HashMap::new()),
            skin_workers: SkinWorkers {
                profiles: std::sync::Arc::new(std::sync::Mutex::new(crate::skins::Resolver::new())),
                downloads: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::skins::Downloader::new(std::env::temp_dir().join("horizon-test-skins")),
                )),
            },
            skin_faces: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    fn notice(uuid: &str) -> RealmsNotification {
        RealmsNotification {
            uuid: uuid.to_owned(),
            dismissable: true,
            seen: false,
            kind: "visitUrl".into(),
            url: Some("https://realms.example/notice".into()),
            button_text: None,
            message: None,
            title: None,
            image: None,
            url_button: None,
        }
    }

    /// `RealmsMainScreen`: a notification alone makes the list, and the first `visitUrl` is
    /// its entry; it is answered seen once (`handledSeenNotifications`).
    #[test]
    fn the_lists_first_visit_url_is_answered_seen_once() {
        let mut screen = listed(Vec::new());
        screen.notifications = vec![notice("a"), notice("b")];
        assert_eq!(screen.layout_state(), LayoutState::List);
        screen.mark_notifications_seen();
        assert_eq!(screen.handled_seen, vec!["a".to_owned()]);
        screen.mark_notifications_seen();
        assert_eq!(screen.handled_seen, vec!["a".to_owned()], "once");
        // An infoPopup is not the list's entry.
        let mut info = notice("c");
        info.kind = "infoPopup".into();
        screen.notifications = vec![info];
        assert!(screen.notice().is_none());
    }

    /// `RealmsMainScreen.dismissNotification`: a dismissable entry leaves at once; without a
    /// session no request goes out.
    #[test]
    fn dismissing_a_notification_takes_it_off_the_list() {
        let mut screen = listed(Vec::new());
        screen.notifications = vec![notice("a")];
        screen.dismiss_notification("a");
        assert!(screen.notifications.is_empty());
        assert!(screen.dismiss_job.is_none());
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
                last: Box::new(SubScreen::Main),
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
        // `LongRunningTask.error` replaces the connecting screen with the generic error
        // screen; its OK returns to the list.
        assert!(matches!(
            screen.subscreen,
            SubScreen::Error { ref next, .. } if matches!(**next, SubScreen::Main)
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
        // `LongRunningTask.error` replaces the connecting screen with the generic error
        // screen; its OK returns to the list.
        assert!(matches!(
            screen.subscreen,
            SubScreen::Error { ref next, .. } if matches!(**next, SubScreen::Main)
        ));
    }

    /// `GetServerDetailsTask`: a 6002 answer shows `RealmsTermsScreen`, and Disagree
    /// returns to the screen the join started from.
    #[test]
    fn a_6002_join_shows_the_terms_and_disagree_goes_back() {
        let mut screen = listed(vec![server(7, None)]);
        let job = Arc::new(Mutex::new(Some(Err(RealmsError::Service {
            status: 400,
            code: Some(6002),
            reason: None,
            message: None,
        }))));
        screen.joining = Some((job, Arc::new(AtomicBool::new(false))));
        screen.subscreen = SubScreen::Connecting {
            server: server(7, None),
            error: None,
            last: Box::new(SubScreen::Main),
        };
        assert!(matches!(screen.poll(), Outcome::None));
        assert!(matches!(screen.subscreen, SubScreen::Terms { .. }));
        // `RealmsTermsScreen.init`: Agree and Disagree are 23 and 24; Disagree is the
        // screen the join started from.
        let buttons = screen.buttons(800.0, 600.0);
        assert!(buttons.iter().any(|b| b.5 == 23));
        assert!(buttons.iter().any(|b| b.5 == 24));
        assert!(matches!(
            screen.click(Some(24), 0.0, 0.0, (800.0, 600.0)),
            Outcome::None
        ));
        assert!(matches!(screen.subscreen, SubScreen::Main));
    }

    #[test]
    fn cancelling_a_join_aborts_its_worker() {
        let mut screen = listed(vec![server(7, None)]);
        let aborted = Arc::new(AtomicBool::new(false));
        screen.joining = Some((Arc::new(Mutex::new(None)), Arc::clone(&aborted)));
        screen.subscreen = SubScreen::Connecting {
            server: server(7, None),
            error: None,
            last: Box::new(SubScreen::Main),
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

    /// `InviteCodeExpiration.closestTo`: an existing expiration reads back as the choice
    /// nearest to the time left.
    #[test]
    fn an_expiration_reads_back_as_the_closest_choice() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert_eq!(closest_expiration(now + 30 * 60 * 1000), 0);
        assert_eq!(closest_expiration(now + 60 * 60 * 1000), 1);
        assert_eq!(closest_expiration(now + 6 * 24 * 60 * 60 * 1000), 5);
        // A week is the longest timed choice: anything further reads back as it. (The
        // "never" row is the fallback for a code with no expiration, not a nearest.)
        assert_eq!(closest_expiration(now + 30 * 24 * 60 * 60 * 1000), 5);
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

    /// `ScrollableLayout`'s scissor: the region and open/close buttons live inside the
    /// scroll view, so one below it must not be drawn (nor clickable) outside the tab.
    #[test]
    fn the_settings_buttons_leave_the_scroll_view() {
        let mut screen = listed(vec![server(7, None)]);
        let server = server(7, None);
        screen.subscreen = SubScreen::Configure {
            server: server.clone(),
            tab: ConfigureTab::Settings,
            subscription: None,
            subscription_job: None,
            detail: None,
            detail_job: None,
            deleting: None,
            member_job: None,
            settings: Some(SettingsDraft::of(&server)),
            players_scroll: crate::ui::Scroll::default(),
            tier: None,
            tier_job: None,
            regions: None,
            regions_job: None,
            saving: None,
            error: None,
            job: None,
        };
        let ids =
            |h: f32| -> Vec<u8> { screen.buttons(800.0, h).into_iter().map(|b| b.5).collect() };
        // 230 tall: the toggle (row 5) is past the 24..h-33 view, the region button is in.
        let short = ids(230.0);
        assert!(short.contains(&89));
        assert!(!short.contains(&30), "{short:?}");
        // Tall enough for the whole column: both are offered.
        let tall = ids(400.0);
        assert!(tall.contains(&89) && tall.contains(&30), "{tall:?}");
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

    /// `RealmsWorldSlotButton.State`'s texture order: the minigame's image (or
    /// `isles.png`), the empty frame, the slot's template image, the panoramas.
    #[test]
    fn the_slot_picture_follows_vanillas_order() {
        assert_eq!(
            slot_picture(4, true, Some("img")),
            SlotPicture::Image("img")
        );
        assert_eq!(slot_picture(4, true, None), SlotPicture::Preset);
        assert_eq!(slot_picture(4, false, None), SlotPicture::Preset);
        assert_eq!(slot_picture(2, true, Some("img")), SlotPicture::Empty);
        assert_eq!(
            slot_picture(2, false, Some("img")),
            SlotPicture::Image("img")
        );
        assert_eq!(
            slot_picture(1, false, None),
            SlotPicture::Panorama("panorama_0")
        );
        assert_eq!(
            slot_picture(2, false, None),
            SlotPicture::Panorama("panorama_2")
        );
        assert_eq!(
            slot_picture(3, false, None),
            SlotPicture::Panorama("panorama_3")
        );
    }

    /// `RealmsSelectWorldTemplateScreen`'s empty message: vanilla passes `"%link"` as the
    /// translation's `%s` argument, so only that line carries the link.
    #[test]
    fn the_empty_template_message_links_one_line() {
        let lines = template_none_lines("Oops.\nPlease check back,\n%s.", "submit something");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], ("Oops.".into(), None, String::new()));
        assert_eq!(lines[1], ("Please check back,".into(), None, String::new()));
        assert_eq!(
            lines[2],
            ("".into(), Some("submit something".into()), ".".into())
        );
        // The Chinese translation's third line is `%s。`.
        let zh = template_none_lines("抱歉。\n请稍后再来，\n%s。", "提交内容");
        assert_eq!(zh[2], ("".into(), Some("提交内容".into()), "。".into()));
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
            saves: None,
            news_link: None,
            news_job: None,
            news_unread: false,
            pending_invites: 0,
            pending_invites_job: None,
            join_width: std::cell::Cell::new(90.0),
            pdp_row: None,
            list_clicked: false,
            link_buttons: std::cell::Cell::new([(0.0, 0.0); 3]),
            terms_link: std::cell::Cell::new([0.0; 4]),
            notifications: Vec::new(),
            notifications_job: None,
            handled_seen: Vec::new(),
            dismiss_job: None,
            notice_rects: std::cell::Cell::new([[0.0; 4]; 2]),
            notice_height: std::cell::Cell::new(0.0),
            notice_buttons: std::cell::Cell::new([[0.0; 4]; 2]),
            popup_buttons: std::cell::Cell::new([[0.0; 4]; 2]),
            main_scroll: crate::ui::Scroll::default(),
            subscription_buttons: std::cell::Cell::new((0.0, 0.0)),
            players_button_rects: std::cell::RefCell::new(Vec::new()),
            backup_row_buttons: std::cell::RefCell::new(Vec::new()),
            settings_scroll: crate::ui::Scroll::default(),
            settings_slider: None,
            other_slider: None,
            after_save: None,
            redirect_to_pdp: false,
            template_link_rect: std::cell::Cell::new([0.0; 4]),
            trial: false,
            trial_job: None,
            live_players: std::collections::HashMap::new(),
            live_job: None,
            last_list_refresh: Instant::now(),
            last_pending_refresh: Instant::now(),
            last_news_refresh: Instant::now(),
            region_screen_requested: false,
            template_images: std::cell::RefCell::new(std::collections::HashMap::new()),
            world_icons: std::cell::RefCell::new(std::collections::HashMap::new()),
            skin_workers: SkinWorkers {
                profiles: std::sync::Arc::new(std::sync::Mutex::new(crate::skins::Resolver::new())),
                downloads: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::skins::Downloader::new(std::env::temp_dir().join("horizon-test-skins")),
                )),
            },
            skin_faces: std::cell::RefCell::new(std::collections::HashMap::new()),
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

    /// `InviteCodeExpiration.closestTo`: the choice nearest the time left on the code.
    fn closest_expiration(expiration_millis: i64) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let left = (expiration_millis - now).max(0) / 1000;
        let mut best = EXPIRATIONS.len() - 1;
        let mut best_delta = i64::MAX;
        for (i, (_, seconds)) in EXPIRATIONS.iter().enumerate() {
            let Some(seconds) = seconds else { continue };
            let delta = (left - seconds).abs();
            if delta < best_delta {
                best_delta = delta;
                best = i;
            }
        }
        best
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

    /// `ConfirmLinkScreen`: Open in Browser, Copy to Clipboard and Cancel, 100 wide each,
    /// and every answer — Escape's no included — returns to the screen that asked.
    #[test]
    fn a_link_asks_three_ways_and_returns_to_its_parent() {
        let mut screen = listed(Vec::new());
        // The product page asks (its news button, `RealmsHeader.openNews`).
        screen.click(Some(13), 0.0, 0.0, (854.0, 480.0));
        let url = "https://realms.example/news".to_owned();
        screen.news_link = Some(url.clone());
        screen.click(Some(16), 0.0, 0.0, (854.0, 480.0));
        assert!(matches!(
            &screen.subscreen,
            SubScreen::Popup(Popup::OpenLink { .. })
        ));
        let answers: Vec<(u8, f32)> = screen
            .buttons(854.0, 480.0)
            .iter()
            .map(|b| (b.5, b.2))
            .collect();
        assert_eq!(answers, vec![(53, 100.0), (54, 100.0), (55, 100.0)]);
        // Cancel returns to the product page.
        screen.click(Some(55), 0.0, 0.0, (854.0, 480.0));
        assert!(screen.pdp_list());
        // Copy answers the link and returns there too.
        screen.click(Some(16), 0.0, 0.0, (854.0, 480.0));
        match screen.click(Some(54), 0.0, 0.0, (854.0, 480.0)) {
            Outcome::Copy(copied) => assert_eq!(copied, url),
            _ => panic!("the copy answer"),
        }
        assert!(screen.pdp_list());
        // So does Open.
        screen.click(Some(16), 0.0, 0.0, (854.0, 480.0));
        match screen.click(Some(53), 0.0, 0.0, (854.0, 480.0)) {
            Outcome::OpenUri(opened) => assert_eq!(opened, url),
            _ => panic!("the open answer"),
        }
        assert!(screen.pdp_list());
        // Escape is the no button.
        screen.click(Some(16), 0.0, 0.0, (854.0, 480.0));
        assert!(!screen.escape());
        assert!(screen.pdp_list());
    }

    /// `ObjectSelectionList.nextFocusPath`: the arrows move through the product page's
    /// rows, the first one taking the end it heads to (`nextEntry`'s null start).
    #[test]
    fn the_pdp_rows_take_the_arrows_at_their_ends() {
        let mut screen = listed(Vec::new());
        screen.click(Some(13), 0.0, 0.0, (854.0, 480.0));
        assert!(screen.pdp_arrow(false));
        assert_eq!(screen.pdp_row, Some(0));
        assert!(screen.pdp_arrow(false));
        assert_eq!(screen.pdp_row, Some(1));
        assert!(screen.pdp_arrow(true));
        assert_eq!(screen.pdp_row, Some(0));
        // The ends hold.
        assert!(screen.pdp_arrow(true));
        assert_eq!(screen.pdp_row, Some(0));
        screen.pdp_row = None;
        assert!(screen.pdp_arrow(true));
        assert_eq!(screen.pdp_row, Some(pdp_entries().len() - 1));
        // Another screen has no product page.
        screen.subscreen = SubScreen::Main;
        assert!(!screen.pdp_arrow(false));
    }

    /// `RealmsPdpScreen.AbstractPdpEntry.getNarration`: the title and its descriptions,
    /// the trial's only when the service says a trial is available (`PdpList`'s header).
    #[test]
    fn the_pdp_rows_narrate_their_lines() {
        let description = crate::lang::get("mco.pdp.friends.description");
        assert!(pdp_row_lines(1, false).contains(&description));
        let trial = crate::lang::get("mco.pdp.trial.description");
        assert!(!pdp_row_lines(0, false).contains(&trial));
        assert!(pdp_row_lines(0, true).contains(&trial));
    }

    /// `RealmsResetWorldScreen.init`'s grid: 60-wide tiles, 16 padding a side, the spacer row.
    #[test]
    fn the_reset_tiles_are_three_columns_with_vanillas_spacer() {
        // The 154-tall grid sits at min(63, h - 187): 56 belongs to a 243-tall screen.
        assert_eq!(reset_content_y(243.0), 56.0);
        assert_eq!(reset_content_y(480.0), 63.0);
        assert_eq!(reset_tile(200.0, 56.0, 0), (78.0, 56.0));
        assert_eq!(reset_tile(200.0, 56.0, 2), (262.0, 56.0));
        assert_eq!(reset_tile(200.0, 56.0, 3), (78.0, 141.0));
        assert_eq!(reset_tile(200.0, 56.0, 5), (262.0, 141.0));
    }

    #[test]
    fn the_template_groups_are_vanillas_four_in_its_order() {
        let types: Vec<&str> = TemplateGroup::ALL.iter().map(|g| g.world_type()).collect();
        assert_eq!(
            types,
            ["NORMAL", "ADVENTUREMAP", "EXPERIENCE", "INSPIRATION"]
        );
        let titles: Vec<&str> = TemplateGroup::ALL.iter().map(|g| g.title()).collect();
        assert_eq!(
            titles,
            [
                "mco.reset.world.template",
                "mco.reset.world.adventure",
                "mco.reset.world.experience",
                "mco.reset.world.inspiration"
            ]
        );
    }

    /// `RealmsResetWorldScreen`'s Generate New World tile: `RealmsCreateWorldFlow` gets
    /// the Realm, slot and reset kind to upload into.
    #[test]
    fn the_generate_tile_asks_for_the_create_world_screen() {
        for (size, (tx, ty)) in [
            ((854.0, 480.0), (305.0, 63.0)),
            ((400.0, 243.0), (78.0, 56.0)),
        ] {
            for kind in [ResetKind::Reset, ResetKind::Empty, ResetKind::NewRealm] {
                let creation = (kind == ResetKind::NewRealm)
                    .then(|| ("Realm name".to_owned(), "Description".to_owned()));
                let mut screen = listed(vec![server(7, None)]);
                screen.subscreen = SubScreen::ResetWorld {
                    server: server(7, None),
                    slot: 2,
                    kind,
                    creation: creation.clone(),
                    templates: None,
                    job: None,
                    reset: None,
                    error: None,
                };
                for (hit, (x, y)) in [
                    (Some(70), (0.0, 0.0)),
                    (None, (tx, ty)),
                    (None, (tx + 30.0, ty + 34.0)),
                    (None, (tx + 59.5, ty + 68.5)),
                ] {
                    match screen.click(hit, x, y, size) {
                        Outcome::CreateWorld(request) => {
                            assert_eq!((request.server.id, request.slot), (7, 2));
                            assert_eq!(request.kind, kind);
                            assert_eq!(request.creation, creation);
                        }
                        _ => panic!("the Generate New World tile asks for Create World"),
                    }
                }
                for (dx, dy) in [(-1.0, 0.0), (60.0, 0.0), (0.0, -1.0), (0.0, 69.0)] {
                    assert!(matches!(
                        screen.click(None, tx + dx, ty + dy, size),
                        Outcome::None
                    ));
                }
                let abort = Arc::new(AtomicBool::new(false));
                if let SubScreen::ResetWorld { reset, .. } = &mut screen.subscreen {
                    *reset = Some(RunningTask {
                        job: Arc::new(Mutex::new(None)),
                        title: Arc::new(Mutex::new(String::new())),
                        abort: Arc::clone(&abort),
                    });
                }
                for hit in [
                    None,
                    Some(70),
                    Some(71),
                    Some(72),
                    Some(73),
                    Some(74),
                    Some(75),
                ] {
                    assert!(matches!(screen.click(hit, tx, ty, size), Outcome::None));
                    assert!(matches!(
                        screen.subscreen,
                        SubScreen::ResetWorld { reset: Some(_), .. }
                    ));
                    assert!(!abort.load(Ordering::Relaxed));
                }
            }
        }
    }

    /// `RealmsCreateWorldFlow.createTemporaryWorldFolder`: level.dat without
    /// `singleplayer_uuid`, the world generation and game rules, and no player file or
    /// weather/clocks (the Realm's server makes those on its own).
    #[test]
    fn a_generated_upload_world_carries_only_what_the_realm_needs() {
        let mut level = crate::save::Level {
            name: "New World".into(),
            seed: 4242,
            day_time: 0,
            health: 20.0,
            food: 20,
            saturation: 5.0,
            ..Default::default()
        };
        level.difficulty = "normal".into();
        level.enabled_features = vec!["minecraft:vanilla".into()];
        level.data_packs = Some((vec!["vanilla".into()], Vec::new()));
        let world = CreatedWorld {
            level,
            world_gen: crate::save::WorldGen::default(),
            flat_settings: None,
            single_biome: None,
            data_packs: (vec!["vanilla".into()], Vec::new()),
            pack_dir: None,
        };
        let dir = create_temporary_world(&world).unwrap();
        let data = crate::save::read_dat(&dir.join("level.dat")).unwrap();
        let level = data.get("Data").expect("the level's data");
        assert_eq!(
            crate::save::text(level.get("LevelName")).as_deref(),
            Some("New World")
        );
        assert!(level.get("singleplayer_uuid").is_none());
        assert!(!dir.join("players").exists());
        assert!(dir.join("data/minecraft/world_gen_settings.dat").is_file());
        assert!(dir.join("data/minecraft/game_rules.dat").is_file());
        assert!(!dir.join("data/minecraft/weather.dat").exists());
        assert!(!dir.join("data/minecraft/world_clocks.dat").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `WorldTemplatePaginatedList.isLastPage`: page*size >= total, all three set.
    #[test]
    fn a_template_page_knows_when_it_is_the_last() {
        assert!(!last_template_page(1, 10, 11));
        assert!(last_template_page(2, 10, 11));
        assert!(last_template_page(1, 10, 10));
        assert!(!last_template_page(1, 10, 0));
        assert!(!last_template_page(0, 10, 10));
    }

    /// `RealmsWorldsTab.switchToMinigame`'s list returns to the Worlds tab when cancelled
    /// (Back and Escape); a reset screen's template list still cancels back to the reset
    /// screen.
    #[test]
    fn leaving_the_minigame_template_list_returns_to_the_worlds_tab() {
        let mut screen = listed(vec![server(7, None)]);
        let template_list = |group: Option<TemplateGroup>| SubScreen::TemplateList {
            server: server(7, None),
            slot: 1,
            kind: ResetKind::Reset,
            creation: None,
            group,
            templates: Vec::new(),
            selected: None,
            scroll: crate::ui::Scroll::default(),
            job: None,
            select: None,
            icons: std::cell::RefCell::new(Vec::new()),
            error: None,
        };
        screen.subscreen = template_list(None);
        let _ = screen.click(Some(78), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(
            &screen.subscreen,
            SubScreen::Configure {
                tab: ConfigureTab::Worlds,
                ..
            }
        ));
        screen.subscreen = template_list(None);
        assert!(!screen.escape());
        assert!(matches!(
            &screen.subscreen,
            SubScreen::Configure {
                tab: ConfigureTab::Worlds,
                ..
            }
        ));
        screen.subscreen = template_list(Some(TemplateGroup::ALL[0]));
        let _ = screen.click(Some(78), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(&screen.subscreen, SubScreen::ResetWorld { .. }));
    }

    fn backups_screen(server: RealmServer, list: Vec<Backup>) -> Screen {
        let mut screen = listed(vec![server.clone()]);
        screen.subscreen = SubScreen::Backups {
            server,
            slot_id: 1,
            backups: Some(Ok(list)),
            list_job: None,
            restore_job: None,
            download_task: None,
            selected: None,
            scroll: crate::ui::Scroll::default(),
        };
        screen
    }

    fn download_screen(size_job: Option<Job<u64>>) -> Screen {
        let mut screen = listed(vec![server(7, None)]);
        screen.subscreen = SubScreen::DownloadLatest {
            server: server(7, None),
            slot_id: 1,
            download: WorldDownload::default(),
            world_name: "Friends".into(),
            size_job,
            progress: Arc::new(download::DownloadProgress::default()),
            job: None,
            error: None,
            done: false,
            started: Instant::now(),
            speed: std::cell::Cell::new((0, Instant::now(), 0)),
        };
        screen
    }

    /// `RealmsUtil.convertToAgePresentationFromInstant`: the backup's age is the wall
    /// clock's distance, not the animation clock's (`just now` for every row was wrong).
    #[test]
    fn a_backups_age_is_wall_clock_time() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert_eq!(
            age_presentation(now - 2 * 3_600_000),
            crate::lang::tr("mco.time.hoursAgo", &[&2i64 as &dyn std::fmt::Display])
        );
        assert_eq!(
            age_presentation(now - 3 * 86_400_000),
            crate::lang::tr("mco.time.daysAgo", &[&3i64 as &dyn std::fmt::Display])
        );
    }

    /// `RealmsBackupScreen.init`: Download Latest starts disabled and turns on with a
    /// non-empty list; its click asks first (`downloadClicked`'s `infoPopupScreen`).
    #[test]
    fn the_download_button_needs_a_backup_and_asks_first() {
        let empty = backups_screen(server(7, None), Vec::new());
        let button = |screen: &Screen| {
            screen
                .buttons(800.0, 600.0)
                .into_iter()
                .find(|b| b.5 == 192)
                .expect("the download button")
        };
        assert!(!button(&empty).4);
        let mut full = backups_screen(server(7, None), vec![Backup::default()]);
        assert!(button(&full).4);
        // A click opens the info question over the list.
        full.click(Some(192), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(
            &full.subscreen,
            SubScreen::Popup(Popup::DownloadLatest { .. })
        ));
        // Cancel returns to a freshly read list.
        full.click(Some(51), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(
            &full.subscreen,
            SubScreen::Backups { backups: None, .. }
        ));
    }

    /// `checkDownloadSize`: a link of 5 GiB or more asks with
    /// `warningAcknowledgePopupScreen` before any download starts.
    #[test]
    fn a_five_gigabyte_world_asks_before_downloading() {
        let answered: Job<u64> = Arc::new(Mutex::new(Some(download::SIZE_LIMIT)));
        let mut screen = download_screen(Some(answered));
        let _ = screen.poll();
        assert!(matches!(
            &screen.subscreen,
            SubScreen::Popup(Popup::DownloadOversized { .. })
        ));
        // OK starts the download; without a saves folder that fails at once.
        let _ = screen.click(Some(194), 0.0, 0.0, (800.0, 600.0));
        let SubScreen::DownloadLatest { error, done, .. } = &screen.subscreen else {
            panic!("the download screen");
        };
        assert!(*done);
        assert!(error.is_some());
    }

    /// `RealmsDownloadLatestWorldScreen`: the worker's answer turns Cancel into Done, and
    /// Done returns to the backup list.
    #[test]
    fn a_finished_download_offers_done() {
        let mut screen = download_screen(None);
        if let SubScreen::DownloadLatest { job, .. } = &mut screen.subscreen {
            let answered: Job<Result<(), download::DownloadError>> =
                Arc::new(Mutex::new(Some(Ok(()))));
            *job = Some(answered);
        }
        let _ = screen.poll();
        let buttons = screen.buttons(800.0, 600.0);
        let done = buttons
            .iter()
            .find(|b| b.5 == 195)
            .expect("the footer button");
        assert_eq!(done.3, crate::lang::get("gui.done"));
        let _ = screen.click(Some(195), 0.0, 0.0, (800.0, 600.0));
        assert!(matches!(
            &screen.subscreen,
            SubScreen::Backups { backups: None, .. }
        ));
    }

    /// `RestoreTask.abortTask`: the task screen's Cancel stops the worker and returns to
    /// the Worlds tab.
    #[test]
    fn cancelling_a_restore_aborts_its_worker() {
        let mut screen = backups_screen(server(7, None), Vec::new());
        let abort = Arc::new(AtomicBool::new(false));
        let job: Job<Result<(), RealmsError>> = Arc::new(Mutex::new(None));
        if let SubScreen::Backups { restore_job, .. } = &mut screen.subscreen {
            *restore_job = Some(RestoreTask {
                job,
                abort: Arc::clone(&abort),
            });
        }
        let _ = screen.click(Some(193), 0.0, 0.0, (800.0, 600.0));
        assert!(abort.load(Ordering::Relaxed));
        assert!(matches!(
            &screen.subscreen,
            SubScreen::Configure {
                tab: ConfigureTab::Worlds,
                ..
            }
        ));
    }
}
