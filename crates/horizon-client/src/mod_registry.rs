//! Horizon's mod registry from inside the game (Options > Mods > Browse mods,
//! docs/design/mod-hotload.md): the same `index.json` the launcher reads, the
//! projects this game can install, and installing one exactly as the launcher
//! does (`launcher/src/browser/horizon.rs`, `install_version`): downloaded into
//! `mod-browser/downloads/`, checked against the index (size, SHA-256, the
//! author's signature, the file's own manifest), moved into `horizon-mods/`
//! and recorded in `mod-browser.json`, so the launcher lists, updates and
//! removes it as one of its own.
//!
//! The launcher is a separate workspace, so this is the small part of it the
//! game needs. Everything here blocks: the game runs it on a worker
//! (`game/mod_browser.rs`).
use crate::mod_browser::Listing;
use horizon_mods::index::{self, Index, Kind, Project, Trust, Version};
use std::fs;
use std::sync::Arc;
use std::time::Duration;
// The existing managed service transport owns close-only socket leases.
pub use horizon_cosmetics::transport::Cancellation;
use horizon_cosmetics::transport::Connector;
mod storage;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where the registry publishes its index (the launcher's `INDEX_URL`).
pub const INDEX_URL: &str = "https://horizon-rust.github.io/horizon-mods/index.json";
/// The Minecraft version this game is: the registry's `game`.
pub const GAME: &str = horizon_core::version::VERSION_ID;
/// The registry's pages on the website, `<SITE>/<id>`.
pub const SITE: &str = "https://playhorizon.dev/mods";
/// The last index fetched, beside `options.txt`: the launcher keeps its own copy there too.
pub const CACHE: &str = "mod-browser/horizon-index.json";
/// What the launcher's browser installed (`installed::MANIFEST`).
pub const MANIFEST: &str = "mod-browser.json";
/// Where a download waits for its checks.
const DOWNLOADS: &str = "mod-browser/downloads";
/// Larger than any index should be.
const MAX_INDEX: u64 = 16 << 20;
/// The game asks the registry by this name.
const AGENT: &str = concat!(
    "horizon-rust/horizon/",
    env!("CARGO_PKG_VERSION"),
    " (in-game mod browser)"
);

/// Where the index comes from: `HORIZON_MOD_INDEX` (a file or an `https://`
/// address, for a test registry), else [`INDEX_URL`]; as the launcher reads it.
pub fn location() -> String {
    std::env::var("HORIZON_MOD_INDEX")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| INDEX_URL.to_owned())
}

/// A local test registry: its files may be `file://` paths.
fn is_local(at: &str) -> bool {
    !at.starts_with("https://")
}

/// The index from `at`, kept in `home`'s [`CACHE`]; the kept copy when the
/// registry can't be reached.
pub fn load_index(home: &Path, at: &str) -> Result<Index, String> {
    load_index_managed(home, at, &Arc::new(Cancellation::default()))
}

pub fn load_index_managed(
    home: &Path,
    at: &str,
    cancel: &Arc<Cancellation>,
) -> Result<Index, String> {
    check_cancel(cancel)?;
    let local = is_local(at);
    let parse = |text: &str| {
        if local {
            Index::parse_local(text)
        } else {
            Index::parse(text)
        }
    };
    let fetched = fetch_text(at, cancel).and_then(|text| parse(&text).map(|index| (index, text)));
    check_cancel(cancel)?;
    let cache = home.join(CACHE);
    match fetched {
        Ok((index, text)) => {
            let _ = storage::atomic_write(&cache, text.as_bytes());
            Ok(index)
        }
        Err(error) => {
            let kept = read_text_file(&cache, MAX_INDEX, cancel).map_err(|_| error.clone())?;
            parse(&kept).map_err(|_| error)
        }
    }
}

/// Finish rollback of an interrupted install before startup reads content mods.
/// Blocking filesystem work: call on a loader worker, never the window thread.
pub fn recover_home(home: &Path) -> Result<(), String> {
    with_content_snapshot(home, || ())
}

