//! Player skins from profile `textures` properties: authlib's
//! `MinecraftServicesSessionService.unpackTextures`, vanilla's
//! `SkinManager` lookup (which texture a player shows, and when the default
//! one) and `SkinTextureDownloader.processLegacySkin`.

#[cfg(test)]
mod upload_tests;
mod workers;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Retains a stopped IO service across a guest/Game teardown. Poll this on
/// the owner until complete before dropping the last handle on a frame thread.
#[derive(Clone)]
pub struct WorkHandle(Arc<workers::Pool>);
impl WorkHandle {
    pub fn shutdown(&self) -> bool {
        self.0.shutdown()
    }
}

fn io_active(stopping: &AtomicBool, scope: &AtomicBool) -> bool {
    !stopping.load(Ordering::Acquire) && !scope.load(Ordering::Acquire)
}

/// `Property.hasSignature` and the services key check, as authlib's
/// `SignatureState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    Unsigned,
    Signed,
    Invalid,
}

/// `MinecraftProfileTextures`: the texture URLs a profile names. Empty when
/// the payload doesn't decode or names a URL outside the allowed domains.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Textures {
    pub skin: Option<String>,
    pub cape: Option<String>,
    pub elytra: Option<String>,
    pub signed: bool,
    /// The skin's `metadata.model` is `slim` (`PlayerSkin.Model.SLIM`, the
    /// three-pixel arms); any other value or none is wide.
    pub slim: bool,
}

/// The `getTexture` endpoint's valid URIs in production's services discovery
/// document (`MinecraftServicesEnvironment.PROD`).
pub const TEXTURE_URIS: [&str; 2] = [
    "https://textures.minecraft.net/texture/{textureId}",
    "http://textures.minecraft.net/texture/{textureId}",
];

/// `MinecraftServicesDiscoveryService`'s valid texture URIs: the discovery
/// document's `profiles` / `getTexture` `validUris`, which
/// `MinecraftServicesSessionService.isAllowedTextureDomain` checks texture URLs
/// against. Production's document lists [`TEXTURE_URIS`], so they are used
/// without asking it; another environment (`HORIZON_API_DISCOVERY`) is asked
/// once per game, and keeps [`TEXTURE_URIS`] when its document lists none or
/// can't be fetched.
pub fn texture_uris() -> &'static [String] {
    static URIS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    URIS.get_or_init(|| {
        crate::auth::discovery_overridden()
            .then(|| crate::auth::discovery().ok())
            .flatten()
            .and_then(|doc| discovery_texture_uris(&doc))
            .unwrap_or_else(|| TEXTURE_URIS.map(str::to_owned).to_vec())
    })
}

/// The `profiles` / `getTexture` endpoint's `validUris` in a discovery
/// document, when it lists any.
fn discovery_texture_uris(doc: &serde_json::Value) -> Option<Vec<String>> {
    let uris: Vec<String> = doc
        .pointer("/discovery/profiles/endpoints/getTexture/validUris")?
        .as_array()?
        .iter()
        .filter_map(|u| u.as_str().map(str::to_owned))
        .collect();
    (!uris.is_empty()).then_some(uris)
}

/// `java.util.Base64.getDecoder().decode`.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::{
        Engine,
        engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    };
    STANDARD
        .decode(text)
        .or_else(|_| STANDARD_NO_PAD.decode(text))
        .ok()
}

/// `unpackTextures(property)`: the textures of a `textures` property's
/// base64 JSON payload.
pub fn unpack(value: &str, signature: Signature, valid_uris: &[impl AsRef<str>]) -> Textures {
    let empty = Textures::default();
    let Some(json) = base64_decode(value)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return empty;
    };
    let Some(textures) = json.get("textures").and_then(|t| t.as_object()) else {
        return empty;
    };
    if textures.is_empty() {
        return empty;
    }
    let mut out = Textures {
        signed: signature == Signature::Signed,
        ..empty.clone()
    };
    for (kind, texture) in textures {
        let Some(url) = texture.get("url").and_then(|u| u.as_str()) else {
            return empty;
        };
        let allowed = valid_uris
            .iter()
            .any(|u| url.starts_with(&u.as_ref().replace("{textureId}", "")));
        if !allowed {
            return empty;
        }
        if kind == "SKIN" {
            out.slim = texture
                .get("metadata")
                .and_then(|m| m.get("model"))
                .and_then(|m| m.as_str())
                == Some("slim");
        }
        let slot = match kind.as_str() {
            "SKIN" => &mut out.skin,
            "CAPE" => &mut out.cape,
            "ELYTRA" => &mut out.elytra,
            _ => continue,
        };
        *slot = Some(url.to_owned());
    }
    out
}

/// The skin URL of a `minecraft:profile` component's property list. Player
/// heads and mannequins read their profile this way, and `SkinManager` lets
/// those textures be unsigned (`getInsecureSkinInformation`).
pub fn profile_skin_url(profile: &serde_json::Value) -> Option<String> {
    let (value, signature) = parse_profile(profile).textures?;
    unpack(&value, signature, texture_uris()).skin
}

/// The textures of a Minecraft services `/minecraft/profile` answer: the
/// `ACTIVE` skin and cape (`MinecraftProfileResponse`). Unsigned, like the
/// local player's own textures, and only URLs under the allowed domains count.
pub fn services_profile_textures(profile: &serde_json::Value) -> Textures {
    let active = |key: &str| {
        profile
            .get(key)?
            .as_array()?
            .iter()
            .find(|t| t.get("state").and_then(|s| s.as_str()) == Some("ACTIVE"))
    };
    let skin_entry = active("skins");
    let skin = skin_entry
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .filter(|url| {
            texture_uris()
                .iter()
                .any(|u| url.starts_with(&u.replace("{textureId}", "")))
        })
        .map(str::to_owned);
    let slim = skin_entry
        .and_then(|t| t.get("variant"))
        .and_then(|v| v.as_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("slim"));
    let cape = active("capes")
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .filter(|url| {
            texture_uris()
                .iter()
                .any(|u| url.starts_with(&u.replace("{textureId}", "")))
        })
        .map(str::to_owned);
    Textures {
        skin,
        slim,
        cape,
        elytra: None,
        signed: false,
    }
}

/// `profile_skin_url` on a block entity's `profile` NBT (`SkullBlockEntity`):
/// the same `properties` list, held as a compound. A name-only profile (a
/// string tag) has no properties and no skin URL.
pub fn profile_skin_url_nbt(profile: &horizon_world::nbt::Tag) -> Option<String> {
    let properties = match profile.get("properties")? {
        horizon_world::nbt::Tag::List(list) => list,
        _ => return None,
    };
    let value = properties.iter().find_map(|p| {
        let name = nbt_text(p.get("name"))?;
        (name == "textures")
            .then(|| nbt_text(p.get("value")))
            .flatten()
    })?;
    unpack(&value, Signature::Unsigned, texture_uris()).skin
}

/// `ResolvableProfile.skinPatch().body()` of a skull's `profile` NBT: the
/// `texture` id that replaces its skin.
pub fn profile_texture_nbt(profile: &horizon_world::nbt::Tag) -> Option<String> {
    nbt_text(profile.get("texture"))
}

/// The profile's uuid as u128: `id` as an int array (block entity NBT) or a
/// uuid string (older and component forms). Used for `DefaultPlayerSkin.get`
/// when a head's skin is not downloaded yet.
pub fn profile_uuid_nbt(profile: &horizon_world::nbt::Tag) -> Option<u128> {
    match profile.get("id")? {
        horizon_world::nbt::Tag::IntArray(v) if v.len() == 4 => Some(
            v.iter()
                .fold(0u128, |a, &w| (a << 32) | u128::from(w as u32)),
        ),
        horizon_world::nbt::Tag::String(s) => {
            let digits: String = s.to_string_lossy().chars().filter(|c| *c != '-').collect();
            u128::from_str_radix(&digits, 16).ok()
        }
        _ => None,
    }
}

pub(crate) fn nbt_text(tag: Option<&horizon_world::nbt::Tag>) -> Option<String> {
    match tag {
        Some(horizon_world::nbt::Tag::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

pub use crate::chunk::block_entity::NameOrId;

/// `GameProfile` as `fetchProfile` returns it: the account's id, name and
/// property map (its `textures` property is the skin).
#[derive(Clone, Debug)]
pub struct ResolvedProfile {
    pub id: u128,
    pub name: String,
    /// `(name, value, signature)` properties.
    pub properties: Vec<(String, String, Option<String>)>,
}
impl ResolvedProfile {
    /// `SessionService.getPackedTextures`: the `textures` property.
    pub fn textures(&self) -> Option<(String, Signature)> {
        self.properties
            .iter()
            .find(|(n, _, _)| n == "textures")
            .map(|(_, v, s)| {
                (
                    v.clone(),
                    if s.is_some() {
                        Signature::Signed
                    } else {
                        Signature::Unsigned
                    },
                )
            })
    }
}

/// Where a `Dynamic` profile's resolution stands (`resolveProfile`'s future).
#[derive(Clone, Debug)]
pub enum Resolve {
    Pending,
    /// The services' answer: `None` for no profile or no answer, which the
    /// cache holds like any result (`Optional.empty`).
    Done(Option<ResolvedProfile>),
}

/// A caller's captured profile future. Its result outlives eviction from the
/// shared resolver cache, as ClientMannequin.skinLookup does in vanilla.
#[derive(Clone, Debug, Default)]
pub struct ProfileLookup(std::sync::Arc<std::sync::OnceLock<Option<ResolvedProfile>>>);
impl ProfileLookup {
    pub fn state(&self) -> Resolve {
        match self.0.get() {
            Some(resolved) => Resolve::Done(resolved.clone()),
            None => Resolve::Pending,
        }
    }
    fn complete(&self, resolved: Option<ResolvedProfile>) {
        let _ = self.0.set(resolved);
    }
    fn completed(resolved: Option<ResolvedProfile>) -> Self {
        let lookup = Self::default();
        lookup.complete(resolved);
        lookup
    }
}

/// `ProfileResolver.Cached`: resolves a `Dynamic` profile through the
/// services — `profileCacheByName` asks `profiles`/`getByName` for the id,
/// `profileCacheById` asks `session`/`getProfileById` for the profile —
/// at most 256 entries kept 10 minutes after their last use, fetched on
/// background threads.
pub struct Resolver {
    entries: std::collections::HashMap<NameOrId, Entry>,
    render_entries: std::collections::HashMap<NameOrId, RenderEntry>,
    done: std::sync::mpsc::Receiver<ProfileCompletion>,
    send: std::sync::mpsc::Sender<ProfileCompletion>,
    last_sweep: std::time::Instant,
    next_request: u64,
    workers: Arc<workers::Pool>,
    scope: Arc<AtomicBool>,
    heads: HeadProfiles,
}
type ProfileCompletion = (NameOrId, u64, ProfileLookup);
struct Entry {
    /// The fetch that owns this entry, even if this key was evicted and requested again.
    request: u64,
    touched: std::time::Instant,
    lookup: ProfileLookup,
}
struct RenderEntry {
    touched: std::time::Instant,
    lookup: ProfileLookup,
}
/// `ProfileResolver.Cached`'s cache bounds.
const MAX_PROFILES: usize = 256;
const PROFILE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// PlayerSkinRenderCache keeps rendering futures five minutes after access,
/// independently of ProfileResolver's maximum of 256 service entries.
const RENDER_PROFILE_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// `StringUtil.isValidPlayerName` (the check `fetchByName` makes first).
fn valid_player_name(name: &str) -> bool {
    name.encode_utf16().count() <= 16 && name.encode_utf16().all(|c| c > 32 && c < 127)
}

/// `fetchByNameOrId`: `getByName` answers `{id, name}`; `getProfileById`
/// (`fetchProfile(id, requireSecure)`) answers `{id, name, properties}` —
/// asked `unsigned=false` so the `textures` property is signed.
fn resolve_profile(key: &NameOrId, doc: &serde_json::Value) -> Option<ResolvedProfile> {
    resolve_profile_cancellable(key, doc, &|| true)
}
fn resolve_profile_cancellable(
    key: &NameOrId,
    doc: &serde_json::Value,
    active: &impl Fn() -> bool,
) -> Option<ResolvedProfile> {
    if !active() {
        return None;
    }
    let id = match key {
        NameOrId::Id(id) => *id,
        NameOrId::Name(name) => {
            let url = crate::auth::endpoint(doc, "profiles", "getByName")?
                .replace("{name}", &name.to_lowercase());
            let body = http_get_cancellable(&url, active)?;
            let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
            crate::auth::parse_uuid(v.get("id")?.as_str()?)?
        }
    };
    let url = crate::auth::endpoint(doc, "session", "getProfileById")?
        .replace("{profileId}", &format!("{id:032x}"));
    let separator = if url.contains('?') { '&' } else { '?' };
    let body = http_get_cancellable(&format!("{url}{separator}unsigned=false"), active)?;
    let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
    Some(ResolvedProfile {
        id: crate::auth::parse_uuid(v.get("id")?.as_str()?)?,
        name: v.get("name")?.as_str()?.to_owned(),
        properties: v
            .get("properties")?
            .as_array()?
            .iter()
            .map(|p| {
                Some((
                    p.get("name")?.as_str()?.to_owned(),
                    p.get("value")?.as_str()?.to_owned(),
                    p.get("signature")
                        .and_then(|s| s.as_str())
                        .map(str::to_owned),
                ))
            })
            .collect::<Option<Vec<_>>>()?,
    })
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}
impl Resolver {
    pub fn new() -> Self {
        let (send, done) = std::sync::mpsc::channel();
        Self {
            entries: Default::default(),
            render_entries: Default::default(),
            done,
            send,
            last_sweep: std::time::Instant::now(),
            next_request: 0,
            workers: workers::Pool::new("Skin profiles", 8),
            scope: Arc::new(AtomicBool::new(false)),
            heads: HeadProfiles::default(),
        }
    }
    /// Ely lookups for a remote session share this game's owned IO service.
    /// Dropping that session cancels its scope without joining on the frame thread.
    pub fn ely_skins(&self) -> ElySkins {
        ElySkins::with_workers(self.workers.clone())
    }
    pub fn shutdown(&mut self) -> bool {
        self.workers.shutdown()
    }
    pub fn retire_workers(&self) -> WorkHandle {
        self.workers.request_shutdown();
        WorkHandle(self.workers.clone())
    }

    /// `CacheBuilder.expireAfterAccess` with `maximumSize`.
    fn sweep(&mut self) {
        if self.last_sweep.elapsed() < std::time::Duration::from_secs(30) {
            return;
        }
        self.last_sweep = std::time::Instant::now();
        self.render_entries
            .retain(|_, e| e.touched.elapsed() < RENDER_PROFILE_TTL);
        self.entries
            .retain(|_, e| e.touched.elapsed() < PROFILE_TTL);
        while self.entries.len() > MAX_PROFILES
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.touched)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
    }
    /// Drain completed jobs into matching shared entries. Captured futures
    /// are settled by their worker even when no cache entry remains.
    pub fn poll(&mut self) {
        while let Ok((key, request, lookup)) = self.done.try_recv() {
            // A cache eviction does not stop its worker. Only the fetch that
            // owns the current entry may settle it; an old answer must neither
            // replace a newer profile nor bring an evicted entry back.
            if let Some(entry) = self.entries.get_mut(&key)
                && entry.request == request
            {
                entry.touched = std::time::Instant::now();
                entry.lookup = lookup;
            }
        }
        self.sweep();
    }
    /// Capture this request for a live caller, independently of the shared
    /// cache's capacity and expiry. Drop the handle when the caller changes.
    pub fn lookup(&mut self, key: &NameOrId) -> ProfileLookup {
        self.state(key);
        self.entries
            .get(key)
            .expect("lookup creates its entry")
            .lookup
            .clone()
    }
    /// A head or inline player object owns its rendering future independently
    /// of the bounded service cache, like PlayerSkinRenderCache.lookup.
    pub fn render_state(&mut self, key: &NameOrId) -> Resolve {
        self.poll();
        if let Some(entry) = self
            .render_entries
            .get_mut(key)
            .filter(|entry| entry.touched.elapsed() < RENDER_PROFILE_TTL)
        {
            entry.touched = std::time::Instant::now();
            return entry.lookup.state();
        }
        let lookup = self.lookup(key);
        let state = lookup.state();
        self.render_entries.insert(
            key.clone(),
            RenderEntry {
                touched: std::time::Instant::now(),
                lookup,
            },
        );
        state
    }
    /// How the shared cache's resolution stands, starting it the first time.
    pub fn state(&mut self, key: &NameOrId) -> Resolve {
        self.poll();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.touched = std::time::Instant::now();
            return entry.lookup.state();
        }
        // `fetchByName` refuses an invalid name without a fetch; like every
        // other empty answer that is the cached result.
        let resolved = self.workers.stopped()
            || matches!(key, NameOrId::Name(name) if !valid_player_name(name));
        self.next_request = self.next_request.wrapping_add(1);
        let request = self.next_request;
        let lookup = if resolved {
            ProfileLookup::completed(None)
        } else {
            ProfileLookup::default()
        };
        self.entries.insert(
            key.clone(),
            Entry {
                request,
                touched: std::time::Instant::now(),
                lookup: lookup.clone(),
            },
        );
        if resolved {
            return Resolve::Done(None);
        }
        let (key, send) = (key.clone(), self.send.clone());
        let canceled = lookup.clone();
        self.workers.submit(
            self.scope.clone(),
            move |stopping, scope| {
                let active = || io_active(stopping, scope);
                let resolved = active()
                    .then(crate::auth::discovery)
                    .and_then(Result::ok)
                    .filter(|_| active())
                    .and_then(|doc| resolve_profile_cancellable(&key, &doc, &active));
                // Captured futures settle even after cache eviction. A canceled
                // game can settle its old future but cannot publish another fetch.
                lookup.complete(if active() { resolved } else { None });
                let _ = send.send((key, request, lookup));
            },
            move || canceled.complete(None),
        );
        Resolve::Pending
    }
}

impl Drop for Resolver {
    fn drop(&mut self) {
        self.workers.cancel_scope(&self.scope);
        self.workers.request_shutdown();
    }
}

/// `ResolvableProfile` (an item's `minecraft:profile` component, a skull's
/// `profile` tag): the `textures` property it carries, the key a `Dynamic`
/// profile resolves by, and `partialProfile.id` — the fallback skin's key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileInfo {
    /// `GameProfile.id` (`DefaultPlayerSkin.get`).
    pub id: u128,
    /// The `textures` property's value and signature state, when present.
    pub textures: Option<(String, Signature)>,
    /// `Dynamic` profiles resolve by name or id.
    pub dynamic: Option<NameOrId>,
}

