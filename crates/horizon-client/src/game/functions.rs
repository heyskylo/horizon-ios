//! Data packs in the game: functions (`/function`, `/return`, tick and load
//! tags, `/schedule`), predicates, `/reload` and `/datapack`.
use super::{
    Game,
    commands::{
        R, Target, err,
        execute::{ExecState, Source},
    },
};
use horizon_client::{
    command::{Arg, Context},
    datapack::{self, DataPack, DataStore, Function},
    save,
    scoreboard::bracketed,
    text::{self, Text},
};
use horizon_world::nbt::Tag;

/// A `TimerQueue` event.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Scheduled {
    /// `ns:function`, or `#ns:tag` for a function tag.
    pub id: String,
    pub time: i64,
    pub seq: u64,
}
impl FnState {
    /// The file `path` of the highest enabled pack that has it.
    pub(super) fn pack_file(&self, path: &str) -> Option<Vec<u8>> {
        self.enabled
            .iter()
            .rev()
            .filter_map(|id| self.packs.iter().find(|p| &p.id == id))
            .find_map(|pack| pack.read(path))
    }
}
#[derive(Default)]
pub(super) struct FnState {
    pub store: DataStore,
    packs: Vec<DataPack>,
    /// Enabled pack ids, in load order (later overrides earlier).
    enabled: Vec<String>,
    loaded: bool,
    /// Run the `minecraft:load` tag on the next tick.
    load_pending: bool,
    pub schedule: Vec<Scheduled>,
    next_seq: u64,
    /// A dedicated server's `function-permission-level`.
    pub permission: Option<u8>,
}
/// An immutable server-side candidate. Dropping it publishes nothing and releases its root lease.
pub(super) struct PreparedReload {
    baseline: ReloadBaseline,
    root: std::path::PathBuf,
    content: Option<std::sync::Arc<super::mods::PreparedContent>>,
    tables: Option<super::WorldTables>,
    health: horizon_client::health::HealthResources,
    functions: PreparedFunctions,
    sync: std::sync::Arc<horizon_client::lan::data::SyncData>,
    tree: std::sync::Arc<horizon_client::advancements::Tree>,
    recipe_payloads: Vec<Vec<u8>>,
}
struct ReloadBaseline {
    world_root: std::path::PathBuf,
    resources: std::path::PathBuf,
    generation: u64,
}
struct PreparedFunctions {
    store: DataStore,
    packs: Vec<DataPack>,
    enabled: Vec<String>,
}

/// What a function reported through `return`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    NoReturn,
    Value(i32),
    Fail,
}

/// The world's pack stack: the built-in pack (unless disabled), the packs
/// of `<world>/datapacks` and the resources root's feature packs, with the
/// enabled ids from `datapacks.json` (`PackRepository.reload`).
pub(super) struct PackStack {
    pub builtin: Option<DataPack>,
    pub packs: Vec<DataPack>,
    pub enabled: Vec<String>,
}
impl PackStack {
    /// The enabled packs in `enabled` (the selected) order, later overriding
    /// earlier: `PackRepository.openAllSelected` keeps the selected order.
    pub(super) fn enabled_packs(&self) -> impl Iterator<Item = &DataPack> {
        self.enabled
            .iter()
            .filter_map(|id| self.packs.iter().find(|p| &p.id == id))
    }
}
pub(super) fn pack_stack(
    world_root: &std::path::Path,
    resources: &std::path::Path,
    vanilla_disabled: bool,
    enabled_features: &[String],
) -> PackStack {
    let builtin = (!vanilla_disabled).then(|| DataPack::builtin(resources));
    let packs = datapack::discover_with_features(world_root, resources);
    let enabled = datapack::enabled_ids(world_root, &packs, enabled_features);
    PackStack {
        builtin,
        packs,
        enabled,
    }
}

/// The world's enabled packs' content files (`recipe/`, `loot_table/`,
/// `advancement/`, `tags/…`) for the tables to merge over the resource
/// root's own. Empty when the world has no packs.
pub(super) fn content_overlay(
    world_root: &std::path::Path,
    resources: &std::path::Path,
    vanilla_disabled: bool,
    enabled_features: &[String],
) -> Vec<(String, Vec<Vec<u8>>)> {
    let stack = pack_stack(world_root, resources, vanilla_disabled, enabled_features);
    let packs: Vec<&DataPack> = stack.enabled_packs().collect();
    if packs.is_empty() {
        return Vec::new();
    }
    DataStore::load(&packs).files.into_iter().collect()
}

/// The files the content tables load from: the world's enabled packs'
/// (`content_overlay`) with what native mods contribute for this load
/// (`native_data_pack_files`) below them. `PackOverlay::top` is a path's last
/// file, so a pack's file still overrides a mod's for the same path, and a
/// later contribution overrides an earlier one, as packs do. `Game` builds
/// its tables through here on a world's first open and on `/reload`, so the
/// mods are asked once per load.
pub(super) fn overlay_files(
    world_root: &std::path::Path,
    resources: &std::path::Path,
    vanilla_disabled: bool,
    enabled_features: &[String],
    contributed: Vec<(String, Vec<u8>)>,
) -> Vec<(String, Vec<Vec<u8>>)> {
    let mut files = content_overlay(world_root, resources, vanilla_disabled, enabled_features);
    if contributed.is_empty() {
        return files;
    }
    // A path's mod files in contribution order, under the pack files.
    let mut mods: std::collections::BTreeMap<String, Vec<Vec<u8>>> = Default::default();
    for (path, bytes) in contributed {
        // One bad file would fail its whole table and drop the world's pack
        // content with it, so it is skipped alone: every kind is JSON.
        if let Err(e) = serde_json::from_slice::<Box<serde_json::value::RawValue>>(&bytes) {
            log::warn!("Horizon: a native mod's data pack file {path} is not JSON, skipped: {e}");
            continue;
        }
        mods.entry(path).or_default().push(bytes);
    }
    for (path, versions) in files.iter_mut() {
        let Some(under) = mods.remove(path.as_str()) else {
            continue;
        };
        let mut merged = under;
        merged.append(versions);
        *versions = merged;
    }
    // The paths only a mod has.
    files.extend(mods);
    files
}

impl FnState {
    /// The level function files compile at (`ServerFunctionLibrary`'s
    /// `functionCompilationPermissions`): `function-permission-level` on a
    /// dedicated server, game master (2) otherwise. Whoever runs a function,
    /// its commands need no more than this.
    fn command_level(&self) -> u8 {
        self.permission.unwrap_or(2)
    }
}

