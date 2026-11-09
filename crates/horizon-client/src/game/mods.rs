//! Mods in the game (`docs/mods.md`): content mods' blocks, items, data and
//! assets (`content_root`), and the Mods page's rows. Code mods are native
//! (`native_hooks.rs`); WebAssembly code mods don't run any more.
use super::Game;
use horizon_client::{
    lang,
    mod_list::{self, Installed},
    text::Text,
};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct ModsState {
    /// `--dev-mod PATH`: always on.
    pub(super) dev: Vec<PathBuf>,
    /// The WebAssembly mods found that no longer run, for the one-time notice.
    stopped: Vec<String>,
    /// Browse mods' worker (`mod_browser`).
    pub(super) jobs: super::mod_browser::Jobs,
    /// The content mods the loaded resources were built with: a switch the
    /// game can apply live moves it to the player's choice (`switch_mods`).
    pub(super) applied: Option<std::collections::BTreeSet<String>>,
    /// A live switch's resource reload is running: when it applied, the server
    /// reloads its data from the new root. Its start, for the log.
    pub(super) live: Option<std::time::Instant>,
    /// The content mods installed, and those of them with only assets and
    /// data (`mod_content::live_switch`): they switch with a world open.
    content_ids: std::collections::BTreeSet<String>,
    live_ids: std::collections::BTreeSet<String>,
    /// The server's data comes from a root a live switch built, not the one
    /// `res` was loaded from: the world tables are built from it even with no
    /// data pack (`build_world_tables`).
    pub(super) data_switched: bool,
}

/// Freeze native component IDs before reading content or opening a connection.
/// With no mod registering any, nothing is installed, which every reader
/// treats as no custom components, and `registries.json` isn't read.
pub(super) fn register_components(root: &std::path::Path) -> horizon_client::Result<()> {
    let mut names = Vec::new();
    Game::native_component_types(&mut names);
    if names.is_empty() {
        return Ok(());
    }
    let path =
        horizon_protocol::registry::Builtin::find(root).ok_or("registries.json not found")?;
    let builtin = horizon_protocol::registry::Builtin::load(&path)?;
    let types = builtin
        .get("minecraft:data_component_type")
        .ok_or("missing data component registry")?;
    let first = i32::try_from(types.names.len())?;
    horizon_protocol::item::custom::install(first, names)
        .map_err(|error| format!("invalid native component registration: {error}"))?;
    Ok(())
}

/// The blocks and items native mods declare in code (`native_blocks`,
/// `native_items`): asked once, so every content root of this run, a pack
/// reload's included, has the same ones.
fn native_declared() -> &'static (
    Vec<horizon_mods::content::Block>,
    Vec<horizon_mods::content::Item>,
) {
    static DECLARED: std::sync::OnceLock<(
        Vec<horizon_mods::content::Block>,
        Vec<horizon_mods::content::Item>,
    )> = std::sync::OnceLock::new();
    DECLARED.get_or_init(|| {
        let (mut blocks, mut items) = (Vec::new(), Vec::new());
        Game::native_blocks(&mut blocks);
        Game::native_items(&mut items);
        (blocks, items)
    })
}
/// The native mods' standalone models (`native_standalone_models`), set once
/// before the resources first load.
pub(super) fn register_standalone_models() {
    let mut names = Vec::new();
    Game::native_standalone_models(&mut names);
    horizon_client::models::register_standalone(names);
}
/// The native mods' entity model layers (`native_layer_definitions`), kept
/// once at start for `native_bake_layer`.
pub(super) fn register_layer_definitions() {
    let mut layers = Vec::new();
    Game::native_layer_definitions(&mut layers);
    horizon_client::mobs::model_layers::register(layers);
}

/// `root` with the content mods that are on layered over it
/// (docs/modpacks.md): their blocks, items, data and assets. Read once at
/// start, before the resources load; a mod whose content is refused is left
/// out here and its row on the Mods page says why.
pub(super) fn content_root(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    disabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) -> std::path::PathBuf {
    let here = std::env::current_dir().unwrap_or_default();
    let version = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_owned()));
    content_root_at(root, enabled, disabled, dev, &here, version.as_deref())
}