/// A profile's `id`: `UUIDUtil.CODEC` (an int array of four, or a UUID
/// string).
fn profile_id(v: &serde_json::Value) -> Option<u128> {
    match v {
        serde_json::Value::Array(a) => {
            let ints: Option<Vec<i64>> = a.iter().map(|x| x.as_i64()).collect();
            let i = ints?;
            (i.len() == 4).then(|| {
                ((i[0] as u32 as u128) << 96)
                    | ((i[1] as u32 as u128) << 64)
                    | ((i[2] as u32 as u128) << 32)
                    | i[3] as u32 as u128
            })
        }
        serde_json::Value::String(s) => crate::auth::parse_uuid(s),
        _ => None,
    }
}

/// `ResolvableProfile`'s read of a `profile` value: a bare name is
/// `createUnresolved` (the `PLAYER_NAME` codec's alternative), a compound
/// `name` + `id` + `properties` (`Partial.MAP_CODEC` or a `GameProfile`).
pub fn parse_profile(v: &serde_json::Value) -> ProfileInfo {
    if let serde_json::Value::String(name) = v {
        return ProfileInfo {
            id: horizon_protocol::crypto::offline_uuid(name),
            textures: None,
            dynamic: valid_player_name(name).then(|| NameOrId::Name(name.clone())),
        };
    }
    let name = v.get("name").and_then(|n| n.as_str()).map(str::to_owned);
    let id = v.get("id").and_then(profile_id);
    let properties = v.get("properties").and_then(|p| p.as_array());
    let textures = properties
        .into_iter()
        .flatten()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
        .and_then(|p| {
            Some((
                p.get("value")?.as_str()?.to_owned(),
                if p.get("signature").is_some() {
                    Signature::Signed
                } else {
                    Signature::Unsigned
                },
            ))
        });
    // `ResolvableProfile.create`: a Partial is Dynamic with no properties
    // and exactly one of a name or an id.
    let dynamic =
        (properties.is_none_or(|p| p.is_empty()) && name.is_some() != id.is_some()).then(|| {
            name.as_deref().map_or_else(
                || NameOrId::Id(id.unwrap()),
                |n| NameOrId::Name(n.to_owned()),
            )
        });
    ProfileInfo {
        // `createPartialProfile`: the id, else the name's offline UUID, else nil.
        id: id
            .or_else(|| name.as_deref().map(horizon_protocol::crypto::offline_uuid))
            .unwrap_or(0),
        textures,
        dynamic,
    }
}

/// `ResolvableProfile.create`'s `Dynamic` key from a skull's `profile` tag:
/// a bare name string is `createUnresolved`, and a compound without
/// properties and with exactly one of a name or an id is `Dynamic`.
pub fn dynamic_nbt(profile: &horizon_world::nbt::Tag) -> Option<NameOrId> {
    if let horizon_world::nbt::Tag::String(s) = profile {
        let name = s.to_string_lossy();
        return valid_player_name(&name).then_some(NameOrId::Name(name));
    }
    if matches!(profile.get("properties"), Some(horizon_world::nbt::Tag::List(l)) if !l.is_empty())
    {
        return None;
    }
    match (nbt_text(profile.get("name")), profile_uuid_nbt(profile)) {
        (Some(name), None) => Some(NameOrId::Name(name)),
        (None, Some(id)) => Some(NameOrId::Id(id)),
        _ => None,
    }
}

const MAX_HEAD_PROFILES: usize = 4096;

#[derive(Debug, PartialEq)]
struct HeadProfile {
    info: ProfileInfo,
    skin: Option<(String, String)>,
}

fn prepare_head(profile: &serde_json::Value) -> HeadProfile {
    prepare_head_info(parse_profile(profile))
}

fn prepare_head_info(info: ProfileInfo) -> HeadProfile {
    let skin = info.textures.as_ref().and_then(|(value, signature)| {
        let url = unpack(value, *signature, texture_uris()).skin?;
        let location = skin_location(&texture_hash(&url)?);
        Some((url, location))
    });
    HeadProfile { info, skin }
}

#[derive(Default)]
struct HeadProfiles {
    entries: std::collections::HashMap<serde_json::Value, std::sync::Arc<HeadProfile>>,
    nbt: std::collections::HashMap<horizon_world::nbt::Tag, std::sync::Arc<serde_json::Value>>,
    resolved: [std::collections::HashMap<String, std::sync::Arc<HeadProfile>>; 2],
}

impl HeadProfiles {
    fn get_resolved(&mut self, value: &str, signed: bool) -> std::sync::Arc<HeadProfile> {
        let cache = &mut self.resolved[usize::from(signed)];
        if let Some(parsed) = cache.get(value) {
            return parsed.clone();
        }
        if cache.len() >= MAX_HEAD_PROFILES {
            cache.clear();
        }
        // This is the same properties-only profile as the service supplied,
        // without rebuilding an intermediate JSON property map on every miss.
        let parsed = std::sync::Arc::new(prepare_head_info(ProfileInfo {
            textures: Some((
                value.to_owned(),
                if signed {
                    Signature::Signed
                } else {
                    Signature::Unsigned
                },
            )),
            ..ProfileInfo::default()
        }));
        cache.insert(value.to_owned(), parsed.clone());
        parsed
    }

    fn get(&mut self, profile: &serde_json::Value) -> std::sync::Arc<HeadProfile> {
        if let Some(parsed) = self.entries.get(profile) {
            return parsed.clone();
        }
        if self.entries.len() >= MAX_HEAD_PROFILES {
            self.entries.clear();
        }
        let parsed = std::sync::Arc::new(prepare_head(profile));
        self.entries.insert(profile.clone(), parsed.clone());
        parsed
    }
}

impl Resolver {
    /// Cache by NBT value rather than address: equipment can be replaced or
    /// mutated in place, and neither must leave a stale profile behind.
    pub fn head_profile_nbt(
        &mut self,
        profile: &horizon_world::nbt::Tag,
    ) -> std::sync::Arc<serde_json::Value> {
        if let Some(json) = self.heads.nbt.get(profile) {
            return json.clone();
        }
        if self.heads.nbt.len() >= MAX_HEAD_PROFILES {
            self.heads.nbt.clear();
        }
        let json = std::sync::Arc::new(crate::scoreboard::tag_to_json(profile));
        self.heads.nbt.insert(profile.clone(), json.clone());
        json
    }
}

/// `PlayerSkinRenderCache.getOrDefault` for a head: the resolved skin once
/// its texture is in the atlas (`atlas`), else `partialProfile`'s default.
pub fn head_skin(
    profile: &serde_json::Value,
    resolver: &mut Resolver,
    downloads: &mut Downloader,
    atlas: impl Fn(&str) -> bool,
) -> String {
    if let Some(texture) = profile.get("texture").and_then(|t| t.as_str())
        && let Some(sprite) = patch_skin(texture, downloads, &atlas)
    {
        return sprite;
    }
    let p = resolver.heads.get(profile);
    // Dynamic resolution is deliberately not cached: pending answers and the
    // resolver's expiring service results must still advance on every draw.
    let resolved = p.info.dynamic.as_ref().and_then(|key| {
        if p.info.textures.is_some() {
            return None;
        }
        match resolver.render_state(key) {
            Resolve::Done(Some(profile)) => {
                let (_, value, signature) = profile
                    .properties
                    .iter()
                    .find(|(n, _, _)| n == "textures")?;
                Some(resolver.heads.get_resolved(value, signature.is_some()))
            }
            _ => None,
        }
    });
    if let Some((url, location)) = resolved.as_deref().unwrap_or(&p).skin.as_ref() {
        match downloads.state(Kind::Skin, url) {
            Download::Ready if atlas(location) => {
                downloads.drawn(location);
                return location.clone();
            }
            // Its slot was evicted: offer the image again.
            Download::Ready => {
                downloads.again(location);
            }
            _ => {}
        }
    }
    crate::tab_list::default_skin(p.info.id).to_owned()
}

/// `PlayerSkin.Patch.apply` for a head: the sprite of a profile's `texture`
/// override (`ClientAsset.ResourceTexture`, a resource pack's
/// `<namespace>:textures/<path>.png`), which replaces the downloaded or
/// default skin. Vanilla textures are in the atlas already; another
/// namespace's loads into a skin slot, and `None` answers until it is there
/// (or when the packs lack it).
pub fn patch_skin(
    texture: &str,
    downloads: &mut Downloader,
    atlas: impl Fn(&str) -> bool,
) -> Option<String> {
    let (namespace, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
    if !valid_texture_id(namespace, path) {
        return None;
    }
    if namespace == "minecraft" && atlas(path) {
        return Some(path.to_owned());
    }
    let location = format!("patch/{namespace}/{path}");
    if atlas(&location) {
        downloads.drawn(&location);
        return Some(location);
    }
    downloads.resource(&location, namespace, path);
    None
}

/// `Identifier`'s characters (`isValidNamespace`, `isValidPath`), and no
/// `.`, `..` or empty segment (`FileUtil.decomposePath`, as the packs read
/// it): a server's `texture` never names a file outside the packs.
fn valid_texture_id(namespace: &str, path: &str) -> bool {
    let segment = |s: &str| !s.is_empty() && s != "." && s != "..";
    !namespace.is_empty()
        && namespace
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.'))
        && path
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '/'))
        && segment(namespace)
        && path.split('/').all(segment)
}

/// A `width`x`height` RGBA image as the 64x64 skin it maps onto: each texel
/// the nearest of the image's at the same normalised UV.
fn resample_64(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let mut out = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let (sx, sy) = (x * w / 64, y * h / 64);
            let s = (sy * w + sx) * 4;
            let d = (y * 64 + x) * 4;
            out[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    Some(out)
}

/// `MinecraftProfileTexture.getHash`: the base name of the URL's path.
pub fn texture_hash(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let path = &rest[rest.find('/').unwrap_or(rest.len())..];
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    let name = &path[path.rfind(['/', '\\']).map_or(0, |i| i + 1)..];
    Some(name[..name.rfind('.').unwrap_or(name.len())].to_owned())
}

/// `SkinManager.TextureCache.registerTexture`: the texture id of a skin,
/// `skins/` and the SHA-1 of its hash's UTF-16 code units.
pub fn skin_location(hash: &str) -> String {
    location("skins/", hash)
}

/// The cape's texture id, as [`skin_location`] in `capes/`.
pub fn cape_location(hash: &str) -> String {
    location("capes/", hash)
}

/// `SkinManager.TextureCache` for a profile's own wing texture.
pub fn elytra_location(hash: &str) -> String {
    location("elytra/", hash)
}

fn location(folder: &str, hash: &str) -> String {
    let units: Vec<u8> = hash.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let digest = sha1(&units);
    let mut out = String::from(folder);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `PlayerSkin.cape`: the cape texture of textures whose lookup gave a skin
/// (every download ready), if they name one.
pub fn cape(textures: &Textures) -> Option<String> {
    textures
        .cape
        .as_deref()
        .and_then(texture_hash)
        .map(|hash| cape_location(&hash))
}

/// A cape in a 64x64 skin slot: the 64x32 picture (or the older 22x17,
/// which has the same layout) in its top half. Vanilla draws a cape of any
/// size; a larger one is scaled down to 64x32 here.
pub fn process_cape(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let mut img = vec![0u8; 64 * 64 * 4];
    let (fit_w, fit_h) = if w <= 64 && h <= 32 { (w, h) } else { (64, 32) };
    for y in 0..fit_h {
        for x in 0..fit_w {
            let (sx, sy) = (x * w / fit_w, y * h / fit_h);
            let (s, d) = ((sy * w + sx) * 4, (y * 64 + x) * 4);
            img[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    Some(img)
}

/// The cape the launcher gives an offline name (`HORIZON_OFFLINE_CAPE`, a
/// PNG's path; launcher/src/skins.rs), read once as `process_cape` lays it out.
pub fn offline_cape() -> Option<&'static [u8]> {
    static CAPE: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    CAPE.get_or_init(|| {
        let path = std::env::var_os("HORIZON_OFFLINE_CAPE").filter(|p| !p.is_empty())?;
        let image = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| crate::hud::decode_bytes(&bytes).map_err(|e| e.to_string()))
            .and_then(|(w, h, rgba)| process_cape(w, h, &rgba).ok_or("not an image".into()));
        image
            .inspect_err(|e| eprintln!("Horizon: no offline cape ({}): {e}", path.display()))
            .ok()
    })
    .as_deref()
}

/// The skin the launcher gives an offline name (`HORIZON_OFFLINE_SKIN`, a
/// PNG's path, and `HORIZON_OFFLINE_SKIN_MODEL`, `classic` or `slim`;
/// launcher/src/skins.rs): its 64x64 RGBA as `process_legacy_skin` lays it
/// out, and whether it is slim. Read once.
pub fn offline_skin() -> Option<&'static (Vec<u8>, bool)> {
    static SKIN: std::sync::OnceLock<Option<(Vec<u8>, bool)>> = std::sync::OnceLock::new();
    SKIN.get_or_init(|| {
        let path = std::env::var_os("HORIZON_OFFLINE_SKIN").filter(|p| !p.is_empty())?;
        let model = std::env::var("HORIZON_OFFLINE_SKIN_MODEL").unwrap_or_default();
        read_offline_skin(std::path::Path::new(&path), &model)
            .inspect_err(|e| eprintln!("Horizon: no offline skin ({}): {e}", path.display()))
            .ok()
    })
    .as_ref()
}
fn read_offline_skin(path: &std::path::Path, model: &str) -> Result<(Vec<u8>, bool), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let (w, h, rgba) = crate::hud::decode_bytes(&bytes).map_err(|e| e.to_string())?;
    let skin = process_legacy_skin(w, h, &rgba).ok_or("not a 64x64 or 64x32 skin")?;
    Ok((skin, model == "slim"))
}

/// The signed-in account's own profile properties (`Minecraft.profileFuture`:
/// `fetchProfile(id, true)`, so its `textures` property is signed), or empty
/// without an answer. Blocks on the network.
pub fn fetch_own_properties(id: u128) -> Vec<(String, String, Option<String>)> {
    crate::auth::discovery()
        .ok()
        .and_then(|doc| resolve_profile(&NameOrId::Id(id), &doc))
        .map(|p| p.properties)
        .unwrap_or_default()
}

/// Where a texture download stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download {
    Ready,
    Pending,
    Failed,
}