impl Game<'_> {
    // ---- loading -----------------------------------------------------------

    fn datapack_settings_path(&self) -> std::path::PathBuf {
        self.world_root.join("datapacks.json")
    }
    /// Discover and load the data packs of the world (once, or on `/reload`).
    pub(super) fn ensure_datapacks(&mut self) {
        if self.fns.loaded || self.world_root.as_os_str().is_empty() {
            return;
        }
        self.reload_datapacks(false);
    }
    pub(super) fn reload_datapacks(&mut self, run_load: bool) {
        if run_load {
            if let Err(error) = self.reload_world_tables() {
                log::warn!("Horizon: data pack reload failed; active data retained: {error}");
                self.system_message(Text::translate("commands.reload.failure", &[]));
            }
            return;
        }
        let prepared = self.prepare_functions(&self.options.resources);
        self.install_functions(prepared);
    }
    fn prepare_functions(&self, root: &std::path::Path) -> PreparedFunctions {
        let stack = pack_stack(
            &self.world_root,
            root,
            self.cmds.vanilla_disabled,
            &self.level.enabled_features,
        );
        let packs: Vec<&DataPack> = stack.builtin.iter().chain(stack.enabled_packs()).collect();
        let store = DataStore::load(&packs);
        // Vanilla skips invalid individual functions, reporting their diagnostics.
        for (id, error) in &store.errors {
            log::warn!("Horizon: failed to load function {id}: {error}");
        }
        PreparedFunctions {
            store,
            packs: stack.packs,
            enabled: stack.enabled,
        }
    }
    fn install_functions(&mut self, prepared: PreparedFunctions) {
        let first_load = !self.fns.loaded;
        self.fns.store = prepared.store;
        self.fns.packs = prepared.packs;
        self.fns.enabled = prepared.enabled;
        self.fns.loaded = true;
        self.fns.load_pending = true;
        // Existing scheduled functions survive a reload; a first load adopts the saved queue.
        if first_load && self.fns.schedule.is_empty() {
            self.load_schedule();
        }
        self.sync_function_registries();
    }
    fn reload_baseline(&self) -> ReloadBaseline {
        ReloadBaseline {
            world_root: self.world_root.clone(),
            resources: self.options.resources.clone(),
            generation: self.damage_tag_generation,
        }
    }
    fn check_reload_baseline(&self, baseline: &ReloadBaseline) -> horizon_client::Result<()> {
        if self.remote.is_some()
            || self.world_root != baseline.world_root
            || self.options.resources != baseline.resources
            || self.damage_tag_generation != baseline.generation
        {
            return Err("Data reload was superseded or belongs to another world".into());
        }
        Ok(())
    }
    pub(super) fn prepare_content_reload(
        &mut self,
        content: std::sync::Arc<super::mods::PreparedContent>,
    ) -> horizon_client::Result<PreparedReload> {
        self.prepare_reload_at(content.root.clone(), Some(content))
    }
    pub(super) fn prepare_reload_at(
        &mut self,
        root: std::path::PathBuf,
        content: Option<std::sync::Arc<super::mods::PreparedContent>>,
    ) -> horizon_client::Result<PreparedReload> {
        if self.remote.is_some() {
            return Err("Only the authoritative server can prepare data reloads".into());
        }
        // Capture before every native query: a callback can reenter reload.
        // Its own side effects are not rolled back, but its changes cannot make
        // the tables already being prepared look like the new generation.
        let baseline = self.reload_baseline();
        let (tables, health) = self.build_world_tables_at(
            &root,
            true,
            &self.level.enabled_features,
            content.is_some() || self.mods.data_switched,
        )?;
        let overlay = super::pack_files(&tables);
        let functions = self.prepare_functions(&root);
        let sync = if let Some(host) = &self.lan {
            let kept = host.hub.lan.shared.sync();
            let mut sync = horizon_client::lan::data::SyncData::reload(&root, &overlay, &kept)?;
            sync.features.clone_from(&kept.features);
            std::sync::Arc::new(sync)
        } else {
            super::lan::prepare_cached_sync_data(&root, &overlay)?
        };
        let tree = std::sync::Arc::new(horizon_client::advancements::Tree::load_with(
            &root.join("data"),
            &overlay,
        ));
        let mut types = Vec::new();
        self.native_datapack_sync(&mut types);
        types.sort();
        types.dedup();
        let sources = if types.is_empty() {
            Vec::new()
        } else {
            horizon_client::crafting::Recipes::sources(&root, &overlay)?
        };
        let recipe_payloads = horizon_client::lan::recipes::encode_content(
            &types,
            &sources,
            horizon_client::lan::recipes::sent_before(),
        );
        self.check_reload_baseline(&baseline)?;
        Ok(PreparedReload {
            baseline,
            root,
            content,
            tables,
            health,
            functions,
            sync,
            tree,
            recipe_payloads,
        })
    }
    pub(super) fn commit_content_reload(
        &mut self,
        prepared: PreparedReload,
    ) -> horizon_client::Result<()> {
        self.check_reload_baseline(&prepared.baseline)?;
        // Everything fallible has finished. Publish the generation on this server's own thread.
        self.options.resources = prepared.root;
        if let Some(content) = prepared.content {
            self.mods.content = Some(content);
            self.mods.data_switched = true;
        }
        self.world_tables = prepared.tables;
        self.health_resources = prepared.health;
        self.damage_tag_generation = self.damage_tag_generation.wrapping_add(1);
        self.recipe_tables.categories = None;
        horizon_client::structure_blocks::forget_loot_data();
        self.install_functions(prepared.functions);
        horizon_client::lan::recipes::set_content(prepared.recipe_payloads);
        super::lan::install_cached_sync_data(
            prepared.sync.clone(),
            !self.pack_files().files.is_empty(),
        );
        if let Some(host) = self.lan.as_mut() {
            host.hub.reload_resources(prepared.sync);
            self.run_native_advancement_grants();
        }
        self.adv.tree = prepared.tree;
        self.adv.reset_tree();
        self.native_reload_listeners();
        self.native_server_data_loaded(true);
        Ok(())
    }
    fn save_datapack_settings(&self) -> std::io::Result<()> {
        let disabled: Vec<&String> = self
            .fns
            .packs
            .iter()
            .map(|p| &p.id)
            .filter(|id| !self.fns.enabled.contains(id))
            .collect();
        let json = serde_json::json!({"enabled": self.fns.enabled, "disabled": disabled});
        std::fs::write(self.datapack_settings_path(), json.to_string())
    }
    pub(super) fn sync_function_registries(&mut self) {
        let reg = &mut self.ui.registries;
        reg.functions = self.fns.store.functions.keys().cloned().collect();
        reg.function_tags = self.fns.store.function_tags.keys().cloned().collect();
        reg.predicates = self.fns.store.predicates.keys().cloned().collect();
        reg.item_modifiers = self.fns.store.item_modifiers.keys().cloned().collect();
        reg.schedules = self.fns.schedule.iter().map(|s| s.id.clone()).collect();
        reg.datapacks_enabled = std::iter::once("vanilla".to_owned())
            .chain(self.fns.enabled.iter().cloned())
            .collect();
        reg.datapacks_disabled = self
            .fns
            .packs
            .iter()
            .map(|p| p.id.clone())
            .filter(|id| !self.fns.enabled.contains(id))
            .collect();
        // A pack's worldgen ids join the completion registries
        // (`command-registries.json` lists only the game's own), and so do
        // its worldgen tag ids for the `#tag` completions. The packs are the
        // ones the world's worldgen is built from.
        let packs = horizon_client::datapack::discover_with_features(
            &self.world_root,
            &self.options.resources,
        );
        let enabled = horizon_client::datapack::enabled_ids(
            &self.world_root,
            &packs,
            &self.level.enabled_features,
        );
        let enabled: Vec<_> = enabled
            .iter()
            .filter_map(|id| packs.iter().find(|p| &p.id == id))
            .collect();
        let (overrides, _) = horizon_client::datapack::worldgen_overrides(&enabled, |_| true);
        for (bundle, registry) in [
            ("biomes", "minecraft:worldgen/biome"),
            ("structures", "minecraft:worldgen/structure"),
            ("structure_sets", "minecraft:worldgen/structure_set"),
            ("template_pools", "minecraft:worldgen/template_pool"),
            ("processor_lists", "minecraft:worldgen/processor_list"),
        ] {
            let entry = reg.by_registry.entry(registry.to_owned()).or_default();
            for (id, _) in overrides.entries(bundle) {
                if !entry.ids.contains(&id.to_owned()) {
                    entry.ids.push(id.to_owned());
                }
            }
            entry.ids.sort();
        }
        let entry = reg
            .by_registry
            .entry("minecraft:worldgen/biome".to_owned())
            .or_default();
        for (id, _, _) in overrides.tags("biome_tags") {
            if !entry.tags.contains(&id.to_owned()) {
                entry.tags.push(id.to_owned());
            }
        }
        entry.tags.sort();
        let entry = reg
            .by_registry
            .entry("minecraft:worldgen/structure".to_owned())
            .or_default();
        for (id, _, _) in overrides.tags("structure_tags") {
            if !entry.tags.contains(&id.to_owned()) {
                entry.tags.push(id.to_owned());
            }
        }
        entry.tags.sort();
    }

    // ---- scheduled events ------------------------------------------------------

    fn schedule_path(&self) -> std::path::PathBuf {
        self.world_root.join("data/minecraft/scheduled_events.dat")
    }
    fn load_schedule(&mut self) {
        let Some(root) = save::read_dat(&self.schedule_path()) else {
            return;
        };
        let Some(Tag::List(events)) = root.get("data").and_then(|d| d.get("events")) else {
            return;
        };
        for e in events {
            let (Some(Tag::Long(time)), Some(Tag::String(id)), Some(cb)) =
                (e.get("trigger_time"), e.get("id"), e.get("callback"))
            else {
                continue;
            };
            let _ = cb;
            self.fns.next_seq += 1;
            self.fns.schedule.push(Scheduled {
                id: id.to_string_lossy(),
                time: *time,
                seq: self.fns.next_seq,
            });
        }
    }
    pub(super) fn save_schedule(&self) {
        if !self.fns.loaded || self.remote.is_some() {
            return;
        }
        let mut events = self.fns.schedule.clone();
        events.sort_by_key(|e| (e.time, e.seq));
        let list = events
            .iter()
            .map(|e| {
                let (kind, name) = match e.id.strip_prefix('#') {
                    Some(tag) => ("minecraft:function_tag", tag),
                    None => ("minecraft:function", e.id.as_str()),
                };
                Tag::compound([
                    ("trigger_time", Tag::Long(e.time)),
                    ("id", Tag::String(e.id.as_str().into())),
                    (
                        "callback",
                        Tag::compound([
                            ("type", Tag::String(kind.into())),
                            ("id", Tag::String(name.into())),
                        ]),
                    ),
                ])
            })
            .collect();
        if events.is_empty() && !self.schedule_path().exists() {
            return;
        }
        let _ = save::write_dat(
            &self.schedule_path(),
            Tag::compound([
                ("data", Tag::compound([("events", Tag::List(list))])),
                ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
            ]),
        );
    }

    // ---- ticking -----------------------------------------------------------------

    /// `ServerFunctionManager.tick` and the scheduled events.
    pub(super) fn tick_functions(&mut self) {
        self.ensure_datapacks();
        if self.remote.is_some() {
            return;
        }
        self.exec = ExecState::default();
        let source = self.server_source();
        if self.fns.load_pending {
            self.fns.load_pending = false;
            for f in self.fns.store.tag_functions("minecraft:load") {
                self.run_function_by_id(&f, None, source);
            }
        }
        let now = self.world.game_time;
        loop {
            let due = self
                .fns
                .schedule
                .iter()
                .enumerate()
                .filter(|(_, e)| e.time <= now)
                .min_by_key(|(_, e)| (e.time, e.seq))
                .map(|(i, _)| i);
            let Some(i) = due else { break };
            let event = self.fns.schedule.remove(i);
            match event.id.strip_prefix('#') {
                Some(tag) => {
                    for f in self.fns.store.tag_functions(tag) {
                        self.run_function_by_id(&f, None, source);
                    }
                }
                None => self.run_function_by_id(&event.id, None, source),
            }
        }
        for f in self.fns.store.tag_functions("minecraft:tick") {
            self.run_function_by_id(&f, None, source);
        }
    }
    fn run_function_by_id(&mut self, id: &str, args: Option<&Tag>, source: Source) {
        self.exec.commands = 0;
        if let Some(f) = self.fns.store.functions.get(id).cloned() {
            // `ServerFunctionManager.execute` logs a function that failed to
            // instantiate or run; its lines' own failures stay silent.
            if let Err(e) = self.run_function(&f, args, source) {
                log::warn!(
                    "Horizon: failed to run function {id}: {}",
                    e.message.plain()
                );
            }
        }
    }

    /// The function branch of advancement rewards: resolve the optional function
    /// and execute it as the rewarded player, with suppressed output and GAMEMASTER permission.
    pub(super) fn run_advancement_reward_function(&mut self, id: &str) {
        self.ensure_datapacks();
        let Some(function) = self.fns.store.functions.get(id).cloned() else {
            return;
        };
        let Some(pos) = self.target_position(Target::Player) else {
            return;
        };
        // createCommandSourceStack belongs to the player, not an enclosing
        // execute's position, rotation, output sink or store callback.
        let source = Source {
            pos,
            rot: self.target_rotation(Target::Player),
            silent: true,
            ..self.player_source()
        };
        // The existing runner applies this level to nested function calls too.
        let permission = self
            .fns
            .permission
            .replace(horizon_client::command::vanilla::GAMEMASTERS);
        let callback = self.exec.callback.take();
        let result = self.run_function(&function, None, source);
        self.exec.callback = callback;
        self.fns.permission = permission;
        if let Err(error) = result {
            log::warn!(
                "Horizon: failed to run advancement reward function {id}: {}",
                error.message.plain()
            );
        }
    }

    // ---- running -----------------------------------------------------------------

    /// Run one function with `source`: its commands in order, until a `return`.
    pub(in crate::game) fn run_function(
        &mut self,
        f: &Function,
        args: Option<&Tag>,
        source: Source,
    ) -> R<Outcome> {
        let lines = f.instantiate(args).map_err(|(key, a)| {
            let args: Vec<&dyn std::fmt::Display> =
                a.iter().map(|s| s as &dyn std::fmt::Display).collect();
            err(
                "commands.function.instantiationFailure",
                &[&f.id, &horizon_client::text::Text::tr(&key, &args).plain()],
            )
        })?;
        if self.exec.function_depth >= 128 {
            return Ok(Outcome::NoReturn);
        }
        self.exec.function_depth += 1;
        let outer = self.exec.returned.take();
        let level = std::mem::replace(&mut self.ui.registries.permission, self.fns.command_level());
        let inner = Source {
            silent: true,
            store: None,
            ..source
        };
        for line in lines {
            let _ = self.execute_line(&line, inner);
            if self.exec.returned.is_some() {
                break;
            }
            if self.exec.commands > self.ui.rules.int("max_command_sequence_length").max(1) as u32 {
                break;
            }
        }
        self.ui.registries.permission = level;
        let returned = std::mem::replace(&mut self.exec.returned, outer);
        self.exec.function_depth -= 1;
        Ok(match returned {
            None => Outcome::NoReturn,
            Some(Some(v)) => Outcome::Value(v),
            Some(None) => Outcome::Fail,
        })
    }
    /// The functions a `FunctionArgument` names: a function or a tag.
    pub(in crate::game) fn functions_named(&mut self, name: &str) -> R<Vec<Function>> {
        self.ensure_datapacks();
        match name.strip_prefix('#') {
            // ServerFunctionLibrary.getTag: an unknown tag is empty.
            Some(tag) => {
                if !self.fns.store.function_tags.contains_key(tag) {
                    return Ok(Vec::new());
                }
                Ok(self
                    .fns
                    .store
                    .tag_functions(tag)
                    .iter()
                    .filter_map(|id| self.fns.store.functions.get(id).cloned())
                    .collect())
            }
            None => self
                .fns
                .store
                .functions
                .get(name)
                .cloned()
                .map(|f| vec![f])
                .ok_or_else(|| err("arguments.function.unknown", &[&name])),
        }
    }
    /// `if function`: what each function returns (None if it did not).
    pub(in crate::game) fn call_function_ref(
        &mut self,
        name: &str,
        args: Option<&Tag>,
    ) -> R<Vec<Option<i32>>> {
        let functions = self.functions_named(name)?;
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let mut out = Vec::new();
        for f in functions {
            out.push(match self.run_function(&f, args, source)? {
                Outcome::NoReturn => None,
                Outcome::Value(v) => Some(v),
                Outcome::Fail => Some(0),
            });
        }
        Ok(out)
    }

    // ---- commands ------------------------------------------------------------------

    /// `FunctionCommand.decorateOutputIfNeeded`: report returned values even for failure.
    pub(super) fn cmd_function(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::Resource(name)) = ctx.get("name").cloned() else {
            return Ok(0);
        };
        let functions = self.functions_named(&name)?;
        if functions.is_empty() {
            let id = name.trim_start_matches('#');
            return Err(err("commands.function.scheduled.no_functions", &[&id]));
        }
        let args = if ctx.has("with") {
            let r = self.data_ref_named(ctx, 0, "source")?;
            let data = self.data_of(&r)?;
            let tag = match ctx.get("path") {
                Some(Arg::Str(p)) => {
                    let path = horizon_client::nbt_path::NbtPath::parse(p)
                        .map_err(horizon_client::data_cmd::path_error)?;
                    horizon_client::data_cmd::single_tag(&path, &data)?
                }
                _ => data,
            };
            if !matches!(tag, Tag::Compound(_)) {
                return Err(err(
                    "commands.function.error.argument_not_compound",
                    &[&"tag"],
                ));
            }
            Some(tag)
        } else if let Some(Arg::Str(text)) = ctx.get("arguments") {
            Some(
                horizon_world::nbt::parse_snbt(text)
                    .map_err(|e| err("argument.nbt.invalid", &[&e]))?
                    .0,
            )
        } else {
            None
        };
        let message = if functions.len() == 1 {
            Text::translate(
                "commands.function.scheduled.single",
                &[Text::literal(functions[0].id.clone())],
            )
        } else {
            Text::translate(
                "commands.function.scheduled.multiple",
                &[horizon_client::scoreboard_cmd::format_list(
                    functions
                        .iter()
                        .map(|f| Text::literal(f.id.clone()))
                        .collect(),
                )],
            )
        };
        self.feedback(message);
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let mut sum: i32 = 0;
        let mut any = false;
        let mut failed = false;
        for f in functions {
            match self.run_function(&f, args.as_ref(), source)? {
                Outcome::NoReturn => {}
                Outcome::Value(v) => {
                    any = true;
                    sum = sum.wrapping_add(v);
                    let message = Text::translate(
                        "commands.function.result",
                        &[Text::literal(f.id.clone()), Text::literal(v.to_string())],
                    );
                    self.feedback(message);
                }
                Outcome::Fail => {
                    any = true;
                    failed = true;
                    self.feedback(Text::translate(
                        "commands.function.result",
                        &[Text::literal(f.id.clone()), Text::literal("0")],
                    ));
                }
            }
        }
        self.exec.callback = Some(if failed && sum == 0 {
            Some((false, 0))
        } else if any {
            Some((true, sum))
        } else {
            None
        });
        Ok(sum)
    }
    /// `return <value>` and `return fail` (`return run` is a chain).
    pub(super) fn cmd_return(&mut self, id: &str, ctx: &Context) -> R<i32> {
        if id == "return.fail" {
            self.finish_return(None);
            self.exec.callback = Some(Some((false, 0)));
            return Ok(0);
        }
        let value = match ctx.get("value") {
            Some(Arg::Int(v)) => *v,
            _ => 0,
        };
        self.finish_return(Some(value));
        Ok(value)
    }
    pub(super) fn cmd_schedule(&mut self, id: &str, ctx: &Context) -> R<i32> {
        self.ensure_datapacks();
        if id == "schedule.clear" {
            let Some(Arg::Resource(name)) = ctx.get("function").cloned() else {
                return Ok(0);
            };
            let before = self.fns.schedule.len();
            self.fns.schedule.retain(|e| e.id != name);
            let count = before - self.fns.schedule.len();
            if count == 0 {
                return Err(err("commands.schedule.cleared.failure", &[&name]));
            }
            self.feedback(Text::translate(
                "commands.schedule.cleared.success",
                &[Text::literal(count.to_string()), Text::literal(name)],
            ));
            self.sync_function_registries();
            return Ok(count as i32);
        }
        let Some(Arg::Resource(name)) = ctx.get("function").cloned() else {
            return Ok(0);
        };
        let time = match ctx.get("time") {
            Some(Arg::Int(t)) => *t,
            _ => 0,
        };
        if time == 0 {
            return Err(err("commands.schedule.same_tick", &[]));
        }
        let replace = !ctx.has("append");
        let tick_time = self.world.game_time + i64::from(time);
        let schedule_id = match name.strip_prefix('#') {
            // An unknown tag is empty, and scheduling it is allowed.
            Some(_) => name.clone(),
            None => {
                let Some(f) = self.fns.store.functions.get(&name) else {
                    return Err(err("arguments.function.unknown", &[&name]));
                };
                if f.is_macro() {
                    return Err(err("commands.schedule.macro", &[]));
                }
                name.clone()
            }
        };
        if replace {
            self.fns.schedule.retain(|e| e.id != schedule_id);
        }
        if !self
            .fns
            .schedule
            .iter()
            .any(|e| e.id == schedule_id && e.time == tick_time)
        {
            self.fns.next_seq += 1;
            let seq = self.fns.next_seq;
            self.fns.schedule.push(Scheduled {
                id: schedule_id,
                time: tick_time,
                seq,
            });
        }
        let key = if name.starts_with('#') {
            "commands.schedule.created.tag"
        } else {
            "commands.schedule.created.function"
        };
        self.feedback(Text::translate(
            key,
            &[
                Text::literal(name.trim_start_matches('#').to_owned()),
                Text::literal(time.to_string()),
                Text::literal(tick_time.to_string()),
            ],
        ));
        self.sync_function_registries();
        Ok((tick_time.rem_euclid(i64::from(i32::MAX))) as i32)
    }
    /// ReloadCommand: the message, then the (asynchronous) reload; returns 0.
    pub(super) fn cmd_reload(&mut self) -> R<i32> {
        self.feedback(Text::translate("commands.reload.success", &[]));
        self.reload_datapacks(true);
        Ok(0)
    }
    /// `PackLocationInfo.createChatLink`: the id decorated with its source
    /// (`pack.nameAndSource`, grey) in brackets, green when enabled.
    fn pack_link(&self, id: &str, enabled: bool) -> Text {
        let hint = if enabled { "disable" } else { "enable" };
        let style = text::Style {
            color: Some(if enabled { text::GREEN } else { text::RED }),
            click: Some(text::ClickEvent::SuggestCommand(format!(
                "/datapack {hint} \"{id}\""
            ))),
            ..text::Style::default()
        };
        let description = self
            .fns
            .packs
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| "Default data pack".to_owned(), DataPack::description);
        let style = text::Style {
            hover: Some(Box::new(Text::literal(format!("{id}\n{description}")))),
            ..style
        };
        let source = if id == "vanilla" {
            "pack.source.builtin"
        } else if FEATURE_PACKS.iter().any(|(p, _)| *p == id) {
            "pack.source.feature"
        } else {
            "pack.source.world"
        };
        let name = Text::translate(
            "pack.nameAndSource",
            &[Text::literal(id), Text::translate(source, &[])],
        )
        .with_color(text::GRAY);
        bracketed(name).with_style(&style)
    }
    /// The selected packs, lowest priority first.
    fn selected_packs(&self) -> Vec<String> {
        let vanilla = (!self.cmds.vanilla_disabled).then(|| "vanilla".to_owned());
        vanilla
            .into_iter()
            .chain(self.fns.enabled.iter().cloned())
            .collect()
    }
    /// `FileUtil.isPathPartPortable`: no trailing dot and no Windows device
    /// name (CON, PRN, AUX, NUL, COM and LPT with a digit from 1 to 9).
    fn is_portable_name(name: &str) -> bool {
        let stem = name.split('.').next().unwrap_or_default();
        let device = match stem.len() {
            3 => matches!(stem, "com" | "con" | "prn" | "aux" | "nul"),
            4 => matches!(&stem[..3], "com" | "lpt") && matches!(stem.as_bytes()[3], b'1'..=b'9'),
            _ => false,
        };
        !device && !name.ends_with('.')
    }
    /// `DataPackCommand.createPack`: a pack folder with its `data` folder and a
    /// `pack.mcmeta` in the world's datapacks folder.
    fn create_pack(&mut self, ctx: &Context) -> R<i32> {
        let id = match ctx.get("id") {
            Some(Arg::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let description = self.component_arg(ctx, "description")?;
        // FileUtil.isValidPathSegment: lowercase letters, digits, '-', '.' and '_'.
        let valid = !id.is_empty()
            && id != "."
            && id != ".."
            && id
                .chars()
                .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-' | '.' | '_'));
        if !valid {
            return Err(err("commands.datapack.create.invalid_name", &[&id]));
        }
        if !Self::is_portable_name(&id) {
            return Err(err("commands.datapack.create.invalid_full_name", &[&id]));
        }
        // Without a world there is no datapacks folder to write to.
        if self.world_root.as_os_str().is_empty() {
            return Err(err("commands.datapack.create.io_failure", &[&id]));
        }
        let datapacks = self.world_root.join("datapacks");
        let pack = datapacks.join(&id);
        if pack.exists() {
            return Err(err("commands.datapack.create.already_exists", &[&id]));
        }
        // The data pack version (SharedConstants), written as a bare major
        // number for both bounds, as vanilla does.
        let pack_format = horizon_core::version::DATA_PACK_FORMAT.0;
        let mcmeta = serde_json::json!({
            "pack": {
                "description": description.component(),
                "min_format": pack_format,
                "max_format": pack_format,
            }
        });
        let written = std::fs::create_dir_all(&datapacks)
            .and_then(|()| std::fs::create_dir(&pack))
            .and_then(|()| std::fs::create_dir(pack.join("data")))
            .and_then(|()| {
                std::fs::write(
                    pack.join("pack.mcmeta"),
                    serde_json::to_string_pretty(&mcmeta).unwrap_or_default(),
                )
            });
        if written.is_err() {
            return Err(err("commands.datapack.create.io_failure", &[&id]));
        }
        self.feedback(Text::translate(
            "commands.datapack.create.success",
            &[Text::literal(id)],
        ));
        Ok(1)
    }
    pub(super) fn cmd_datapack(&mut self, id: &str, ctx: &Context) -> R<i32> {
        self.ensure_datapacks();
        let name = match ctx.get("name") {
            Some(Arg::Str(n)) => Some(n.clone()),
            _ => None,
        };
        let selected = self.selected_packs();
        // `DataPackCommand.listAvailablePacks`: a feature pack the world's
        // features do not cover is not available.
        let available: Vec<String> = std::iter::once("vanilla".to_owned())
            .chain(self.fns.packs.iter().map(|p| p.id.clone()))
            .filter(|p| !selected.contains(p))
            .filter(|p| {
                FEATURE_PACKS
                    .iter()
                    .find(|(f, _)| *f == p.as_str())
                    .is_none_or(|(_, flag)| self.level.enabled_features.iter().any(|f| f == flag))
            })
            .collect();
        let known = |g: &Self, n: &str| {
            n == "vanilla"
                || FEATURE_PACKS.iter().any(|(p, _)| *p == n)
                || g.fns.packs.iter().any(|p| p.id == n)
        };
        match id {
            "datapack.create" => self.create_pack(ctx),
            "datapack.list" => {
                let want_enabled = ctx.has("enabled") || (!ctx.has("available"));
                let want_available = ctx.has("available") || (!ctx.has("enabled"));
                let mut total = 0;
                if want_enabled {
                    if selected.is_empty() {
                        self.feedback(Text::translate("commands.datapack.list.enabled.none", &[]));
                    } else {
                        let links: Vec<Text> =
                            selected.iter().map(|p| self.pack_link(p, true)).collect();
                        total += links.len();
                        self.feedback(Text::translate(
                            "commands.datapack.list.enabled.success",
                            &[
                                Text::literal(links.len().to_string()),
                                horizon_client::scoreboard_cmd::format_list(links),
                            ],
                        ));
                    }
                }
                if want_available {
                    if available.is_empty() {
                        self.feedback(Text::translate(
                            "commands.datapack.list.available.none",
                            &[],
                        ));
                    } else {
                        let links: Vec<Text> =
                            available.iter().map(|p| self.pack_link(p, false)).collect();
                        total += links.len();
                        self.feedback(Text::translate(
                            "commands.datapack.list.available.success",
                            &[
                                Text::literal(links.len().to_string()),
                                horizon_client::scoreboard_cmd::format_list(links),
                            ],
                        ));
                    }
                }
                Ok(total as i32)
            }
            "datapack.enable" => {
                let name = name.unwrap_or_default();
                if !known(self, &name) {
                    return Err(err("commands.datapack.unknown", &[&name]));
                }
                if selected.contains(&name) {
                    return Err(err("commands.datapack.enable.failed", &[&name]));
                }
                if let Some((_, flag)) = FEATURE_PACKS.iter().find(|(p, _)| *p == name)
                    && !self.level.enabled_features.iter().any(|f| f == flag)
                {
                    // `DataPackCommand`: the pack's requested features must
                    // be a subset of the world's enabled ones.
                    return Err(err(
                        "commands.datapack.enable.failed.no_flags",
                        &[&name, flag],
                    ));
                }
                let previous = (self.fns.enabled.clone(), self.cmds.vanilla_disabled);
                if name == "vanilla" {
                    self.cmds.vanilla_disabled = false;
                } else {
                    let existing = match ctx.get("existing") {
                        Some(Arg::Str(e)) => Some(e.clone()),
                        _ => None,
                    };
                    let position = if ctx.has("first") {
                        0
                    } else if let Some(e) = &existing {
                        let Some(i) = self.fns.enabled.iter().position(|p| p == e) else {
                            return Err(err("commands.datapack.unknown", &[e]));
                        };
                        if ctx.has("before") { i } else { i + 1 }
                    } else {
                        self.fns.enabled.len()
                    };
                    self.fns.enabled.insert(position, name.clone());
                }
                self.feedback(Text::translate(
                    "commands.datapack.modify.enable",
                    &[self.pack_link(&name, true)],
                ));
                self.apply_pack_change(previous)?;
                Ok(self.selected_packs().len() as i32)
            }
            _ => {
                let name = name.unwrap_or_default();
                if !known(self, &name) {
                    return Err(err("commands.datapack.unknown", &[&name]));
                }
                if !selected.contains(&name) {
                    return Err(err("commands.datapack.disable.failed", &[&name]));
                }
                if FEATURE_PACKS.iter().any(|(p, _)| *p == name) {
                    // `DataPackCommand`'s ERROR_CANNOT_DISABLE_FEATURE: the
                    // flag stays on for the world's life.
                    return Err(err("commands.datapack.disable.failed.feature", &[&name]));
                }
                let previous = (self.fns.enabled.clone(), self.cmds.vanilla_disabled);
                if name == "vanilla" {
                    self.cmds.vanilla_disabled = true;
                } else {
                    self.fns.enabled.retain(|p| p != &name);
                }
                self.feedback(Text::translate(
                    "commands.datapack.modify.disable",
                    &[self.pack_link(&name, true)],
                ));
                self.apply_pack_change(previous)?;
                Ok(self.selected_packs().len() as i32)
            }
        }
    }
    /// `ReloadCommand.reloadPacks`: no message of its own. It reloads the
    /// server's data as `/reload` does (`reload_datapacks`), the content
    /// tables with the functions.
    fn apply_pack_change(&mut self, previous: (Vec<String>, bool)) -> R<()> {
        if let Err(error) = self.save_datapack_settings() {
            self.fns.enabled = previous.0;
            self.cmds.vanilla_disabled = previous.1;
            log::warn!("Horizon: data pack selection could not be saved: {error}");
            return Err(err("commands.reload.failure", &[]));
        }
        if let Err(error) = self.reload_world_tables() {
            self.fns.enabled = previous.0;
            self.cmds.vanilla_disabled = previous.1;
            if let Err(restore) = self.save_datapack_settings() {
                log::warn!(
                    "Horizon: previous data pack selection could not be restored: {restore}"
                );
            }
            log::warn!("Horizon: data pack selection was not applied: {error}");
            return Err(err("commands.reload.failure", &[]));
        }
        Ok(())
    }

    // ---- predicates -------------------------------------------------------------------

    /// `ResourceOrIdArgument.getLootPredicate`: accept the inline or named
    /// predicate, then test it with the existing source entity and origin.
    pub(in crate::game) fn check_predicate(&mut self, id: &str) -> R<bool> {
        self.ensure_datapacks();
        // ResourceOrIdArgument retains either an inline value or a named id.
        // Both use the same evaluator and command-source context below.
        let json = if id.starts_with('{') {
            let (tag, _) = horizon_world::nbt::parse_snbt(id)
                .map_err(|e| err("argument.nbt.invalid", &[&e]))?;
            horizon_client::scoreboard::tag_to_json(&tag)
        } else {
            self.fns
                .store
                .predicates
                .get(id)
                .cloned()
                .ok_or_else(|| err("argument.predicate.unknown", &[&id]))?
        };
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let view = source.entity.map(|t| self.entity_view_asked(t, &json));
        Ok(self.conditions_hold(&json, view.as_ref(), source.pos))
    }
}

/// The built-in feature packs and the feature flag each needs (none of them
/// enabled in a world created without experiments).
const FEATURE_PACKS: &[(&str, &str)] = &[
    ("minecart_improvements", "minecraft:minecart_improvements"),
    ("redstone_experiments", "minecraft:redstone_experiments"),
    ("trade_rebalance", "minecraft:trade_rebalance"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Two packs override the same recipe: the later one in the selected
    /// (`datapacks.json`) order wins, not the later one alphabetically.
    #[test]
    fn content_overlay_follows_enabled_order() {
        for order in [["file/a", "file/b"], ["file/b", "file/a"]] {
            let dir = std::env::temp_dir().join(format!(
                "horizon-overlay-{}-{}",
                std::process::id(),
                order[0].replace('/', "_")
            ));
            let _ = std::fs::remove_dir_all(&dir);
            for name in ["a", "b"] {
                let pack = dir.join("datapacks").join(name);
                std::fs::create_dir_all(pack.join("data/x/recipe")).unwrap();
                std::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
                std::fs::write(pack.join("data/x/recipe/r.json"), name).unwrap();
            }
            let json = serde_json::json!({"enabled": order, "disabled": []});
            std::fs::write(dir.join("datapacks.json"), json.to_string()).unwrap();
            let files = content_overlay(&dir, std::path::Path::new(""), true, &[]);
            let (_, versions) = files.iter().find(|(id, _)| id == "recipe/x:r").unwrap();
            let last = versions.last().unwrap();
            assert_eq!(
                last.as_slice(),
                order[1].trim_start_matches("file/").as_bytes()
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// What a native mod contributes (`native_data_pack_files`) joins the load
    /// the tables are built from, a world's first open and `/reload` alike
    /// (`Game::build_world_tables` is both), and sits below the world's own
    /// packs. Each load asks again, so it is the current contribution that
    /// counts.
    #[test]
    fn contributed_files_join_the_load_below_the_worlds_packs() {
        let dir = std::env::temp_dir().join(format!("horizon-contributed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pack = dir.join("datapacks").join("pack");
        std::fs::create_dir_all(pack.join("data/pkg/tags/item")).unwrap();
        std::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
        std::fs::write(
            pack.join("data/pkg/tags/item/both.json"),
            r#"{"replace": true, "values": ["pkg:from_pack"]}"#,
        )
        .unwrap();
        let settings = serde_json::json!({"enabled": ["file/pack"], "disabled": []});
        std::fs::write(dir.join("datapacks.json"), settings.to_string()).unwrap();
        let tag = |entry: &str| format!(r#"{{"values": ["{entry}"]}}"#).into_bytes();
        let files = overlay_files(
            &dir,
            std::path::Path::new(""),
            true,
            &[],
            vec![
                ("tags/pkg:item/both".to_owned(), tag("pkg:from_mod")),
                ("tags/pkg:item/mod_only".to_owned(), tag("pkg:only_mod")),
            ],
        );
        // The pack's file is the path's last (`PackOverlay::top`), and its
        // `replace` drops what the mod put below it.
        let (_, versions) = files
            .iter()
            .find(|(id, _)| id == "tags/pkg:item/both")
            .unwrap();
        assert!(horizon_world::loot::PackOverlay::top(versions).starts_with(b"{\"replace\""));
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let tags = horizon_items::items::load_tags(&dir, "item", &overlay).unwrap();
        assert!(tags["pkg:both"].contains("pkg:from_pack"));
        assert!(!tags["pkg:both"].contains("pkg:from_mod"));
        // A path only the mod has reaches the tables too.
        assert_eq!(
            tags["pkg:mod_only"].iter().collect::<Vec<_>>(),
            ["pkg:only_mod"]
        );
        // The next load asks again: the new contribution, not the first's.
        let files = overlay_files(
            &dir,
            std::path::Path::new(""),
            true,
            &[],
            vec![("tags/pkg:item/mod_only".to_owned(), tag("pkg:second_load"))],
        );
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let tags = horizon_items::items::load_tags(&dir, "item", &overlay).unwrap();
        assert_eq!(
            tags["pkg:mod_only"].iter().collect::<Vec<_>>(),
            ["pkg:second_load"]
        );
        // A file that isn't JSON is skipped alone, not the whole load.
        let files = overlay_files(
            &dir,
            std::path::Path::new(""),
            true,
            &[],
            vec![
                ("tags/pkg:item/mod_only".to_owned(), b"{not json".to_vec()),
                ("tags/pkg:item/good".to_owned(), tag("pkg:good")),
            ],
        );
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let tags = horizon_items::items::load_tags(&dir, "item", &overlay).unwrap();
        assert!(!tags.contains_key("pkg:mod_only"));
        assert!(tags["pkg:good"].contains("pkg:good"));
        // With nothing contributed the overlay is the packs' own.
        assert_eq!(
            overlay_files(&dir, std::path::Path::new(""), true, &[], Vec::new()),
            content_overlay(&dir, std::path::Path::new(""), true, &[])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod reload_transaction_tests {
    use super::*;
    use crate::game::{
        Audio, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
    };
    use std::{path::PathBuf, sync::Arc};

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Exercise real resource loaders and Game state: rejected data must not
    /// publish functions, tables, a new root, or a load-tag tick. A second
    /// candidate prepared against the same old generation is superseded.
    #[test]
    #[ignore = "requires prepared HORIZON_TEST_RESOURCES and isolated HORIZON_HOME"]
    fn prepared_reload_failure_and_supersession_keep_the_running_generation() {
        assert!(
            std::env::var_os("HORIZON_HOME").is_some(),
            "use an isolated test home"
        );
        let root =
            PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources"));
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!("no loading window"),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world = Scratch(
            std::env::temp_dir().join(format!("horizon-reload-{}-{unique}", std::process::id())),
        );
        std::fs::create_dir_all(&world.0).unwrap();
        game.world_root = world.0.clone();
        game.fns.enabled = vec!["fixture:old".into()];
        game.fns.loaded = true;
        game.fns.load_pending = false;
        let schedule = vec![Scheduled {
            id: "fixture:later".into(),
            time: 123,
            seq: 1,
        }];
        game.fns.schedule = schedule.clone();
        let candidate = |root: PathBuf| {
            Arc::new(super::super::mods::PreparedContent {
                root,
                catalog: Default::default(),
                selection: Default::default(),
                lease: Default::default(),
                data_changed: true,
            })
        };
        let pack = world.0.join("datapacks/fixture");
        std::fs::create_dir_all(pack.join("data/fixture/function")).unwrap();
        std::fs::create_dir_all(pack.join("data/minecraft/tags/function")).unwrap();
        std::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
        std::fs::write(
            world.0.join("datapacks.json"),
            r#"{"enabled":["file/fixture"],"disabled":[]}"#,
        )
        .unwrap();
        for (name, body) in [
            (
                "load",
                "scoreboard objectives add reload_fixture dummy\nscoreboard players set order reload_fixture 1\n",
            ),
            ("later", "scoreboard players add order reload_fixture 10\n"),
            ("tick", "scoreboard players add order reload_fixture 100\n"),
        ] {
            std::fs::write(
                pack.join(format!("data/fixture/function/{name}.mcfunction")),
                body,
            )
            .unwrap();
        }
        for name in ["load", "tick"] {
            std::fs::write(
                pack.join(format!("data/minecraft/tags/function/{name}.json")),
                format!(r#"{{"values":["fixture:{name}"]}}"#),
            )
            .unwrap();
        }
        let generation = game.damage_tag_generation;
        // Both preparation and commit use this guard. Exercise changes to the
        // actual Game fields a reentrant native callback could replace.
        let baseline = game.reload_baseline();
        assert!(game.check_reload_baseline(&baseline).is_ok());
        game.world_root = world.0.join("another-world");
        assert!(game.check_reload_baseline(&baseline).is_err());
        game.world_root = baseline.world_root.clone();
        game.options.resources = world.0.join("another-generation");
        assert!(game.check_reload_baseline(&baseline).is_err());
        game.options.resources = baseline.resources.clone();
        game.damage_tag_generation = generation.wrapping_add(1);
        assert!(game.check_reload_baseline(&baseline).is_err());
        game.damage_tag_generation = generation;
        assert!(game.check_reload_baseline(&baseline).is_ok());
        assert!(
            game.prepare_content_reload(candidate(world.0.join("missing-resources")))
                .is_err()
        );
        assert_eq!(game.options.resources, root);
        assert_eq!(game.damage_tag_generation, generation);
        assert_eq!(game.fns.enabled, ["fixture:old"]);
        assert!(!game.fns.load_pending);
        assert_eq!(game.fns.schedule, schedule);
        assert!(game.mods.content.is_none());

        let original_sync =
            super::super::lan::sync_data(&root, &horizon_world::loot::PackOverlay::NONE).unwrap();
        #[cfg(unix)]
        {
            let broken = world.0.join("bad-wire-resources");
            std::fs::create_dir_all(&broken).unwrap();
            for entry in std::fs::read_dir(&root).unwrap().flatten() {
                if entry.file_name() != "registries.json" {
                    std::os::unix::fs::symlink(entry.path(), broken.join(entry.file_name()))
                        .unwrap();
                }
            }
            std::fs::write(broken.join("registries.json"), "{").unwrap();
            // Prove the candidate's tables succeed, so the failure below is the
            // later wire-data validation rather than an early missing-root error.
            assert!(
                game.build_world_tables_at(&broken, true, &game.level.enabled_features, true)
                    .is_ok()
            );
            let error = match game.prepare_content_reload(candidate(broken)) {
                Err(error) => error,
                Ok(_) => panic!("broken wire registries were accepted"),
            };
            assert!(error.to_string().contains("registries.json"), "{error}");
            assert_eq!(game.options.resources, root);
            assert_eq!(game.damage_tag_generation, generation);
            assert_eq!(game.fns.enabled, ["fixture:old"]);
            assert!(!game.fns.load_pending);
            assert_eq!(game.fns.schedule, schedule);
            assert!(game.mods.content.is_none());
        }

        let still_synced =
            super::super::lan::sync_data(&root, &horizon_world::loot::PackOverlay::NONE).unwrap();
        assert!(Arc::ptr_eq(&original_sync, &still_synced));
        let content = candidate(root.clone());
        let accepted = game.prepare_content_reload(content.clone()).unwrap();
        let accepted_sync = accepted.sync.clone();
        let obsolete = game
            .prepare_content_reload(candidate(root.clone()))
            .unwrap();
        assert_eq!(game.damage_tag_generation, generation);
        assert_eq!(game.fns.enabled, ["fixture:old"]);
        assert!(!game.fns.load_pending);
        game.commit_content_reload(accepted).unwrap();
        let installed_sync = super::super::lan::sync_data(&root, &game.pack_files()).unwrap();
        assert!(Arc::ptr_eq(&accepted_sync, &installed_sync));
        assert_eq!(game.damage_tag_generation, generation.wrapping_add(1));
        assert!(Arc::ptr_eq(game.mods.content.as_ref().unwrap(), &content));
        assert!(game.fns.load_pending);
        assert_eq!(game.fns.schedule, schedule);
        let enabled = game.fns.enabled.clone();
        // The load tag runs first, then a due preserved schedule, then the tick
        // tag. It must run once, not once per prepared or superseded candidate.
        game.world.game_time = 123;
        game.tick_functions();
        assert_eq!(
            game.score
                .board
                .score("order", "reload_fixture")
                .map(|s| s.value),
            Some(111)
        );
        assert!(game.fns.schedule.is_empty());
        assert!(!game.fns.load_pending);
        assert!(game.commit_content_reload(obsolete).is_err());
        assert!(Arc::ptr_eq(game.mods.content.as_ref().unwrap(), &content));
        assert_eq!(game.damage_tag_generation, generation.wrapping_add(1));
        assert_eq!(game.fns.enabled, enabled);
        assert!(!game.fns.load_pending);
        assert!(game.fns.schedule.is_empty());
        game.tick_functions();
        assert_eq!(
            game.score
                .board
                .score("order", "reload_fixture")
                .map(|s| s.value),
            Some(211)
        );
        let future = Scheduled {
            id: "fixture:later".into(),
            time: 999,
            seq: 2,
        };
        game.fns.schedule.push(future.clone());
        game.reload_datapacks(true);
        assert_eq!(game.damage_tag_generation, generation.wrapping_add(2));
        assert!(game.fns.load_pending);
        assert_eq!(game.fns.schedule.as_slice(), std::slice::from_ref(&future));
        game.tick_functions();
        assert_eq!(
            game.score
                .board
                .score("order", "reload_fixture")
                .map(|s| s.value),
            Some(101)
        );
        assert_eq!(game.fns.schedule, [future]);
    }
}
