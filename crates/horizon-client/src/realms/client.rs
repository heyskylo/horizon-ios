//! The Realms service client: vanilla's `com.mojang.realmsclient.client.RealmsClient` and its
//! `Request`, for the signed-in player's own Microsoft account only. Every request carries
//! what `RealmsClient.execute` adds and nothing more: the `sid`, `user` and `version` cookies
//! and the three version headers. The access token goes only into the `sid` cookie (and the
//! `x-mojangauth` header vanilla adds for the invite-code service); it is never logged, never
//! put in an error and never sent anywhere else.
use horizon_core::version;
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;

/// `RealmsClient.Environment.PRODUCTION`'s base URL.
pub const DEFAULT_REALMS_HOST: &str = "https://pc.realms.minecraft.net";
/// `RealmsClient.Environment.PRODUCTION`'s `frontendV2Url` (invite codes).
pub const DEFAULT_FRONTEND_V2_HOST: &str = "https://java.frontend.realms.minecraft-services.net";

/// `Request.DEFAULT_CONNECT_TIMEOUT`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// `Request.DEFAULT_READ_TIMEOUT`.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// `RealmsClient.join`: `Request.get(url, 5000, 30000)`.
const JOIN_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Vanilla sets no user agent: `HttpURLConnection` sends the JVM's own, `Java/<version>`.
fn user_agent() -> String {
    format!("Java/{}", version::JAVA_VERSION)
}

/// Gson leaves a field at its default when the JSON has `null`; serde would fail.
fn nullable<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `RealmsServer`: the fields the screens read.
#[derive(Clone, Debug, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RealmServer {
    #[serde(default, deserialize_with = "nullable")]
    pub id: i64,
    #[serde(default)]
    pub remote_subscription_id: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub motd: String,
    #[serde(default, deserialize_with = "nullable")]
    pub state: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(rename = "ownerUUID", default)]
    pub owner_uuid: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub expired: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub expired_trial: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub days_left: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub world_type: String,
    #[serde(default, deserialize_with = "nullable")]
    pub active_slot: i32,
    #[serde(default)]
    pub minigame_name: Option<String>,
    #[serde(default)]
    pub minigame_id: Option<i32>,
    #[serde(default)]
    pub minigame_image: Option<String>,
    /// The invited players: only in the detail answer (`RealmsClient.getOwnRealm`).
    #[serde(default, deserialize_with = "nullable")]
    pub players: Vec<PlayerInfo>,
    /// The world slots by id: only in the detail answer too (`RealmsServer.slots`).
    #[serde(default = "empty_slots", deserialize_with = "slots")]
    pub slots: std::collections::HashMap<i32, RealmSlot>,
    /// The settings tab's region preference (`RealmsServer.regionSelectionPreference`).
    #[serde(default)]
    pub region_selection_preference: Option<RegionSelectionPreferenceDto>,
}

/// `PlayerInfo`: a Realm's invited player.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerInfo {
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub operator: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub accepted: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub online: bool,
}

/// `Ops`: the Realm's operators, by name (`RealmsClient.op`/`deop`).
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
pub struct Ops {
    #[serde(default, deserialize_with = "nullable")]
    pub ops: Vec<String>,
}

/// `RealmsSetting`: a slot's named setting (the hardcore marker).
#[derive(Clone, Debug, Deserialize, Serialize, Default, PartialEq)]
pub struct RealmSetting {
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub value: String,
}

/// `RealmsSlot`: one of the Realm's world slots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealmSlot {
    pub slot_id: i32,
    pub options: RealmWorldOptions,
    pub settings: Vec<RealmSetting>,
}

/// `RealmsServer.finalizeSlots`: the JSON's `slots` list (`slotList`) by slot id, each slot's
/// options a JSON string of their own (`RealmsSlot.RealmsWorldOptionsJsonAdapter`), and the
/// world slots 1 to 3 it lacks empty (`RealmsSlot.defaults`).
fn slots<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<std::collections::HashMap<i32, RealmSlot>, D::Error> {
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        #[serde(default, deserialize_with = "nullable")]
        slot_id: i32,
        #[serde(default)]
        options: Option<String>,
        #[serde(default, deserialize_with = "nullable")]
        settings: Vec<RealmSetting>,
    }
    let list: Vec<Raw> = nullable(d)?;
    let mut slots = empty_slots();
    for raw in list {
        // `RealmsWorldOptions.parse`: the defaults when the string is missing or not JSON.
        let options = raw
            .options
            .and_then(|o| serde_json::from_str(&o).ok())
            .unwrap_or_default();
        slots.insert(
            raw.slot_id,
            RealmSlot {
                slot_id: raw.slot_id,
                options,
                settings: raw.settings,
            },
        );
    }
    Ok(slots)
}