/// `MinecraftProfileTexture.Type`: which texture cache a download is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Skin,
    Cape,
    Elytra,
}

/// `SkinManager.createLookup(profile, requireSecure).get()`: the skin
/// texture a player shows, or `None` for their default skin. `download`
/// tells how the skin, cape and elytra texture downloads stand.
pub fn lookup(
    textures: &Textures,
    require_secure: bool,
    download: impl FnMut(Kind, &str) -> Download,
) -> Option<String> {
    lookup_locations(textures, require_secure, download).and_then(|s| s.skin)
}

/// The registered textures of one resolved PlayerSkin. A missing SKIN uses
/// the profile UUID's default body/model at the caller; cape and Elytra are
/// still valid in that case (`SkinManager.registerTextures`).
#[derive(Debug)]
pub struct SkinLocations {
    pub skin: Option<String>,
    pub cape: Option<String>,
    pub elytra: Option<String>,
}
pub fn lookup_locations(
    textures: &Textures,
    require_secure: bool,
    mut download: impl FnMut(Kind, &str) -> Download,
) -> Option<SkinLocations> {
    let mut locations = SkinLocations {
        skin: None,
        cape: None,
        elytra: None,
    };
    let mut pending = false;
    let mut failed = false;
    for (kind, url) in [
        (Kind::Skin, &textures.skin),
        (Kind::Cape, &textures.cape),
        (Kind::Elytra, &textures.elytra),
    ] {
        let Some(url) = url else { continue };
        // Native TextureCache.getOrLoad asks getHash before registering the
        // future. A malformed accessory URL fails the complete profile too.
        let hash = texture_hash(url)?;
        match download(kind, url) {
            // Native registerTextures combines all three futures with allOf:
            // an exceptional download falls back to the whole default profile.
            Download::Failed => failed = true,
            Download::Pending => pending = true,
            Download::Ready => match kind {
                Kind::Skin => locations.skin = Some(skin_location(&hash)),
                Kind::Cape => locations.cape = Some(cape_location(&hash)),
                Kind::Elytra => locations.elytra = Some(elytra_location(&hash)),
            },
        }
    }
    if failed || pending || require_secure && !textures.signed {
        return None;
    }
    Some(locations)
}

/// `WingsLayer.getPlayerElytraTexture`: a dedicated texture wins regardless
/// of the CAPE model-part option; only the cape fallback obeys that option.
pub fn wings_texture<'a>(
    elytra: Option<&'a str>,
    cape: Option<&'a str>,
    show_cape: bool,
    available: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    elytra
        .filter(|s| available(s))
        .or_else(|| cape.filter(|s| show_cape && available(s)))
}

/// `SkinTextureDownloader.processLegacySkin` on RGBA pixels: 64x32 skins
/// get the 64x64 layout, and the opaque parts lose their alpha. `None` for
/// any other size (the download fails).
pub fn process_legacy_skin(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width != 64 || height != 32 && height != 64 {
        return None;
    }
    let legacy = height == 32;
    let mut img = vec![0u8; 64 * 64 * 4];
    let n = rgba.len().min(img.len());
    img[..n].copy_from_slice(&rgba[..n]);
    let at = |x: i32, y: i32| ((y * 64 + x) * 4) as usize;
    if legacy {
        let rects = [
            (4, 16, 16, 32, 4, 4),
            (8, 16, 16, 32, 4, 4),
            (0, 20, 24, 32, 4, 12),
            (4, 20, 16, 32, 4, 12),
            (8, 20, 8, 32, 4, 12),
            (12, 20, 16, 32, 4, 12),
            (44, 16, -8, 32, 4, 4),
            (48, 16, -8, 32, 4, 4),
            (40, 20, 0, 32, 4, 12),
            (44, 20, -8, 32, 4, 12),
            (48, 20, -16, 32, 4, 12),
            (52, 20, -8, 32, 4, 12),
        ];
        for (sx, sy, ox, oy, w, h) in rects {
            for y in 0..h {
                for x in 0..w {
                    let (s, d) = (at(sx + x, sy + y), at(sx + ox + w - 1 - x, sy + oy + y));
                    let p: [u8; 4] = img[s..s + 4].try_into().unwrap();
                    img[d..d + 4].copy_from_slice(&p);
                }
            }
        }
    }
    let no_alpha = |img: &mut Vec<u8>, x0: i32, y0: i32, x1: i32, y1: i32| {
        for x in x0..x1 {
            for y in y0..y1 {
                img[at(x, y) + 3] = 255;
            }
        }
    };
    no_alpha(&mut img, 0, 0, 32, 16);
    if legacy {
        // doNotchTransparencyHack: a fully opaque hat layer is cleared.
        let opaque = (32..64).all(|x| (0..32).all(|y| img[at(x, y) + 3] >= 128));
        if opaque {
            for x in 32..64 {
                for y in 0..32 {
                    img[at(x, y) + 3] = 0;
                }
            }
        }
    }
    no_alpha(&mut img, 0, 16, 64, 32);
    no_alpha(&mut img, 16, 48, 48, 64);
    Some(img)
}

/// `SkinTextureDownloader`: fetches textures on background threads, from
/// the local cache (`<root>/<2 hex>/<sha1>`) or over HTTP, keeping each one
/// by kind and hash as vanilla's `TextureCache`s do. A texture that can't
/// be fetched fails, and the player keeps their default skin.
pub struct Downloader {
    root: std::path::PathBuf,
    states: std::collections::HashMap<(Kind, String), Download>,
    done: std::sync::mpsc::Receiver<((Kind, String), Option<Vec<u8>>)>,
    send: std::sync::mpsc::Sender<((Kind, String), Option<Vec<u8>>)>,
    ready: Vec<(String, Vec<u8>)>,
    /// Locations already in `ready`, so `again` does not double-queue.
    queued: std::collections::HashSet<String>,
    /// Decoded skin and cape images by location, kept so a skin evicted from the
    /// atlas uploads again (`again`); the disk cache backs anything dropped.
    images: std::collections::HashMap<String, Vec<u8>>,
    image_order: std::collections::VecDeque<String>,
    /// Skins and capes drawn from the atlas since `take_drawn`.
    drawn: std::collections::HashSet<String>,
    /// References of the entity currently being built, committed only if its
    /// CPU/GPU geometry survives culling. Reused between entities.
    draw_scope: Option<Vec<String>>,
    draw_scratch: Vec<String>,
    /// The resource packs' root, where a profile's `texture` patch is read.
    resource_root: std::path::PathBuf,
    /// How each `patch/` texture of the resource packs loaded, by location.
    resources: std::collections::HashMap<String, Download>,
    workers: Arc<workers::Pool>,
    scope: Arc<AtomicBool>,
    /// When each failed texture download was last tried, so it can be retried:
    /// a transient network error must not pin a player to the default skin for
    /// the rest of the session.
    failed: std::collections::HashMap<(Kind, String), std::time::Instant>,
    /// How long a failed texture download waits before it is tried again.
    retry_after: std::time::Duration,
}
/// `images` is bounded only so a long session on a busy server does not grow
/// without end; the disk cache holds everything it drops.
const MAX_SKIN_IMAGES: usize = 1024;
impl Downloader {
    /// The decoded 64×64 RGBA image an atlas location holds, when it is
    /// still in memory (3D skin layers builds its geometry from it, and the
    /// disk cache backs whatever was dropped).
    pub fn image(&self, location: &str) -> Option<&[u8]> {
        self.images.get(location).map(Vec::as_slice)
    }
    /// Keeps a decoded image under `location` for the images read back later
    /// (a launcher's offline skin, which skips `queue`).
    pub fn remember_image(&mut self, location: &str, rgba: &[u8]) {
        self.remember(location.to_owned(), rgba.to_vec());
    }
    pub fn new(root: std::path::PathBuf) -> Self {
        let (send, done) = std::sync::mpsc::channel();
        Self {
            root,
            states: Default::default(),
            done,
            send,
            ready: Vec::new(),
            queued: Default::default(),
            images: Default::default(),
            image_order: Default::default(),
            drawn: Default::default(),
            draw_scope: None,
            draw_scratch: Vec::new(),
            resource_root: Default::default(),
            resources: Default::default(),
            workers: workers::Pool::new("Skin textures", 4),
            scope: Arc::new(AtomicBool::new(false)),
            failed: Default::default(),
            retry_after: std::time::Duration::from_secs(60),
        }
    }
    pub fn shutdown(&mut self) -> bool {
        self.workers.shutdown()
    }
    pub fn retire_workers(&self) -> WorkHandle {
        self.workers.request_shutdown();
        WorkHandle(self.workers.clone())
    }

    /// The resource packs changed (or were first loaded) at `root`: patch
    /// textures load again from there.
    pub fn set_resource_root(&mut self, root: std::path::PathBuf) {
        let removed: std::collections::HashSet<_> =
            std::mem::take(&mut self.resources).into_keys().collect();
        for location in &removed {
            self.images.remove(location);
        }
        self.image_order
            .retain(|location| !removed.contains(location));
        // A completed old-pack texture may still be waiting for its atlas
        // upload. Leaving it queued would register the old pixels in the new
        // atlas, whose presence then prevents the new pack from being read.
        self.ready
            .retain(|(location, _)| !removed.contains(location));
        self.queued.retain(|location| !removed.contains(location));
        self.drawn.retain(|location| !removed.contains(location));
        if let Some(scope) = &mut self.draw_scope {
            scope.retain(|location| !removed.contains(location));
        }
        self.draw_scratch
            .retain(|location| !removed.contains(location));
        self.resource_root = root;
    }
    /// `ClientAsset.ResourceTexture`: loads `<namespace>:textures/<path>.png`
    /// from the resource packs into a skin slot as `location`, once. A
    /// texture of another size is sampled as the model's normalised UVs would
    /// sample it (nearest texel) into the 64x64 slot.
    fn resource(&mut self, location: &str, namespace: &str, path: &str) {
        match self.resources.get(location) {
            Some(Download::Failed) => return,
            Some(_) => {
                self.again(location);
                return;
            }
            None => {}
        }
        let file = self
            .resource_root
            .join(format!("assets/{namespace}/textures/{path}.png"));
        let image = std::fs::read(file)
            .ok()
            .and_then(|b| crate::hud::decode_bytes(&b).ok())
            .and_then(|(w, h, rgba)| resample_64(w, h, &rgba));
        let state = match image {
            Some(rgba) => {
                self.queue(location.to_owned(), rgba);
                Download::Ready
            }
            None => Download::Failed,
        };
        self.resources.insert(location.to_owned(), state);
    }
    /// How the download of `url` stands, starting it the first time. A
    /// failure is permanent for `retry_after`, then tried again.
    pub fn state(&mut self, kind: Kind, url: &str) -> Download {
        while let Ok((key, image)) = self.done.try_recv() {
            let state = match image {
                Some(rgba) => {
                    match key.0 {
                        Kind::Skin => self.queue(skin_location(&key.1), rgba),
                        Kind::Cape => self.queue(cape_location(&key.1), rgba),
                        Kind::Elytra => self.queue(elytra_location(&key.1), rgba),
                    }
                    self.failed.remove(&key);
                    Download::Ready
                }
                None => {
                    self.failed.insert(key.clone(), std::time::Instant::now());
                    Download::Failed
                }
            };
            self.states.insert(key, state);
        }
        let Some(hash) = texture_hash(url) else {
            return Download::Failed;
        };
        let key = (kind, hash);
        if let Some(state) = self.states.get(&key).copied() {
            let due = self
                .failed
                .get(&key)
                .is_none_or(|at| at.elapsed() >= self.retry_after);
            if state != Download::Failed || !due {
                return state;
            }
            self.states.remove(&key);
            self.failed.remove(&key);
        }
        if self.workers.stopped() {
            return Download::Failed;
        }
        self.states.insert(key.clone(), Download::Pending);
        let digest: String = skin_location(&key.1)["skins/".len()..].to_owned();
        let file = self
            .root
            .join(if digest.len() > 2 { &digest[..2] } else { "xx" })
            .join(&digest);
        let (url, send) = (url.to_owned(), self.send.clone());
        let (canceled_key, canceled_send) = (key.clone(), send.clone());
        self.workers.submit(
            self.scope.clone(),
            move |stopping, scope| {
                let active = || io_active(stopping, scope);
                let image = cached_texture_cancellable(
                    &file,
                    kind,
                    || http_get_cancellable(&url, &active),
                    &active,
                );
                let _ = send.send((key, if active() { image } else { None }));
            },
            move || {
                let _ = canceled_send.send((canceled_key, None));
            },
        );
        Download::Pending
    }
    /// The synchronous cache path used by compatibility fixtures; production
    /// submits the same codec through the owned cancellable IO service.
    #[cfg(test)]
    fn load_texture(
        root: &std::path::Path,
        kind: Kind,
        digest: &str,
        url: &str,
        fetch: impl FnOnce(&str) -> Option<Vec<u8>>,
    ) -> Option<Vec<u8>> {
        let file = root
            .join(if digest.len() > 2 { &digest[..2] } else { "xx" })
            .join(digest);
        cached_texture(&file, kind, || fetch(url))
    }

