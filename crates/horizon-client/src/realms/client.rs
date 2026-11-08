//! The Realms service client: vanilla's `com.mojang.realmsclient.client.RealmsClient` and its
//! `Request`, for the signed-in player's own Microsoft account only. Every request carries
//! what `RealmsClient.execute` adds and nothing more: the `sid`, `user` and `version` cookies
//! and the three version headers. The access token goes only into the `sid` cookie (and the
//! `x-mojangauth` header vanilla adds for the invite-code service); it is never logged, never
//! put in an error and never sent anywhere else.
use horizon_core::version;
use serde::{Deserialize, Deserializer, Serialize};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// The active world's summary the list row shows (`RealmsServer.gameMode`, -1 when
    /// the service does not say; the service sends `null` for some Realms).
    #[serde(rename = "gameMode", default, deserialize_with = "nullable")]
    pub game_mode: i32,
    #[serde(rename = "isHardcore", default, deserialize_with = "nullable")]
    pub is_hardcore: bool,
    /// The active world's version, right-aligned on the row's first line
    /// (`RealmsServer.activeVersion`).
    #[serde(rename = "activeVersion", default, deserialize_with = "nullable")]
    pub active_version: String,
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
    /// `RealmsServer.compatibility`: whether this client's version can play the Realm.
    /// Gson keeps `UNVERIFIABLE` when the service omits it, so the default does too.
    #[serde(default = "unverifiable_compatibility", deserialize_with = "nullable")]
    pub compatibility: String,
}

/// `RealmsServer.Compatibility.UNVERIFIABLE`, Gson's default for an absent field.
fn unverifiable_compatibility() -> String {
    "UNVERIFIABLE".to_owned()
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
    #[serde(default, deserialize_with = "setting_value")]
    pub value: String,
}

/// `JsonPrimitive.getAsString`: the service sends a setting's value as a JSON bool or
/// number where Gson reads a string, so either reads as its text.
fn setting_value<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value,
        _ => String::new(),
    })
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

/// `RealmTierConfigurationDto`: the Realm tier's two distance ranges.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealmTierConfiguration {
    #[serde(default, deserialize_with = "nullable")]
    pub render_distance: Option<RealmTierRange>,
    #[serde(default, deserialize_with = "nullable")]
    pub sim_distance: Option<RealmTierRange>,
}

/// `RealmTierConfigurationDto.RealmTierRangeDto`: the values a slot's distance may take.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealmTierRange {
    #[serde(default, deserialize_with = "nullable")]
    pub min: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub max: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub default_value: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub current: Option<i32>,
}

impl RealmTierRange {
    /// `RealmsSettingsTab.currentOrDefault`.
    pub fn current_or_default(&self) -> i32 {
        self.current.unwrap_or(self.default_value)
    }

    /// `OptionInstance.IntRange`: a slider position to a value in the range.
    pub fn from_slider(&self, value: f32) -> i32 {
        self.min + (value * (self.max - self.min) as f32).round() as i32
    }
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

/// `InviteCode`: one of the Realm's shareable invite codes.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InviteCode {
    #[serde(default, deserialize_with = "nullable")]
    pub code: String,
    /// Epoch milliseconds; none never expires.
    #[serde(default, deserialize_with = "nullable")]
    pub expiration_date: Option<i64>,
    #[serde(default, deserialize_with = "nullable")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct InviteCodeListResponse {
    #[serde(default, deserialize_with = "nullable")]
    result: Vec<InviteCode>,
}

/// `Backup`: one of the Realm's automatic backups. `changeList` is not on the wire — the
/// backup screen fills it from the metadata's differences with the older backup.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    #[serde(default, deserialize_with = "nullable")]
    pub backup_id: String,
    /// `Backup.lastModifiedDate`: epoch milliseconds (`JsonUtils.getDateOr`).
    #[serde(
        rename = "lastModifiedDate",
        default,
        deserialize_with = "epoch_millis"
    )]
    pub last_modified: i64,
    #[serde(default, deserialize_with = "nullable")]
    pub size: i64,
    /// `Backup.metadata`: every value as the service's JSON text (`getAsString`); null
    /// entries are left out.
    #[serde(default, deserialize_with = "metadata_map")]
    pub metadata: std::collections::HashMap<String, String>,
    /// `Backup.uploadedVersion`: set when the metadata's `uploaded` key is in the list.
    #[serde(skip)]
    pub uploaded_version: bool,
    /// Filled by the list screen from `metadata`; never deserialized.
    #[serde(skip)]
    pub change_list: std::collections::HashMap<String, String>,
}

/// `JsonUtils.getDateOr`: the epoch milliseconds as a number or a numeric string.
fn epoch_millis<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or(0))
}

/// `Backup.parse`'s metadata loop: non-null values as their JSON text.
fn metadata_map<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<std::collections::HashMap<String, String>, D::Error> {
    let Some(object) = Option::<serde_json::Map<String, serde_json::Value>>::deserialize(d)? else {
        return Ok(std::collections::HashMap::new());
    };
    Ok(object
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| {
            let text = match value {
                serde_json::Value::String(text) => text,
                other => other.to_string(),
            };
            (key, text)
        })
        .collect())
}

#[derive(Clone, Debug, Deserialize, Default)]
struct BackupListResponse {
    #[serde(default, deserialize_with = "nullable")]
    backups: Vec<Backup>,
}

/// `WorldDownload`: where the latest world is fetched and the pack that came with it
/// (`RealmsClient.requestDownloadInfo`). Every field is the empty string when the service
/// leaves it out (`JsonUtils.getStringOr`).
#[derive(Clone, Debug, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct WorldDownload {
    #[serde(deserialize_with = "nullable")]
    pub download_link: String,
    #[serde(deserialize_with = "nullable")]
    pub resource_pack_url: String,
    #[serde(deserialize_with = "nullable")]
    pub resource_pack_hash: String,
}