fn content_root_at(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    disabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
    here: &std::path::Path,
    version: Option<&std::path::Path>,
) -> std::path::PathBuf {
    use horizon_client::mod_content::{self, ContentMod};
    use horizon_mods::{Capability, Content, Package};
    let _step = horizon_client::spans::startup::step("mods.content_root");
    let native = version
        .map(|p| horizon_mods::find(&p.join("native-content")))
        .unwrap_or_default();
    let native_ids: std::collections::BTreeSet<_> = native
        .iter()
        .filter_map(|path| Package::read(path).ok().map(|p| p.manifest.id))
        .collect();
    let mut paths = native.clone();
    paths.extend(horizon_mods::find(&here.join(mod_list::DIR)));
    paths.retain(|p| !dev.contains(p));
    paths.extend(dev.iter().cloned());
    let mut known = None;
    let mut mods: Vec<ContentMod> = Vec::new();
    // The first load's list stays (`set_local`): a reload does not hash the files again.
    let announce = !horizon_protocol::mod_list::is_set();
    let mut announced = Vec::new();
    for path in &paths {
        let Ok(package) = Package::read(path) else {
            continue;
        };
        let m = &package.manifest;
        // A legacy content copy kept for an older build cannot replace this build's snapshot,
        // including when the native mod is off.
        if !native.contains(path) && native_ids.contains(&m.id) && !dev.contains(path) {
            continue;
        }
        let on = if native.contains(path) {
            !disabled.contains(&m.id)
        } else {
            enabled.contains(&m.id)
        } || dev.contains(path);
        if !on
            || !m.capabilities.contains(&Capability::Registries)
            || mods.iter().any(|c| c.id == m.id)
        {
            continue;
        }
        // The items this mod's native code declares may be a file item's crafting remainder.
        let declared: Vec<String> = native_declared()
            .1
            .iter()
            .filter(|i| i.id.split_once(':').is_some_and(|(ns, _)| ns == m.id))
            .map(|i| i.id.clone())
            .collect();
        let content = match Content::read_declaring(path, m, &declared) {
            Ok(content) => content,
            Err(e) => {
                log::warn!("Horizon: mod {} content not loaded: {e}", m.id);
                continue;
            }
        };
        // Read as `Content::read` reads it: `start_mods` need not read it again.
        if declared.is_empty() {
            remember_verdict(path, &content);
        }
        let known = match &mut known {
            Some(types) => types,
            None => match mod_content::known_components(root) {
                Ok(types) => known.insert(types),
                Err(e) => {
                    log::warn!("Horizon: content mods not loaded: {e}");
                    register_mob_effects(&[]);
                    return root.to_owned();
                }
            },
        };
        let candidate = ContentMod {
            id: m.id.clone(),
            content,
        };
        match mod_content::accept_mod(&mut mods, candidate, known) {
            Ok(_) if announce => announced.push(announced_entry(m, path)),
            Ok(_) => {}
            Err(e) => log::warn!("Horizon: mod {} content not loaded: {e}", m.id),
        }
    }
    let (blocks, items) = native_declared();
    if !blocks.is_empty() || !items.is_empty() {
        if known.is_none() {
            match mod_content::known_components(root) {
                Ok(types) => known = Some(types),
                Err(e) => log::warn!("Horizon: native mods' blocks and items not loaded: {e}"),
            }
        }
        if let Some(known) = &known {
            for e in mod_content::accept_declared(&mut mods, blocks, items, known) {
                log::warn!("Horizon: {e}");
            }
            // A file item's remainder named a code-declared item that was refused: that mod
            // alone is left out, not every content mod through the overlay's check.
            for e in mod_content::drop_dangling_remainders(&mut mods) {
                log::warn!("Horizon: {e}");
            }
        }
    }
    let overlay = {
        let _step = horizon_client::spans::startup::step("mods.overlay");
        mod_content::resource_root(root, &mods, &here.join(".horizon-cache"))
    };
    match overlay {
        Ok(out) => {
            if announce {
                horizon_protocol::mod_list::set_local(announced);
            }
            if out != root {
                log::info!(
                    "Horizon: content from {} mod(s) in {}",
                    mods.len(),
                    out.display()
                );
            }
            register_mob_effects(&mods);
            register_entity_types(&mods);
            register_menu_types(&mods);
            out
        }
        Err(e) => {
            log::warn!("Horizon: content mods not loaded: {e}");
            register_mob_effects(&[]);
            register_entity_types(&[]);
            register_menu_types(&[]);
            root.to_owned()
        }
    }
}

/// A loaded mod as a server tells joining clients (`horizon_protocol::mod_list`): its manifest's
/// id, version and side, and the SHA-256 of its `.hmod` (none for a folder).
fn announced_entry(
    m: &horizon_mods::Manifest,
    path: &std::path::Path,
) -> horizon_protocol::mod_list::Entry {
    use horizon_mods::Side;
    use horizon_protocol::mod_list;
    use sha2::{Digest, Sha256};
    let sha256 = if path.is_file() {
        std::fs::read(path)
            .ok()
            .map(|bytes| Sha256::digest(bytes).into())
    } else {
        None
    };
    mod_list::Entry {
        id: m.id.clone(),
        version: m.version.clone(),
        sha256,
        side: match m.side {
            Side::Both => mod_list::Side::Both,
            Side::Server => mod_list::Side::Server,
            Side::Client => mod_list::Side::Client,
        },
    }
}

/// The mods' mob effects join vanilla's (`brewing::effect`), once the root
/// holding their network ids is built. The startup root's effects, none
/// included, stay until restart: a pack reload after a Mods switch builds a
/// root with others.
fn register_mob_effects(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::brewing::{self, Category};
    use horizon_mods::content::EffectCategory;
    let effects = horizon_client::mod_content::mob_effects(mods);
    let ids: Vec<String> = effects.iter().map(|e| e.id.clone()).collect();
    let set = brewing::register_mod_effects(effects.into_iter().map(|e| {
        let category = match e.category {
            EffectCategory::Beneficial => Category::Beneficial,
            EffectCategory::Harmful => Category::Harmful,
            EffectCategory::Neutral => Category::Neutral,
        };
        (e.id.clone(), e.color, category)
    }));
    if !set
        && brewing::mod_effects()
            .iter()
            .map(|e| e.id)
            .ne(ids.iter().map(String::as_str))
    {
        log::warn!("Horizon: the mods' mob effects changed; restart the game to use them");
    }
}

/// the mods' thrown entity types, as their mob effects: set once, from the
/// startup root. Living types are mobs, loaded with vanilla's from the
/// content root's `entity-types.json`.
fn register_entity_types(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::projectiles;
    let mut types = horizon_client::mod_content::entity_types(mods);
    types.retain(|t| t.item.is_some());
    let set = projectiles::register_thrown_types(
        types
            .iter()
            .filter_map(|t| Some((t.id.clone(), t.item.clone()?, t.damage))),
    );
    if !set
        && projectiles::thrown_types()
            .iter()
            .map(|t| t.id)
            .ne(types.iter().map(|t| t.id.as_str()))
    {
        log::warn!("Horizon: the mods' entity types changed; restart the game to use them");
    }
}