/// Read one complete content generation under the same gate as publication.
/// The callback is finite read/snapshot work on a loader worker or headless
/// path, never on the window thread and never recursively installing a mod.
pub fn with_content_snapshot<T>(home: &Path, read: impl FnOnce() -> T) -> Result<T, String> {
    with_content_snapshot_managed(home, &Cancellation::default(), read)
}

/// An owned worker's snapshot uses the same cancellation token while waiting
/// for publication, recovering, reading, and returning its candidate generation.
pub fn with_content_snapshot_managed<T>(
    home: &Path,
    cancel: &Cancellation,
    read: impl FnOnce() -> T,
) -> Result<T, String> {
    check_cancel(cancel)?;
    let _lock = storage::lock_home(home, cancel)?;
    check_cancel(cancel)?;
    storage::recover(home)?;
    check_cancel(cancel)?;
    let snapshot = read();
    check_cancel(cancel)?;
    Ok(snapshot)
}

pub(crate) fn check_cancel(cancel: &Cancellation) -> Result<(), String> {
    if cancel.cancelled() {
        Err("mod browser operation cancelled".into())
    } else {
        Ok(())
    }
}

fn read_bounded(reader: impl Read, max: u64, cancel: &Cancellation) -> Result<Vec<u8>, String> {
    let count = max.checked_add(1).ok_or("invalid download limit")?;
    let mut reader = reader.take(count);
    let mut bytes = Vec::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if bytes.len() as u64 > max {
            return Err("mod browser file exceeds its size limit".into());
        }
    }
    check_cancel(cancel)?;
    Ok(bytes)
}
fn read_text_file(path: &Path, max: u64, cancel: &Cancellation) -> Result<String, String> {
    String::from_utf8(read_bounded(
        fs::File::open(path).map_err(|e| e.to_string())?,
        max,
        cancel,
    )?)
    .map_err(|e| e.to_string())
}
fn agent(cancel: &Arc<Cancellation>) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .tls_connector(Arc::new(Connector::new(cancel.clone())))
        .build()
}
fn fetch_text(at: &str, cancel: &Arc<Cancellation>) -> Result<String, String> {
    check_cancel(cancel)?;
    if !at.starts_with("https://") {
        return read_text_file(Path::new(at), MAX_INDEX, cancel);
    }
    let response = agent(cancel)
        .get(at)
        .set("User-Agent", AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    String::from_utf8(read_bounded(response.into_reader(), MAX_INDEX, cancel)?)
        .map_err(|e| e.to_string())
}

/// Whether version `v` is a `.hmod` with code: the game doesn't run those any
/// more, and the launcher neither lists nor installs them (`needs_port`).
fn needs_port(v: &Version) -> bool {
    v.file.name.ends_with(".hmod")
        && v.capabilities
            .iter()
            .any(|c| *c != horizon_mods::Capability::Registries)
}

/// The version of content mod `p` this game installs: the newest for this
/// Minecraft version that wasn't taken down and has no code.
pub fn installable<'a>(index: &Index, p: &'a Project) -> Option<&'a Version> {
    if p.kind != Kind::Mod {
        return None;
    }
    index
        .installable(p, GAME, horizon_mods::api_level::LEVEL)
        .filter(|v| !needs_port(v))
}

/// The registry's projects the browser offers, most downloaded first: content
/// mods this game installs, with the version installed in `home` when there is
/// one.
pub fn listings(index: &Index, home: &Path) -> Vec<Listing> {
    let installed = Installed::load(home);
    let mut out: Vec<Listing> = index
        .projects
        .iter()
        .filter_map(|p| {
            let v = installable(index, p)?;
            Some(Listing {
                id: p.id.clone(),
                name: p.name.clone(),
                summary: p.summary.clone(),
                description: p.description.clone(),
                authors: p.authors.clone(),
                license: p.license.clone(),
                categories: p.categories.clone(),
                tags: p.tags.clone(),
                version: v.version.clone(),
                published: v.published.clone(),
                changelog: v.changelog.clone(),
                downloads: p.downloads,
                page: format!("{SITE}/{}", p.id),
                installed: installed.version_of(home, &p.id),
            })
        })
        .collect();
    out.sort_by(|a, b| b.downloads.cmp(&a.downloads).then(a.name.cmp(&b.name)));
    out
}