impl InviteCode {
    /// `InviteCodeList.parse`: the answer is an array, or an object whose `result` is one;
    /// an entry without its code is dropped, as `InviteCode.parse`'s null is.
    pub fn parse_list(json: &str) -> Vec<Self> {
        let list = if let Ok(list) = serde_json::from_str::<Vec<Self>>(json) {
            list
        } else {
            serde_json::from_str::<InviteCodeListResponse>(json)
                .unwrap_or_default()
                .result
        };
        list.into_iter()
            .filter(|code| !code.code.is_empty())
            .collect()
    }
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

/// `RealmsNotification`: a message the service has for the player. The `visitUrl` kind is
/// the one `RealmsMainScreen` shows at the top of its list; its text is a [`RealmsText`].
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RealmsNotification {
    #[serde(rename = "notificationUuid", default, deserialize_with = "nullable")]
    pub uuid: String,
    #[serde(default = "default_true")]
    pub dismissable: bool,
    #[serde(default)]
    pub seen: bool,
    #[serde(rename = "type", default, deserialize_with = "nullable")]
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "buttonText", default)]
    pub button_text: Option<RealmsText>,
    #[serde(default)]
    pub message: Option<RealmsText>,
    #[serde(default)]
    pub title: Option<RealmsText>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(rename = "urlButton", default)]
    pub url_button: Option<RealmsUrlButton>,
}

/// `RealmsNotification.UrlButton`: the info popup's link and its label.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RealmsUrlButton {
    #[serde(default)]
    pub url: String,
    #[serde(rename = "urlText", default)]
    pub url_text: Option<RealmsText>,
}

fn default_true() -> bool {
    true
}

/// `RealmsText`: a translation key and its arguments (`Component.translatable`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RealmsText {
    #[serde(rename = "translationKey", default, deserialize_with = "nullable")]
    pub translation_key: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
}

impl RealmsNotification {
    /// `RealmsNotification.parseList`: the service's `{"notifications":[...]}`. An entry
    /// without `notificationUuid` is dropped, as `parse` throws for it.
    pub fn parse_list(json: &str) -> Vec<Self> {
        #[derive(Deserialize, Default)]
        struct List {
            #[serde(default)]
            notifications: Vec<RealmsNotification>,
        }
        serde_json::from_str::<List>(json)
            .unwrap_or_default()
            .notifications
            .into_iter()
            .filter(|notification| !notification.uuid.is_empty())
            .collect()
    }

    /// `VisitUrl.getMessage`: the message's translation, or its default line when the
    /// language has no such key (`RealmsText.createComponent`'s null).
    pub fn visit_message(&self) -> String {
        text_or(
            self.message.as_ref(),
            "mco.notification.visitUrl.message.default",
        )
    }

    /// `VisitUrl.buildOpenLinkButton`'s label, with its default too.
    pub fn visit_button_text(&self) -> String {
        text_or(
            self.button_text.as_ref(),
            "mco.notification.visitUrl.buttonText.default",
        )
    }

    /// `InfoPopup.buildScreen`'s title: `None` when the language has no translation, which
    /// makes vanilla skip the popup (`title.createComponent()`).
    pub fn info_title(&self) -> Option<String> {
        let title = self.title.as_ref()?;
        crate::lang::has(&title.translation_key).then(|| translate_text(title))
    }

    /// `InfoPopup`'s message, the empty component when it has no translation.
    pub fn info_message(&self) -> String {
        match &self.message {
            Some(message) if crate::lang::has(&message.translation_key) => translate_text(message),
            _ => String::new(),
        }
    }

    /// `InfoPopup`'s URL button: its URL and `urlText` (the default label when missing).
    pub fn info_url_button(&self) -> Option<(String, String)> {
        let button = self.url_button.as_ref()?;
        Some((
            button.url.clone(),
            text_or(
                button.url_text.as_ref(),
                "mco.notification.visitUrl.buttonText.default",
            ),
        ))
    }
}

/// `RealmsText.createComponent(fallback)`: the translation, or `fallback` when the
/// language has no `translationKey`.
fn text_or(text: Option<&RealmsText>, fallback: &str) -> String {
    let Some(text) = text else {
        return crate::lang::get(fallback);
    };
    if !crate::lang::has(&text.translation_key) {
        return crate::lang::get(fallback);
    }
    translate_text(text)
}

/// `RealmsText.createComponent`: its translation with the arguments it carries.
fn translate_text(text: &RealmsText) -> String {
    let args: Vec<&dyn std::fmt::Display> = text
        .args
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|arg| arg as &dyn std::fmt::Display)
        .collect();
    crate::lang::tr(&text.translation_key, &args)
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
    /// `Subscription.SubscriptionType`: `NORMAL` or `RECURRING` — the service's field
    /// is `subscriptionType` (`JsonUtils.getStringOr("subscriptionType", ..)`).
    #[serde(rename = "subscriptionType", default, deserialize_with = "nullable")]
    pub kind: String,
}

/// `WorldTemplate`: one entry of `RealmsSelectWorldTemplateScreen`'s list.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldTemplate {
    #[serde(default, deserialize_with = "nullable")]
    pub id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub version: String,
    #[serde(default, deserialize_with = "nullable")]
    pub author: String,
    /// The publisher's page and the trailer, opened in the browser.
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub trailer: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub recommended_players: String,
    /// `WorldTemplate.WorldTemplateType`: `WORLD_TEMPLATE` when the answer has none.
    #[serde(
        rename = "type",
        default = "world_template",
        deserialize_with = "template_type"
    )]
    pub kind: String,
}

/// `WorldTemplatePaginatedList`: a page of a world type's templates.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct WorldTemplateList {
    #[serde(default, deserialize_with = "nullable")]
    pub templates: Vec<WorldTemplate>,
    #[serde(default, deserialize_with = "nullable")]
    pub page: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub size: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub total: i32,
}