/// the mods' menu types, as their entity types: set once, from the startup
/// root.
fn register_menu_types(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::containers;
    let menus = horizon_client::mod_content::menu_types(mods);
    let set = containers::register_mod_menu_types(menus.iter().map(|m| {
        let results = m.results.iter().map(|&r| usize::from(r)).collect();
        let crafting = m.crafting.map(|c| containers::ModCrafting {
            grid: c.grid.into(),
            size: c.size.into(),
            result: c.result.into(),
        });
        let links = m.links.map(|l| containers::ModLinks {
            from: l.from.into(),
            count: l.count.into(),
        });
        (
            m.id.clone(),
            m.slots.into(),
            m.data_slots.into(),
            results,
            crafting,
            links,
        )
    }));
    if !set
        && containers::mod_menu_types()
            .iter()
            .map(|m| m.id)
            .ne(menus.iter().map(|m| m.id.as_str()))
    {
        log::warn!("Horizon: the mods' menu types changed; restart the game to use them");
    }
}

/// What the launcher writes beside a game it built with native mods: their ids under `mods`.
const BUILD_INFO: &str = "native-mods.json";

/// `native-mods.json`'s `mods` list is not empty: the game was built with mods.
fn build_lists_mods(built: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(built)
        .ok()
        .and_then(|info| info["mods"].as_array().map(|mods| !mods.is_empty()))
        .unwrap_or(false)
}

/// Whether the game playing was built with native mods: horizon-mixins applied bundles to its
/// source (`HORIZON_MIXIN_MODS`, set by the build script from the record it leaves, as with a mod
/// author's own build that has no [`BUILD_INFO`]), trait mods are installed, or the launcher wrote
/// their ids into [`BUILD_INFO`] beside the executable. A release or Nightly has none of them.
pub(super) fn built_with_native_mods() -> bool {
    option_env!("HORIZON_MIXIN_MODS").is_some_and(|mods| !mods.is_empty())
        || !crate::mod_api::mods().is_empty()
        || std::env::current_exe()
            .ok()
            .and_then(|exe| std::fs::read_to_string(exe.with_file_name(BUILD_INFO)).ok())
            .is_some_and(|built| build_lists_mods(&built))
}

/// The native mods the launcher built this game with, `id version, id version`, for the crash
/// report (`built`: its [`BUILD_INFO`]). None for a release or Nightly, which have no mods.
fn native_mods_detail(built: &str) -> Option<String> {
    let info: serde_json::Value = serde_json::from_str(built).unwrap_or_default();
    let list = info["mods"]
        .as_array()?
        .iter()
        .map(|m| {
            let text = |key: &str| m[key].as_str().unwrap_or("?").to_owned();
            format!("{} {}", text("id"), text("version"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    (!list.is_empty()).then_some(list)
}

/// Whether a content mod's files can switch with a world open (`mod_content::live_switch`);
/// `None` when its content can't be read.
type Verdict = Option<std::result::Result<(), &'static str>>;

/// Each mod's [`Verdict`] by its path and modification time. Reading a mod's content walks and
/// checks every file (seconds for a large pack), and a start read each mod three times: the
/// content root, then the window's and the server thread's `start_mods`.
/// shortcut: a file edited inside an unpacked mod folder keeps the folder's time; `--dev-mod`
/// folders are never remembered, a player's are replaced whole by the launcher.
static VERDICTS: std::sync::Mutex<
    std::collections::BTreeMap<(PathBuf, std::time::SystemTime), Verdict>,
> = std::sync::Mutex::new(std::collections::BTreeMap::new());

fn verdict_key(path: &std::path::Path) -> Option<(PathBuf, std::time::SystemTime)> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some((path.to_owned(), modified))
}

/// Remembers the verdict of `content`, read from `path` as [`horizon_mods::Content::read`] does.
fn remember_verdict(path: &std::path::Path, content: &horizon_mods::Content) {
    if let Some(key) = verdict_key(path) {
        let verdict = Some(horizon_client::mod_content::live_switch(content));
        VERDICTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, verdict);
    }
}

/// The mod at `path`'s [`Verdict`], read once per version of it (never remembered for `dev`).
fn verdict(path: &std::path::Path, manifest: &horizon_mods::Manifest, dev: bool) -> Verdict {
    let key = if dev { None } else { verdict_key(path) };
    if let Some(key) = &key
        && let Some(verdict) = VERDICTS.lock().unwrap_or_else(|e| e.into_inner()).get(key)
    {
        return *verdict;
    }
    let verdict = horizon_mods::Content::read(path, manifest)
        .ok()
        .map(|content| horizon_client::mod_content::live_switch(&content));
    if let Some(key) = key {
        VERDICTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, verdict);
    }
    verdict
}

/// Notes a content mod's id, and whether its files can switch with a world
/// open (`mod_content::live_switch`).
fn live_switch_of(
    path: &std::path::Path,
    package: &horizon_mods::Package,
    dev: bool,
    content_ids: &mut std::collections::BTreeSet<String>,
    live_ids: &mut std::collections::BTreeSet<String>,
) {
    let id = &package.manifest.id;
    content_ids.insert(id.clone());
    match verdict(path, &package.manifest, dev) {
        Some(Ok(())) if package.wasm.is_none() => {
            live_ids.insert(id.clone());
        }
        Some(Err(why)) => log::debug!("Horizon: mod {id} switches on a restart: {why}"),
        _ => {}
    }
}

/// A row for each native mod the launcher installed in `dir` (`<id>.hnmod`) that this game wasn't
/// built with (`built`: its [`BUILD_INFO`], empty for a release or Nightly) and that has no row
/// yet. The page said "No mods" before, and players copied the files into `horizon-mods` (#4770).
/// Its toggle is the `mod.<id>` line the launcher's next build follows.
fn not_built(dir: &std::path::Path, built: &str, rows: &[Installed]) -> Vec<Installed> {
    let info: serde_json::Value = serde_json::from_str(built).unwrap_or_default();
    let in_build = |id: &str| {
        info["mods"]
            .as_array()
            .is_some_and(|mods| mods.iter().any(|m| m["id"] == id))
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|e| e == "hnmod"));
    paths.sort();
    let mut out: Vec<Installed> = Vec::new();
    for path in paths {
        let Ok(bundle) = horizon_mods::index::native::Bundle::read(&path) else {
            continue;
        };
        let m = bundle.manifest;
        let listed = rows.iter().chain(&out).any(|row| row.id == m.id);
        if !listed && !in_build(&m.id) {
            out.push(Installed {
                title: format!("{} {}", m.id, m.version),
                id: m.id,
                description: String::new(),
                problem: Some(mod_list::NOT_BUILT.to_owned()),
                dev: false,
                native: None,
            });
        }
    }
    out
}