    /// Keeps a decoded texture for `again`, dropping the oldest past the bound.
    fn remember(&mut self, location: String, rgba: Vec<u8>) {
        if !self.images.contains_key(&location) {
            self.image_order.push_back(location.clone());
            while self.image_order.len() > MAX_SKIN_IMAGES
                && let Some(old) = self.image_order.pop_front()
            {
                self.images.remove(&old);
            }
        }
        self.images.insert(location, rgba);
    }
    /// An image off to the atlas: remembered for `again` and marked queued
    /// so nothing uploads the same location twice.
    fn queue(&mut self, location: String, rgba: Vec<u8>) {
        self.remember(location.clone(), rgba.clone());
        self.queued.insert(location.clone());
        self.ready.push((location, rgba));
    }
    /// A ready skin or cape whose atlas slot was evicted (`show_skin` answered no):
    /// its image for upload again, from memory or the disk cache.
    pub fn again(&mut self, location: &str) -> bool {
        if self.queued.contains(location) {
            self.prioritize(location);
            return true;
        }
        if location == "horizon/offline_skin" || location.starts_with("patch/") {
            let Some(image) = self.images.get(location).cloned() else {
                self.resources.remove(location);
                return false;
            };
            self.queue(location.to_owned(), image);
            self.prioritize(location);
            return true;
        }
        let (kind, digest) = if let Some(digest) = location.strip_prefix("skins/") {
            (Kind::Skin, digest)
        } else if let Some(digest) = location.strip_prefix("capes/") {
            (Kind::Cape, digest)
        } else if let Some(digest) = location.strip_prefix("elytra/") {
            (Kind::Elytra, digest)
        } else {
            return false;
        };
        let image = self.images.get(location).cloned().or_else(|| {
            let file = self
                .root
                .join(if digest.len() > 2 { &digest[..2] } else { "xx" })
                .join(digest);
            cached_texture_cancellable(&file, kind, || None, &|| true)
        });
        let Some(image) = image else {
            return false;
        };
        self.queue(location.to_owned(), image);
        self.prioritize(location);
        true
    }
    /// A texture requested for an actual draw goes ahead of unrelated
    /// completed downloads; the atlas keeps its existing frame pin budget.
    fn prioritize(&mut self, location: &str) {
        if let Some(at) = self.ready.iter().position(|(name, _)| name == location)
            && at != 0
        {
            let ready = self.ready.remove(at);
            self.ready.insert(0, ready);
        }
    }
    /// Skins and capes that finished since the last call: texture id and 64x64 RGBA.
    pub fn take_ready(&mut self) -> Vec<(String, Vec<u8>)> {
        for (location, _) in &self.ready {
            self.queued.remove(location);
        }
        std::mem::take(&mut self.ready)
    }
    /// A skin or cape in the atlas drawn this frame: `SkinSlots` keeps its slot.
    pub fn drawn(&mut self, location: &str) {
        if self.drawn.contains(location) {
            return;
        }
        if let Some(scope) = &mut self.draw_scope {
            if !scope.iter().any(|name| name == location) {
                scope.push(location.to_owned());
            }
        } else {
            self.drawn.insert(location.to_owned());
        }
    }
    /// Capture texture references while an entity's geometry is built.
    pub fn begin_draw(&mut self) {
        debug_assert!(self.draw_scope.is_none());
        self.draw_scope = Some(std::mem::take(&mut self.draw_scratch));
    }
    /// A culled entity must not pin its body, cape or head-item textures.
    pub fn finish_draw(&mut self, submitted: bool) {
        if let Some(mut scope) = self.draw_scope.take() {
            if submitted {
                self.drawn.extend(scope.drain(..));
            } else {
                scope.clear();
            }
            self.draw_scratch = scope;
        }
    }
    /// The skins and capes drawn since the last call.
    pub fn take_drawn(&mut self) -> std::collections::HashSet<String> {
        std::mem::take(&mut self.drawn)
    }
}

/// Which downloaded skins and capes hold the atlas's skin slots
/// (`hud::SKIN_SLOTS`). Vanilla's `SkinManager.TextureCache.getOrLoad`
/// registers a texture per skin and keeps it. Here the slots are shared: a
/// new skin takes a free slot, else the least recently drawn skin's, but
/// never one drawn in the last frame. With every slot drawn, the new skin
/// waits (its player keeps the default skin) instead of pushing out one in
/// use, which then came back by pushing out another, so players vanished
/// for a frame and heads swapped textures as the slots went round.
pub struct SkinSlots {
    capacity: usize,
    /// Stored locations and their slot, the least recently drawn first.
    slots: Vec<(String, usize)>,
    /// Drawn in the last frame, or stored since: kept from eviction.
    kept: std::collections::HashSet<String>,
}
impl SkinSlots {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            slots: Vec::new(),
            kept: Default::default(),
        }
    }
    /// The atlas was replaced (a resource pack reload): every slot is blank,
    /// so no skin counts as stored and each uploads again.
    pub fn forget(&mut self) {
        self.slots.clear();
        self.kept.clear();
    }
    /// Whether `location` is in a slot.
    pub fn contains(&self, location: &str) -> bool {
        self.slots.iter().any(|(l, _)| l == location)
    }
    /// A new frame: the skins `drawn` in the last one
    /// (`Downloader::take_drawn`) become the most recently drawn, and keep
    /// their slots through this frame's `place`s.
    pub fn next_frame(&mut self, drawn: std::collections::HashSet<String>) {
        let (recent, old): (Vec<_>, Vec<_>) = std::mem::take(&mut self.slots)
            .into_iter()
            .partition(|(l, _)| drawn.contains(l));
        self.slots = old;
        self.slots.extend(recent);
        self.kept = drawn;
    }
    /// A slot for `location`: its own, a free one, else the least recently
    /// drawn one not kept, with the location it held. `None` while every
    /// slot is kept.
    pub fn place(&mut self, location: String) -> Option<(usize, Option<String>)> {
        if let Some((_, slot)) = self.slots.iter().find(|(l, _)| *l == location) {
            return Some((*slot, None));
        }
        let (slot, evicted) = if self.slots.len() < self.capacity {
            (self.slots.len(), None)
        } else {
            let i = self
                .slots
                .iter()
                .position(|(l, _)| !self.kept.contains(l))?;
            let (old, slot) = self.slots.remove(i);
            (slot, Some(old))
        };
        self.kept.insert(location.clone());
        self.slots.push((location, slot));
        Some((slot, evicted))
    }
}

/// Reserve a shared skin slot and publish the sprite's UVs for its pixel
/// upload. Used by the game for bodies, capes and dedicated wing textures.
pub fn atlas_upload(
    slots: &mut SkinSlots,
    atlas: &mut crate::hud::HudAtlas,
    location: String,
    rgba: Vec<u8>,
) -> Option<crate::font::Patch> {
    let (slot, evicted) = slots.place(location.clone())?;
    if let Some(old) = evicted {
        atlas.sprites.remove(&old);
    }
    let mut rect = *atlas.sprites.get(&format!("horizon/skin{slot}"))?;
    let patch = crate::font::Patch {
        x: (rect.u0 * atlas.width as f32).round() as u32,
        y: (rect.v0 * atlas.height as f32).round() as u32,
        width: 64,
        height: 64,
        rgba,
    };
    if location.starts_with("capes/") || location.starts_with("elytra/") {
        rect.v1 = (rect.v0 + rect.v1) / 2.0;
        rect.h /= 2;
    }
    atlas.sprites.insert(location, rect);
    Some(patch)
}

impl Drop for Downloader {
    fn drop(&mut self) {
        self.workers.cancel_scope(&self.scope);
        self.workers.request_shutdown();
    }
}

/// Decode before trusting or persisting a cached response: truncated PNGs and HTTP error bodies
/// must not become a permanent default-skin result on subsequent launches.
#[cfg(test)]
fn cached_texture(
    file: &std::path::Path,
    kind: Kind,
    fetch: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    cached_texture_with(file, kind, fetch, &|| true, true)
}
fn cached_texture_cancellable(
    file: &std::path::Path,
    kind: Kind,
    fetch: impl FnOnce() -> Option<Vec<u8>>,
    active: &impl Fn() -> bool,
) -> Option<Vec<u8>> {
    cached_texture_with(file, kind, fetch, active, false)
}
fn cached_texture_with(
    file: &std::path::Path,
    kind: Kind,
    fetch: impl FnOnce() -> Option<Vec<u8>>,
    active: &impl Fn() -> bool,
    keep_elytra_pixels: bool,
) -> Option<Vec<u8>> {
    if !active() {
        return None;
    }
    let decode = |bytes: &[u8]| match kind {
        Kind::Cape => streamed_texture(bytes, kind, active),
        // Wings use the same bounded slot pool as capes, with normalized
        // 64x32 UVs. Never retain a full downloaded canvas in the result queue.
        Kind::Elytra if !keep_elytra_pixels => streamed_texture(bytes, kind, active),
        _ => decode_texture(bytes, kind, |b| crate::hud::decode_bytes(b).ok()),
    };
    if let Some(image) = std::fs::read(file).ok().and_then(|bytes| decode(&bytes)) {
        return Some(image);
    }
    let bytes = fetch()?;
    if !active() {
        return None;
    }
    let image = decode(&bytes)?;
    if !active() {
        return None;
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
        let _ = std::fs::write(file, &bytes);
    }
    Some(image)
}

fn decode_texture(
    bytes: &[u8],
    kind: Kind,
    decode: impl FnOnce(&[u8]) -> Option<(u32, u32, Vec<u8>)>,
) -> Option<Vec<u8>> {
    if matches!(kind, Kind::Cape) {
        return streamed_texture(bytes, kind, &|| true);
    }
    if matches!(kind, Kind::Skin) {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        // Match the existing HUD/NativeImage decoder's lenient CRC policy.
        decoder.ignore_checksums(true);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let reader = decoder.read_info().ok()?;
        if reader.info().width != 64 || !matches!(reader.info().height, 32 | 64) {
            return None;
        }
    }
    let (w, h, rgba) = decode(bytes)?;
    match kind {
        Kind::Skin => process_legacy_skin(w, h, &rgba),
        Kind::Cape => process_cape(w, h, &rgba),
        Kind::Elytra => Some(rgba),
    }
}

/// Read every PNG row, retaining only the texels the cape atlas actually uses.
/// The decoder still validates the whole first frame, including rows outside
/// the sampled cape area. A failed/unfinished response must not enter the cache.
/// Elytra uses the whole source image at normalized UVs; unlike the legacy
/// cape layout, small wing images fill the slot rather than being padded.
fn streamed_texture(bytes: &[u8], kind: Kind, active: &impl Fn() -> bool) -> Option<Vec<u8>> {
    debug_assert!(matches!(kind, Kind::Cape | Kind::Elytra));
    if !active() {
        return None;
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.ignore_checksums(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    // Preserve NativeImage's existing accepted image-size range without
    // allocating a full image (or imposing a smaller cape dimension limit).
    const MAX_DECODE_BYTES: usize = 256 << 20;
    reader
        .output_buffer_size()
        .filter(|&n| n <= MAX_DECODE_BYTES)?;
    let info = reader.info();
    let (w, h) = info
        .frame_control
        .as_ref()
        .map_or((info.width, info.height), |f| (f.width, f.height));
    let (w, h) = (w as usize, h as usize);
    w.checked_mul(h)?
        .checked_mul(4)
        .filter(|&n| n <= MAX_DECODE_BYTES)?;
    let interlaced = info.interlaced;
    let channels = reader.output_color_type().0.samples();
    let mut result = vec![0; 64 * 64 * 4];
    let (fit_w, fit_h) = if kind == Kind::Cape && w <= 64 && h <= 32 {
        (w, h)
    } else {
        (64, 32)
    };
    // PNG's seven Adam7 passes, expressed as first x/y and x/y strides. Reading
    // the passes lets us select the same nearest source texels as process_cape
    // without expanding the interlaced image into a width*height allocation.
    let passes: &[(usize, usize, usize, usize)] = if interlaced {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    } else {
        &[(0, 0, 1, 1)]
    };
    for &(first_x, first_y, step_x, step_y) in passes {
        if first_x >= w {
            continue;
        }
        for source_y in (first_y..h).step_by(step_y) {
            if !active() {
                return None;
            }
            let row = reader.next_row().ok()??;
            for y in 0..fit_h {
                let sample_y = if kind == Kind::Elytra {
                    (2 * y + 1) * h / (2 * fit_h)
                } else {
                    y * h / fit_h
                };
                if sample_y != source_y {
                    continue;
                }
                for x in 0..fit_w {
                    let source_x = if kind == Kind::Elytra {
                        (2 * x + 1) * w / (2 * fit_w)
                    } else {
                        x * w / fit_w
                    };
                    if source_x < first_x || !(source_x - first_x).is_multiple_of(step_x) {
                        continue;
                    }
                    let index = (source_x - first_x) / step_x * channels;
                    let pixel = row.data().get(index..index + channels)?;
                    let rgba = match channels {
                        4 => [pixel[0], pixel[1], pixel[2], pixel[3]],
                        3 => [pixel[0], pixel[1], pixel[2], 255],
                        2 => [pixel[0], pixel[0], pixel[0], pixel[1]],
                        _ => [pixel[0], pixel[0], pixel[0], 255],
                    };
                    let dest = (y * 64 + x) * 4;
                    result[dest..dest + 4].copy_from_slice(&rgba);
                }
            }
        }
    }
    // Flush and validate the remaining IDAT stream, just as next_frame does.
    if !active() || reader.next_row().ok()?.is_some() {
        return None;
    }
    Some(result)
}

/// A GET over HTTP or HTTPS (textures.minecraft.net serves both; vanilla
/// uses https): the body of a 2xx answer, 16 MiB at most, after redirects.
#[cfg(test)]
fn http_get(url: &str) -> Option<Vec<u8>> {
    http_get_cancellable(url, &|| true)
}
fn http_get_cancellable(url: &str, active: &impl Fn() -> bool) -> Option<Vec<u8>> {
    use std::io::Read;
    if !active() {
        return None;
    }
    let response = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Java")
        .build()
        .get(url)
        .call()
        .ok()?;
    if !active() {
        return None;
    }
    let mut body = Vec::new();
    response
        .into_reader()
        .take(16 << 20)
        .read_to_end(&mut body)
        .ok()?;
    active().then_some(body)
}

/// SHA-1 (FIPS 180-4).
/// Ely.by's skin system: a skin from a player's name alone, with no account and no signature, so offline
/// players (single player, LAN, offline-mode servers) wear the skin they chose there. Ely.by answers with its
/// own skins, or Mojang's for a name it doesn't hold, and `204 No Content` for neither.
pub const ELY_TEXTURES: &str = "https://skinsystem.ely.by/textures/";

/// The textures of an Ely.by `textures/<name>` answer: `{"SKIN":{"url":…,"metadata":{"model":"slim"}},"CAPE":…}`.
/// Counted as signed: the game asked Ely.by for them itself, so nobody else chose them.
pub fn ely_textures(body: &str) -> Textures {
    let mut out = Textures {
        signed: true,
        ..Textures::default()
    };
    let Ok(serde_json::Value::Object(json)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return Textures::default();
    };
    for (kind, texture) in json {
        let Some(url) = texture.get("url").and_then(|u| u.as_str()) else {
            continue;
        };
        // Ely.by names its own files over plain HTTP; the same address answers over HTTPS.
        let url = match url.strip_prefix("http://ely.by/") {
            Some(rest) => format!("https://ely.by/{rest}"),
            None => url.to_owned(),
        };
        match kind.as_str() {
            "SKIN" => {
                out.slim = texture
                    .get("metadata")
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
                    == Some("slim");
                out.skin = Some(url);
            }
            "CAPE" => out.cape = Some(url),
            _ => {}
        }
    }
    out
}

/// The Mojang account `name`'s textures: its profile id, then the session server's signed profile. Counted
/// as signed for the same reason as Ely.by's.
fn mojang_textures_cancellable(name: &str, active: &impl Fn() -> bool) -> Option<Textures> {
    let parse = |body: Vec<u8>| serde_json::from_slice::<serde_json::Value>(&body).ok();
    let id = parse(http_get_cancellable(
        &format!("{MOJANG_NAMES}{name}"),
        active,
    )?)?;
    let profile = parse(http_get_cancellable(
        &format!("{MOJANG_PROFILES}{}", id.get("id")?.as_str()?),
        active,
    )?)?;
    mojang_profile_textures(&profile)
}

/// The textures of a session server profile (`{"properties":[{"name":"textures","value":…}]}`).
fn mojang_profile_textures(profile: &serde_json::Value) -> Option<Textures> {
    let value = profile
        .get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))?
        .get("value")?
        .as_str()?;
    Some(unpack(value, Signature::Signed, &TEXTURE_URIS)).filter(|t| t.skin.is_some())
}