/// `RealmsSlot.defaults` for world slots 1 to 3: empty, not hardcore.
fn empty_slots() -> std::collections::HashMap<i32, RealmSlot> {
    (1..=3)
        .map(|slot_id| {
            let slot = RealmSlot {
                slot_id,
                options: RealmWorldOptions {
                    empty: true,
                    ..Default::default()
                },
                settings: Vec::new(),
            };
            (slot_id, slot)
        })
        .collect()
}

impl RealmSlot {
    /// `RealmsSlot.isHardcore`: `RealmsSetting.isHardcore` of the settings.
    pub fn is_hardcore(&self) -> bool {
        self.settings
            .iter()
            .any(|s| s.name == "hardcore" && s.value == "true")
    }
}

/// `RegionSelectionPreference`'s ids, as `RegionSelectionPreferenceJsonAdapter` writes them.
pub const AUTOMATIC_PLAYER: i32 = 0;
pub const AUTOMATIC_OWNER: i32 = 1;
pub const MANUAL: i32 = 2;

/// `RegionSelectionPreferenceDto`: how the Realm picks its region. The preference travels
/// as its id ([`AUTOMATIC_PLAYER`], [`AUTOMATIC_OWNER`], [`MANUAL`]) and the region as its
/// `RealmsRegion.nameId` (`EastUs2`).
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegionSelectionPreferenceDto {
    /// `RegionSelectionPreferenceDto.DEFAULT`'s when the service sends none.
    #[serde(default = "automatic_owner", deserialize_with = "preference_id")]
    pub region_selection_preference: i32,
    #[serde(default)]
    pub preferred_region: Option<String>,
}

fn automatic_owner() -> i32 {
    AUTOMATIC_OWNER
}

/// `RegionSelectionPreferenceJsonAdapter.read`: an id it does not know (or null) is
/// `DEFAULT_SELECTION`, automatic by player.
fn preference_id<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    Ok(Option::<i32>::deserialize(d)?
        .filter(|id| (AUTOMATIC_PLAYER..=MANUAL).contains(id))
        .unwrap_or(AUTOMATIC_PLAYER))
}

/// `RegionDataDto`: one region's service quality, from `preferredRegions`.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegionDataDto {
    /// `RealmsRegion.nameId`.
    #[serde(rename = "regionName", default, deserialize_with = "nullable")]
    pub region: String,
    /// `ServiceQuality.value`: 1 great to 4 poor, 5 unknown.
    #[serde(default, deserialize_with = "nullable")]
    pub service_quality: i32,
}

/// `PreferredRegionsDto`.
#[derive(Clone, Debug, Deserialize, Default)]
struct PreferredRegionsResponse {
    #[serde(rename = "regionDataList", default, deserialize_with = "nullable")]
    region_data: Vec<RegionDataDto>,
}

/// `RealmsRegion`: each region's `nameId` and its translation key.
pub fn region_translation_key(name_id: &str) -> Option<&'static str> {
    const REGIONS: [(&str, &str); 27] = [
        (
            "AustraliaEast",
            "realms.configuration.region.australia_east",
        ),
        (
            "AustraliaSoutheast",
            "realms.configuration.region.australia_southeast",
        ),
        ("BrazilSouth", "realms.configuration.region.brazil_south"),
        (
            "CanadaCentral",
            "realms.configuration.region.canada_central",
        ),
        ("CentralIndia", "realms.configuration.region.central_india"),
        ("CentralUs", "realms.configuration.region.central_us"),
        ("EastAsia", "realms.configuration.region.east_asia"),
        ("EastUs", "realms.configuration.region.east_us"),
        ("EastUs2", "realms.configuration.region.east_us_2"),
        (
            "FranceCentral",
            "realms.configuration.region.france_central",
        ),
        ("JapanEast", "realms.configuration.region.japan_east"),
        ("JapanWest", "realms.configuration.region.japan_west"),
        ("KoreaCentral", "realms.configuration.region.korea_central"),
        (
            "MexicoCentral",
            "realms.configuration.region.mexico_central",
        ),
        (
            "NorthCentralUs",
            "realms.configuration.region.north_central_us",
        ),
        ("NorthEurope", "realms.configuration.region.north_europe"),
        (
            "SouthCentralUs",
            "realms.configuration.region.south_central_us",
        ),
        (
            "SoutheastAsia",
            "realms.configuration.region.southeast_asia",
        ),
        (
            "SouthAfricaNorth",
            "realms.configuration.region.south_africa_north",
        ),
        (
            "SwedenCentral",
            "realms.configuration.region.sweden_central",
        ),
        ("UAENorth", "realms.configuration.region.uae_north"),
        ("UKSouth", "realms.configuration.region.uk_south"),
        (
            "WestCentralUs",
            "realms.configuration.region.west_central_us",
        ),
        ("WestEurope", "realms.configuration.region.west_europe"),
        ("WestUs", "realms.configuration.region.west_us"),
        ("WestUs2", "realms.configuration.region.west_us_2"),
        ("WestUs3", "realms.configuration.region.west_us_3"),
    ];
    REGIONS
        .iter()
        .find(|(id, _)| *id == name_id)
        .map(|(_, key)| *key)
}