/// A WebAssembly code mod's row on the Mods page.
const STOPPED: &str = "Doesn't run any more: needs a native port";
/// The player was told once about the WebAssembly mods that stopped (`horizon-mods/<this>`).
const NOTICE_SEEN: &str = ".native-notice";

impl Game<'_> {
    /// Lists the installed mods on the Mods page. Content mods load with the resources
    /// (`content_root`); native ones are built into the game (`native_hooks.rs`). WebAssembly code
    /// mods don't run any more: their rows say so, and the player hears it once in the chat
    /// (`tell_native_now`).
    pub(super) fn start_mods(&mut self) {
        let dir = Self::game_dir().join(mod_list::DIR);
        let mut paths = horizon_mods::find(&dir);
        paths.retain(|p| !self.mods.dev.contains(p));
        paths.extend(self.mods.dev.iter().cloned());
        let (mut content_ids, mut live_ids) = Default::default();
        self.settings.mods.installed = paths
            .iter()
            .map(|path| {
                let file = path.file_name().unwrap_or_default().to_string_lossy();
                let package = match horizon_mods::Package::read(path) {
                    Ok(package) => package,
                    Err(e) => {
                        return Installed {
                            id: String::new(),
                            title: file.into_owned(),
                            description: String::new(),
                            problem: Some(format!("Can't be read: {e}")),
                            dev: false,
                            native: None,
                        };
                    }
                };
                let m = &package.manifest;
                let problem = package.wasm.is_some().then(|| STOPPED.to_owned());
                let dev = self.mods.dev.contains(path);
                live_switch_of(path, &package, dev, &mut content_ids, &mut live_ids);
                Installed {
                    id: m.id.clone(),
                    title: format!("{} {}", m.name, m.version),
                    description: m.description.clone(),
                    problem,
                    dev,
                    native: None,
                }
            })
            .collect();
        (self.mods.content_ids, self.mods.live_ids) = (content_ids, live_ids);
        self.ui.options.mods_live = self.mods_live_now();
        if self.mods.applied.is_none() {
            self.mods.applied = Some(self.settings.mods.enabled.clone());
        }
        // A mod with a native version built in says so instead, and isn't news.
        self.list_native_mods();
        self.sync_native_switches();
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| std::fs::read_to_string(exe.with_file_name(BUILD_INFO)).ok())
            .unwrap_or_default();
        let built_mods = native_mods_detail(&built);
        let built_with_mods = built_mods.is_some();
        if let Some(list) = built_mods {
            horizon_client::crash::set_detail("Native Mods", list);
        }
        horizon_client::gfx::native_render::set_mods_built_in(
            built_with_mods
                || built_with_native_mods()
                || self
                    .settings
                    .mods
                    .installed
                    .iter()
                    .any(|m| m.native.is_some()),
        );
        let native_dir = Self::game_dir().join(mod_list::NATIVE_DIR);
        let unbuilt = not_built(&native_dir, &built, &self.settings.mods.installed);
        self.settings.mods.installed.extend(unbuilt);
        let installed = &self.settings.mods.installed;
        self.mods.stopped = installed
            .iter()
            .filter(|row| row.problem.as_deref() == Some(STOPPED))
            .map(|row| row.title.clone())
            .collect();
    }
    /// The content mods the Mods page lets the player switch with a world
    /// open: in single player and on an open LAN world, those with only
    /// assets and data; none on another server, which owns its data.
    pub(super) fn mods_live_now(&self) -> std::collections::BTreeSet<String> {
        match self.integrated.is_some() || self.remote.is_none() {
            true => self.mods.live_ids.clone(),
            false => Default::default(),
        }
    }
    /// The content mods the resources load with: the ones applied, which a
    /// switch that needs a restart doesn't change.
    pub(super) fn applied_mods(&self) -> std::collections::BTreeSet<String> {
        self.mods
            .applied
            .clone()
            .unwrap_or_else(|| self.settings.mods.enabled.clone())
    }
    /// The player turned content mods on or off (Options > Mods, or an
    /// install from Browse Mods): those with only assets and data apply now
    /// (docs/design/mod-hotload.md), through the resource reload and then the
    /// server's data reload (`mods_reloaded`); those that add blocks, items or
    /// worldgen wait for a restart, and the Mods page says so.
    pub(super) fn switch_mods(&mut self) {
        let Some(applied) = self.mods.applied.clone() else {
            return;
        };
        let wanted = self.settings.mods.enabled.clone();
        if applied == wanted {
            return;
        }
        // Native mods' `mod.<id>` lines follow the player's choice as they are
        // (`sync_native_switches`); a content mod with blocks, items or
        // worldgen keeps the state it loaded with until a restart.
        let content = &self.mods.content_ids;
        let mut next = wanted.clone();
        let mut waiting = Vec::new();
        for id in applied.symmetric_difference(&wanted) {
            if !content.contains(id) || self.mods.live_ids.contains(id) {
                continue;
            }
            log::info!("Horizon: mod {id} needs a restart to switch");
            waiting.push(id.clone());
            if applied.contains(id) {
                next.insert(id.clone());
            } else {
                next.remove(id);
            }
        }
        self.ui.options.mods_note = (!waiting.is_empty())
            .then(|| lang::tr("horizon.mods.switch.restart", &[&waiting.join(", ")]));
        let changed = applied
            .symmetric_difference(&next)
            .any(|id| content.contains(id));
        if !changed {
            self.mods.applied = Some(next);
            return;
        }
        log::info!(
            "Horizon: content mods switched live: {:?} -> {:?}",
            applied,
            next
        );
        self.mods.applied = Some(next);
        self.mods.live = Some(std::time::Instant::now());
        self.reload_packs();
    }
    /// A live switch's resources applied: the server reloads its data packs
    /// from the new root, as `/reload` does (recipes, tags, loot tables,
    /// advancements, functions), and sends them to its players.
    pub(super) fn mods_reloaded(&mut self, applied: bool) {
        let Some(started) = self.mods.live.take() else {
            return;
        };
        let resources_ms = started.elapsed().as_millis();
        if !applied {
            return;
        }
        let root = self.options.resources.clone();
        let enabled = self.applied_mods();
        let reload = move |g: &mut Game<'_>| {
            let at = std::time::Instant::now();
            g.options.resources = root;
            g.settings.mods.enabled = enabled.clone();
            g.mods.applied = Some(enabled);
            g.mods.data_switched = true;
            if g.world_root.as_os_str().is_empty() {
                return;
            }
            g.reload_datapacks(true);
            log::info!(
                "Horizon: content mods switched live: server data reloaded in {} ms",
                at.elapsed().as_millis()
            );
        };
        log::info!("Horizon: content mods switched live: resources reloaded in {resources_ms} ms");
        if let Some(server) = &self.integrated {
            server.post(reload);
        } else if self.remote.is_none() {
            reload(self);
        }
    }
    /// Trait mods follow the player's switches at once
    /// (`mod_api::set_mod_on`): the next dispatch skips a mod turned off.
    /// Mixin code checks `native_on` itself.
    pub(super) fn sync_native_switches(&self) {
        for id in horizon_client::mod_api::mod_ids() {
            let on = !self.settings.mods.disabled.contains(*id);
            horizon_client::mod_api::set_mod_on(id, on);
        }
    }
    /// Once, in the first world after the update: which WebAssembly mods stopped.
    pub(super) fn tell_native_now(&mut self) {
        if self.mods.stopped.is_empty() || self.player.is_none() {
            return;
        }
        let names = std::mem::take(&mut self.mods.stopped).join(", ");
        let seen = Self::game_dir().join(mod_list::DIR).join(NOTICE_SEEN);
        if seen.exists() {
            return;
        }
        self.system_message(Text::literal(format!(
            "Horizon mods are native now: {names} stopped running until they have a native version. The launcher's Mods page has the native ones."
        )));
        let _ = std::fs::write(&seen, "");
    }
}