const MOJANG_NAMES: &str = "https://api.mojang.com/users/profiles/minecraft/";
const MOJANG_PROFILES: &str = "https://sessionserver.mojang.com/session/minecraft/profile/";

/// Whether `name` is one Ely.by could hold, and safe to put in its URL as it is.
fn ely_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The Ely.by lookups of one connection: each player asked once, on a thread of its own, and the textures
/// found handed back by [`ElySkins::take`].
pub struct ElySkins {
    workers: Arc<workers::Pool>,
    scope: Arc<AtomicBool>,
    asked: std::collections::HashSet<u128>,
    send: std::sync::mpsc::Sender<(u128, Textures)>,
    done: std::sync::mpsc::Receiver<(u128, Textures)>,
}

impl Default for ElySkins {
    fn default() -> Self {
        Self::with_workers(workers::Pool::new("Ely profiles", 2))
    }
}
impl ElySkins {
    fn with_workers(workers: Arc<workers::Pool>) -> Self {
        let (send, done) = std::sync::mpsc::channel();
        Self {
            workers,
            scope: Arc::new(AtomicBool::new(false)),
            asked: Default::default(),
            send,
            done,
        }
    }
    /// Ask Ely.by once for this session, using the game's owned IO service.
    pub fn ask(&mut self, uuid: u128, name: &str) {
        if !ely_name(name) || !self.asked.insert(uuid) {
            return;
        }
        let (url, send, name) = (
            format!("{ELY_TEXTURES}{name}"),
            self.send.clone(),
            name.to_owned(),
        );
        self.workers.submit(
            self.scope.clone(),
            move |stopping, scope| {
                let active = || io_active(stopping, scope);
                let ely = http_get_cancellable(&url, &active)
                    .map(|body| ely_textures(&String::from_utf8_lossy(&body)))
                    .filter(|t| t.skin.is_some());
                if let Some(textures) = ely.or_else(|| mojang_textures_cancellable(&name, &active))
                    && active()
                {
                    let _ = send.send((uuid, textures));
                }
            },
            || {},
        );
    }
    /// The skins found since the last call.
    pub fn take(&mut self) -> Vec<(u128, Textures)> {
        self.done.try_iter().collect()
    }
}

impl Drop for ElySkins {
    fn drop(&mut self) {
        self.workers.cancel_scope(&self.scope);
    }
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    use sha1::{Digest, Sha1};
    Sha1::digest(data).into()
}