/// `RealmsSlotUpdateDto`'s JSON for `slot`.
fn slot_update(slot: &RealmSlot) -> serde_json::Value {
    serde_json::json!({
        "slotId": slot.slot_id,
        "spawnProtection": slot.options.spawn_protection,
        "forceGameMode": slot.options.force_game_mode,
        "difficulty": slot.options.difficulty,
        "gameMode": slot.options.game_mode,
        "slotName": slot.options.slot_name,
        "version": slot.options.version,
        "compatibility": slot.options.compatibility,
        "worldTemplateId": slot.options.template_id,
        "worldTemplateImage": slot.options.template_image,
        "hardcore": slot.is_hardcore(),
    })
}

/// `RealmsConfigurationDto`'s JSON.
fn configuration_body(
    name: &str,
    description: &str,
    preference: &RegionSelectionPreferenceDto,
    slot: &RealmSlot,
) -> serde_json::Value {
    serde_json::json!({
        "options": slot_update(slot),
        "settings": slot.settings,
        "regionSelectionPreference": {
            "regionSelectionPreference": preference.region_selection_preference,
            "preferredRegion": preference.preferred_region,
        },
        "description": { "name": name, "description": description },
    })
}

/// `RealmsWorldOptions`: a slot's options. Fields the JSON lacks keep the values of its
/// constructor (`Default`); `empty` is never in the JSON (`RealmsSlot.defaults` sets it).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RealmWorldOptions {
    #[serde(deserialize_with = "nullable")]
    pub spawn_protection: i32,
    #[serde(deserialize_with = "nullable")]
    pub force_game_mode: bool,
    #[serde(deserialize_with = "nullable")]
    pub difficulty: i32,
    #[serde(deserialize_with = "nullable")]
    pub game_mode: i32,
    #[serde(deserialize_with = "nullable")]
    pub slot_name: String,
    #[serde(deserialize_with = "nullable")]
    pub version: String,
    #[serde(deserialize_with = "nullable")]
    pub compatibility: String,
    #[serde(rename = "worldTemplateId", deserialize_with = "nullable")]
    pub template_id: i64,
    #[serde(rename = "worldTemplateImage")]
    pub template_image: Option<String>,
    #[serde(skip)]
    pub empty: bool,
}

impl Default for RealmWorldOptions {
    /// `new RealmsWorldOptions()`: normal difficulty, no template.
    fn default() -> Self {
        Self {
            spawn_protection: 0,
            force_game_mode: false,
            difficulty: 2,
            game_mode: 0,
            slot_name: String::new(),
            version: String::new(),
            compatibility: "UNVERIFIABLE".into(),
            template_id: -1,
            template_image: None,
            empty: false,
        }
    }
}

impl RealmServer {
    /// `RealmsServer.State.OPEN` and not expired: the Play button's condition.
    pub fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case("OPEN") && !self.expired
    }
    /// `Minecraft.isLocalPlayer(server.ownerUUID)`.
    pub fn owned_by(&self, uuid: u128) -> bool {
        self.owner_uuid
            .as_deref()
            .and_then(horizon_client_render::auth::parse_uuid)
            == Some(uuid)
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
struct RealmServerList {
    #[serde(default, deserialize_with = "nullable")]
    servers: Vec<RealmServer>,
}

/// `RealmsJoinInformation`: where to connect, and the world's resource pack.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealmAddress {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub resource_pack_url: Option<String>,
    #[serde(default)]
    pub resource_pack_hash: Option<String>,
}

/// `PendingInvite`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingInviteDto {
    #[serde(default, deserialize_with = "nullable")]
    pub invitation_id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub world_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub world_owner_name: String,
    #[serde(default)]
    pub world_owner_uuid: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub date: i64,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct PendingInvitesResponse {
    #[serde(default, deserialize_with = "nullable")]
    invites: Vec<PendingInviteDto>,
}

/// `Subscription`: the realm's own, as `RealmsClient.subscriptionFor` reads it.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// `Subscription.startDate`: epoch milliseconds.
    #[serde(default, deserialize_with = "nullable")]
    pub start_date: i64,
    #[serde(default, deserialize_with = "nullable")]
    pub days_left: i32,
    /// `Subscription.SubscriptionType`: `NORMAL` or `RECURRING`.
    #[serde(rename = "type", default, deserialize_with = "nullable")]
    pub kind: String,
}