/// Mob types loaded from a scratch root whose `entity-types.json` has a
/// zombie, a villager, and the rows a content root writes for a mod's types
/// (`mod_content`): `ruby:ghoul` (a zombie with 40 health), `ruby:settler` (a
/// villager as it is) and `ruby:bolt` (a thrown snowball, not a mob).
#[cfg(test)]
pub(super) fn living_mod_types()
-> std::collections::HashMap<String, std::sync::Arc<horizon_client::mobs::MobType>> {
    let zombie =
        r#""width":0.6,"height":1.95,"eye_height":1.74,"category":"monster","fire_immune":false"#;
    let villager =
        r#""width":0.6,"height":1.95,"eye_height":1.62,"category":"misc","fire_immune":false"#;
    let trader =
        r#""width":0.6,"height":1.95,"eye_height":1.62,"category":"creature","fire_immune":false"#;
    let table = format!(
        r#"{{"minecraft:zombie":{{{zombie},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.23000000417232513}}}},
        "minecraft:villager":{{{villager},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.5}}}},
        "minecraft:wandering_trader":{{{trader},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.7}}}},
        "minecraft:snowball":{{"width":0.25,"height":0.25,"eye_height":0.0,"category":"misc","fire_immune":false}},
        "ruby:ghoul":{{{zombie},"attributes":{{"minecraft:max_health":40.0,"minecraft:movement_speed":0.23000000417232513}},"template":"minecraft:zombie"}},
        "ruby:settler":{{{villager},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.5}},"template":"minecraft:villager"}},
        "ruby:merchant":{{{trader},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.7}},"template":"minecraft:wandering_trader"}},
        "ruby:bolt":{{"width":0.25,"height":0.25,"eye_height":0.0,"category":"misc","fire_immune":false,"template":"minecraft:snowball"}}}}"#
    );
    // A folder per call: tests run in parallel in one process, and each removes its own.
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "horizon-living-mod-types-{}-{call}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("entity-types.json"), table).unwrap();
    let types = horizon_client::mobs::MobTypes::load_for_server(&root).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    types
        .types
        .into_iter()
        .map(|(id, t)| (id, std::sync::Arc::new(t)))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sound_mod_dispatch_changes_real_channels_and_cancellation_short_circuits() {
        use crate::mod_api::{Flow, Hooks, SoundPlayEvent};
        use horizon_client::sound_engine::{
            Attenuation, PlayResult, SoundEngine, SoundInstance, test_backend,
        };
        use horizon_client::sound_events::SoundRegistry;
        use horizon_core::random::LegacyRandom;
        struct First;
        impl Hooks for First {
            fn sound_play(&self, event: &mut SoundPlayEvent<'_>) -> Flow {
                assert_eq!(event.sound.identifier, "test:beep");
                event.volume *= 0.5;
                Flow::Continue
            }
        }
        struct Second;
        impl Hooks for Second {
            fn sound_play(&self, event: &mut SoundPlayEvent<'_>) -> Flow {
                assert_eq!(event.volume, 0.4);
                event.pitch = 2.0;
                Flow::Continue
            }
        }
        struct Cancel;
        impl Hooks for Cancel {
            fn sound_play(&self, _: &mut SoundPlayEvent<'_>) -> Flow {
                Flow::Cancel
            }
        }
        struct Never;
        impl Hooks for Never {
            fn sound_play(&self, _: &mut SoundPlayEvent<'_>) -> Flow {
                panic!("dispatch after cancellation")
            }
        }
        static MODS: std::sync::LazyLock<Vec<Box<dyn Hooks>>> =
            std::sync::LazyLock::new(|| vec![Box::new(First), Box::new(Second)]);
        static CANCEL: std::sync::LazyLock<Vec<Box<dyn Hooks>>> =
            std::sync::LazyLock::new(|| vec![Box::new(Cancel), Box::new(Never)]);
        let mut registry = SoundRegistry::default();
        registry.add_file("test", br#"{"beep":{"sounds":["beep"]}}"#, &|_, _| true);
        let mut engine = SoundEngine::new(registry);
        engine.listening = true;
        let mut backend = test_backend::Null::default();
        let instance = || {
            SoundInstance::new(
                "test:beep",
                4,
                0.8,
                1.0,
                LegacyRandom::new(1),
                false,
                0,
                Attenuation::None,
                [0.0; 3],
                true,
            )
        };
        engine.set_test_playback_hook(|sound, volume, pitch, cancel| {
            crate::mod_api::dispatch_sound_play(&MODS, sound, volume, pitch, cancel)
        });
        let (id, result) = engine.play_new(instance(), false, &mut backend);
        assert_eq!(result, PlayResult::Started);
        let channel = engine.channel(id).unwrap();
        assert_eq!(backend.volume[&channel], 0.4);
        assert_eq!(backend.pitch[&channel], 2.0);
        let notifications = engine.notifications.len();
        engine.set_test_playback_hook(|sound, volume, pitch, cancel| {
            crate::mod_api::dispatch_sound_play(&CANCEL, sound, volume, pitch, cancel)
        });
        let (_, result) = engine.play_new(instance(), false, &mut backend);
        assert_eq!(result, PlayResult::NotStarted);
        assert_eq!(backend.volume.len(), 1);
        assert_eq!(engine.notifications.len(), notifications);
    }
    use super::*;
    use std::io::Write;

    #[test]
    #[ignore = "prepared resources: run explicitly for native content"]
    fn built_native_sound_content_reaches_the_resource_root_and_obeys_disable() {
        let base = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources required"),
        );
        let scratch =
            std::env::temp_dir().join(format!("horizon-native-content-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let version = scratch.join("version");
        let home = scratch.join("game");
        std::fs::create_dir_all(&home).unwrap();
        let content = version.join("native-content/soundproof");
        std::fs::create_dir_all(content.join("assets/soundproof/sounds")).unwrap();
        std::fs::write(content.join("horizon-mod.toml"), "id=\"soundproof\"\nname=\"Sound proof\"\nversion=\"1\"\napi=\"0.2\"\ncapabilities=[\"registries\"]\n").unwrap();
        std::fs::write(
            content.join("assets/soundproof/sounds.json"),
            r#"{"proof":{"sounds":["soundproof:proof"]}}"#,
        )
        .unwrap();
        std::fs::write(
            content.join("assets/soundproof/sounds/proof.ogg"),
            b"synthetic existence marker; no audio decode",
        )
        .unwrap();
        let legacy = home.join(mod_list::DIR).join("soundproof");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::copy(
            content.join("horizon-mod.toml"),
            legacy.join("horizon-mod.toml"),
        )
        .unwrap();
        let enabled = ["soundproof".to_owned()].into();
        let empty = Default::default();
        let root = content_root_at(&base, &empty, &empty, &[], &home, Some(&version));
        assert_ne!(root, base);
        let sounds = horizon_client::sound_events::SoundRegistry::load(&root);
        assert!(!sounds.tracks("soundproof:proof").is_empty());
        let disabled = ["soundproof".to_owned()].into();
        let off = content_root_at(&base, &enabled, &disabled, &[], &home, Some(&version));
        assert_eq!(off, base, "disabled native mods contribute no assets");
        std::fs::remove_dir_all(scratch).unwrap();
    }

    /// A content mod's living type is a mob of its template's species under
    /// its own id, with its own attributes: it saves its id with every field
    /// its template's class saves, loads back as itself, and rolls its own
    /// loot table. A thrown type stays a projectile.
    #[test]
    fn a_mod_living_type_is_its_templates_mob_with_its_own_attributes() {
        use horizon_client::mobs::{Mob, Species, drops::death_loot_table};
        use horizon_core::geometry::Vec3;
        let types = living_mod_types();
        let (zombie, ghoul) = (&types["minecraft:zombie"], &types["ruby:ghoul"]);
        assert_eq!(ghoul.species, Species::Zombie);
        assert_eq!(ghoul.class_id(), "minecraft:zombie");
        assert_eq!(types["ruby:settler"].species, Species::Villager);
        assert!(!types.contains_key("ruby:bolt"), "a thrown type is no mob");
        assert_eq!((zombie.max_health, ghoul.max_health), (20.0, 40.0));
        assert_eq!(ghoul.speed.to_bits(), zombie.speed.to_bits());
        assert_eq!(
            (ghoul.sounds, ghoul.texture.as_str()),
            (zombie.sounds, zombie.texture.as_str())
        );

        let at = Vec3::new(0.5, 64.0, 0.5);
        let (zombie, ghoul) = (
            Mob::new(1, zombie.clone(), at, 0.0),
            Mob::new(2, ghoul.clone(), at, 0.0),
        );
        assert_eq!(ghoul.health, 40.0);
        let saved =
            |m: &Mob| horizon_client::entity_nbt::mob_to_nbt(m, [0, 0, 0, 2], None, None, |_| None);
        let keys = |tag: &horizon_world::nbt::Tag| -> Vec<String> {
            let horizon_world::nbt::Tag::Compound(map) = tag else {
                panic!("a mob saves a compound");
            };
            map.keys().map(|k| k.to_string_lossy()).collect()
        };
        let (zombie_tag, ghoul_tag) = (saved(&zombie), saved(&ghoul));
        let Some(horizon_world::nbt::Tag::String(id)) = ghoul_tag.get("id") else {
            panic!("a saved mob has its id");
        };
        assert_eq!(id.to_string_lossy(), "ruby:ghoul");
        assert_eq!(
            keys(&ghoul_tag),
            keys(&zombie_tag),
            "it saves what a zombie saves"
        );
        for key in ["IsBaby", "CanBreakDoors", "DrownedConversionTime"] {
            assert!(keys(&ghoul_tag).iter().any(|k| k == key), "{key}");
        }
        let loaded = horizon_client::entity_nbt::mob_from_nbt(&ghoul_tag, &types, 3, None)
            .expect("it loads back");
        assert_eq!(
            (loaded.kind.id.as_str(), loaded.health),
            ("ruby:ghoul", 40.0)
        );
        assert_eq!(
            death_loot_table(&ghoul, true).as_deref(),
            Some("ruby:entities/ghoul")
        );
        assert_eq!(
            death_loot_table(&zombie, true).as_deref(),
            Some("minecraft:entities/zombie")
        );
    }

    /// A content mod's type from the wandering trader, the neutral template
    /// whose behaviour is all goals: a trader under its own id, with a
    /// trader's goals, that saves what a trader saves and loads back.
    #[test]
    fn a_mod_trader_type_is_a_wandering_trader_under_its_own_id() {
        use horizon_client::mobs::{Mob, Species};
        use horizon_world::nbt::Tag;
        let types = living_mod_types();
        let merchant = &types["ruby:merchant"];
        assert_eq!(merchant.species, Species::WanderingTrader);
        assert_eq!(merchant.class_id(), "minecraft:wandering_trader");
        let at = horizon_core::geometry::Vec3::new(0.5, 64.0, 0.5);
        let trader = Mob::new(1, types["minecraft:wandering_trader"].clone(), at, 0.0);
        let merchant = Mob::new(2, merchant.clone(), at, 0.0);
        assert!(merchant.ai.goals.has("Float") && trader.ai.goals.has("Float"));
        let saved =
            |m: &Mob| horizon_client::entity_nbt::mob_to_nbt(m, [0, 0, 0, 2], None, None, |_| None);
        let keys = |tag: &Tag| -> Vec<String> {
            let Tag::Compound(map) = tag else {
                panic!("a mob saves a compound");
            };
            map.keys().map(|k| k.to_string_lossy()).collect()
        };
        let tag = saved(&merchant);
        assert_eq!(
            keys(&tag),
            keys(&saved(&trader)),
            "it saves what a trader saves"
        );
        let loaded =
            horizon_client::entity_nbt::mob_from_nbt(&tag, &types, 3, None).expect("it loads back");
        assert_eq!(
            (loaded.kind.id.as_str(), loaded.species()),
            ("ruby:merchant", Species::WanderingTrader)
        );
    }

    /// A mod's live-switch verdict is read once per version of the mod: a later look finds
    /// it without reading the files again, a `--dev-mod` is always read, and the mod
    /// replaced (a new modification time) is read again.
    #[test]
    fn a_mods_live_switch_verdict_is_read_once_per_version_of_it() {
        let dir = std::env::temp_dir().join(format!("horizon-verdict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ruby = dir.join("ruby");
        std::fs::create_dir_all(ruby.join("assets/ruby/lang")).unwrap();
        std::fs::create_dir_all(ruby.join("data/ruby")).unwrap();
        std::fs::write(ruby.join("assets/ruby/lang/en_us.json"), "{}").unwrap();
        let manifest = horizon_mods::Manifest::parse(
            "id = \"ruby\"\nname = \"Ruby\"\nversion = \"0.1.0\"\napi = \"0.2\"\n\
             side = \"both\"\ncapabilities = [\"registries\"]\n",
        )
        .unwrap();
        assert_eq!(verdict(&ruby, &manifest, false), Some(Ok(())));
        // Worldgen added below `data/ruby`: the mod folder's own time stays.
        let worldgen = ruby.join("data/ruby/worldgen/configured_feature");
        std::fs::create_dir_all(&worldgen).unwrap();
        std::fs::write(worldgen.join("ore.json"), "{}").unwrap();
        let switches_on_restart = Some(Err("horizon.mods.switch.worldgen"));
        assert_eq!(verdict(&ruby, &manifest, true), switches_on_restart);
        assert_eq!(verdict(&ruby, &manifest, false), Some(Ok(())));
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::open(&ruby)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(verdict(&ruby, &manifest, false), switches_on_restart);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Only a build whose `native-mods.json` lists a mod counts as built with mods.
    #[test]
    fn only_a_listed_mod_makes_a_build_a_mod_build() {
        assert!(!build_lists_mods(""));
        assert!(!build_lists_mods("{}"));
        assert!(!build_lists_mods(r#"{"mods":[]}"#));
        assert!(!build_lists_mods(r#"{"mods":"worldmap"}"#));
        assert!(build_lists_mods(r#"{"mods":[{"id":"worldmap"}]}"#));
    }

    /// A minimal `.hnmod` the registry accepts: its manifest and its mixin's.
    fn hnmod(dir: &std::path::Path, id: &str) {
        let horizon = "a".repeat(40);
        let manifest = serde_json::json!({
            "schema": 1, "id": id, "version": "0.3.0", "game": "26.3",
            "horizon": horizon, "mixins": "b".repeat(40),
        });
        let mixin = format!(
            "[mod]\nid = \"{id}\"\nversion = \"0.3.0\"\n[compatibility]\nhorizon_revision = \"{horizon}\"\n"
        );
        let file = std::fs::File::create(dir.join(format!("{id}.hnmod"))).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("horizon-native.json", options).unwrap();
        zip.write_all(manifest.to_string().as_bytes()).unwrap();
        zip.start_file(format!("mixins/{id}/horizon-mixin.toml"), options)
            .unwrap();
        zip.write_all(mixin.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn the_crash_report_lists_the_builds_native_mods() {
        let built = r#"{"mods":[{"id":"machine_sample","version":"0.1.0","sha256":null},{"id":"pipes","version":"2.0"}]}"#;
        assert_eq!(
            native_mods_detail(built).as_deref(),
            Some("machine_sample 0.1.0, pipes 2.0")
        );
        // A release has no file; a build without mods has an empty list.
        assert_eq!(native_mods_detail(""), None);
        assert_eq!(native_mods_detail(r#"{"mods":[]}"#), None);
    }

    #[test]
    fn native_mods_installed_but_not_built_get_a_row_saying_so() {
        let dir = std::env::temp_dir().join(format!("horizon-not-built-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for id in ["veinminer", "zoom", "minimap"] {
            hnmod(&dir, id);
        }
        std::fs::write(dir.join("notes.txt"), "not a mod").unwrap();
        std::fs::write(dir.join("broken.hnmod"), "not a zip").unwrap();
        // A release: none is built in; the page lists them, sorted, instead of "No mods".
        let rows = not_built(&dir, "", &[]);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["minimap", "veinminer", "zoom"]);
        assert_eq!(rows[1].title, "veinminer 0.3.0");
        assert_eq!(rows[1].problem.as_deref(), Some(mod_list::NOT_BUILT));
        assert!(rows.iter().all(|r| r.native.is_none() && !r.dev));
        // A native build with zoom, and minimap's row already listed by its own hook.
        let built =
            r#"{"base":"nightly-2026-10-30-1da354b","mods":[{"id":"zoom","version":"0.3.0"}]}"#;
        let listed = Installed {
            id: "minimap".into(),
            ..rows[0].clone()
        };
        let rows = not_built(&dir, built, &[listed]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "veinminer");
        // No folder: no rows.
        assert!(not_built(&dir.join("missing"), "", &[]).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A trait mod's `register_goals` (`Mob.registerGoals`) gets every new
    /// mob: the goals of its type replace its template's, its `Float` goal
    /// floats the navigation as FloatGoal's constructor does, and the
    /// template's other types keep their goals. The only test here that
    /// installs the client library's hooks.
    #[test]
    fn a_mods_register_goals_replaces_its_types_goals() {
        use horizon_client::mobs::{
            Mob,
            goals::{self, Cx, Goal},
        };
        use horizon_client::mod_api::{Hooks, RegisterGoalsEvent, dispatch_register_goals};
        use horizon_client::native_client_hooks::{NativeClientHooks, install};
        use horizon_core::geometry::Vec3;
        // docs/native-mods.md's example.
        #[derive(Clone, Debug)]
        struct ToPost {
            post: [f64; 3],
        }
        impl Goal for ToPost {
            fn flags(&self) -> u8 {
                goals::MOVE
            }
            fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
                let [x, y, z] = self.post;
                let (dx, dy, dz) = (m.position.x - x, m.position.y - y, m.position.z - z);
                dx * dx + dy * dy + dz * dz > 16.0
            }
            fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
                !m.ai.nav.is_done()
            }
            fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
                m.nav_move_to(cx.w, self.post, 1.0);
            }
            fn box_clone(&self) -> Box<dyn Goal> {
                Box::new(self.clone())
            }
        }
        struct Guards;
        impl Hooks for Guards {
            fn register_goals(&self, event: &mut RegisterGoalsEvent<'_>) {
                if event.mob.kind.id == "ruby:guard" {
                    let ai = &mut event.mob.ai;
                    ai.goals.clear();
                    ai.targets.clear();
                    ai.goals.add(0, goals::Float);
                    ai.goals.add(
                        1,
                        ToPost {
                            post: [0.5, 64.0, 0.5],
                        },
                    );
                }
            }
        }
        static MODS: std::sync::LazyLock<Vec<Box<dyn Hooks>>> =
            std::sync::LazyLock::new(|| vec![Box::new(Guards)]);
        let installed = install(NativeClientHooks {
            menu_slot_rules: |_, _, _, _, _| {},
            sound_play: |_, _, _, _| {},
            living_heal: |_, _, _| {},
            player_chat: |_, _, _, _, _| {},
            player_chat_v2: |_, _, _, _, _, _| {},
            chat_decorate: |_, _, _, _| {},
            living_breathe: |_, _, _, _, _| {},
            living_drown: |_, _, _, _, _, _| {},
            living_fall: |_, _, _, _, _| {},
            living_jump: |_, _| {},
            register_goals: |mob| dispatch_register_goals(&MODS, mob),
            player_pre_login: |_, _, _, _| {},
            player_profile: |_, _, _| {},
            ingredient: |_, _, _| {},
            ingredient_test: |_, _, _, _| {},
            log_line: |_, _, _, _| {},
            handshake: |_, _, _, _, _| {},
        });
        assert!(installed.is_ok());

        let types = living_mod_types();
        let mut guard = (*types["ruby:ghoul"]).clone();
        guard.id = "ruby:guard".into();
        let at = Vec3::new(0.5, 64.0, 0.5);
        let guard = Mob::new(1, std::sync::Arc::new(guard), at, 0.0);
        let ghoul = Mob::new(2, types["ruby:ghoul"].clone(), at, 0.0);
        assert!(guard.ai.goals.has("Float") && guard.ai.goals.has("ToPost"));
        assert!(!guard.ai.goals.has("RandomLookAround"));
        assert!(!guard.ai.targets.has("NearestTarget"));
        assert!(guard.ai.nav.can_float);
        assert!(ghoul.ai.goals.has("RandomLookAround") && !ghoul.ai.goals.has("ToPost"));
        assert!(ghoul.ai.targets.has("NearestTarget"));
        assert!(!ghoul.ai.nav.can_float, "a zombie has no FloatGoal");
    }
}