/// What [`install`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub id: String,
    pub version: String,
    /// Checked against the author's key, or unsigned (the SHA-256 still pins
    /// the reviewed file).
    pub verified: bool,
}

/// Installs the newest installable version of the registry's content mod `id`
/// into the game folder `home`, as the launcher does: `horizon-mods/<id>.hmod`,
/// recorded in `mod-browser.json`. Turning it on is the caller's
/// (`horizon-mods.txt` belongs to the game's settings). `get` downloads a file
/// of at most `max` bytes (the network, or a test's files).
pub fn install(
    index: &Index,
    home: &Path,
    id: &str,
    get: &dyn Fn(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<Done, String> {
    install_managed(index, home, id, get, &Cancellation::default())
}

pub fn install_managed(
    index: &Index,
    home: &Path,
    id: &str,
    get: &dyn Fn(&str, u64) -> Result<Vec<u8>, String>,
    cancel: &Cancellation,
) -> Result<Done, String> {
    check_cancel(cancel)?;
    if !index::valid_id(id) {
        return Err("invalid registry project id".into());
    }
    let p = index
        .project(id)
        .ok_or_else(|| format!("{id} isn't in Horizon's registry"))?;
    let v = installable(index, p)
        .ok_or_else(|| format!("{} has no version this game can install", p.name))?;
    let relative = format!("{}/{id}.hmod", crate::mod_list::DIR);
    let target = home.join(&relative);
    {
        let _lock = storage::lock_home(home, cancel)?;
        storage::recover(home)?;
        if let Err(done) = prepare_install(home, id, &relative, &target, v)? {
            return Ok(done);
        }
    }
    let bytes = get(&v.file.url, v.file.size)?;
    check_cancel(cancel)?;
    if bytes.len() as u64 > v.file.size || v.file.size > index::MAX_FILE {
        return Err("mod download exceeds the registry size limit".into());
    }
    let download = storage::stage(&home.join(DOWNLOADS), &bytes)?;
    let trust = index::check_file(index, p, v, &download.0)?;
    check_cancel(cancel)?;
    let _lock = storage::lock_home(home, cancel)?;
    storage::recover(home)?;
    let mut list = match prepare_install(home, id, &relative, &target, v)? {
        Ok(list) => list,
        Err(done) => return Ok(done),
    };
    let verified = trust == Trust::Verified;
    list.record(serde_json::json!({
        "source": "horizon",
        "project": p.id,
        "slug": p.id,
        "title": p.name,
        "kind": "mod",
        "version": v.version,
        "number": v.version,
        "file": relative,
        "checksum": format!("SHA-256:{}", v.file.sha256),
        "page": format!("{SITE}/{}", p.id),
        "dependency": false,
        "verdict": if verified { "Verified" } else { "Unsigned" },
    }));
    let manifest = list.bytes()?;
    check_cancel(cancel)?;
    storage::commit(home, id, &download.0, &manifest)?;
    Ok(Done {
        id: id.to_owned(),
        version: v.version.clone(),
        verified,
    })
}

fn prepare_install(
    home: &Path,
    id: &str,
    relative: &str,
    target: &Path,
    v: &Version,
) -> Result<Result<Installed, Done>, String> {
    let list = Installed::load_checked(home)?;
    let ours = list.position(id);
    let old_file = ours.and_then(|i| list.entries[i]["file"].as_str().map(str::to_owned));
    if let Some(i) = ours
        && list.entries[i]["version"].as_str() == Some(v.version.as_str())
        && target.exists()
    {
        return Ok(Err(Done {
            id: id.to_owned(),
            version: v.version.clone(),
            verified: list.entries[i]["verdict"].as_str() == Some("Verified"),
        }));
    }
    // Never over a mod the player put there themselves.
    let theirs = target.exists() && old_file.as_deref() != Some(relative);
    if theirs || home.join(crate::mod_list::DIR).join(id).is_dir() {
        return Err(format!(
            "a mod with the id {id} is already in {}: move it away first",
            crate::mod_list::DIR
        ));
    }
    if let Some(other) = list.taken_by(relative, id) {
        return Err(format!("{relative} belongs to {other}: remove it first"));
    }
    Ok(Ok(list))
}

/// Downloads `url` (at most `max` bytes): `https://` only, or a `file://` path
/// when the index itself is a local test registry.
pub fn download(url: &str, max: u64) -> Result<Vec<u8>, String> {
    download_managed(url, max, &Arc::new(Cancellation::default()))
}

pub fn download_managed(
    url: &str,
    max: u64,
    cancel: &Arc<Cancellation>,
) -> Result<Vec<u8>, String> {
    check_cancel(cancel)?;
    let max = max.min(index::MAX_FILE);
    if let Some(path) = url.strip_prefix("file://") {
        if !is_local(&location()) {
            return Err(format!("{url} is not an https address"));
        }
        return read_bounded(
            fs::File::open(path).map_err(|e| e.to_string())?,
            max,
            cancel,
        );
    }
    if !url.starts_with("https://") {
        return Err(format!("{url} is not an https address"));
    }
    let response = agent(cancel)
        .get(url)
        .set("User-Agent", AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    read_bounded(response.into_reader(), max, cancel)
}

/// `mod-browser.json`, kept as JSON values so the launcher's entries this
/// game doesn't model (Modrinth resource packs, ported Java mods) are written
/// back as they were.
#[derive(Debug, Default)]
struct Installed {
    entries: Vec<serde_json::Value>,
}

impl Installed {
    fn load(home: &Path) -> Self {
        Self::load_checked(home).unwrap_or_default()
    }

    fn load_checked(home: &Path) -> Result<Self, String> {
        let path = home.join(MANIFEST);
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        let bytes = read_bounded(file, MAX_INDEX, &Cancellation::default())?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let entries = value
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .ok_or("invalid installed-mod manifest")?;
        Ok(Self { entries })
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({ "entries": self.entries }))
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_INDEX {
            return Err("installed-mod manifest exceeds its size limit".into());
        }
        Ok(bytes)
    }

    /// The registry's entry for project `id`.
    fn position(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e["source"] == "horizon" && e["project"] == id)
    }

    /// The title of another entry that owns `file`.
    fn taken_by(&self, file: &str, id: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|e| e["file"] == file && !(e["source"] == "horizon" && e["project"] == id))
            .map(|e| e["title"].as_str().unwrap_or("another mod").to_owned())
    }

    fn record(&mut self, entry: serde_json::Value) {
        let id = entry["project"].clone();
        self.entries
            .retain(|e| !(e["source"] == "horizon" && e["project"] == id));
        self.entries.push(entry);
    }

    /// The version of `id` installed from the registry whose file is still
    /// there; for a `horizon-mods/<id>.hmod` put there another way, its
    /// manifest's version.
    fn version_of(&self, home: &Path, id: &str) -> Option<String> {
        if let Some(e) = self.position(id).map(|i| &self.entries[i])
            && let (Some(version), Some(file)) = (e["version"].as_str(), e["file"].as_str())
            && home.join(file).exists()
        {
            return Some(version.to_owned());
        }
        let path: PathBuf = home.join(crate::mod_list::DIR).join(format!("{id}.hmod"));
        horizon_mods::Package::read(&path)
            .ok()
            .filter(|p| p.manifest.id == id)
            .map(|p| p.manifest.version)
    }
}

#[cfg(test)]
mod tests;