/// Why a Realms call failed (`RealmsServiceException` and its `RealmsError`). None of them
/// holds a request header: the text a screen shows can't contain the token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RealmsError {
    /// `CustomError.connectivityError`: no answer (the transport error, which names the URL).
    Connectivity(String),
    /// `AuthenticationError`: a 401, with its `WWW-Authenticate` header.
    Authentication(String),
    /// `RetryCallException`: 503 or 277, with the `Retry-After` seconds (5 when missing).
    Retry(u64),
    /// `RealmsError.parse`: any other answer, with the body's `errorCode`, `reason` and
    /// `errorMsg` (or the raw body).
    Service {
        status: u16,
        code: Option<i32>,
        reason: Option<String>,
        message: Option<String>,
    },
    /// An answer whose JSON doesn't read.
    Parse,
    /// `GetServerDetailsTask`'s `TimeoutException`: still busy after 40 tries.
    Timeout,
}

impl RealmsError {
    /// `RealmsError.errorCode`.
    pub fn code(&self) -> i32 {
        match self {
            Self::Connectivity(_) => 500,
            Self::Authentication(_) => 401,
            Self::Retry(_) => 503,
            Self::Service { status, code, .. } => code.unwrap_or(i32::from(*status)),
            Self::Parse | Self::Timeout => 500,
        }
    }
    /// `RealmsError.errorMessage`, as `RealmsGenericErrorScreen` shows it after the
    /// `mco.errorMessage.realmsService.realmsError` line.
    pub fn text(&self) -> String {
        let no_details = || crate::lang::get("mco.errorMessage.noDetails");
        let detail = match self {
            Self::Connectivity(e) => crate::lang::tr(
                "mco.errorMessage.realmsService.connectivity",
                &[e as &dyn std::fmt::Display],
            ),
            Self::Authentication(header) if !header.is_empty() => header.clone(),
            Self::Authentication(_) => no_details(),
            Self::Retry(_) => crate::lang::get("mco.errorMessage.retry"),
            Self::Service { status: 429, .. } => crate::lang::get("mco.errorMessage.serviceBusy"),
            Self::Service {
                code,
                reason,
                message,
                ..
            } => {
                let code_key = code.map(|c| format!("mco.errorMessage.{c}"));
                let reason_key = reason.as_ref().map(|r| format!("mco.errorReason.{r}"));
                match (code_key, reason_key) {
                    (Some(k), _) if crate::lang::has(&k) => crate::lang::get(&k),
                    (_, Some(k)) if crate::lang::has(&k) => crate::lang::get(&k),
                    _ => message.clone().unwrap_or_else(no_details),
                }
            }
            Self::Parse | Self::Timeout => crate::lang::get("mco.errorMessage.connectionFailure"),
        };
        let code = self.code();
        format!(
            "{} {detail}",
            crate::lang::tr(
                "mco.errorMessage.realmsService.realmsError",
                &[&code as &dyn std::fmt::Display]
            )
        )
    }
    /// `RealmsError.parse` of a non-2xx answer.
    fn from_answer(status: u16, body: &str, html: bool) -> Self {
        let body = body.trim();
        let service = |code, reason, message| Self::Service {
            status,
            code,
            reason,
            message,
        };
        if status == 429 || body.is_empty() {
            return service(None, None, None);
        }
        if html {
            // CustomError.htmlPayload: the text between <body> and </body>.
            let inner = body
                .find("<body>")
                .zip(body.find("</body>"))
                .filter(|(a, b)| b > a)
                .map(|(a, b)| body[a + 6..b].trim().to_owned());
            return service(None, None, inner);
        }
        if let Ok(serde_json::Value::Object(o)) = serde_json::from_str::<serde_json::Value>(body) {
            let text = |k: &str| o.get(k).and_then(|v| v.as_str()).map(str::to_owned);
            let code = o
                .get("errorCode")
                .and_then(serde_json::Value::as_i64)
                .and_then(|c| i32::try_from(c).ok());
            let (reason, message) = (text("reason"), text("errorMsg"));
            if code.is_some() || reason.is_some() || message.is_some() {
                return service(code, reason, message);
            }
        }
        // ErrorWithRawPayload.
        service(None, None, Some(body.chars().take(256).collect()))
    }
}

/// The client for one signed-in account.
#[derive(Clone)]
pub struct RealmsClient {
    base_url: String,
    frontend_url: String,
    player_name: String,
    player_uuid: u128,
    access_token: String,
}

impl std::fmt::Debug for RealmsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealmsClient")
            .field("base_url", &self.base_url)
            .field("player_name", &self.player_name)
            .field("access_token", &"<hidden>")
            .finish()
    }
}