/// `WorldTemplate.parse` keeps vanilla's default type when the answer has none.
fn world_template() -> String {
    "WORLD_TEMPLATE".to_owned()
}

/// `WorldTemplate.parse` keeps vanilla's default type for a missing or unknown one.
fn template_type<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    const TYPES: [&str; 5] = [
        "WORLD_TEMPLATE",
        "MINIGAME",
        "ADVENTUREMAP",
        "EXPERIENCE",
        "INSPIRATION",
    ];
    let kind = Option::<String>::deserialize(d)?.unwrap_or_default();
    Ok(if TYPES.contains(&kind.as_str()) {
        kind
    } else {
        "WORLD_TEMPLATE".to_owned()
    })
}

/// `UploadInfo`: where `FileUpload` posts a packed world, and the token for its cookie.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UploadInfo {
    /// `UploadInfo.worldClosed`: the Realm said its world is closed; an upload needs that.
    pub world_closed: bool,
    pub token: Option<String>,
    /// `UploadInfo.uploadEndpoint`, as the answer names it (a scheme when it has one).
    pub endpoint: String,
    /// `UploadInfo.port`: the answer's `port`, `-1` when it names none.
    pub port: i32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UploadInfoAnswer {
    #[serde(default)]
    upload_endpoint: Option<String>,
    #[serde(default = "minus_one", deserialize_with = "port_or_minus_one")]
    port: i32,
    #[serde(default, deserialize_with = "nullable")]
    world_closed: bool,
    #[serde(default)]
    token: Option<String>,
}

/// `JsonUtils.getIntOr("port", object, -1)`: a missing or null port is -1.
fn minus_one() -> i32 {
    -1
}

fn port_or_minus_one<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    Ok(Option::<i32>::deserialize(d)?.unwrap_or(-1))
}

impl UploadInfo {
    /// `UploadInfo.parse`: an answer without an endpoint is no upload target at all.
    fn parse(json: &str) -> Option<Self> {
        let answer = serde_json::from_str::<UploadInfoAnswer>(json).ok()?;
        let endpoint = answer.upload_endpoint?;
        Some(Self {
            world_closed: answer.world_closed,
            token: answer.token,
            endpoint,
            port: answer.port,
        })
    }

    /// `UploadInfo.assembleUri` and `FileUpload`'s `resolve("/upload/{realm}/{slot}")`: a
    /// scheme when the answer names none, `http`'s 8080 when neither it nor a port says
    /// otherwise.
    fn upload_url(&self, realm_id: i64, slot_id: i32) -> Option<String> {
        let endpoint = if self.endpoint.contains("://") {
            self.endpoint.clone()
        } else {
            format!("http://{}", self.endpoint)
        };
        let (scheme, rest) = endpoint.split_once("://")?;
        let authority = rest.split('/').next().filter(|a| !a.is_empty())?;
        let (host, parsed_port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                (host, port.parse::<u16>().ok())
            }
            _ => (authority, None),
        };
        if host.is_empty() {
            return None;
        }
        let port = if self.port >= 0 {
            u16::try_from(self.port).ok()?
        } else {
            parsed_port.unwrap_or(8080)
        };
        Some(format!(
            "{scheme}://{host}:{port}/upload/{realm_id}/{slot_id}"
        ))
    }
}

/// Why `FileUpload`'s body request stopped: the service's `getSimplifiedErrorMessage`, or the
/// cancel the screen asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadFailure {
    Failed(String),
    Cancelled,
}

/// `FileUpload.UploadCountingInputStream`: what the socket took, and a cancelled read.
struct CountingReader<'a, R> {
    inner: R,
    written: &'a AtomicU64,
    cancel: &'a AtomicBool,
}

impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "upload cancelled",
            ));
        }
        let read = self.inner.read(buf)?;
        self.written.fetch_add(read as u64, Ordering::Relaxed);
        Ok(read)
    }
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
        // `Request.Post`: a body goes out as application/json (the frontend answers 415
        // to a text/plain POST).
        let request = match body {
            Some(_) => request.set("Content-Type", "application/json; charset=utf-8"),
            None => request,
        };
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

    /// `RealmsClient.backupsFor`: `GET worlds/{id}/backups`.
    pub fn backups_for(&self, realm_id: i64) -> Result<Vec<Backup>, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/backups"));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
        serde_json::from_str::<BackupListResponse>(&json)
            .map(|r| r.backups)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.restoreWorld`: `PUT worlds/{id}/backups?backupId={backupId}`, with
    /// vanilla's 40 s/600 s request timeouts.
    pub fn restore_world(&self, realm_id: i64, backup_id: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/backups"));
        let query = format!("backupId={}", urlencoded(backup_id)?);
        Self::execute(
            self.request("PUT", &format!("{url}?{query}"), Duration::from_secs(600)),
            Some(""),
        )
        .map(drop)
    }

    /// `RealmsClient.requestDownloadInfo`: `GET worlds/{id}/slot/{slot}/download`.
    pub fn request_download_info(
        &self,
        realm_id: i64,
        slot: i32,
    ) -> Result<WorldDownload, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/slot/{slot}/download"));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
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

    /// `RealmsClient.getLiveStats`: each Realm's online players, by realm id. The
    /// `playerList` is a JSON string inside the JSON (`LenientJsonParser`), each entry a
    /// profile with `playerId`.
    pub fn live_players(&self) -> Result<std::collections::HashMap<i64, Vec<String>>, RealmsError> {
        let json = Self::execute(
            self.request("GET", &self.url("activities/liveplayerlist"), READ_TIMEOUT),
            None,
        )?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| RealmsError::Parse)?;
        let mut lists = std::collections::HashMap::new();
        let Some(entries) = value.get("lists").and_then(|v| v.as_array()) else {
            return Ok(lists);
        };
        for entry in entries {
            let id = entry.get("serverId").and_then(|v| v.as_i64()).unwrap_or(-1);
            let players = entry
                .get("playerList")
                .and_then(|v| v.as_str())
                .and_then(|list| serde_json::from_str::<serde_json::Value>(list).ok())
                .and_then(|list| list.as_array().cloned())
                .unwrap_or_default();
            let ids = players
                .iter()
                .filter_map(|player| Some(player.get("playerId")?.as_str()?.to_owned()))
                .collect();
            lists.insert(id, ids);
        }
        Ok(lists)
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

    /// `RealmsClient.subscriptionFor`: `GET subscriptions/{id}` (no `worlds/` prefix).
    pub fn subscription_for(&self, realm_id: i64) -> Result<Subscription, RealmsError> {
        let url = self.url(&format!("subscriptions/{realm_id}"));
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

    /// `RealmsClient.op`: `POST ops/{id}/{uuid}` with an empty body, then the ops.
    pub fn op_player(&self, realm_id: i64, uuid: &str) -> Result<Ops, RealmsError> {
        let url = self.url(&format!("ops/{realm_id}/{}", undashed(uuid)?));
        let json = Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(""))?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.deop`: `DELETE ops/{id}/{uuid}`, then the ops.
    pub fn deop_player(&self, realm_id: i64, uuid: &str) -> Result<Ops, RealmsError> {
        let url = self.url(&format!("ops/{realm_id}/{}", undashed(uuid)?));
        let json = Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.invite`: `POST invites/{id}` with `{"name": ...}`; the answer is the
    /// Realm as `RealmsServer.parse` reads it, and its `players` are the invited list.
    pub fn invite_player(&self, realm_id: i64, name: &str) -> Result<Vec<PlayerInfo>, RealmsError> {
        let body = serde_json::json!({"name": name}).to_string();
        let url = self.url(&format!("invites/{realm_id}"));
        let json = Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(&body))?;
        serde_json::from_str::<RealmServer>(&json)
            .map(|server| server.players)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.uninvite`: `DELETE invites/{id}/invite/{uuid}`.
    pub fn uninvite_player(&self, realm_id: i64, uuid: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/{realm_id}/invite/{}", undashed(uuid)?));
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

    /// `RealmsClient.getRealmTierConfiguration`: `GET worlds/{id}/configuration/tier`. A range
    /// the answer lacks is a configuration error, as `getRealmTierConfiguration`'s parse makes it.
    pub fn get_tier_configuration(
        &self,
        realm_id: i64,
    ) -> Result<RealmTierConfiguration, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/configuration/tier"));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
        let tier: RealmTierConfiguration =
            serde_json::from_str(&json).map_err(|_| RealmsError::Parse)?;
        if tier.render_distance.is_some() && tier.sim_distance.is_some() {
            Ok(tier)
        } else {
            Err(RealmsError::Parse)
        }
    }

    /// `RealmsClient.updateRenderDistance`: `PUT worlds/{id}/configuration/renderDistance/{n}`.
    pub fn update_render_distance(
        &self,
        realm_id: i64,
        render_distance: i32,
    ) -> Result<(), RealmsError> {
        let url = self.url(&format!(
            "worlds/{realm_id}/configuration/renderDistance/{render_distance}"
        ));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.updateSimulationDistance`: `PUT worlds/{id}/configuration/simDistance/{n}`.
    pub fn update_simulation_distance(
        &self,
        realm_id: i64,
        simulation_distance: i32,
    ) -> Result<(), RealmsError> {
        let url = self.url(&format!(
            "worlds/{realm_id}/configuration/simDistance/{simulation_distance}"
        ));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.fetchWorldTemplates`: `GET worlds/templates/{type}?page={page}&pageSize={size}`.
    /// `WorldTemplatePaginatedList.parse` leaves a page it can't read empty rather than failing.
    pub fn fetch_world_templates(
        &self,
        page: i32,
        page_size: i32,
        world_type: &str,
    ) -> Result<WorldTemplateList, RealmsError> {
        let world_type = path_segment(world_type)?;
        let url = self.url(&format!(
            "worlds/templates/{world_type}?page={page}&pageSize={page_size}"
        ));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    }

    /// `RealmsClient.resetWorldWithTemplate`: `POST worlds/{id}/reset` with
    /// `RealmsWorldResetDto` as `resetWorldWithTemplate` builds it (no seed, level type -1).
    pub fn reset_world_with_template(
        &self,
        realm_id: i64,
        template_id: i64,
    ) -> Result<bool, RealmsError> {
        let body = serde_json::json!({
            "seed": serde_json::Value::Null,
            "worldTemplateId": template_id,
            "levelType": -1,
            "generateStructures": false,
            "experiments": Vec::<String>::new(),
        })
        .to_string();
        let url = self.url(&format!("worlds/{realm_id}/reset"));
        // `Request.post(..., 30000, 80000)`: the reset generates the world before it answers.
        let json = Self::execute(
            self.request("POST", &url, Duration::from_secs(80)),
            Some(&body),
        )?;
        json.trim().parse::<bool>().map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.getNotifications`: `GET notifications`.
    pub fn notifications(&self) -> Result<Vec<RealmsNotification>, RealmsError> {
        let json = Self::execute(
            self.request("GET", &self.url("notifications"), READ_TIMEOUT),
            None,
        )?;
        Ok(RealmsNotification::parse_list(&json))
    }

    /// `RealmsClient.notificationsSeen`: `POST notifications/seen` with the UUIDs.
    pub fn notifications_seen(&self, uuids: &[String]) -> Result<(), RealmsError> {
        self.notification_answer("notifications/seen", uuids)
    }

    /// `RealmsClient.notificationsDismiss`: `POST notifications/dismiss` with the UUIDs.
    pub fn notifications_dismiss(&self, uuids: &[String]) -> Result<(), RealmsError> {
        self.notification_answer("notifications/dismiss", uuids)
    }

    /// `uuidListToJsonArray`, the body both answers carry.
    fn notification_answer(&self, path: &str, uuids: &[String]) -> Result<(), RealmsError> {
        let body = serde_json::to_string(uuids).unwrap_or_else(|_| "[]".to_owned());
        let url = self.url(path);
        Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(&body)).map(drop)
    }

    /// `RealmsClient.getNews`: `GET mco/v1/news`, the page the header's news button opens.
    pub fn news_link(&self) -> Result<Option<String>, RealmsError> {
        let json = Self::execute(
            self.request("GET", &self.url("mco/v1/news"), Duration::from_secs(10)),
            None,
        )?;
        #[derive(Deserialize, Default)]
        struct News {
            #[serde(rename = "newsLink", default)]
            news_link: Option<String>,
        }
        Ok(serde_json::from_str::<News>(&json)
            .ok()
            .and_then(|news| news.news_link))
    }

    /// `RealmsClient.trialAvailable`: `GET trial`, the service's yes or no. The product
    /// page shows its trial note only when this says yes.
    pub fn trial_available(&self) -> Result<bool, RealmsError> {
        let json = Self::execute(self.request("GET", &self.url("trial"), READ_TIMEOUT), None)?;
        json.trim().parse::<bool>().map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.initializeRealm`: `POST worlds/{id}/initialize` with
    /// `RealmsDescriptionDto` — the name and description a new Realm starts with
    /// (`RealmCreationTask`).
    pub fn initialize_realm(
        &self,
        realm_id: i64,
        name: &str,
        description: &str,
    ) -> Result<(), RealmsError> {
        let body = serde_json::json!({ "name": name, "description": description }).to_string();
        let url = self.url(&format!("worlds/{realm_id}/initialize"));
        Self::execute(self.request("POST", &url, READ_TIMEOUT), Some(&body)).map(drop)
    }

    /// `RealmsClient.putIntoMinigameMode`: `PUT worlds/minigames/{minigame}/{realm}`, the
    /// service's yes or no (`SwitchMinigameTask`).
    pub fn put_into_minigame_mode(
        &self,
        realm_id: i64,
        minigame_id: &str,
    ) -> Result<bool, RealmsError> {
        let minigame_id = path_segment(minigame_id)?;
        let url = self.url(&format!("worlds/minigames/{minigame_id}/{realm_id}"));
        let json = Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some(""))?;
        json.trim().parse::<bool>().map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.requestUploadInfo`: `PUT worlds/{id}/backups/upload`, the token the last
    /// answer gave as `UploadInfo.createRequest` sends it. `None` when it names no target.
    pub fn request_upload_info(
        &self,
        realm_id: i64,
        previous_token: Option<&str>,
    ) -> Result<Option<UploadInfo>, RealmsError> {
        let body = match previous_token {
            Some(token) => serde_json::json!({ "token": token }).to_string(),
            None => "{}".to_owned(),
        };
        let url = self.url(&format!("worlds/{realm_id}/backups/upload"));
        let json = Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some(&body))?;
        Ok(UploadInfo::parse(&json))
    }

    /// `FileUpload.startUpload`: `POST {endpoint}/upload/{realm}/{slot}` with the packed world.
    /// The reader counts what it hands the socket (`UploadCountingInputStream`) and fails the
    /// read once `cancel` is set; a `Retry-After` answer sends the body again (`MAX_RETRIES`),
    /// counting from zero as `UploadStatus.restart` does.
    #[allow(clippy::too_many_arguments)]
    pub fn upload_world(
        &self,
        realm_id: i64,
        slot_id: i32,
        info: &UploadInfo,
        world_version: &str,
        archive: &std::path::Path,
        written: &AtomicU64,
        cancel: &AtomicBool,
    ) -> Result<(), UploadFailure> {
        const MAX_RETRIES: usize = 5;
        let url = info
            .upload_url(realm_id, slot_id)
            .ok_or_else(|| UploadFailure::Failed("no upload endpoint".to_owned()))?;
        let token = info.token.as_deref().unwrap_or("null");
        let cookie = format!(
            "sid=token:{}:{:032x};token={token};user={};version={};worldVersion={world_version}",
            self.access_token,
            self.player_uuid,
            self.player_name,
            version::VANILLA_VERSION,
        );
        let mut attempt = 0usize;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(UploadFailure::Cancelled);
            }
            let file =
                std::fs::File::open(archive).map_err(|e| UploadFailure::Failed(e.to_string()))?;
            let counted = CountingReader {
                inner: file,
                written,
                cancel,
            };
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(15))
                .timeout(Duration::from_secs(600))
                // A redirect must not carry the upload cookie to another host.
                .redirects(0)
                .user_agent(&user_agent())
                .build();
            let request = agent
                .post(&url)
                .set("Cookie", &cookie)
                .set("Content-Type", "application/octet-stream")
                .set(
                    "Content-Length",
                    &std::fs::metadata(archive)
                        .map(|m| m.len())
                        .unwrap_or(0)
                        .to_string(),
                );
            let result = request.send(counted);
            if cancel.load(Ordering::Relaxed) {
                return Err(UploadFailure::Cancelled);
            }
            let response = match result {
                Ok(response) => response,
                Err(ureq::Error::Status(_, response)) => response,
                Err(ureq::Error::Transport(e)) => {
                    return Err(UploadFailure::Failed(e.to_string()));
                }
            };
            let retry_after = response
                .header("Retry-After")
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            let status = response.status();
            let body = response.into_string().unwrap_or_default();
            if retry_after > 0 && attempt + 1 < MAX_RETRIES {
                attempt += 1;
                written.store(0, Ordering::Relaxed);
                std::thread::sleep(Duration::from_secs(retry_after.min(60)));
                continue;
            }
            // `UploadResult.getSimplifiedErrorMessage`.
            if (200..300).contains(&status) {
                return Ok(());
            }
            if status == 400
                && let Some(message) = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| {
                        v.get("errorMsg")
                            .and_then(|m| m.as_str())
                            .map(str::to_owned)
                    })
            {
                return Err(UploadFailure::Failed(message));
            }
            return Err(UploadFailure::Failed(status.to_string()));
        }
    }

    /// The invite-code service: `frontendV2Request`, with the token in `x-mojangauth`.
    fn frontend(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, RealmsError> {
        let url = format!("{}/{path}", self.frontend_url);
        let request = self
            .request(method, &url, READ_TIMEOUT)
            .set("x-mojangauth", &self.access_token)
            .set("Accept", "application/json");
        Self::execute(request, body)
    }

    /// `RealmsClient.redeemInviteCode`: `POST api/v1.0/invite/codes/use/{code}`.
    pub fn redeem_invite_code(&self, code: &str) -> Result<(), RealmsError> {
        let code = path_segment(code.trim())?;
        self.frontend(
            "POST",
            &format!("api/v1.0/invite/codes/use/{code}"),
            Some(""),
        )
        .map(drop)
    }

    /// `RealmsClient.inviteCodes`: `GET api/v1.0/invite/codes?realmId={id}`.
    pub fn invite_codes(&self, realm_id: i64) -> Result<Vec<InviteCode>, RealmsError> {
        let json = self.frontend(
            "GET",
            &format!("api/v1.0/invite/codes?realmId={realm_id}"),
            None,
        )?;
        Ok(InviteCode::parse_list(&json))
    }

    /// `RealmsClient.createInviteCode`: `POST api/v1.0/invite/codes` with
    /// `InviteCodeRequestDto`'s JSON.
    pub fn create_invite_code(
        &self,
        realm_id: i64,
        enabled: bool,
        expiration: Option<i64>,
    ) -> Result<(), RealmsError> {
        let body = serde_json::json!({
            "code": null,
            "realmId": realm_id,
            "enabled": enabled,
            "expirationDate": expiration,
        })
        .to_string();
        self.frontend("POST", "api/v1.0/invite/codes", Some(&body))
            .map(drop)
    }

    /// `RealmsClient.updateInviteCode`: `POST api/v1.0/invite/codes/update`.
    pub fn update_invite_code(
        &self,
        realm_id: i64,
        code: &str,
        enabled: bool,
        expiration: Option<i64>,
    ) -> Result<(), RealmsError> {
        let body = serde_json::json!({
            "code": code,
            "realmId": realm_id,
            "enabled": enabled,
            "expirationDate": expiration,
        })
        .to_string();
        self.frontend("POST", "api/v1.0/invite/codes/update", Some(&body))
            .map(drop)
    }

    /// `RealmsClient.deleteInviteCode`: `DELETE api/v1.0/invite/codes/delete/{code}`.
    pub fn delete_invite_code(&self, code: &str) -> Result<(), RealmsError> {
        self.frontend(
            "DELETE",
            &format!("api/v1.0/invite/codes/delete/{}", path_segment(code)?),
            None,
        )
        .map(drop)
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

/// A query value: percent-encoded the way `URI` encodes it, so a backup id cannot add a
/// parameter or a fragment.
fn urlencoded(value: &str) -> Result<String, RealmsError> {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    if out.is_empty() {
        return Err(RealmsError::Parse);
    }
    Ok(out)
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

    /// `InviteCodeList.parse`: the answer is an array or `{"result":[...]}`, and an entry
    /// without its code is dropped.
    #[test]
    fn the_invite_code_list_reads_an_array_or_its_result() {
        let array = r#"[{"code":"A","enabled":true},{"code":"","enabled":true}]"#;
        let codes = InviteCode::parse_list(array);
        assert_eq!(codes.len(), 1, "an entry without a code is dropped");
        assert_eq!(codes[0].code, "A");
        assert!(codes[0].enabled);
        let wrapped = r#"{"result":[{"code":"B","expirationDate":17}]}"#;
        let codes = InviteCode::parse_list(wrapped);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].expiration_date, Some(17));
    }

    /// `RealmsNotification.parseList`: the `visitUrl` message and button text, the defaults
    /// for a missing `dismissable`, and no entry without its UUID.
    #[test]
    fn notifications_parse_their_kinds_and_text() {
        let raw = r#"{"notifications":[
            {"notificationUuid":"a","dismissable":false,"seen":true,"type":"visitUrl",
             "url":"https://x","buttonText":{"translationKey":"mco.notification.visitUrl.buttonText.default"},
             "message":{"translationKey":"mco.notification.visitUrl.message.default"}},
            {"notificationUuid":"b","type":"infoPopup","image":"icon/info"},
            {"seen":false,"type":"visitUrl"}]}"#;
        let list = RealmsNotification::parse_list(raw);
        assert_eq!(list.len(), 2, "an entry without an id is dropped");
        assert_eq!(list[0].uuid, "a");
        assert!(!list[0].dismissable);
        assert!(list[0].seen);
        assert_eq!(list[0].url.as_deref(), Some("https://x"));
        assert_eq!(list[0].kind, "visitUrl");
        assert_eq!(list[1].kind, "infoPopup");
        assert!(list[1].dismissable, "dismissable defaults to true");
    }

    /// `notificationsSeen`/`notificationsDismiss`: a JSON array of the UUIDs.
    #[test]
    fn a_notification_answer_posts_its_uuids() {
        let answer = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (base, server) = serve_request(answer);
        client(&base)
            .notifications_seen(&["a".to_owned(), "b".to_owned()])
            .unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /notifications/seen HTTP/1.1"),
            "{request}"
        );
        assert!(request.contains("[\"a\",\"b\"]"), "{request}");

        let (base, server) = serve_request(answer);
        client(&base)
            .notifications_dismiss(&["c".to_owned()])
            .unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /notifications/dismiss HTTP/1.1"),
            "{request}"
        );
        assert!(request.contains("[\"c\"]"), "{request}");
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
    #[test]
    fn the_tier_configuration_names_both_ranges_and_their_default() {
        let body = r#"{"renderDistance":{"min":2,"max":16,"defaultValue":8,"current":12},"simDistance":{"min":3,"max":12,"defaultValue":6,"current":null}}"#;
        let answer: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        );
        let (base, server) = serve_once(answer);
        let tier = client(&base).get_tier_configuration(7).unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("GET /worlds/7/configuration/tier HTTP/1.1"),
            "{request}"
        );
        // `currentOrDefault`: the current value, else the default.
        assert_eq!(tier.render_distance.unwrap().current_or_default(), 12);
        let sim = tier.sim_distance.unwrap();
        assert_eq!(sim.current_or_default(), 6);
        assert_eq!((sim.min, sim.max), (3, 12));
        // `getRealmTierConfiguration`: a range it cannot read is a parse error.
        let short: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{{\"renderDistance\":{{\"min\":2,\"max\":16}}}}",
                "{\"renderDistance\":{\"min\":2,\"max\":16}}".len()
            )
            .into_boxed_str(),
        );
        let (base, server) = serve_once(short);
        assert!(client(&base).get_tier_configuration(7).is_err());
        server.join().unwrap();
    }

    #[test]
    fn a_distance_update_puts_its_value_into_the_path() {
        for (realm, distance, path) in [
            (
                7,
                8,
                "PUT /worlds/7/configuration/renderDistance/8 HTTP/1.1",
            ),
            (12, 3, "PUT /worlds/12/configuration/simDistance/3 HTTP/1.1"),
        ] {
            let (base, server) =
                serve_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            if path.contains("renderDistance") {
                client(&base)
                    .update_render_distance(realm, distance)
                    .unwrap();
            } else {
                client(&base)
                    .update_simulation_distance(realm, distance)
                    .unwrap();
            }
            let request = server.join().unwrap();
            assert!(request.starts_with(path), "{request}");
        }
    }

    /// `Backup.parse`: the date is `lastModifiedDate` (epoch millis), metadata values are
    /// their JSON text and nulls are dropped; `changeList` is local, not on the wire.
    #[test]
    fn a_backup_reads_vanillas_fields() {
        let list: BackupListResponse = serde_json::from_str(
            r#"{"backups":[{"backupId":"abc","lastModifiedDate":1700000000000,"size":42,
                "metadata":{"name":"Friends","game_mode":1,"uploaded":true,"dead":null}}]}"#,
        )
        .unwrap();
        let backup = &list.backups[0];
        assert_eq!(backup.backup_id, "abc");
        assert_eq!(backup.last_modified, 1_700_000_000_000);
        assert_eq!(backup.size, 42);
        assert_eq!(
            backup.metadata.get("name").map(String::as_str),
            Some("Friends")
        );
        assert_eq!(
            backup.metadata.get("game_mode").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            backup.metadata.get("uploaded").map(String::as_str),
            Some("true")
        );
        assert!(!backup.metadata.contains_key("dead"));
        assert!(backup.change_list.is_empty());
    }

    /// `RealmsClient.restoreWorld` puts the backup id in the query: a `&` or `/` in it
    /// must not add parameters or paths.
    #[test]
    fn a_backup_id_is_a_safe_query_value() {
        assert_eq!(urlencoded("abc-123_x.y~z").unwrap(), "abc-123_x.y~z");
        assert_eq!(urlencoded("a b&c=d/e").unwrap(), "a%20b%26c%3Dd%2Fe");
        assert!(urlencoded("").is_err());
    }

    #[test]
    fn a_download_answer_reads_its_links() {
        let download: WorldDownload = serde_json::from_str(
            r#"{"downloadLink":"https://blob.example/backup.tar.gz",
                "resourcePackUrl":"https://blob.example/pack.tar.gz","resourcePackHash":"abc123"}"#,
        )
        .unwrap();
        assert_eq!(download.download_link, "https://blob.example/backup.tar.gz");
        assert_eq!(
            download.resource_pack_url,
            "https://blob.example/pack.tar.gz"
        );
        assert_eq!(download.resource_pack_hash, "abc123");
        // `JsonUtils.getStringOr`: a missing or null field is the empty string.
        let bare: WorldDownload =
            serde_json::from_str(r#"{"downloadLink":null,"resourcePackUrl":null}"#).unwrap();
        assert_eq!(bare.download_link, "");
        assert_eq!(bare.resource_pack_url, "");
        assert_eq!(bare.resource_pack_hash, "");
    }

    /// `RealmsClient.op`/`deop`: the `ops` endpoint, not the world one (a POST to
    /// `worlds/{id}/{uuid}` answers 404).
    #[test]
    fn player_permissions_go_to_the_ops_endpoint() {
        let uuid = "1b7b6c60-dd98-4748-a5e9-4bb21feaffc1";
        let (base, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        client(&base).op_player(7, uuid).unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /ops/7/1b7b6c60dd984748a5e94bb21feaffc1 HTTP/1.1"),
            "{request}"
        );
        let (base, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        client(&base).deop_player(7, uuid).unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("DELETE /ops/7/1b7b6c60dd984748a5e94bb21feaffc1 HTTP/1.1"),
            "{request}"
        );
    }

    /// `RealmsClient.invite`/`uninvite`: the `invites` endpoint, the `{"name": ...}` body,
    /// and the answered Realm's `players` (not the answer itself as a player list).
    #[test]
    fn invites_go_to_the_invites_endpoint() {
        let body = r#"{"id":7,"name":"Friends","players":[{"name":"Alex","uuid":"01234567-89ab-cdef-0123-456789abcdef","operator":false,"accepted":false,"online":false}]}"#;
        let answer: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        );
        let (base, server) = serve_once(answer);
        let players = client(&base).invite_player(7, "Alex").unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /invites/7 HTTP/1.1"), "{request}");
        assert!(request.contains("\"name\":\"Alex\""), "{request}");
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].name, "Alex");

        let (base, server) =
            serve_once("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        client(&base)
            .uninvite_player(7, "1b7b6c60-dd98-4748-a5e9-4bb21feaffc1")
            .unwrap();
        let request = server.join().unwrap();
        assert!(
            request
                .starts_with("DELETE /invites/7/invite/1b7b6c60dd984748a5e94bb21feaffc1 HTTP/1.1"),
            "{request}"
        );
    }

    #[test]
    fn a_slider_position_maps_into_the_range() {
        // `OptionInstance.IntRange` with min 2, max 16: the ends and the middle snap.
        let range = RealmTierRange {
            min: 2,
            max: 16,
            default_value: 8,
            current: None,
        };
        assert_eq!(range.from_slider(0.0), 2);
        assert_eq!(range.from_slider(1.0), 16);
        assert_eq!(range.from_slider(0.5), 9);
        assert_eq!(range.current_or_default(), 8);
    }

    /// One HTTP exchange whose whole request (the Content-Length body included) is returned.
    fn serve_request(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
                if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&seen[..end]).into_owned();
                    let length = header(&headers, "Content-Length")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    if seen.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        (base, handle)
    }

    #[test]
    fn a_reset_posts_the_template_with_no_seed() {
        let (base, server) =
            serve_request("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue");
        assert!(client(&base).reset_world_with_template(7, 42).unwrap());
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /worlds/7/reset HTTP/1.1"),
            "{request}"
        );
        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
        // `RealmsWorldResetDto` as `resetWorldWithTemplate` builds it.
        assert!(body.contains("\"worldTemplateId\":42"), "{request}");
        assert!(body.contains("\"seed\":null"), "{request}");
        assert!(body.contains("\"levelType\":-1"), "{request}");
        assert!(body.contains("\"generateStructures\":false"), "{request}");
    }

    #[test]
    fn world_templates_are_asked_for_by_page_and_world_type() {
        let body = r#"{"templates":[{"id":"42","name":"Skyblock","version":"1.21","author":"Mojang","recommendedPlayers":"1-4","type":"MINIGAME"}],"page":2,"size":10,"total":11}"#;
        let answer: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        );
        let (base, server) = serve_once(answer);
        let list = client(&base)
            .fetch_world_templates(2, 10, "ADVENTUREMAP")
            .unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("GET /worlds/templates/ADVENTUREMAP?page=2&pageSize=10 HTTP/1.1"),
            "{request}"
        );
        assert_eq!((list.page, list.size, list.total), (2, 10, 11));
        assert_eq!(list.templates[0].name, "Skyblock");
        assert_eq!(list.templates[0].kind, "MINIGAME");
    }

    #[test]
    fn a_template_without_a_known_type_is_a_world_template() {
        // `WorldTemplate.parse`: a missing or unknown `type` reads as WORLD_TEMPLATE.
        let missing: WorldTemplateList =
            serde_json::from_str(r#"{"templates":[{"id":"1"}]}"#).unwrap();
        assert_eq!(missing.templates[0].kind, "WORLD_TEMPLATE");
        let unknown: WorldTemplateList =
            serde_json::from_str(r#"{"templates":[{"type":"nonsense"}]}"#).unwrap();
        assert_eq!(unknown.templates[0].kind, "WORLD_TEMPLATE");
        assert_eq!((unknown.page, unknown.size, unknown.total), (0, 0, 0));
    }

    #[test]
    fn an_upload_answer_gives_the_endpoint_and_its_port() {
        let info = UploadInfo::parse(
            r#"{"uploadEndpoint":"upload.example.net","port":8081,"worldClosed":true,"token":"abc"}"#,
        )
        .unwrap();
        assert_eq!(info.endpoint, "upload.example.net");
        assert_eq!(info.port, 8081);
        assert!(info.world_closed);
        assert_eq!(info.token.as_deref(), Some("abc"));
        // `FileUpload`: resolve("/upload/{realm}/{slot}") against the endpoint.
        assert_eq!(
            info.upload_url(7, 2).unwrap(),
            "http://upload.example.net:8081/upload/7/2"
        );
        // `UploadInfo.assembleUri`: a named scheme and port stay; otherwise 8080.
        let info = UploadInfo::parse(r#"{"uploadEndpoint":"https://upload.example.net:9000/v1"}"#)
            .unwrap();
        assert_eq!(
            info.upload_url(7, 2).unwrap(),
            "https://upload.example.net:9000/upload/7/2"
        );
        let info = UploadInfo::parse(r#"{"uploadEndpoint":"https://upload.example.net"}"#).unwrap();
        assert_eq!(
            info.upload_url(7, 2).unwrap(),
            "https://upload.example.net:8080/upload/7/2"
        );
        // An answer without an endpoint is no target at all.
        assert!(UploadInfo::parse("{}").is_none());
    }

    #[test]
    fn a_minigame_switch_puts_the_template_in_the_path() {
        let (base, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue");
        assert!(client(&base).put_into_minigame_mode(7, "12").unwrap());
        let request = server.join().unwrap();
        assert!(
            request.starts_with("PUT /worlds/minigames/12/7 HTTP/1.1"),
            "{request}"
        );
        // A template id is one path segment: anything else never reaches the service.
        let error = client(&base)
            .put_into_minigame_mode(7, "../worlds")
            .unwrap_err();
        assert!(matches!(error, RealmsError::Service { status: 400, .. }));
    }
}