#[cfg(test)]
mod tests {
    fn io_test_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[53, 91, 127, 255].repeat((width * height) as usize))
                .unwrap();
            writer.finish().unwrap();
        }
        bytes
    }

    #[test]
    fn unsupported_skin_canvas_is_rejected_before_full_pixel_decode() {
        use super::{Kind, decode_texture};
        let large = io_test_png(1024, 1024);
        assert!(
            decode_texture(&large, Kind::Skin, |_| panic!(
                "full pixel decode before skin dimensions check"
            ))
            .is_none()
        );
        // It is a valid image the ordinary HUD decoder would allocate in full.
        let (width, height, rgba) = crate::hud::decode_bytes(&large).unwrap();
        assert_eq!((width, height, rgba.len()), (1024, 1024, 4 * 1024 * 1024));
        for height in [32, 64] {
            let valid = io_test_png(64, height);
            assert_eq!(
                decode_texture(&valid, Kind::Skin, |b| crate::hud::decode_bytes(b).ok())
                    .unwrap()
                    .len(),
                64 * 64 * 4
            );
        }
        let mut unchecked_crc = io_test_png(64, 64);
        unchecked_crc[29] ^= 1; // The existing decoder accepts an incorrect IHDR CRC.
        assert!(
            decode_texture(&unchecked_crc, Kind::Skin, |b| crate::hud::decode_bytes(b)
                .ok())
            .is_some()
        );
        // Do not introduce the skin's fixed-size rule for capes or elytras.
        let cape = io_test_png(128, 64);
        assert!(decode_texture(&cape, Kind::Cape, |b| crate::hud::decode_bytes(b).ok()).is_some());
        assert!(
            decode_texture(&large, Kind::Elytra, |b| crate::hud::decode_bytes(b).ok()).is_some()
        );
    }

    #[test]
    fn streamed_capes_preserve_pixels_for_color_and_interlace_formats() {
        use std::io::Write;
        fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(data);
            // Test the same lenient checksums as the ordinary PNG path.
            bytes.extend_from_slice(&[0; 4]);
        }
        fn interlaced(w: u32, h: u32) -> Vec<u8> {
            let mut raw = Vec::new();
            // Derive each pixel's Adam7 pass from the public 8x8 pattern,
            // independently of the production pass-stride traversal.
            let pass_at = [
                [1, 6, 4, 6, 2, 6, 4, 6],
                [7, 7, 7, 7, 7, 7, 7, 7],
                [5, 6, 5, 6, 5, 6, 5, 6],
                [7, 7, 7, 7, 7, 7, 7, 7],
                [3, 6, 4, 6, 3, 6, 4, 6],
                [7, 7, 7, 7, 7, 7, 7, 7],
                [5, 6, 5, 6, 5, 6, 5, 6],
                [7, 7, 7, 7, 7, 7, 7, 7],
            ];
            for pass in 1..=7 {
                for y in 0..h {
                    let mut row = Vec::new();
                    for x in 0..w {
                        if pass_at[y as usize % 8][x as usize % 8] == pass {
                            row.extend_from_slice(&[
                                x as u8,
                                y as u8,
                                (x ^ y) as u8,
                                (x + y) as u8,
                            ]);
                        }
                    }
                    if !row.is_empty() {
                        raw.push(0); // PNG filter None.
                        raw.extend_from_slice(&row);
                    }
                }
            }
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            z.write_all(&raw).unwrap();
            let mut result = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut header = Vec::new();
            header.extend_from_slice(&w.to_be_bytes());
            header.extend_from_slice(&h.to_be_bytes());
            header.extend_from_slice(&[8, 6, 0, 0, 1]);
            chunk(&mut result, b"IHDR", &header);
            chunk(&mut result, b"IDAT", &z.finish().unwrap());
            chunk(&mut result, b"IEND", &[]);
            result
        }
        let mut cases: Vec<_> = [(1, 1), (22, 17), (37, 19), (128, 64), (1024, 257)]
            .into_iter()
            .map(|(w, h)| interlaced(w, h))
            .collect();
        for color in [
            png::ColorType::Grayscale,
            png::ColorType::GrayscaleAlpha,
            png::ColorType::Rgb,
            png::ColorType::Rgba,
            png::ColorType::Indexed,
        ] {
            for depth in [png::BitDepth::Eight, png::BitDepth::Sixteen] {
                if color == png::ColorType::Indexed && depth == png::BitDepth::Sixteen {
                    continue;
                }
                let mut bytes = Vec::new();
                let mut encoder = png::Encoder::new(&mut bytes, 127, 63);
                encoder.set_color(color);
                encoder.set_depth(depth);
                if color == png::ColorType::Indexed {
                    encoder.set_palette(vec![0, 7, 9, 31, 127, 253]);
                    encoder.set_trns(vec![0, 137]);
                }
                let mut writer = encoder.write_header().unwrap();
                let samples = 127
                    * 63
                    * color.samples()
                    * if depth == png::BitDepth::Sixteen {
                        2
                    } else {
                        1
                    };
                let pixels: Vec<_> = (0..samples)
                    .map(|n| {
                        if color == png::ColorType::Indexed {
                            (n % 2) as u8
                        } else {
                            (n % 251) as u8
                        }
                    })
                    .collect();
                writer.write_image_data(&pixels).unwrap();
                writer.finish().unwrap();
                cases.push(bytes);
            }
        }
        for depth in [png::BitDepth::One, png::BitDepth::Two, png::BitDepth::Four] {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, 127, 63);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(depth);
            let mut writer = encoder.write_header().unwrap();
            let samples = (127usize * depth as usize).div_ceil(8) * 63;
            writer
                .write_image_data(&(0..samples).map(|n| (n % 251) as u8).collect::<Vec<_>>())
                .unwrap();
            writer.finish().unwrap();
            cases.push(bytes);
        }
        for bytes in cases {
            let (w, h, rgba) = crate::hud::decode_bytes(&bytes).unwrap();
            let expected = super::process_cape(w, h, &rgba).unwrap();
            assert_eq!(
                super::streamed_texture(&bytes, super::Kind::Cape, &|| true),
                Some(expected)
            );
            let wing = super::streamed_texture(&bytes, super::Kind::Elytra, &|| true).unwrap();
            assert_eq!(wing.len(), 64 * 64 * 4);
            for y in 0..32usize {
                for x in 0..64usize {
                    let source_x = (2 * x + 1) * w as usize / 128;
                    let source_y = (2 * y + 1) * h as usize / 64;
                    let source = (source_y * w as usize + source_x) * 4;
                    assert_eq!(&wing[(y * 64 + x) * 4..][..4], &rgba[source..source + 4]);
                }
            }
            assert!(wing[32 * 64 * 4..].iter().all(|p| *p == 0));
        }
    }

    #[test]
    fn streamed_texture_validates_unsampled_rows_and_cancels_before_publication() {
        use std::cell::Cell;
        let mut bytes = io_test_png(128, 128);
        // Keep the header readable but cut off the end of the compressed pixel
        // stream. Reading just the 32 sampled rows would wrongly accept it.
        let length = u32::from_be_bytes(bytes[33..37].try_into().unwrap()) as usize;
        bytes.truncate(41 + length / 2);
        assert!(super::streamed_texture(&bytes, super::Kind::Cape, &|| true).is_none());
        assert!(super::streamed_texture(&bytes, super::Kind::Elytra, &|| true).is_none());
        let valid = io_test_png(128, 128);
        for kind in [super::Kind::Cape, super::Kind::Elytra] {
            let rows = Cell::new(0);
            assert!(
                super::streamed_texture(&valid, kind, &|| {
                    rows.set(rows.get() + 1);
                    rows.get() < 4
                })
                .is_none()
            );
            assert_eq!(rows.get(), 4);
        }
    }

    #[test]
    fn retired_downloader_keeps_blocked_http_owned_without_publishing_cache() {
        use super::{Downloader, Kind};
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::mpsc,
            time::{Duration, Instant},
        };
        let root =
            std::env::temp_dir().join(format!("horizon-skin-http-shutdown-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/texture/repro", listener.local_addr().unwrap());
        let bytes = io_test_png(64, 64);
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
                if header.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            entered.send(()).unwrap();
            release_rx.recv().unwrap();
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                bytes.len()
            );
            let _ = socket.write_all(&bytes);
        });
        let mut downloader = Downloader::new(root.clone());
        assert_eq!(downloader.state(Kind::Skin, &url), super::Download::Pending);
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let retained = downloader.retire_workers();
        let (dropped, dropped_rx) = mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(downloader);
            dropped.send(()).unwrap();
        });
        let released_owner = dropped_rx.recv_timeout(Duration::from_secs(2));
        let pending = retained.shutdown();
        release.send(()).unwrap();
        server.join().unwrap();
        dropper.join().unwrap();
        assert!(
            released_owner.is_ok(),
            "retiring a guest joined its blocked skin request"
        );
        assert!(!pending, "the response was still in flight");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !retained.shutdown() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            0,
            "a canceled texture populated the cache"
        );
        assert!(retained.shutdown());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remote_ely_scope_drop_does_not_join_or_stop_the_shared_game_service() {
        use super::Resolver;
        use std::{
            sync::{Arc, Condvar, Mutex, mpsc},
            time::{Duration, Instant},
        };
        let mut resolver = Resolver::new();
        resolver.workers = super::workers::Pool::new("Ely shared test", 1);
        let ely = resolver.ely_skins();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (entered, entered_rx) = mpsc::channel();
        for _ in 0..1 {
            let (gate, entered) = (gate.clone(), entered.clone());
            ely.workers.submit(
                ely.scope.clone(),
                move |_, _| {
                    entered.send(()).unwrap();
                    let mut open = gate.0.lock().unwrap();
                    while !*open {
                        open = gate.1.wait(open).unwrap();
                    }
                },
                || {},
            );
        }
        for _ in 0..1 {
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        let stale_fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let fetched = stale_fetches.clone();
        ely.workers.submit(
            ely.scope.clone(),
            move |_, _| {
                fetched.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
            || {},
        );
        let (ready, ready_rx) = mpsc::channel();
        resolver.workers.submit(
            resolver.scope.clone(),
            move |_, _| {
                ready.send(()).unwrap();
            },
            || {},
        );
        let (dropped, dropped_rx) = mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(ely);
            dropped.send(()).unwrap();
        });
        let released = dropped_rx.recv_timeout(Duration::from_secs(2));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        dropper.join().unwrap();
        assert!(
            released.is_ok(),
            "remote drop joined the game's still-active workers"
        );
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(stale_fetches.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(!resolver.workers.stopped());
        let deadline = Instant::now() + Duration::from_secs(3);
        while !resolver.shutdown() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(resolver.shutdown());
    }

    #[test]
    fn a_live_profile_future_survives_pressure_and_expiry_without_retries() {
        use super::{Entry, NameOrId, ProfileLookup, Resolve, ResolvedProfile, Resolver};
        let mut cache = Resolver::new();
        let now = std::time::Instant::now();
        let mut callers = Vec::new();
        for id in 0..501 {
            let lookup = ProfileLookup::default();
            cache.entries.insert(
                NameOrId::Id(id),
                Entry {
                    request: id as u64 + 1,
                    touched: now - std::time::Duration::from_micros(501 - id as u64),
                    lookup: lookup.clone(),
                },
            );
            callers.push(lookup);
        }
        cache.last_sweep = now - std::time::Duration::from_secs(31);
        cache.sweep();
        assert_eq!(cache.entries.len(), super::MAX_PROFILES);
        assert!(!cache.entries.contains_key(&NameOrId::Id(0)));
        // A worker settles exactly the future the live entity captured, not
        // whichever entry now happens to have the same shared-cache key.
        callers[0].complete(Some(ResolvedProfile {
            id: 0,
            name: "VisibleNPC".into(),
            properties: vec![("textures".into(), "custom-skin".into(), None)],
        }));
        cache
            .send
            .send((NameOrId::Id(0), 1, callers[0].clone()))
            .unwrap();
        cache.poll();
        assert!(
            !cache.entries.contains_key(&NameOrId::Id(0)),
            "no cache resurrection"
        );
        callers[0].complete(None); // A future's settled answer cannot roll back.
        for entry in cache.entries.values_mut() {
            entry.touched = now - super::PROFILE_TTL;
        }
        cache.last_sweep = now - std::time::Duration::from_secs(31);
        cache.sweep();
        let Resolve::Done(Some(profile)) = callers[0].state() else {
            panic!("the caller lost its ready skin");
        };
        assert_eq!(profile.properties[0].1, "custom-skin");
        assert!(cache.entries.is_empty());
        assert_eq!(
            cache.next_request, 0,
            "polling a held future starts no replacement fetch"
        );
    }

    #[test]
    fn profile_completions_only_settle_their_current_request() {
        use super::{Entry, NameOrId, ProfileLookup, Resolve, ResolvedProfile, Resolver};
        let key = NameOrId::Name("NPC".into());
        let profile = |value: &str| ResolvedProfile {
            id: 1,
            name: "NPC".into(),
            properties: vec![("textures".into(), value.into(), None)],
        };
        let mut cache = Resolver::new();
        let now = std::time::Instant::now();
        cache.entries.insert(
            key.clone(),
            Entry {
                request: 1,
                touched: now - super::PROFILE_TTL,
                lookup: ProfileLookup::default(),
            },
        );
        cache.last_sweep = now - std::time::Duration::from_secs(31);
        cache.sweep();
        assert!(
            !cache.entries.contains_key(&key),
            "the first fetch was evicted"
        );
        // The same profile was requested again before the first fetch ended.
        cache.entries.insert(
            key.clone(),
            Entry {
                request: 2,
                touched: now,
                lookup: ProfileLookup::default(),
            },
        );
        cache
            .send
            .send((
                key.clone(),
                1,
                ProfileLookup::completed(Some(profile("old"))),
            ))
            .unwrap();
        assert!(matches!(cache.state(&key), Resolve::Pending));
        cache
            .send
            .send((
                key.clone(),
                2,
                ProfileLookup::completed(Some(profile("new"))),
            ))
            .unwrap();
        let Resolve::Done(Some(current)) = cache.state(&key) else {
            panic!("the current worker must resolve its profile");
        };
        assert_eq!(current.properties[0].1, "new");
        // A late old completion must not roll a ready newer skin back either.
        cache
            .send
            .send((
                key.clone(),
                1,
                ProfileLookup::completed(Some(profile("old"))),
            ))
            .unwrap();
        let Resolve::Done(Some(current)) = cache.state(&key) else {
            panic!("the current resolved profile must stay ready");
        };
        assert_eq!(current.properties[0].1, "new");
    }

    #[test]
    fn profile_completions_keep_capacity_evictions_and_current_negative_results() {
        use super::{Entry, NameOrId, ProfileLookup, Resolve, Resolver};
        let key = NameOrId::Id(7);
        let mut cache = Resolver::new();
        let now = std::time::Instant::now();
        cache.entries.insert(
            key.clone(),
            Entry {
                request: 1,
                touched: now - std::time::Duration::from_secs(1),
                lookup: ProfileLookup::default(),
            },
        );
        for id in 100..100 + super::MAX_PROFILES {
            cache.entries.insert(
                NameOrId::Id(id as u128),
                Entry {
                    request: 1,
                    touched: now,
                    lookup: ProfileLookup::completed(None),
                },
            );
        }
        cache.last_sweep = now - std::time::Duration::from_secs(31);
        cache.sweep();
        assert_eq!(cache.entries.len(), super::MAX_PROFILES);
        assert!(!cache.entries.contains_key(&key));
        cache.entries.insert(
            key.clone(),
            Entry {
                request: 2,
                touched: now,
                lookup: ProfileLookup::default(),
            },
        );
        cache
            .send
            .send((key.clone(), 1, ProfileLookup::completed(None)))
            .unwrap();
        assert!(matches!(cache.state(&key), Resolve::Pending));
        cache
            .send
            .send((key.clone(), 2, ProfileLookup::completed(None)))
            .unwrap();
        assert!(matches!(cache.state(&key), Resolve::Done(None)));
        // A genuine missing-profile answer remains cached, as before; this
        // repair does not introduce a new per-frame network retry policy.
        assert!(matches!(cache.state(&key), Resolve::Done(None)));
    }

    #[test]
    fn profile_completions_do_not_revive_evicted_entries() {
        use super::{Entry, NameOrId, ProfileLookup, Resolve, Resolver};
        let key = NameOrId::Id(7);
        let mut cache = Resolver::new();
        cache.entries.insert(
            key.clone(),
            Entry {
                request: 1,
                touched: std::time::Instant::now(),
                lookup: ProfileLookup::default(),
            },
        );
        cache.entries.remove(&key);
        cache
            .send
            .send((key.clone(), 1, ProfileLookup::completed(None)))
            .unwrap();
        // An unrelated invalid-name lookup drains completions without HTTP.
        assert!(matches!(
            cache.state(&NameOrId::Name("invalid name".into())),
            Resolve::Done(None)
        ));
        assert!(!cache.entries.contains_key(&key));
    }

    #[test]
    fn changing_resource_roots_cannot_upload_old_patch_pixels() {
        use super::{Downloader, patch_skin};
        use std::collections::HashMap;
        let root = std::env::temp_dir().join(format!(
            "horizon-skin-root-generation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = root.join("old");
        let new = root.join("new");
        let write = |pack: &std::path::Path, name: &str, colour: [u8; 4]| {
            let file = pack.join(format!("assets/server/textures/entity/{name}.png"));
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&colour.repeat(64 * 64))
                    .unwrap();
            }
            std::fs::write(file, bytes).unwrap();
        };
        write(&old, "npc", [255, 0, 0, 255]);
        write(&old, "other", [255, 0, 0, 255]);
        write(&new, "npc", [0, 0, 255, 255]);
        let patch = "patch/server/entity/npc";
        let other = "patch/server/entity/other";
        let mut downloads = Downloader::new(root.join("url-cache"));
        downloads.set_resource_root(old);
        assert!(patch_skin("server:entity/npc", &mut downloads, |_| false).is_none());
        assert!(patch_skin("server:entity/other", &mut downloads, |_| false).is_none());
        downloads.queue("skins/url".into(), vec![11]);
        downloads.queue("horizon/offline_skin".into(), vec![22]);
        downloads.drawn(patch);
        downloads.drawn("skins/url");
        downloads.drawn("horizon/offline_skin");
        downloads.begin_draw();
        downloads.drawn(other);
        downloads.set_resource_root(new);
        downloads.finish_draw(true);
        let kept = downloads.take_drawn();
        assert!(!kept.contains(patch) && !kept.contains(other));
        assert!(kept.contains("skins/url") && kept.contains("horizon/offline_skin"));
        // The atlas has just been rebuilt. Old queued red pixels must not
        // register a sprite that would bypass reading the new pack below.
        let mut atlas: HashMap<_, _> = downloads.take_ready().into_iter().collect();
        assert!(!atlas.contains_key(patch) && !atlas.contains_key(other));
        assert_eq!(atlas["skins/url"], vec![11]);
        assert_eq!(atlas["horizon/offline_skin"], vec![22]);
        assert!(
            patch_skin("server:entity/npc", &mut downloads, |name| atlas
                .contains_key(name))
            .is_none()
        );
        atlas.extend(downloads.take_ready());
        assert_eq!(&atlas[patch][..4], &[0, 0, 255, 255]);
        assert_eq!(&downloads.image(patch).unwrap()[..4], &[0, 0, 255, 255]);
        assert_eq!(
            patch_skin("server:entity/npc", &mut downloads, |name| atlas
                .contains_key(name))
            .as_deref(),
            Some(patch)
        );
        assert!(downloads.take_drawn().contains(patch));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn head_profile_cache_matches_uncached_and_is_bounded() {
        use base64::Engine;
        let mut cache = super::HeadProfiles::default();
        for (url, slim) in [
            ("https://textures.minecraft.net/texture/abc", false),
            ("https://textures.minecraft.net/texture/abc", true),
            ("https://evil.example/texture/abc", false),
        ] {
            let payload = serde_json::json!({"textures": {
                "SKIN": {"url": url, "metadata": {"model": if slim { "slim" } else { "classic" }}},
                "CAPE": {"url": "http://textures.minecraft.net/texture/cape"}
            }});
            let value = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
            for signed in [false, true] {
                let mut property = serde_json::json!({"name": "textures", "value": value});
                if signed {
                    property["signature"] = serde_json::Value::Null;
                }
                let profile = serde_json::json!({"properties": [property]});
                let parsed = cache.get(&profile);
                assert_eq!(*parsed, super::prepare_head(&profile));
                let resolved = cache.get_resolved(&value, signed);
                assert_eq!(*resolved, super::prepare_head(&profile));
                assert!(std::sync::Arc::ptr_eq(
                    &resolved,
                    &cache.get_resolved(&value, signed)
                ));
                assert!(!std::sync::Arc::ptr_eq(
                    &cache.get_resolved(&value, false),
                    &cache.get_resolved(&value, true)
                ));
                let textures = super::unpack(
                    &value,
                    if signed {
                        super::Signature::Signed
                    } else {
                        super::Signature::Unsigned
                    },
                    &super::TEXTURE_URIS,
                );
                assert_eq!(textures.slim, slim && !url.contains("evil"));
                assert_eq!(textures.cape.is_some(), !url.contains("evil"));
                assert_eq!(parsed.skin.is_some(), !url.contains("evil"));
                assert!(std::sync::Arc::ptr_eq(&parsed, &cache.get(&profile)));
            }
        }
        for profile in [
            serde_json::json!({"properties": [{"name": "textures", "value": "!bad"}]}),
            serde_json::json!({"name": "Notch"}),
            serde_json::json!({}),
        ] {
            let parsed = cache.get(&profile);
            assert_eq!(*parsed, super::prepare_head(&profile));
            assert!(parsed.skin.is_none());
            assert!(std::sync::Arc::ptr_eq(&parsed, &cache.get(&profile)));
        }
        cache.entries.clear();
        for i in 0..super::MAX_HEAD_PROFILES {
            cache.get(&serde_json::json!({"id": i}));
        }
        assert_eq!(cache.entries.len(), super::MAX_HEAD_PROFILES);
        cache.get(&serde_json::json!({}));
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn head_profile_nbt_cache_tracks_value_changes() {
        let mut resolver = super::Resolver::new();
        let mut tag = horizon_world::nbt::Tag::String("Notch".into());
        let first = resolver.head_profile_nbt(&tag);
        assert_eq!(*first, crate::scoreboard::tag_to_json(&tag));
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &resolver.head_profile_nbt(&tag)
        ));
        tag = horizon_world::nbt::Tag::String("Alex".into());
        assert_eq!(*resolver.head_profile_nbt(&tag), serde_json::json!("Alex"));
        for i in 0..super::MAX_HEAD_PROFILES {
            resolver.head_profile_nbt(&horizon_world::nbt::Tag::Int(i as i32));
        }
        assert!(resolver.heads.nbt.len() < super::MAX_HEAD_PROFILES);
    }

    #[test]
    fn memoized_dynamic_heads_keep_the_render_future_after_service_cache_eviction() {
        use super::{
            Download, Downloader, Kind, NameOrId, ProfileLookup, RenderEntry, ResolvedProfile,
            Resolver,
        };
        use base64::Engine;
        let mut resolver = Resolver::new();
        // No service requests are permitted in this test, including a mistaken
        // restart after the shared service cache has evicted this head.
        assert!(resolver.shutdown());
        let lookup = ProfileLookup::default();
        resolver.render_entries.insert(
            NameOrId::Name("VisibleHead".into()),
            RenderEntry {
                touched: std::time::Instant::now(),
                lookup: lookup.clone(),
            },
        );
        let profile = serde_json::json!({"name":"VisibleHead"});
        let mut downloads = Downloader::new(std::env::temp_dir().join("horizon-head-cache-no-io"));
        downloads
            .states
            .insert((Kind::Skin, "abc".into()), Download::Ready);
        let fallback = crate::tab_list::default_skin(super::parse_profile(&profile).id);
        assert_eq!(
            super::head_skin(&profile, &mut resolver, &mut downloads, |_| true),
            fallback
        );
        let value = base64::engine::general_purpose::STANDARD.encode(
            serde_json::json!({"textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/abc"}}}).to_string()
        );
        lookup.complete(Some(ResolvedProfile {
            id: 99,
            name: "VisibleHead".into(),
            properties: vec![("textures".into(), value, None)],
        }));
        assert_eq!(
            super::head_skin(&profile, &mut resolver, &mut downloads, |_| true),
            super::skin_location("abc")
        );
        assert_eq!(
            resolver.next_request, 0,
            "head rendering restarted an evicted service request"
        );
        assert_eq!(resolver.heads.entries.len(), 1);
        assert_eq!(resolver.heads.resolved[0].len(), 1);
        assert!(resolver.entries.is_empty());
    }

    /// A profile's `texture` names a pack texture, never a file outside the
    /// packs: bad characters and `..` segments draw nothing.
    #[test]
    fn a_texture_patch_stays_inside_the_packs() {
        use super::valid_texture_id;
        assert!(valid_texture_id("ethereal", "entity/plush_blank"));
        assert!(valid_texture_id("minecraft", "entity/player/wide/steve"));
        assert!(!valid_texture_id("ethereal", "../../../secret"));
        assert!(!valid_texture_id("..", "entity/x"));
        assert!(!valid_texture_id("ethereal", "entity//x"));
        assert!(!valid_texture_id("Ethereal", "entity/x"));
        assert!(!valid_texture_id("ethereal", "entity\\x"));
        assert!(!valid_texture_id("", "entity/x"));
    }

    /// A pack reload swaps the atlas: its skin slots are blank, so a forgotten
    /// table holds nothing and hands out slots from the start again, even for
    /// skins drawn in the last frame.
    #[test]
    fn a_new_atlas_forgets_every_skin_slot() {
        use super::SkinSlots;
        let mut slots = SkinSlots::new(2);
        assert_eq!(slots.place("skins/a".into()), Some((0, None)));
        assert_eq!(slots.place("skins/b".into()), Some((1, None)));
        assert_eq!(slots.place("skins/c".into()), None);
        slots.forget();
        assert!(!slots.contains("skins/a"));
        assert!(!slots.contains("skins/b"));
        assert_eq!(slots.place("skins/c".into()), Some((0, None)));
        assert_eq!(slots.place("skins/a".into()), Some((1, None)));
    }

    /// The launcher's offline skin: a 64x64 or legacy 64x32 PNG laid out as
    /// a downloaded skin, slim only for the `slim` model; anything else is
    /// refused and the player keeps their default skin.
    #[test]
    fn offline_skins_read_like_downloaded_ones() {
        let png = |w: u32, h: u32| {
            let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
            let mut out = Vec::new();
            let mut encoder = png::Encoder::new(&mut out, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&rgba)
                .unwrap();
            (out, rgba)
        };
        let dir = std::env::temp_dir().join(format!("horizon-offline-skin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("steve.png");
        for (h, model, slim) in [(64, "slim", true), (32, "classic", false), (64, "", false)] {
            let (bytes, rgba) = png(64, h);
            std::fs::write(&file, bytes).unwrap();
            let (skin, is_slim) = super::read_offline_skin(&file, model).unwrap();
            assert_eq!(skin, super::process_legacy_skin(64, h, &rgba).unwrap());
            assert_eq!(skin.len(), 64 * 64 * 4);
            assert_eq!(is_slim, slim);
        }
        std::fs::write(&file, png(32, 32).0).unwrap();
        assert!(super::read_offline_skin(&file, "slim").is_err());
        std::fs::write(&file, b"not a png").unwrap();
        assert!(super::read_offline_skin(&file, "slim").is_err());
        assert!(super::read_offline_skin(&dir.join("missing.png"), "").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `MinecraftServicesDiscoveryService`: another environment's document
    /// names its own texture host; one that lists none names nothing.
    #[test]
    fn texture_uris_come_from_the_discovery_document() {
        use super::{Signature, discovery_texture_uris, unpack};
        let doc = serde_json::json!({"discovery": {"profiles": {"endpoints": {"getTexture": {
            "validUris": ["https://skins.example.org/t/{textureId}"]
        }}}}});
        let uris = discovery_texture_uris(&doc).unwrap();
        assert_eq!(uris, ["https://skins.example.org/t/{textureId}"]);
        // {"textures":{"SKIN":{"url":"https://skins.example.org/t/abc"}}}
        let ours =
            "eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHBzOi8vc2tpbnMuZXhhbXBsZS5vcmcvdC9hYmMifX19";
        assert_eq!(
            unpack(ours, Signature::Signed, &uris).skin.as_deref(),
            Some("https://skins.example.org/t/abc")
        );
        assert_eq!(
            unpack(ours, Signature::Signed, &super::TEXTURE_URIS).skin,
            None
        );
        let none = serde_json::json!({"discovery": {"profiles": {"endpoints": {}}}});
        assert_eq!(discovery_texture_uris(&none), None);
        let empty = serde_json::json!({"discovery": {"profiles": {"endpoints": {"getTexture": {"validUris": []}}}}});
        assert_eq!(discovery_texture_uris(&empty), None);
    }

    /// `MinecraftProfileTexture.getMetadata("model")`: `slim` marks a slim
    /// skin; no metadata is the wide (classic) model.
    #[test]
    fn a_skins_model_comes_from_its_metadata() {
        use super::{Signature, TEXTURE_URIS, unpack};
        let slim = "eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYyIsIm1ldGFkYXRhIjp7Im1vZGVsIjoic2xpbSJ9fX19";
        let wide = "eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYyJ9fX0=";
        let t = unpack(slim, Signature::Signed, &TEXTURE_URIS);
        assert_eq!(
            t.skin.as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
        assert!(t.slim);
        assert!(!unpack(wide, Signature::Signed, &TEXTURE_URIS).slim);
    }
    #[test]
    fn profiles_parse_like_resolvable_profiles() {
        use super::{NameOrId, Signature, parse_profile};
        // A bare name is `createUnresolved`: dynamic by name, and its partial
        // id is the name's offline UUID.
        let p = parse_profile(&serde_json::json!("Notch"));
        assert_eq!(p.dynamic, Some(NameOrId::Name("Notch".into())));
        assert_eq!(p.id, horizon_protocol::crypto::offline_uuid("Notch"));
        assert!(p.textures.is_none());
        // A name-only compound resolves the same way.
        let p = parse_profile(&serde_json::json!({"name": "Notch"}));
        assert_eq!(p.dynamic, Some(NameOrId::Name("Notch".into())));
        // An id-only compound resolves by id (int array or UUID string).
        let id = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef_u128;
        let ints: Vec<i64> = [
            (id >> 96) as i32 as i64,
            (id >> 64) as i32 as i64,
            (id >> 32) as i32 as i64,
            id as i32 as i64,
        ]
        .into();
        let p = parse_profile(&serde_json::json!({"id": ints}));
        assert_eq!(p.dynamic, Some(NameOrId::Id(id)));
        let p = parse_profile(&serde_json::json!({"id": "01234567-89ab-cdef-0123-4567abcdef00"}));
        assert_eq!(
            p.dynamic,
            Some(NameOrId::Id(0x0123_4567_89ab_cdef_0123_4567_abcd_ef00))
        );
        // Name and id but no properties is Static: it never resolves, and its
        // default skin keys off the id.
        let p = parse_profile(&serde_json::json!({"name": "Notch", "id": ints}));
        assert!(p.dynamic.is_none());
        assert_eq!(p.id, id);
        // A `textures` property makes it Static (unsigned and signed).
        let p = parse_profile(&serde_json::json!({
            "properties": [{"name": "textures", "value": "v"}]
        }));
        assert!(p.dynamic.is_none());
        assert_eq!(p.textures, Some(("v".into(), Signature::Unsigned)));
        let p = parse_profile(&serde_json::json!({
            "properties": [{"name": "textures", "value": "v", "signature": "s"}]
        }));
        assert_eq!(p.textures, Some(("v".into(), Signature::Signed)));
        // Empty properties leave it dynamic.
        let p = parse_profile(&serde_json::json!({"name": "Notch", "properties": []}));
        assert_eq!(p.dynamic, Some(NameOrId::Name("Notch".into())));
    }

    #[test]
    fn services_profile_answers_give_the_active_skin_and_cape() {
        let profile = serde_json::json!({
            "skins": [
                {"state": "INACTIVE", "url": "https://textures.minecraft.net/texture/old"},
                {"state": "ACTIVE", "url": "https://textures.minecraft.net/texture/skin1", "variant": "CLASSIC"}
            ],
            "capes": [{"state": "ACTIVE", "url": "http://textures.minecraft.net/texture/cape1"}]
        });
        let t = super::services_profile_textures(&profile);
        assert_eq!(
            t.skin.as_deref(),
            Some("https://textures.minecraft.net/texture/skin1")
        );
        assert_eq!(
            t.cape.as_deref(),
            Some("http://textures.minecraft.net/texture/cape1")
        );
        assert!(!t.signed);
        let elsewhere = serde_json::json!({"skins": [{"state": "ACTIVE", "url": "https://evil.example/texture/x"}]});
        assert_eq!(
            super::services_profile_textures(&elsewhere),
            super::Textures::default()
        );
        assert_eq!(
            super::services_profile_textures(&serde_json::json!({})),
            super::Textures::default()
        );
    }

    /// `getByName` and `getProfileById` answer from the services the
    /// discovery document names; a profile resolves through both.
    #[test]
    fn profiles_resolve_through_the_services() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let reply = |status: &str, body: &str| {
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                reader.read_line(&mut line).unwrap();
                while reader.read_line(&mut String::new()).unwrap() > 2 {}
                let body = if line.contains("/users/profiles/minecraft/") {
                    r#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch"}"#
                } else if line.contains("/session/minecraft/profile/")
                    && line.contains("unsigned=false")
                {
                    r#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch","properties":[{"name":"textures","value":"dmFsdWU=","signature":"c2ln"}]}"#
                } else {
                    "{}"
                };
                stream.write_all(reply("200 OK", body).as_bytes()).unwrap();
            }
        });
        let doc = serde_json::json!({"discovery": {
            "profiles": {"endpoints": {"getByName": {"uri": format!("{base}/users/profiles/minecraft/{{name}}")}}},
            "session": {"endpoints": {"getProfileById": {"uri": format!("{base}/session/minecraft/profile/{{profileId}}")}}},
        }});
        let resolved =
            super::resolve_profile(&super::NameOrId::Name("Notch".into()), &doc).unwrap();
        assert_eq!(resolved.id, 0x069a_79f4_44e9_4726_a5be_fca9_0e38_aaf5);
        assert_eq!(resolved.name, "Notch");
        assert_eq!(
            resolved.properties,
            vec![(
                "textures".to_owned(),
                "dmFsdWU=".to_owned(),
                Some("c2ln".to_owned())
            )]
        );
        let by_id = super::resolve_profile(&super::NameOrId::Id(resolved.id), &doc).unwrap();
        assert_eq!(by_id.name, "Notch");
        // A discovery without `profiles`/`getByName` resolves no name.
        let doc = serde_json::json!({"discovery": {
            "session": {"endpoints": {"getProfileById": {"uri": format!("{base}/session/minecraft/profile/{{profileId}}")}}},
        }});
        assert!(super::resolve_profile(&super::NameOrId::Name("Notch".into()), &doc).is_none());
    }

    #[test]
    fn corrupt_skin_cache_is_refetched_and_invalid_downloads_are_not_saved() {
        let root = std::env::temp_dir().join(format!(
            "skin-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("skin");
        std::fs::write(&file, b"broken PNG").unwrap();
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![255; 64 * 64 * 4])
                .unwrap();
        }
        assert!(
            super::cached_texture(&file, super::Kind::Skin, || Some(png_bytes.clone())).is_some()
        );
        assert_eq!(std::fs::read(&file).unwrap(), png_bytes);
        assert!(
            super::cached_texture(&file, super::Kind::Skin, || panic!(
                "valid cache must not fetch"
            ))
            .is_some()
        );
        let missing = root.join("missing");
        assert!(
            super::cached_texture(&missing, super::Kind::Skin, || Some(b"HTTP error".to_vec()))
                .is_none()
        );
        assert!(!missing.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// More skins drawn every frame than there are slots (#3829): a frame's
    /// uploads used to push out skins drawn the frame before, which then
    /// uploaded again and pushed out others, so the drawn skins went missing
    /// in turn. Those drawn keep their slots; the rest wait.
    #[test]
    fn skins_drawn_last_frame_keep_their_slots() {
        let mut d = super::Downloader::new(std::env::temp_dir().join("horizon-test-skin-slots"));
        let skins: Vec<String> = (0..6).map(|i| format!("skins/{i}")).collect();
        for (i, s) in skins.iter().enumerate() {
            d.remember(s.clone(), vec![i as u8]);
        }
        let mut slots = super::SkinSlots::new(4);
        let mut last: Option<Vec<String>> = None;
        for frame in 0..8 {
            // draw_tab_list: the downloads that finished go to the atlas.
            slots.next_frame(d.take_drawn());
            let mut uploads = 0;
            for (location, _) in d.take_ready() {
                if slots.place(location).is_some() {
                    uploads += 1;
                }
            }
            // update_remote_skins: every skin is drawn, from its slot, or
            // asked for again while it has none.
            let mut shown = Vec::new();
            for s in &skins {
                if slots.contains(s) {
                    d.drawn(s);
                    shown.push(s.clone());
                } else {
                    d.again(s);
                }
            }
            if frame >= 2 {
                assert_eq!(uploads, 0, "frame {frame} uploaded again");
                assert_eq!(Some(&shown), last.as_ref(), "frame {frame} lost a skin");
            }
            // The first frame asks for the skins; from the next, four show.
            assert_eq!(shown.len(), if frame == 0 { 0 } else { 4 });
            last = Some(shown);
        }
        // A skin no longer drawn gives up its slot to a waiting one.
        let mut slots = super::SkinSlots::new(2);
        slots.next_frame(Default::default());
        assert_eq!(slots.place("skins/a".into()), Some((0, None)));
        assert_eq!(slots.place("skins/b".into()), Some((1, None)));
        assert_eq!(slots.place("skins/c".into()), None);
        slots.next_frame(["skins/b".to_owned()].into());
        assert_eq!(
            slots.place("skins/c".into()),
            Some((0, Some("skins/a".into())))
        );
        // Stored this frame, kept through it.
        assert_eq!(slots.place("skins/d".into()), None);
        assert!(slots.contains("skins/b") && slots.contains("skins/c"));
    }

    /// A skin evicted from the atlas re-uploads (`again`), once per ask.
    #[test]
    fn evicted_skins_upload_again() {
        let mut d = super::Downloader::new(std::env::temp_dir().join("horizon-test-skins"));
        d.remember("skins/abc".into(), vec![1, 2, 3]);
        assert!(d.again("skins/abc"));
        assert!(!d.again("skins/missing"));
        // Once queued, more asks do not pile up uploads.
        assert!(d.again("skins/abc"));
        assert_eq!(
            d.take_ready(),
            vec![("skins/abc".to_owned(), vec![1, 2, 3])]
        );
        assert!(d.take_ready().is_empty());
        assert!(d.again("skins/abc"));
        assert_eq!(d.take_ready().len(), 1);
        // The first upload marks the location queued too: an `again` before
        // `take_ready` does not put the same skin in the atlas twice.
        d.queue("skins/def".into(), vec![4]);
        assert!(d.again("skins/def"));
        assert_eq!(d.take_ready(), vec![("skins/def".to_owned(), vec![4])]);
    }

    /// Capes share the atlas with skins and must survive both atlas and decoded-image eviction.
    #[test]
    fn evicted_capes_upload_again_from_memory_and_disk() {
        let root = std::env::temp_dir().join(format!(
            "cape-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut d = super::Downloader::new(root.clone());
        let location = super::cape_location("test-cape");
        let digest = location.strip_prefix("capes/").unwrap();
        let file = root.join(&digest[..2]).join(digest);
        let rgba = vec![7; 64 * 32 * 4];
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 64, 32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&rgba)
                .unwrap();
        }
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"broken PNG").unwrap();
        let image = super::cached_texture(&file, super::Kind::Cape, || Some(png_bytes))
            .expect("corrupt cached cape should refetch");
        assert_eq!(image.len(), 64 * 64 * 4);
        assert_eq!(&image[..rgba.len()], &rgba);
        assert!(image[rgba.len()..].iter().all(|&b| b == 0));
        d.send
            .send(((super::Kind::Cape, "test-cape".into()), Some(image.clone())))
            .unwrap();
        assert_eq!(
            d.state(
                super::Kind::Cape,
                "https://textures.minecraft.net/texture/test-cape"
            ),
            super::Download::Ready
        );
        assert!(d.again(&location));
        assert_eq!(d.take_ready(), vec![(location.clone(), image.clone())]);
        assert!(d.again(&location));
        assert!(d.again(&location));
        assert_eq!(d.take_ready(), vec![(location.clone(), image.clone())]);
        d.images.clear();
        d.image_order.clear();
        assert!(d.again(&location));
        assert_eq!(d.take_ready(), vec![(location, image)]);
        // An invalid response must never be saved as a cape.
        let invalid = root.join("invalid");
        assert!(
            super::cached_texture(&invalid, super::Kind::Cape, || Some(
                b"invalid PNG".to_vec()
            ))
            .is_none()
        );
        assert!(!invalid.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A cape fills the top half of a 64x64 slot; a larger one is scaled to 64x32.
    #[test]
    fn capes_fill_the_top_half_of_a_slot() {
        let at = |x: usize, y: usize| (y * 64 + x) * 4;
        let legacy = super::process_cape(22, 17, &[7; 22 * 17 * 4]).unwrap();
        assert_eq!(legacy.len(), 64 * 64 * 4);
        assert_eq!(legacy[at(21, 16)], 7);
        assert_eq!(legacy[at(22, 16)], 0);
        let mut hd = vec![0u8; 128 * 64 * 4];
        hd[(62 * 128 + 126) * 4] = 9;
        let hd = super::process_cape(128, 64, &hd).unwrap();
        assert_eq!(hd[at(63, 31)], 9);
        assert_eq!(hd[at(0, 32)], 0);
        assert!(super::process_cape(64, 32, &[0; 4]).is_none());
        let textures = super::Textures {
            cape: Some("http://textures.minecraft.net/texture/abc".into()),
            ..Default::default()
        };
        let cape = super::cape(&textures).unwrap();
        assert_eq!(
            cape["capes/".len()..],
            super::skin_location("abc")["skins/".len()..]
        );
    }

    #[test]
    fn profile_components_supply_unsigned_head_skins() {
        let value = "eyJ0ZXh0dXJlcyI6IHsiU0tJTiI6IHsidXJsIjogImh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYzEyMyJ9fX0=";
        let profile = serde_json::json!({"properties": [{"name": "textures", "value": value}]});
        assert_eq!(
            super::profile_skin_url(&profile).as_deref(),
            Some("https://textures.minecraft.net/texture/abc123")
        );
        let other = serde_json::json!({"properties": [{"name": "other", "value": value}]});
        assert_eq!(super::profile_skin_url(&other), None);
        let bad = serde_json::json!({"properties": [{"name": "textures", "value": "not base64"}]});
        assert_eq!(super::profile_skin_url(&bad), None);
        assert_eq!(super::profile_skin_url(&serde_json::json!({})), None);
    }

    /// Textures come over plain HTTP too (a local server here; https is the
    /// same request through rustls), and only 2xx answers count.
    #[test]
    fn http_get_reads_2xx_bodies() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                reader.read_line(&mut line).unwrap();
                while reader.read_line(&mut String::new()).unwrap() > 2 {}
                let reply: &[u8] = if line.contains("/texture/abc") {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nPNG!"
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                stream.write_all(reply).unwrap();
            }
        });
        assert_eq!(
            http_get(&format!("{base}/texture/abc")),
            Some(b"PNG!".to_vec())
        );
        assert_eq!(http_get(&format!("{base}/texture/missing")), None);
    }

    use super::*;

    /// The skin's metadata names its model: `slim` is Alex's arms, anything
    /// else (or no metadata, as most wide skins send) is Steve's.
    #[test]
    fn skin_metadata_names_the_slim_model() {
        let slim = unpack(
            "eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYyIsIm1ldGFkYXRhIjp7Im1vZGVsIjoic2xpbSJ9fX19",
            Signature::Signed,
            &TEXTURE_URIS,
        );
        assert!(slim.slim);
        assert_eq!(
            slim.skin.as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
        assert!(!unpack("eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYyJ9fX0=", Signature::Signed, &TEXTURE_URIS).slim);
        assert!(!unpack("eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYyIsIm1ldGFkYXRhIjp7Im1vZGVsIjoiZGVmYXVsdCJ9fX19", Signature::Signed, &TEXTURE_URIS).slim);
    }

    #[test]
    fn skull_block_entity_profiles_supply_head_skins() {
        let value = "eyJ0ZXh0dXJlcyI6IHsiU0tJTiI6IHsidXJsIjogImh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYzEyMyJ9fX0=";
        let profile = horizon_world::nbt::parse_snbt(&format!(
            r#"{{name:"T",id:[I;-158327916,1572358581,-2125825222,-487948715],properties:[{{name:"textures",value:"{value}"}}]}}"#
        ))
        .unwrap()
        .0;
        assert_eq!(
            super::profile_skin_url_nbt(&profile).as_deref(),
            Some("https://textures.minecraft.net/texture/abc123")
        );
        assert_eq!(
            super::profile_uuid_nbt(&profile),
            Some(
                [-158327916, 1572358581, -2125825222, -487948715]
                    .iter()
                    .fold(0u128, |a, &w: &i32| (a << 32) | u128::from(w as u32))
            )
        );
        // A name-only profile (a string tag) has no properties: no url, no id.
        let named = horizon_world::nbt::Tag::String("MHF_Blaze".into());
        assert_eq!(super::profile_skin_url_nbt(&named), None);
        assert_eq!(super::profile_uuid_nbt(&named), None);
        // A uuid in string form decodes too.
        let text_id = horizon_world::nbt::parse_snbt(
            r#"{name:"T",id:"e843a1f4-0000-0001-0000-000200000003"}"#,
        )
        .unwrap()
        .0;
        assert_eq!(
            super::profile_uuid_nbt(&text_id),
            Some(u128::from_str_radix("e843a1f4000000010000000200000003", 16).unwrap())
        );
    }

    #[test]
    fn sha1_and_base64() {
        let hex: String = sha1(b"abc").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(base64_decode("aGk").unwrap(), b"hi");
        assert!(base64_decode("a").is_none());
        assert!(base64_decode("aGk=x").is_none());
        assert_eq!(
            texture_hash("http://textures.minecraft.net/texture/abc.png?x=1").unwrap(),
            "abc"
        );
    }

    #[test]
    fn mojang_profiles_become_textures() {
        let payload = r#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/ab"},"CAPE":{"url":"http://textures.minecraft.net/texture/cd"}}}"#;
        let value: String = {
            // base64 of the payload, by the standard alphabet.
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            payload
                .as_bytes()
                .chunks(3)
                .flat_map(|c| {
                    let n =
                        c.iter().fold(0u32, |a, &b| a << 8 | u32::from(b)) << (8 * (3 - c.len()));
                    (0..4).map(move |i| {
                        if i > c.len() {
                            '='
                        } else {
                            A[(n >> (18 - 6 * i) & 63) as usize] as char
                        }
                    })
                })
                .collect()
        };
        let profile = serde_json::json!({"properties": [{"name": "textures", "value": value}]});
        let t = super::mojang_profile_textures(&profile).unwrap();
        assert!(t.signed);
        assert_eq!(
            t.cape.as_deref(),
            Some("http://textures.minecraft.net/texture/cd")
        );
        assert!(super::mojang_profile_textures(&serde_json::json!({"properties": []})).is_none());
        // A cape takes the top half of a skin slot; malformed pixels fail.
        assert_eq!(
            super::process_cape(64, 32, &[7; 64 * 32 * 4])
                .unwrap()
                .len(),
            64 * 64 * 4
        );
        assert!(super::process_cape(22, 17, &[]).is_none());
    }

    #[test]
    fn ely_answers_become_textures() {
        let slim = super::ely_textures(
            r#"{"SKIN":{"url":"http://ely.by/storage/skins/69c6.png","metadata":{"model":"slim"}},"CAPE":{"url":"http://ely.by/storage/capes/1.png"}}"#,
        );
        assert_eq!(
            slim.skin.as_deref(),
            Some("https://ely.by/storage/skins/69c6.png")
        );
        assert_eq!(
            slim.cape.as_deref(),
            Some("https://ely.by/storage/capes/1.png")
        );
        assert!(slim.slim && slim.signed);
        let mojang =
            super::ely_textures(r#"{"SKIN":{"url":"http://textures.minecraft.net/texture/2920"}}"#);
        assert_eq!(
            mojang.skin.as_deref(),
            Some("http://textures.minecraft.net/texture/2920")
        );
        assert!(!mojang.slim);
        // 204 No Content: nothing to wear.
        assert_eq!(super::ely_textures(""), super::Textures::default());
        assert!(super::ely_name("Steve_01") && !super::ely_name("a b") && !super::ely_name("../x"));
    }

    /// SkinManager.registerTextures combines the body/cape/wings futures with
    /// allOf. An exceptional future makes createLookup use the default profile,
    /// but registration still submits its sibling textures first.
    #[test]
    fn an_exceptional_accessory_download_uses_the_default_profile() {
        use super::{Download, Kind, Textures};
        let textures = Textures {
            skin: Some("https://textures.minecraft.net/texture/skin1".into()),
            cape: Some("https://textures.minecraft.net/texture/cape1".into()),
            elytra: Some("https://textures.minecraft.net/texture/elytra1".into()),
            signed: true,
            slim: false,
        };
        assert_eq!(
            super::lookup(&textures, true, |_, _| Download::Ready),
            Some(super::skin_location("skin1"))
        );
        for failing in [Kind::Skin, Kind::Cape, Kind::Elytra] {
            let mut submitted = Vec::new();
            let result = super::lookup_locations(&textures, true, |kind, _| {
                submitted.push(kind);
                if kind == failing {
                    Download::Failed
                } else {
                    Download::Ready
                }
            });
            assert!(result.is_none(), "{failing:?}");
            assert_eq!(submitted, [Kind::Skin, Kind::Cape, Kind::Elytra]);
        }
    }

    /// Actual 26.3 getHash -> registerTextures -> cache completion -> createLookup
    /// uses the whole default profile if any URL is malformed. The failure is
    /// synchronous, so textures after the malformed one are not registered.
    #[test]
    fn malformed_texture_urls_fail_the_profile_in_registration_order() {
        use super::{Download, Kind, Textures};
        let order = [Kind::Skin, Kind::Cape, Kind::Elytra];
        for (index, bad) in order.into_iter().enumerate() {
            let mut textures = Textures {
                skin: Some("https://textures.minecraft.net/texture/body".into()),
                cape: Some("https://textures.minecraft.net/texture/cape".into()),
                elytra: Some("https://textures.minecraft.net/texture/elytra".into()),
                signed: true,
                slim: false,
            };
            match bad {
                Kind::Skin => textures.skin = Some("no-scheme".into()),
                Kind::Cape => textures.cape = Some("no-scheme".into()),
                Kind::Elytra => textures.elytra = Some("no-scheme".into()),
            }
            let mut submitted = Vec::new();
            let result = super::lookup_locations(&textures, true, |kind, _| {
                submitted.push(kind);
                Download::Ready
            });
            assert!(result.is_none(), "{bad:?}");
            assert_eq!(submitted, order[..index], "{bad:?}");
        }
    }

    #[test]
    fn accessories_use_default_body_and_wings_respect_model_part_and_atlas_readiness() {
        use super::{Download, Kind, Textures};
        let textures = Textures {
            skin: None,
            cape: Some("https://textures.minecraft.net/texture/cape".into()),
            elytra: Some("https://textures.minecraft.net/texture/elytra".into()),
            signed: true,
            slim: false,
        };
        let ready = super::lookup_locations(&textures, true, |_, _| Download::Ready).unwrap();
        assert!(
            ready.skin.is_none(),
            "the caller must use the UUID's default body/model"
        );
        assert_eq!(
            ready.cape.as_deref(),
            Some(super::cape_location("cape").as_str())
        );
        assert_eq!(
            ready.elytra.as_deref(),
            Some(super::elytra_location("elytra").as_str())
        );
        for show in [false, true] {
            assert_eq!(
                super::wings_texture(ready.elytra.as_deref(), ready.cape.as_deref(), show, |_| {
                    true
                }),
                ready.elytra.as_deref()
            );
        }
        assert_eq!(
            super::wings_texture(None, ready.cape.as_deref(), true, |_| true),
            ready.cape.as_deref()
        );
        assert_eq!(
            super::wings_texture(None, ready.cape.as_deref(), false, |_| true),
            None
        );
        // A valid but evicted wing sprite is requested again by the game. Until
        // the upload arrives, preserve the visible cape or the equipment default.
        let in_atlas = |s: &str| Some(s) == ready.cape.as_deref();
        assert_eq!(
            super::wings_texture(
                ready.elytra.as_deref(),
                ready.cape.as_deref(),
                true,
                in_atlas
            ),
            ready.cape.as_deref()
        );
        assert_eq!(
            super::wings_texture(
                ready.elytra.as_deref(),
                ready.cape.as_deref(),
                false,
                in_atlas
            ),
            None
        );
        for state in [Download::Failed, Download::Pending] {
            assert!(
                super::lookup_locations(&textures, true, |kind, _| if kind == Kind::Elytra {
                    state
                } else {
                    Download::Ready
                })
                .is_none()
            );
        }
    }

    /// A 1x1 PNG of `rgba`.
    fn png(rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&rgba).unwrap();
        writer.finish().unwrap();
        out
    }

    /// A downloaded body reaches the disk cache only if it decodes; a broken
    /// cached file is fetched again and replaced. A failed fetch writes nothing.
    #[test]
    fn the_texture_cache_holds_only_what_decodes() {
        let root =
            std::env::temp_dir().join(format!("horizon-test-skin-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let body = png([7, 8, 9, 255]);
        let file = root.join("ab").join("abcdef");
        let load = |fetched: Option<&[u8]>| {
            let fetched = fetched.map(<[u8]>::to_vec);
            super::Downloader::load_texture(
                &root,
                super::Kind::Elytra,
                "abcdef",
                "https://example/texture/abc.png",
                |_| fetched.clone(),
            )
        };
        assert_eq!(load(Some(&body)), Some(vec![7, 8, 9, 255]));
        assert_eq!(std::fs::read(&file).unwrap(), body);
        // From then on the cache answers; no fetch is needed.
        assert_eq!(load(None), Some(vec![7, 8, 9, 255]));
        // A broken file is fetched again and replaced, not kept.
        std::fs::write(&file, b"<html>not a png</html>").unwrap();
        assert_eq!(load(Some(&body)), Some(vec![7, 8, 9, 255]));
        assert_eq!(std::fs::read(&file).unwrap(), body);
        // A body that does not decode is not cached at all.
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(load(Some(b"<html>not a png</html>")), None);
        assert!(!file.exists());
    }

    fn wait_state(
        d: &mut super::Downloader,
        kind: super::Kind,
        url: &str,
        want: super::Download,
    ) -> super::Download {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let state = d.state(kind, url);
            if state == want || std::time::Instant::now() > deadline {
                return state;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// A texture download that failed is tried again after the wait, instead of
    /// pinning the player to the default skin for the rest of the session.
    #[test]
    fn a_failed_download_is_retried_after_the_wait() {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let body = png([1, 2, 3, 4]);
        let served = std::sync::Arc::new(AtomicUsize::new(0));
        let count = served.clone();
        std::thread::spawn(move || {
            for (i, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                reader.read_line(&mut line).unwrap();
                while reader.read_line(&mut String::new()).unwrap() > 2 {}
                count.fetch_add(1, Ordering::SeqCst);
                if i == 0 {
                    stream
                        .write_all(
                            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                } else {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(head.as_bytes()).unwrap();
                    stream.write_all(&body).unwrap();
                }
            }
        });
        let root =
            std::env::temp_dir().join(format!("horizon-test-skin-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut d = super::Downloader::new(root);
        let url = format!("{base}/texture/retry.png");
        assert_eq!(d.state(super::Kind::Elytra, &url), super::Download::Pending);
        assert_eq!(
            wait_state(&mut d, super::Kind::Elytra, &url, super::Download::Failed),
            super::Download::Failed
        );
        // Within the wait the failure stands: no request goes out.
        assert_eq!(d.state(super::Kind::Elytra, &url), super::Download::Failed);
        assert_eq!(served.load(Ordering::SeqCst), 1);
        // After it, the download is started again and takes the good answer.
        d.retry_after = std::time::Duration::ZERO;
        assert_eq!(d.state(super::Kind::Elytra, &url), super::Download::Pending);
        assert_eq!(
            wait_state(&mut d, super::Kind::Elytra, &url, super::Download::Ready),
            super::Download::Ready
        );
        assert_eq!(served.load(Ordering::SeqCst), 2);
    }
}