impl RealmsClient {
    pub fn new(
        base_url: &str,
        frontend_url: &str,
        session: &horizon_client_render::auth::AuthSession,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            frontend_url: frontend_url.trim_end_matches('/').to_owned(),
            player_name: session.name.clone(),
            player_uuid: session.uuid,
            access_token: session.access_token().to_owned(),
        }
    }
    /// The production service.
    pub fn production(session: &horizon_client_render::auth::AuthSession) -> Self {
        Self::new(DEFAULT_REALMS_HOST, DEFAULT_FRONTEND_V2_HOST, session)
    }

    /// `RealmsClient.execute`'s cookies (`User.getSessionId`, `getName`,
    /// `WorldVersion.name()`) and `Request.addVersionHeaders`, in that order.
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "Cookie",
                format!(
                    "sid=token:{}:{:032x};user={};version={}",
                    self.access_token,
                    self.player_uuid,
                    self.player_name,
                    version::VANILLA_VERSION
                ),
            ),
            ("Client-Ref", version::VERSION_ID.to_owned()),
            (
                "X-NetworkProtocolVersion",
                version::PROTOCOL_VERSION.to_string(),
            ),
            ("Is-Prerelease", (!version::STABLE).to_string()),
        ]
    }

    fn request(&self, method: &str, url: &str, read_timeout: Duration) -> ureq::Request {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(read_timeout)
            .user_agent(&user_agent())
            // A redirect must not carry the session cookie to another host.
            .redirects(0)
            .build();
        self.headers()
            .into_iter()
            .fold(agent.request(method, url), |r, (k, v)| r.set(k, &v))
    }

    /// `RealmsClient.execute`: the body of a 2xx answer, else the error.
    fn execute(request: ureq::Request, body: Option<&str>) -> Result<String, RealmsError> {
        let result = match body {
            Some(body) => request.send_string(body),
            None => request.call(),
        };
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            // The transport error names the URL and the cause, never a header.
            Err(ureq::Error::Transport(e)) => return Err(RealmsError::Connectivity(e.to_string())),
        };
        let status = response.status();
        if status == 503 || status == 277 {
            let pause = response
                .header("Retry-After")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(5);
            return Err(RealmsError::Retry(pause));
        }
        let auth =
            (status == 401).then(|| response.header("WWW-Authenticate").unwrap_or("").to_owned());
        let html = response.content_type().starts_with("text/html");
        let text = response
            .into_string()
            .map_err(|e| RealmsError::Connectivity(e.to_string()))?;
        if (200..300).contains(&status) {
            return Ok(text);
        }
        if let Some(header) = auth {
            return Err(RealmsError::Authentication(header));
        }
        Err(RealmsError::from_answer(status, &text, html))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url)
    }

    /// `RealmsClient.listRealms` (a release client: no `listUserWorldsOfType`).
    pub fn list_worlds(&self) -> Result<Vec<RealmServer>, RealmsError> {
        let json = Self::execute(self.request("GET", &self.url("worlds"), READ_TIMEOUT), None)?;
        serde_json::from_str::<RealmServerList>(&json)
            .map(|l| l.servers)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.join`: `worlds/v1/<id>/join/pc`.
    pub fn join_world(&self, realm_id: i64) -> Result<RealmAddress, RealmsError> {
        let url = self.url(&format!("worlds/v1/{realm_id}/join/pc"));
        let json = Self::execute(self.request("GET", &url, JOIN_READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.pendingInvites`.
    pub fn pending_invites(&self) -> Result<Vec<PendingInviteDto>, RealmsError> {
        let json = Self::execute(
            self.request("GET", &self.url("invites/pending"), READ_TIMEOUT),
            None,
        )?;
        serde_json::from_str::<PendingInvitesResponse>(&json)
            .map(|r| r.invites)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.acceptInvitation`.
    pub fn accept_invite(&self, invitation_id: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/accept/{}", path_segment(invitation_id)?));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.rejectInvitation`.
    pub fn reject_invite(&self, invitation_id: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/reject/{}", path_segment(invitation_id)?));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.uninviteMyselfFrom`: leaves someone else's Realm.
    pub fn uninvite_myself(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/{realm_id}"));
        Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None).map(drop)
    }

    /// `RealmsClient.open`.
    pub fn open_realm(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/open"));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.close`.
    pub fn close_realm(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/close"));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.subscriptionFor`: the configuration screen's subscription tab.
    pub fn subscription_for(&self, realm_id: i64) -> Result<Subscription, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/subscription"));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.deleteRealm` (`Request.delete`).
    pub fn delete_realm(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}"));
        Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None).map(drop)
    }

    /// `RealmsClient.getOwnRealm`: the detail answer with the slots and the players.
    pub fn get_own_realm(&self, realm_id: i64) -> Result<RealmServer, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}"));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.op`: `POST worlds/{id}/{uuid}` with an empty body, then the ops.
    pub fn op_player(&self, realm_id: i64, uuid: &str) -> Result<Ops, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/{}", undashed(uuid)?));
        let json = Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(""))?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.deop`: `DELETE worlds/{id}/{uuid}`, then the ops.
    pub fn deop_player(&self, realm_id: i64, uuid: &str) -> Result<Ops, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/{}", undashed(uuid)?));
        let json = Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.invite`: `POST worlds/{id}` with the player's name, then the players.
    pub fn invite_player(&self, realm_id: i64, name: &str) -> Result<Vec<PlayerInfo>, RealmsError> {
        let body = serde_json::json!({"name": name}).to_string();
        let url = self.url(&format!("worlds/{realm_id}"));
        let json = Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(&body))?;
        serde_json::from_str::<Vec<PlayerInfo>>(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.uninvite`: `DELETE worlds/{id}/invite/{uuid}`.
    pub fn uninvite_player(&self, realm_id: i64, uuid: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/invite/{}", undashed(uuid)?));
        Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None).map(drop)
    }

    /// `RealmsClient.updateSlot`: `POST worlds/{id}/slot/{slotId}` with
    /// `RealmsSlotUpdateDto`'s JSON.
    pub fn update_slot(&self, realm_id: i64, slot: &RealmSlot) -> Result<(), RealmsError> {
        let body = slot_update(slot).to_string();
        let url = self.url(&format!("worlds/{realm_id}/slot/{}", slot.slot_id));
        Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(&body)).map(drop)
    }

    /// `RealmsClient.switchSlot`: `PUT worlds/{id}/slot/{slotId}`, the service's yes or no.
    pub fn switch_slot(&self, realm_id: i64, slot_id: i32) -> Result<bool, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/slot/{slot_id}"));
        let json = Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some(""))?;
        json.trim().parse::<bool>().map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.getPreferredRegionSelections`: every region and its quality.
    pub fn preferred_regions(&self) -> Result<Vec<RegionDataDto>, RealmsError> {
        let json = Self::execute(
            self.request("GET", &self.url("regions/preferredRegions"), READ_TIMEOUT),
            None,
        )?;
        serde_json::from_str::<PreferredRegionsResponse>(&json)
            .map(|r| r.region_data)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.updateConfiguration`: `POST worlds/{id}/configuration` with
    /// `RealmsConfigurationDto`'s JSON: the active slot's `RealmsSlotUpdateDto` as `options`,
    /// its settings, the region preference (its id and the region's `nameId`) and the
    /// name and description.
    pub fn update_configuration(
        &self,
        realm_id: i64,
        name: &str,
        description: &str,
        preference: &RegionSelectionPreferenceDto,
        slot: &RealmSlot,
    ) -> Result<(), RealmsError> {
        let body = configuration_body(name, description, preference, slot).to_string();
        let url = self.url(&format!("worlds/{realm_id}/configuration"));
        Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(&body)).map(drop)
    }

    /// `RealmsClient.redeemInviteCode`: the invite-code service, with the token in
    /// `x-mojangauth` as `frontendV2Request` sets it.
    pub fn redeem_invite_code(&self, code: &str) -> Result<(), RealmsError> {
        let code = path_segment(code.trim())?;
        let url = format!("{}/api/v1.0/invite/codes/use/{code}", self.frontend_url);
        let request = self
            .request("POST", &url, READ_TIMEOUT)
            .set("x-mojangauth", &self.access_token)
            .set("Accept", "application/json");
        Self::execute(request, Some("")).map(drop)
    }
}

/// An id or code that goes into a URL path: vanilla builds those URLs from the server's own
/// ids; anything else (a typed code with `/` or `?`) would reach another path.
fn path_segment(s: &str) -> Result<&str, RealmsError> {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(s)
    } else {
        Err(RealmsError::Service {
            status: 400,
            code: None,
            reason: None,
            message: Some(crate::lang::get("mco.selectServer.joinCode.invalid")),
        })
    }
}

/// `UndashedUuid.toString`: a player's UUID as 32 hex digits, the op/deop/uninvite path
/// segment. Anything else (a name with a `/`, a shortened id) would reach another path.
fn undashed(uuid: &str) -> Result<String, RealmsError> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    if hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(hex)
    } else {
        Err(RealmsError::Parse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client_render::auth::AuthSession;
    use std::io::{Read, Write};

    const TOKEN: &str = "eyJsecret.token.value";
    const UUID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

    /// One HTTP exchange on a local port: the request as the server read it.
    fn serve_once(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            // The body the headers announce, which may come in a later read.
            let head = seen
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map_or(seen.len(), |i| i + 4);
            let length = String::from_utf8_lossy(&seen[..head])
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().ok())?
                })
                .unwrap_or(0usize);
            while seen.len() < head + length {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            stream.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        (base, handle)
    }

    fn client(base: &str) -> RealmsClient {
        RealmsClient::new(base, base, &AuthSession::new("Steve", UUID, TOKEN))
    }

    fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    #[test]
    fn requests_send_the_session_cookie_only_and_errors_never_hold_the_token() {
        let (base, server) = serve_once(
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"Realms\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let error = client(&base).list_worlds().unwrap_err();
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /worlds HTTP/1.1"), "{request}");
        // RealmsClient.execute: sid = User.getSessionId (token:<token>:<undashed uuid>), user, version.
        assert_eq!(
            header(&request, "Cookie"),
            Some(
                "sid=token:eyJsecret.token.value:0123456789abcdef0123456789abcdef;user=Steve;version=26.3"
            )
        );
        assert_eq!(header(&request, "Authorization"), None, "{request}");
        assert_eq!(header(&request, "Client-Ref"), Some("26.3"));
        assert_eq!(header(&request, "X-NetworkProtocolVersion"), Some("777"));
        assert_eq!(header(&request, "Is-Prerelease"), Some("false"));
        assert_eq!(header(&request, "User-Agent"), Some("Java/25"));
        // The token appears once in the whole request: in the cookie.
        assert_eq!(request.matches(TOKEN).count(), 1);
        assert_eq!(
            error,
            RealmsError::Authentication("Bearer realm=\"Realms\"".into())
        );
        assert!(!error.text().contains(TOKEN));
        assert!(!format!("{error:?}").contains(TOKEN));
        assert!(!format!("{:?}", client(&base)).contains(TOKEN));
    }

    #[test]
    fn a_refused_connection_names_no_header() {
        // A port nothing listens on: the transport error.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let error = client(&format!("http://127.0.0.1:{port}"))
            .join_world(7)
            .unwrap_err();
        assert!(matches!(error, RealmsError::Connectivity(_)), "{error:?}");
        assert!(!error.text().contains(TOKEN));
    }

    #[test]
    fn busy_answers_ask_for_a_retry_after_their_delay() {
        let (base, server) = serve_once(
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 3\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(client(&base).join_world(7), Err(RealmsError::Retry(3)));
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("GET /worlds/v1/7/join/pc ")
        );
    }

    #[test]
    fn the_join_answer_reads_vanillas_field_names() {
        let (base, server) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"address\":\"1.2.3.4:25565\",\"resourcePackUrl\":\"https://x.test/p.zip\",\"resourcePackHash\":\"abc\",\"sessionRegionData\":null}",
        );
        let address = client(&base).join_world(42).unwrap();
        server.join().unwrap();
        assert_eq!(
            address,
            RealmAddress {
                address: Some("1.2.3.4:25565".into()),
                resource_pack_url: Some("https://x.test/p.zip".into()),
                resource_pack_hash: Some("abc".into()),
            }
        );
    }

    #[test]
    fn an_invite_code_with_a_path_in_it_is_refused_before_any_request() {
        // Nothing listens: a request would fail as Connectivity, not as the invalid code.
        let error = client("http://127.0.0.1:9")
            .redeem_invite_code("abc/../worlds")
            .unwrap_err();
        assert!(
            matches!(error, RealmsError::Service { status: 400, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn server_list_reads_camel_case_and_nulls() {
        let raw = r#"{"servers":[{"id":12345,"remoteSubscriptionId":"sub","name":"Test Realm",
            "motd":null,"state":"OPEN","owner":"Alex","ownerUUID":"0123456789abcdef0123456789abcdef",
            "daysLeft":25,"expired":false,"expiredTrial":false,"worldType":"NORMAL","activeSlot":2,
            "minigameName":null}]}"#;
        let list: RealmServerList = serde_json::from_str(raw).unwrap();
        let server = &list.servers[0];
        assert_eq!(
            (server.id, server.days_left, server.active_slot),
            (12345, 25, 2)
        );
        assert_eq!(server.motd, "");
        assert!(server.is_open());
        assert!(server.owned_by(UUID));
        assert!(!server.owned_by(UUID + 1));
    }

    /// `RealmsServer.slots` as the detail answer carries them (`slotList`): a list whose
    /// options are a JSON string each, read as `RealmsWorldOptions.parse` reads them, and the
    /// world slots it lacks empty (`finalizeSlots`).
    #[test]
    fn the_detail_answer_s_slot_list_reads_its_options_strings() {
        let raw = r#"{"id":7,"name":"Base","activeSlot":1,"slots":[
            {"slotId":1,"options":"{\"slotName\":\"Spawn\",\"difficulty\":3,\"gameMode\":1,\"spawnProtection\":4,\"worldTemplateId\":42,\"worldTemplateImage\":null}",
             "settings":[{"name":"hardcore","value":"true"}]},
            {"slotId":2,"options":"{}","settings":[]}]}"#;
        let server: RealmServer = serde_json::from_str(raw).unwrap();
        let one = &server.slots[&1];
        assert_eq!(
            (
                one.options.slot_name.as_str(),
                one.options.difficulty,
                one.options.game_mode,
                one.options.spawn_protection,
                one.options.template_id,
            ),
            ("Spawn", 3, 1, 4, 42)
        );
        assert!(one.is_hardcore() && !one.options.empty);
        // `new RealmsWorldOptions()` for what the string leaves out.
        let two = &server.slots[&2].options;
        assert_eq!((two.difficulty, two.template_id, two.empty), (2, -1, false));
        assert!(server.slots[&3].options.empty);
        assert_eq!(server.slots.len(), 3);
        // The list answer has no slots: the empty world slots still.
        let listed: RealmServer = serde_json::from_str(r#"{"id":7,"slots":null}"#).unwrap();
        assert!(listed.slots.values().all(|s| s.options.empty));
    }

    #[test]
    fn error_bodies_read_as_vanilla_parses_them() {
        assert_eq!(
            RealmsError::from_answer(403, r#"{"errorCode":6002,"errorMsg":"tos"}"#, false),
            RealmsError::Service {
                status: 403,
                code: Some(6002),
                reason: None,
                message: Some("tos".into())
            }
        );
        assert_eq!(
            RealmsError::from_answer(500, r#"{"errorCode":6006}"#, false).code(),
            6006
        );
        assert_eq!(
            RealmsError::from_answer(502, "<html><body> Bad gateway </body></html>", true),
            RealmsError::Service {
                status: 502,
                code: None,
                reason: None,
                message: Some("Bad gateway".into())
            }
        );
        assert_eq!(RealmsError::from_answer(404, "", false).code(), 404);
    }

    /// `RealmsClient.updateConfiguration`'s request as gson writes `RealmsConfigurationDto`:
    /// the slot under `options`, the preference as its id and the region as its `nameId`.
    #[test]
    fn a_configuration_update_posts_the_vanilla_dto() {
        let (base, server) =
            serve_once("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let detail: RealmServer = serde_json::from_str(
            r#"{"id":7,"activeSlot":1,"slots":[{"slotId":1,
                "options":"{\"slotName\":\"Spawn\",\"worldTemplateId\":42}",
                "settings":[{"name":"hardcore","value":"false"}]}]}"#,
        )
        .unwrap();
        let preference = RegionSelectionPreferenceDto {
            region_selection_preference: MANUAL,
            preferred_region: Some("UKSouth".into()),
        };
        client(&base)
            .update_configuration(7, "Base", "Hi", &preference, &detail.slots[&1])
            .unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /worlds/7/configuration HTTP/1.1"),
            "{request}"
        );
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["options"]["slotId"], 1);
        assert_eq!(body["options"]["slotName"], "Spawn");
        assert_eq!(body["options"]["worldTemplateId"], 42);
        assert_eq!(
            body["settings"],
            serde_json::json!([{"name": "hardcore", "value": "false"}])
        );
        assert_eq!(
            body["regionSelectionPreference"],
            serde_json::json!({"regionSelectionPreference": 2, "preferredRegion": "UKSouth"})
        );
        assert_eq!(
            body["description"],
            serde_json::json!({"name": "Base", "description": "Hi"})
        );
    }

    /// `PreferredRegionsDto`: `regionDataList`, each region by `regionName` with its
    /// `ServiceQuality` value; and a Realm's preference as its id.
    #[test]
    fn preferred_regions_and_the_realm_s_preference_read_as_vanilla_writes_them() {
        let regions: PreferredRegionsResponse = serde_json::from_str(
            r#"{"regionDataList":[{"regionName":"EastUs2","serviceQuality":1},
                                  {"regionName":"WestEurope","serviceQuality":4}]}"#,
        )
        .unwrap();
        assert_eq!(
            regions.region_data,
            vec![
                RegionDataDto {
                    region: "EastUs2".into(),
                    service_quality: 1
                },
                RegionDataDto {
                    region: "WestEurope".into(),
                    service_quality: 4
                },
            ]
        );
        assert_eq!(
            region_translation_key("EastUs2"),
            Some("realms.configuration.region.east_us_2")
        );
        let server: RealmServer = serde_json::from_str(
            r#"{"id":7,"regionSelectionPreference":{"regionSelectionPreference":1,"preferredRegion":null}}"#,
        )
        .unwrap();
        assert_eq!(
            server.region_selection_preference,
            Some(RegionSelectionPreferenceDto {
                region_selection_preference: AUTOMATIC_OWNER,
                preferred_region: None,
            })
        );
    }
}
