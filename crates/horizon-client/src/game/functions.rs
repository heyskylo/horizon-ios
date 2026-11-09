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
/// What a function reported through `return`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    NoReturn,
    Value(i32),
    Fail,
}

#[derive(Clone)]
struct FunctionCall {
    functions: Vec<Function>,
    args: Option<Tag>,
}
/// One owned call for the still-live parsed invocation, not a generic queue.
/// The context address is compared only, never dereferenced or persisted.
pub(in crate::game) struct PreparedFunctionCall {
    invocation: usize,
    source: Source,
    call: std::sync::Arc<FunctionCall>,
}
impl PreparedFunctionCall {
    fn matches(&self, ctx: &Context, source: Source) -> bool {
        let s = self.source;
        self.invocation == ctx as *const Context as usize
            && s.entity == source.entity
            && s.pos.map(f64::to_bits) == source.pos.map(f64::to_bits)
            && s.rot.map(f32::to_bits) == source.rot.map(f32::to_bits)
            && s.eyes == source.eyes
            && s.silent == source.silent
            && s.store == source.store
            && s.dimension == source.dimension
            && s.level == source.level
            && s.block == source.block
            && s.cart == source.cart
    }
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
    /// `ServerFunctionLibrary.reload`: validate the candidate functions with
    /// the current content before exposing them to execution.
    pub(super) fn reload_datapacks(&mut self, run_load: bool) {
        let stack = pack_stack(
            &self.world_root,
            &self.options.resources,
            self.cmds.vanilla_disabled,
            &self.level.enabled_features,
        );
        self.fns.packs = stack.packs;
        self.fns.enabled = stack.enabled;
        let store = self.load_store();
        self.fns.loaded = true;
        self.fns.load_pending = true;
        // An explicit reload (`/reload`, `/datapack`) reloads the content
        // tables with the functions, as `ReloadableServerResources` does;
        // the lazy first load ran after the world already built them.
        if run_load {
            self.reload_world_tables();
        }
        self.admit_store(store);
        if self.fns.schedule.is_empty() {
            self.load_schedule();
        }
    }
    fn load_store(&self) -> DataStore {
        // The built-in pack first (lowest priority), unless disabled.
        let stack = pack_stack(
            &self.world_root,
            &self.options.resources,
            self.cmds.vanilla_disabled,
            &self.level.enabled_features,
        );
        let packs: Vec<&DataPack> = stack.builtin.iter().chain(stack.enabled_packs()).collect();
        DataStore::load(&packs)
    }
    /// `CommandFunction.parseCommand`: compile ordinary lines without running them.
    /// Macro lines are parsed
    /// only after substitution, but ordinary lines in a macro file still
    /// have to compile (`CommandFunction.fromLines`).
    fn admit_store(&mut self, mut store: DataStore) {
        // On explicit reload the actual content tables have already refreshed.
        // The candidate's named predicates/modifiers must also be available to
        // argument codecs before any of its functions can be exposed.
        self.sync_enchantment_registries();
        // A function's `loot ... loot <id>` names the reloaded packs' tables.
        self.sync_loot_table_ids();
        let mut registries = self.ui.registries.clone();
        registries.permission = self.fns.command_level();
        registries.functions = store.functions.keys().cloned().collect();
        registries.function_tags = store.function_tags.keys().cloned().collect();
        registries.predicates = store.predicates.keys().cloned().collect();
        registries.item_modifiers = store.item_modifiers.keys().cloned().collect();
        registries
            .by_registry
            .entry("minecraft:slot_source".into())
            .or_default()
            .ids = store.slot_sources.keys().cloned().collect();
        for (id, ids) in [
            ("minecraft:predicate", &registries.predicates),
            ("minecraft:item_modifier", &registries.item_modifiers),
        ] {
            if let Some(entry) = registries.by_registry.get_mut(id) {
                entry.ids.clone_from(ids);
            }
        }
        let dispatcher = horizon_client::command::vanilla::game_dispatcher_with_features(
            &self.level.enabled_features,
        );
        store.functions.retain(|id, function| {
            for line in &function.lines {
                if let datapack::FunctionLine::Command(line) = line
                    && let Err(error) = dispatcher.parse(line, &registries).executable()
                {
                    store.errors.push((
                        id.clone(),
                        format!(
                            "Invalid command {line:?}: {} at position {}",
                            error.message.plain(),
                            error.cursor
                        ),
                    ));
                    return false;
                }
            }
            true
        });
        for (id, e) in &store.errors {
            log::warn!("Horizon: failed to load function {id}: {e}");
        }
        self.fns.store = store;
        self.sync_function_registries();
    }
    fn save_datapack_settings(&self) {
        let disabled: Vec<&String> = self
            .fns
            .packs
            .iter()
            .map(|p| &p.id)
            .filter(|id| !self.fns.enabled.contains(id))
            .collect();
        let json = serde_json::json!({"enabled": self.fns.enabled, "disabled": disabled});
        let _ = std::fs::write(self.datapack_settings_path(), json.to_string());
    }
    pub(super) fn sync_function_registries(&mut self) {
        self.sync_enchantment_registries();
        let reg = &mut self.ui.registries;
        reg.functions = self.fns.store.functions.keys().cloned().collect();
        reg.function_tags = self.fns.store.function_tags.keys().cloned().collect();
        reg.predicates = self.fns.store.predicates.keys().cloned().collect();
        reg.item_modifiers = self.fns.store.item_modifiers.keys().cloned().collect();
        reg.by_registry
            .entry("minecraft:slot_source".into())
            .or_default()
            .ids = self.fns.store.slot_sources.keys().cloned().collect();
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

    /// `ServerFunctionManager.tick`: the `#minecraft:load` functions after a
    /// reload, then the `#minecraft:tick` ones, before the levels tick (and
    /// before the game time moves on).
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
        for f in self.fns.store.tag_functions("minecraft:tick") {
            self.run_function_by_id(&f, None, source);
        }
    }
    /// `ServerLevel.tickTime`'s `getScheduledEvents().tick`: the scheduled
    /// functions due at the game time this tick has just reached.
    pub(super) fn run_scheduled_functions(&mut self) {
        self.ensure_datapacks();
        if self.remote.is_some() {
            return;
        }
        self.exec = ExecState::default();
        let source = self.server_source();
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

    /// `FunctionCommand.decorateOutputIfNeeded`: consume only this invocation's
    /// prepared call before its body can invoke another function.
    pub(super) fn cmd_function(&mut self, ctx: &Context) -> R<i32> {
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let call = if self
            .exec
            .prepared_function
            .as_ref()
            .is_some_and(|p| p.matches(ctx, source))
        {
            std::sync::Arc::unwrap_or_clone(self.exec.prepared_function.take().unwrap().call)
        } else {
            let Some(call) = self.prepare_function_call(ctx)? else {
                return Ok(0);
            };
            self.start_function_call(&call.functions);
            call
        };
        self.execute_function_call(call)
    }
    /// `FunctionCommand.getArgumentTag`: retain the existing resolution and argument validation.
    fn prepare_function_call(&mut self, ctx: &Context) -> R<Option<FunctionCall>> {
        let Some(Arg::Resource(name)) = ctx.get("name").cloned() else {
            return Ok(None);
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
                // FunctionCommand.getArgumentTag uses TagType.getName, not the tag's value.
                let names = [
                    "END", "BYTE", "SHORT", "INT", "LONG", "FLOAT", "DOUBLE", "BYTE[]", "STRING",
                    "LIST", "COMPOUND", "INT[]", "LONG[]",
                ];
                return Err(err(
                    "commands.function.error.argument_not_compound",
                    &[&names[usize::from(tag.id())]],
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
        Ok(Some(FunctionCall { functions, args }))
    }
    /// `FunctionCommand.FunctionCustomExecutor.runGuarded`: send the existing start reply.
    fn start_function_call(&mut self, functions: &[Function]) {
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
    }
    /// `FunctionCommand.decorateOutputIfNeeded`: retain body, result and callback delivery.
    fn execute_function_call(&mut self, call: FunctionCall) -> R<i32> {
        let FunctionCall { functions, args } = call;
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
    /// `FunctionCommand.queueFunctionsNoReturn`: starts are emitted while the
    /// eligible direct plain-function calls are prepared, before their bodies.
    pub(in crate::game) fn prepare_function_batch(
        &mut self,
        ctx: &Context,
        sources: &[Source],
    ) -> Option<Vec<PreparedFunctionCall>> {
        if !self.fns.loaded
            || sources.len() < 2
            || self.exec.function_depth != 0
            || self.exec.prepared_function.is_some()
            || sources.iter().any(|s| s.silent)
            || ctx.has("with")
            || ctx.get("arguments").is_some()
        {
            return None;
        }
        let Some(Arg::Resource(name)) = ctx.get("name") else {
            return None;
        };
        if name.starts_with('#') || self.fns.store.functions.get(name)?.is_macro() {
            return None;
        }
        let limit = self.ui.rules.int("max_command_sequence_length").max(1) as u32;
        if sources.len() > limit.saturating_sub(self.exec.commands) as usize {
            return None;
        }
        // No start or slot is published until the existing validation succeeds.
        let call = self
            .with_source(sources[0], |g| g.prepare_function_call(ctx))
            .ok()??;
        let call = std::sync::Arc::new(call);
        let tasks: Vec<_> = sources
            .iter()
            .map(|&source| PreparedFunctionCall {
                invocation: ctx as *const Context as usize,
                source,
                call: call.clone(),
            })
            .collect();
        for task in &tasks {
            self.with_source(task.source, |g| g.start_function_call(&task.call.functions));
        }
        Some(tasks)
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
                self.apply_pack_change();
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
                if name == "vanilla" {
                    self.cmds.vanilla_disabled = true;
                } else {
                    self.fns.enabled.retain(|p| p != &name);
                }
                self.feedback(Text::translate(
                    "commands.datapack.modify.disable",
                    &[self.pack_link(&name, true)],
                ));
                self.apply_pack_change();
                Ok(self.selected_packs().len() as i32)
            }
        }
    }
    /// `ReloadCommand.reloadPacks`: no message of its own. It reloads the
    /// server's data as `/reload` does (`reload_datapacks`), the content
    /// tables with the functions.
    fn apply_pack_change(&mut self) {
        self.save_datapack_settings();
        let store = self.load_store();
        self.fns.load_pending = true;
        self.reload_world_tables();
        self.admit_store(store);
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
        let view = source.entity.map(|t| {
            let scores = self.holder_name(t).map(|holder| {
                self.score
                    .board
                    .holder_scores(&holder)
                    .into_iter()
                    .map(|(objective, value)| (objective.name.clone(), value))
                    .collect()
            });
            let mut view = self.entity_view_asked(t, &json);
            view.scores = scores;
            view
        });
        Ok(self.command_conditions_hold(&json, view.as_ref(), source.pos))
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

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn prepared_function_calls_keep_causality_and_fallback_boundaries() {
        use crate::game::native_machines::tests::server_game;
        use horizon_client::motion::PlayerMotion;
        use horizon_core::geometry::Vec3;
        let mut game = server_game();
        game.options.dedicated = None;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 202.0, 4.5)).unwrap());
        game.creative = true;
        game.world_root = std::env::current_dir().unwrap().join("h155-causal-world");
        let pack = game.world_root.join("datapacks/h155_causal");
        let dir = pack.join("data/h155c/function");
        std::fs::create_dir_all(&dir).unwrap();
        let f = horizon_core::version::DATA_PACK_FORMAT;
        std::fs::write(pack.join("pack.mcmeta"), serde_json::json!({"pack":{"description":"H155 causal controls","pack_format":f.0,"min_format":[f.0,f.1],"max_format":[f.0,f.1]}}).to_string()).unwrap();
        let body = "execute if score @s h155c_id matches 1 run scoreboard players operation #seen_a h155c = #r h155c\nexecute if score @s h155c_id matches 2 run scoreboard players operation #seen_b h155c = #r h155c\nscoreboard players add #calls h155c 1\nscoreboard players operation #trace h155c *= #ten h155c\nscoreboard players operation #trace h155c += @s h155c_id\n";
        for (name, tail) in [
            ("zero", "return 0\n"),
            ("five", "return 5\n"),
            ("fail", "return fail\n"),
            ("none", ""),
        ] {
            std::fs::write(
                dir.join(format!("{name}.mcfunction")),
                format!("{body}{tail}"),
            )
            .unwrap();
        }
        for (name, text) in [
            (
                "early",
                "scoreboard players add #early h155c 1\nreturn 0\nscoreboard players add #late h155c 1\n",
            ),
            ("inner", "scoreboard players add #inner h155c 1\nreturn 0\n"),
            (
                "outer",
                "function h155c:inner\nscoreboard players add #outer h155c 1\nreturn 5\n",
            ),
            (
                "nested",
                "execute as @e[type=pig,tag=h155c,sort=nearest,limit=2] run function h155c:inner\nreturn 0\n",
            ),
            (
                "macro",
                "$scoreboard players add #macro h155c $(amount)\nreturn 0\n",
            ),
        ] {
            std::fs::write(dir.join(format!("{name}.mcfunction")), text).unwrap();
        }
        std::fs::create_dir_all(pack.join("data/h155c/tags/function")).unwrap();
        std::fs::write(
            pack.join("data/h155c/tags/function/pair.json"),
            r#"{"values":["h155c:zero","h155c:five"]}"#,
        )
        .unwrap();
        game.reload_datapacks(false);
        game.build_registries();
        game.set_allow_commands(true);
        assert_eq!(game.fns.store.tag_functions("h155c:pair").len(), 2);
        assert!(
            game.fns
                .store
                .functions
                .get("h155c:macro")
                .unwrap()
                .is_macro()
        );
        assert!(game.run_command("forceload add 0 0 15 15"));
        assert!(game.world.loaded(4, 4));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        for (n, x) in [(1, 4.5), (2, 8.5)] {
            assert!(game.run_command(&format!("summon pig {x} 200 4.5 {{UUID:[I;155,2,0,{n}],Tags:[\"h155c\"],NoAI:1b,NoGravity:1b,Health:10f}}")));
            assert!(game.run_command(&format!("execute if entity @e[type=pig,nbt={{UUID:[I;155,2,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]")));
        }
        assert!(game.run_command("scoreboard objectives add h155c dummy"));
        assert!(game.run_command("scoreboard objectives add h155c_id dummy"));
        for (holder, id) in [
            ("@s".to_owned(), 3),
            ("@e[nbt={UUID:[I;155,2,0,1]},limit=1]".to_owned(), 1),
            ("@e[nbt={UUID:[I;155,2,0,2]},limit=1]".to_owned(), 2),
        ] {
            assert!(game.run_command(&format!("scoreboard players set {holder} h155c_id {id}")));
        }
        assert!(game.run_command("scoreboard players set #ten h155c 10"));
        fn score(g: &Game<'_>, h: &str) -> i32 {
            g.score.board.score(h, "h155c").unwrap().value
        }
        fn reset(g: &mut Game<'_>) {
            for h in [
                "#calls", "#trace", "#seen_a", "#seen_b", "#early", "#late", "#inner", "#outer",
                "#macro",
            ] {
                assert!(g.run_command(&format!("scoreboard players set {h} h155c 0")));
            }
            assert!(g.run_command("scoreboard players set #r h155c 91"));
            assert!(g.run_command("scoreboard players set #s h155c 17"));
        }
        fn capture(g: &mut Game<'_>, line: &str) -> (Vec<String>, Vec<(bool, i32)>) {
            g.chat_capture = Some(Vec::new());
            g.cmds.results = Some(Vec::new());
            assert!(g.run_command(line));
            let texts = g.chat_capture.take().unwrap();
            let callbacks = g.cmds.results.take().unwrap();
            println!("causal raw {line}: {texts:?} callbacks={callbacks:?}");
            assert!(
                g.exec.prepared_function.is_none(),
                "no prepared call after command/error/return"
            );
            (
                texts
                    .iter()
                    .map(|m| m.translation_key().unwrap_or("literal").to_owned())
                    .collect(),
                callbacks,
            )
        }
        let start = "commands.function.scheduled.single".to_owned();
        let result = "commands.function.result".to_owned();
        let prefix = "execute store result score #r h155c store success score #s h155c as @e[type=pig,tag=h155c,sort=nearest,limit=2] run ";
        let clock = game.world.game_time;
        let chunks = game.world.chunks.len();
        for (name, value, success) in [
            ("zero", Some(0), true),
            ("five", Some(5), true),
            ("fail", Some(0), false),
            ("none", None, false),
        ] {
            reset(&mut game);
            let (keys, callbacks) = capture(&mut game, &format!("{prefix}function h155c:{name}"));
            let mut expected = vec![start.clone(); 2];
            if value.is_some() {
                expected.extend(vec![result.clone(); 2]);
            }
            assert_eq!(keys, expected);
            assert_eq!(
                callbacks,
                value.map_or_else(Vec::new, |v| vec![(success, v); 2])
            );
            assert_eq!((score(&game, "#calls"), score(&game, "#trace")), (2, 12));
            assert_eq!(score(&game, "#seen_a"), 91);
            assert_eq!(
                score(&game, "#seen_b"),
                value.unwrap_or(91),
                "first source callback/store precedes second body"
            );
            assert_eq!(score(&game, "#r"), value.unwrap_or(91));
            assert_eq!(
                score(&game, "#s"),
                if value.is_some() {
                    i32::from(success)
                } else {
                    17
                }
            );
        }
        // Each actual actor owns its leaf store; shared ancestors still deliver oldest first.
        reset(&mut game);
        for n in [1, 2] {
            assert!(game.run_command(&format!(
                "scoreboard players set @e[nbt={{UUID:[I;155,2,0,{n}]}},limit=1] h155c 91"
            )));
        }
        let (keys, callbacks) = capture(
            &mut game,
            "execute store result score #chain h155c as @e[type=pig,tag=h155c,sort=nearest,limit=2] store success score #chain h155c store result score @s h155c run function h155c:five",
        );
        assert_eq!(
            keys,
            vec![start.clone(), start.clone(), result.clone(), result.clone()]
        );
        assert_eq!(callbacks, vec![(true, 5); 2]);
        assert_eq!(
            score(&game, "#chain"),
            1,
            "ancestor result then child success"
        );
        assert_eq!((score(&game, "#calls"), score(&game, "#trace")), (2, 12));
        for n in [1, 2] {
            let (_, callbacks) = capture(
                &mut game,
                &format!("scoreboard players get @e[nbt={{UUID:[I;155,2,0,{n}]}},limit=1] h155c"),
            );
            assert_eq!(
                callbacks,
                vec![(true, 5)],
                "actual actor's own result store"
            );
        }
        // These ineligible variants preserve the legacy path, not a Java-order parity claim.
        for line in [
            "execute as @e[type=pig,tag=h155c,sort=nearest,limit=2] if entity @s run function h155c:zero",
            "execute run execute as @e[type=pig,tag=h155c,sort=nearest,limit=2] run function h155c:zero",
            "execute as @e[type=pig,tag=h155c,sort=nearest,limit=2] run function h155c:macro {amount:1}",
        ] {
            reset(&mut game);
            let (keys, callbacks) = capture(&mut game, line);
            assert_eq!(
                keys,
                vec![start.clone(), result.clone(), start.clone(), result.clone()],
                "unchanged ineligible path"
            );
            assert_eq!(callbacks, vec![(true, 0); 2]);
        }
        reset(&mut game);
        let (keys, callbacks) = capture(
            &mut game,
            "execute as @e[type=pig,tag=h155c,sort=nearest,limit=2] run function #h155c:pair",
        );
        assert_eq!(
            keys,
            vec![
                "commands.function.scheduled.multiple".to_owned(),
                result.clone(),
                result.clone(),
                "commands.function.scheduled.multiple".to_owned(),
                result.clone(),
                result.clone()
            ]
        );
        assert_eq!(callbacks, vec![(true, 5); 2]);
        assert_eq!(score(&game, "#calls"), 4);
        reset(&mut game);
        let source = Source {
            silent: true,
            ..game.player_source()
        };
        let (keys, callbacks) = game.with_source(source, |g| {
            capture(g, &format!("{prefix}function h155c:zero"))
        });
        assert!(keys.is_empty());
        assert_eq!(callbacks, vec![(true, 0); 2]);
        assert_eq!(score(&game, "#trace"), 12);
        reset(&mut game);
        let (keys, callbacks) = capture(&mut game, &format!("{prefix}function h155c:missing"));
        assert!(keys.is_empty());
        assert_eq!(callbacks, vec![(false, 0); 2]);
        assert_eq!(score(&game, "#calls"), 0);
        for line in [
            "function h155c:five",
            "execute as @e[type=pig,tag=h155c,sort=nearest,limit=1] run function h155c:five",
        ] {
            reset(&mut game);
            let (keys, callbacks) = capture(&mut game, line);
            assert_eq!(keys, vec![start.clone(), result.clone()]);
            assert_eq!(callbacks, vec![(true, 5)]);
        }
        reset(&mut game);
        let (keys, callbacks) = capture(&mut game, &format!("{prefix}function h155c:outer"));
        assert_eq!(
            keys,
            vec![start.clone(), start.clone(), result.clone(), result.clone()]
        );
        assert_eq!(callbacks, vec![(true, 5); 2]);
        assert_eq!((score(&game, "#inner"), score(&game, "#outer")), (2, 2));
        reset(&mut game);
        let (keys, callbacks) = capture(&mut game, "function h155c:nested");
        assert_eq!(keys, vec![start.clone(), result.clone()]);
        assert_eq!(callbacks, vec![(true, 0)]);
        assert_eq!(score(&game, "#inner"), 2);
        reset(&mut game);
        let (_, callbacks) = capture(&mut game, &format!("{prefix}function h155c:early"));
        assert_eq!(callbacks, vec![(true, 0); 2]);
        assert_eq!((score(&game, "#early"), score(&game, "#late")), (2, 0));
        // These are preservation/cleanup controls, not a Java limit-parity claim.
        for budget in [1, 4] {
            reset(&mut game);
            assert!(game.run_command(&format!("gamerule max_command_sequence_length {budget}")));
            let (keys, callbacks) = capture(&mut game, &format!("{prefix}function h155c:early"));
            if budget == 1 {
                assert!(keys.is_empty() && callbacks.is_empty());
                assert_eq!(score(&game, "#early"), 0);
            } else {
                assert_eq!(callbacks, vec![(true, 0)]);
                assert_eq!((score(&game, "#early"), score(&game, "#late")), (1, 0));
                assert_eq!(keys.iter().filter(|k| **k == start).count(), 2);
            }
            assert!(game.run_command("gamerule max_command_sequence_length 65536"));
            reset(&mut game);
            let (keys, callbacks) = capture(&mut game, &format!("{prefix}function h155c:five"));
            assert_eq!(
                keys,
                vec![start.clone(), start.clone(), result.clone(), result.clone()]
            );
            assert_eq!(callbacks, vec![(true, 5); 2]);
            assert_eq!(score(&game, "#seen_b"), 5);
        }
        assert_eq!(game.world.game_time, clock);
        assert_eq!(game.world.chunks.len(), chunks);
    }

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn forked_function_replies_keep_task_order_and_actual_callbacks() {
        use crate::game::native_machines::tests::server_game;
        use horizon_client::motion::PlayerMotion;
        use horizon_core::geometry::Vec3;
        let mut game = server_game();
        game.options.dedicated = None;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 202.0, 4.5)).unwrap());
        game.creative = true;
        game.world_root = std::env::current_dir().unwrap().join("h155-world");
        let pack = game.world_root.join("datapacks/h155_unit");
        let dir = pack.join("data/h155/function");
        std::fs::create_dir_all(&dir).unwrap();
        let format = horizon_core::version::DATA_PACK_FORMAT;
        std::fs::write(
            pack.join("pack.mcmeta"),
            serde_json::json!({"pack": {
                "description":"H155 actual Game regression", "pack_format":format.0,
                "min_format":[format.0,format.1], "max_format":[format.0,format.1]
            }})
            .to_string(),
        )
        .unwrap();
        let body = "scoreboard players add #calls h155 1\nscoreboard players add @s h155 1\nscoreboard players operation #trace h155 *= #ten h155\nscoreboard players operation #trace h155 += @s h155_id\n";
        for (name, tail) in [
            ("zero", "return 0\n"),
            ("five", "return 5\n"),
            ("fail", "return fail\n"),
            ("fallthrough", ""),
        ] {
            std::fs::write(
                dir.join(format!("{name}.mcfunction")),
                format!("{body}{tail}"),
            )
            .unwrap();
        }
        std::fs::write(
            dir.join("admit.mcfunction"),
            "scoreboard players add #pack h155 1\nreturn 1\n",
        )
        .unwrap();
        game.reload_datapacks(false);
        game.build_registries();
        game.set_allow_commands(true);
        for name in ["zero", "five", "fail", "fallthrough", "admit"] {
            assert!(
                game.fns
                    .store
                    .functions
                    .contains_key(&format!("h155:{name}"))
            );
            assert!(
                game.ui
                    .registries
                    .functions
                    .contains(&format!("h155:{name}"))
            );
        }
        assert!(game.run_command("forceload add 0 0 15 15"));
        assert!(game.world.loaded(4, 4));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        for (n, x) in [(1, 4.5), (2, 8.5)] {
            assert!(game.run_command(&format!("summon pig {x} 200 4.5 {{UUID:[I;155,1,0,{n}],Tags:[\"h155u\"],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f}}")));
            assert!(game.run_command(&format!("execute if entity @e[type=pig,nbt={{UUID:[I;155,1,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]")));
        }
        assert!(game.run_command("scoreboard objectives add h155 dummy"));
        assert!(game.run_command("scoreboard objectives add h155_id dummy"));
        assert!(game.run_command("scoreboard players set #ten h155 10"));
        assert!(game.run_command("scoreboard players set @s h155_id 3"));
        let actor = |n| format!("@e[nbt={{UUID:[I;155,1,0,{n}]}},limit=1]");
        for n in [1, 2] {
            assert!(game.run_command(&format!("scoreboard players set {} h155_id {n}", actor(n))));
        }
        assert!(game.run_command("scoreboard players set #pack h155 0"));
        assert!(game.run_command("function h155:admit"));
        assert_eq!(game.score.board.score("#pack", "h155").unwrap().value, 1);
        let clock = game.world.game_time;
        let chunks = game.world.chunks.len();
        let mut mismatches = Vec::new();
        for (name, value, success) in [
            ("zero", Some(0), true),
            ("five", Some(5), true),
            ("fail", Some(0), false),
            ("fallthrough", None, false),
        ] {
            for route in ["direct", "one", "two", "nonfork"] {
                for holder in [
                    "#calls".to_owned(),
                    "#trace".to_owned(),
                    "@s".to_owned(),
                    actor(1),
                    actor(2),
                ] {
                    assert!(game.run_command(&format!("scoreboard players set {holder} h155 0")));
                }
                assert!(game.run_command("scoreboard players set #r h155 91"));
                assert!(game.run_command("scoreboard players set #s h155 17"));
                let middle = match route {
                    "one" => "as @e[type=pig,tag=h155u,sort=nearest,limit=1] ",
                    "two" => "as @e[type=pig,tag=h155u,sort=nearest,limit=2] ",
                    "nonfork" => "positioned 4.5 202 4.5 ",
                    _ => "",
                };
                game.chat_capture = Some(Vec::new());
                game.cmds.results = Some(Vec::new());
                let accepted = game.run_command(&format!("execute store result score #r h155 store success score #s h155 {middle}run function h155:{name}"));
                let messages = game.chat_capture.take().unwrap();
                let callbacks = game.cmds.results.take().unwrap();
                assert!(accepted, "actual registered function command");
                let count = if route == "two" { 2 } else { 1 };
                assert_eq!(
                    game.score.board.score("#calls", "h155").unwrap().value,
                    count
                );
                assert_eq!(
                    game.score.board.score("#trace", "h155").unwrap().value,
                    match route {
                        "two" => 12,
                        "one" => 1,
                        _ => 3,
                    }
                );
                assert_eq!(
                    callbacks,
                    value.map_or_else(Vec::new, |v| vec![(success, v); count as usize])
                );
                assert_eq!(
                    game.score.board.score("#r", "h155").unwrap().value,
                    value.unwrap_or(91)
                );
                assert_eq!(
                    game.score.board.score("#s", "h155").unwrap().value,
                    if value.is_some() {
                        i32::from(success)
                    } else {
                        17
                    }
                );
                let keys: Vec<_> = messages
                    .iter()
                    .map(|m| m.translation_key().unwrap_or("literal").to_owned())
                    .collect();
                let mut expected =
                    vec!["commands.function.scheduled.single".to_owned(); count as usize];
                if value.is_some() {
                    expected.extend(vec!["commands.function.result".to_owned(); count as usize]);
                }
                if keys != expected {
                    mismatches.push((name, route, keys, expected));
                }
            }
        }
        assert!(game.run_command("scoreboard players set #plain h155 0"));
        game.cmds.results = Some(Vec::new());
        assert!(game.run_command("execute as @e[type=pig,tag=h155u,sort=nearest,limit=2] run scoreboard players add #plain h155 1"));
        assert_eq!(
            game.cmds.results.take().unwrap(),
            vec![(true, 1), (true, 2)]
        );
        assert_eq!(game.score.board.score("#plain", "h155").unwrap().value, 2);
        assert_eq!(game.world.game_time, clock);
        assert_eq!(game.world.chunks.len(), chunks);
        assert!(
            mismatches.is_empty(),
            "actual fork function feedback order mismatches: {mismatches:?}"
        );
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn function_pack_rejects_invalid_ordinary_lines_before_execution() {
        use crate::game::{Audio, Options, Settings, load_resources};
        use std::time::Instant;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &crate::game::Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        game.build_registries();
        game.set_allow_commands(true);
        let dir =
            std::env::temp_dir().join(format!("horizon-function-admission-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pack = dir.join("datapacks/admission");
        let functions = pack.join("data/h144/function");
        let predicates = pack.join("data/h144/predicate");
        std::fs::create_dir_all(&functions).unwrap();
        std::fs::create_dir_all(&predicates).unwrap();
        std::fs::write(pack.join("pack.mcmeta"), r#"{"pack":{"description":"Function admission","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}"#).unwrap();
        std::fs::write(
            predicates.join("ready.json"),
            r#"{"type":"minecraft:all_of","terms":[]}"#,
        )
        .unwrap();
        for (name, text) in [
            (
                "good",
                "execute if predicate h144:ready run scoreboard players add good h144 1\n",
            ),
            (
                "bad",
                "scoreboard players set bad h144 1\nh144_unknown_command\nscoreboard players set bad h144 2\n",
            ),
            (
                "forbidden",
                "scoreboard players set forbidden h144 1\ntick freeze\nscoreboard players set forbidden h144 2\n",
            ),
            (
                "macro",
                "$h144_unknown_$(name)\nscoreboard players set macro h144 1\n",
            ),
            ("macro_bad", "$say $(name)\nh144_unknown_command\n"),
            (
                "sequence",
                "scoreboard players \\\nset sequence h144 17\nreturn 5\n",
            ),
            ("slash", "/say invalid\n"),
        ] {
            std::fs::write(functions.join(format!("{name}.mcfunction")), text).unwrap();
        }
        game.world_root = dir.clone();
        let run = |game: &mut Game<'_>, line: &str| {
            game.exec = ExecState::default();
            let source = game.player_source();
            game.execute_line(line, source)
        };
        run(&mut game, "scoreboard objectives add h144 dummy").unwrap();
        for name in ["good", "bad", "forbidden", "macro", "sequence"] {
            run(
                &mut game,
                &format!("scoreboard players set {name} h144 -99"),
            )
            .unwrap();
        }
        let clock = game.world.game_time;
        // This is the actual pack loader. Admission must not run any line.
        game.reload_datapacks(false);
        for name in ["good", "bad", "forbidden", "macro", "sequence"] {
            assert_eq!(game.score.board.score(name, "h144").unwrap().value, -99);
        }
        // Intended old failure: the invalid file previously ran both valid lines.
        assert!(run(&mut game, "function h144:bad").is_err());
        assert_eq!(game.score.board.score("bad", "h144").unwrap().value, -99);
        for name in ["bad", "forbidden", "macro_bad", "slash"] {
            assert!(
                !game
                    .fns
                    .store
                    .functions
                    .contains_key(&format!("h144:{name}"))
            );
            assert!(
                !game
                    .ui
                    .registries
                    .functions
                    .contains(&format!("h144:{name}"))
            );
            assert!(
                game.fns
                    .store
                    .errors
                    .iter()
                    .any(|(id, _)| id == &format!("h144:{name}"))
            );
        }
        assert!(run(&mut game, "function h144:forbidden").is_err());
        assert!(game.fns.store.functions.contains_key("h144:macro"));
        assert!(game.fns.store.functions["h144:macro"].is_macro());
        run(&mut game, "function h144:good").unwrap();
        assert_eq!(game.score.board.score("good", "h144").unwrap().value, -98);
        run(&mut game, "function h144:sequence").unwrap();
        assert_eq!(
            game.score.board.score("sequence", "h144").unwrap().value,
            17
        );
        assert_eq!(game.ui.registries.permission, 4);
        // A fresh same-pack predicate must be in the compilation snapshot on reload.
        std::fs::write(
            predicates.join("reloaded.json"),
            r#"{"type":"minecraft:all_of","terms":[]}"#,
        )
        .unwrap();
        std::fs::write(
            functions.join("good.mcfunction"),
            "execute if predicate h144:reloaded run scoreboard players add good h144 1\n",
        )
        .unwrap();
        game.reload_datapacks(true);
        assert!(game.fns.store.functions.contains_key("h144:good"));
        assert!(!game.fns.store.functions.contains_key("h144:forbidden"));
        assert!(
            !game
                .ui
                .registries
                .functions
                .contains(&"h144:forbidden".into())
        );
        assert_eq!(game.score.board.score("good", "h144").unwrap().value, -98);
        run(&mut game, "function h144:good").unwrap();
        assert_eq!(game.score.board.score("good", "h144").unwrap().value, -97);
        assert_eq!(
            game.score.board.score("forbidden", "h144").unwrap().value,
            -99
        );
        assert_eq!(game.world.game_time, clock);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn unless_function_keeps_a_fallthrough_after_running_it_once() {
        use crate::game::{Audio, Options, Settings, load_resources};
        use horizon_client::datapack::FunctionLine;
        use std::time::Instant;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &crate::game::Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        game.build_registries();
        game.set_allow_commands(true);
        game.fns.loaded = true;
        let run = |game: &mut Game<'_>, line: &str| {
            game.exec = ExecState::default();
            let source = game.player_source();
            game.execute_line(line, source).unwrap();
        };
        run(&mut game, "scoreboard objectives add h154 dummy");
        let clock = game.world.game_time;
        for (name, tail) in [
            ("counter", None),
            ("zero", Some("return 0")),
            ("five", Some("return 5")),
            ("fail", Some("return fail")),
        ] {
            let id = format!("h154:{name}");
            let mut lines = vec![FunctionLine::Command(
                "scoreboard players add #calls h154 1".into(),
            )];
            if let Some(tail) = tail {
                lines.push(FunctionLine::Command(tail.into()));
            }
            game.fns.store.functions.insert(
                id.clone(),
                Function {
                    id,
                    lines,
                    parameters: Vec::new(),
                },
            );
        }
        // Explicit returns establish both signs before the old fallthrough
        // failure. The commands run through the actual function/execute route.
        for name in ["zero", "five", "fail", "counter"] {
            for sign in ["if", "unless"] {
                run(&mut game, "scoreboard players set #calls h154 0");
                run(&mut game, "scoreboard players set #out h154 99");
                run(
                    &mut game,
                    &format!(
                        "execute {sign} function h154:{name} run scoreboard players set #out h154 7"
                    ),
                );
                assert_eq!(game.score.board.score("#calls", "h154").unwrap().value, 1);
                let keep = (name == "five") == (sign == "if");
                assert_eq!(
                    game.score.board.score("#out", "h154").unwrap().value,
                    if keep { 7 } else { 99 },
                    "{sign} {name}"
                );
            }
        }
        assert_eq!(game.world.game_time, clock);
    }

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
                std::fs::write(
                    pack.join("pack.mcmeta"),
                    r#"{"pack":{"description":"fixture"}}"#,
                )
                .unwrap();
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
        std::fs::write(
            pack.join("pack.mcmeta"),
            r#"{"pack":{"description":"fixture"}}"#,
        )
        .unwrap();
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
mod entity_score_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn named_scores_follow_the_registered_target_holder_and_mutations() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        game.build_registries();

        game.set_allow_commands(true);
        game.ensure_datapacks();
        game.fns.store.predicates.insert(
            "h270:min2".into(),
            serde_json::json!({
                "type": "minecraft:entity_scores", "entity": "this", "scores": {"pts": {"min": 2}}
            }),
        );
        assert!(game.run_command("scoreboard objectives add pts dummy"));
        let mut actors = Vec::new();
        for (index, score) in [Some(0), Some(2), None].into_iter().enumerate() {
            let uuid = [270, 1, 0, index as i32 + 1];
            let tag = Tag::compound([
                ("id", Tag::String("minecraft:pig".into())),
                ("UUID", Tag::IntArray(uuid.to_vec())),
                (
                    "Pos",
                    Tag::List(vec![
                        Tag::Double((3.5 + index as f64).to_bits()),
                        Tag::Double(200.0_f64.to_bits()),
                        Tag::Double(4.5_f64.to_bits()),
                    ]),
                ),
                ("Health", Tag::Float(10.0_f32.to_bits())),
                ("NoAI", Tag::Byte(1)),
                ("NoGravity", Tag::Byte(1)),
            ]);
            let id = game.load_mob(tag, None).unwrap();
            assert_eq!(game.persist.uuids.get(&id), Some(&uuid));
            let holder = game.holder_name(Target::Mob(id)).unwrap();
            assert_eq!(holder, horizon_client::save::uuid_text(uuid));
            if let Some(score) = score {
                assert!(game.run_command(&format!("scoreboard players set {holder} pts {score}")));
            }
            assert_eq!(
                game.score.board.score(&holder, "pts").map(|s| s.value),
                score
            );
            actors.push((id, holder));
        }
        let time = game.world.game_time;
        let matches = |game: &mut Game<'_>, id| {
            let mut source = game.player_source();
            source.entity = Some(Target::Mob(id));
            source.pos = game.target_position(Target::Mob(id)).unwrap();
            game.command_source = Some(source);
            let result = game.check_predicate("h270:min2").unwrap();
            game.command_source = None;
            result
        };
        assert!(!matches(&mut game, actors[0].0));
        assert!(matches(&mut game, actors[1].0));
        assert!(!matches(&mut game, actors[2].0));
        for (index, score, expected) in [(0, 3, true), (1, 1, false)] {
            let (id, holder) = &actors[index];
            assert!(game.run_command(&format!("scoreboard players set {holder} pts {score}")));
            assert_eq!(
                game.score.board.score(holder, "pts").map(|s| s.value),
                Some(score)
            );
            assert_eq!(matches(&mut game, *id), expected);
        }
        assert!(!matches(&mut game, actors[2].0));
        assert!(game.score.board.score(&actors[2].1, "pts").is_none());
        assert_eq!(game.world.game_time, time);
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn folder_metadata_rejection_reaches_actual_function_commands() {
        use crate::game::{Audio, Options, Settings, load_resources};
        use std::time::Instant;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &crate::game::Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        game.build_registries();
        game.set_allow_commands(true);
        let dir = std::env::temp_dir().join(format!(
            "horizon-game-folder-metadata-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let cases = [
            ("broken", Some("{"), false),
            ("root_empty", Some("{}"), false),
            ("root_array", Some("[]"), false),
            ("pack_empty", Some(r#"{"pack":{}}"#), false),
            ("pack_string", Some(r#"{"pack":"bad"}"#), false),
            ("missing", None, false),
            (
                "full",
                Some(
                    r#"{"pack":{"description":"full","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}"#,
                ),
                true,
            ),
            (
                "description",
                Some(r#"{"pack":{"description":"description only"}}"#),
                true,
            ),
        ];
        for (name, metadata, _) in cases {
            let pack = dir.join("datapacks").join(name);
            std::fs::create_dir_all(pack.join("data/h146/function")).unwrap();
            std::fs::write(
                pack.join(format!("data/h146/function/{name}.mcfunction")),
                format!("scoreboard players set {name} h146 17\n"),
            )
            .unwrap();
            if let Some(metadata) = metadata {
                std::fs::write(pack.join("pack.mcmeta"), metadata).unwrap();
            }
        }
        game.world_root = dir.clone();
        let run = |game: &mut Game<'_>, line: &str| {
            game.exec = ExecState::default();
            let source = game.player_source();
            game.execute_line(line, source)
        };
        run(&mut game, "scoreboard objectives add h146 dummy").unwrap();
        let clock = game.world.game_time;
        for reload in [false, true] {
            for (name, _, _) in cases {
                run(
                    &mut game,
                    &format!("scoreboard players set {name} h146 -99"),
                )
                .unwrap();
            }
            game.reload_datapacks(reload);
            for (name, _, valid) in cases {
                assert_eq!(game.score.board.score(name, "h146").unwrap().value, -99);
                let result = run(&mut game, &format!("function h146:{name}"));
                // Intended old failure: a malformed pack's valid function executes.
                assert_eq!(result.is_ok(), valid, "{name}, reload={reload}");
                assert_eq!(
                    game.score.board.score(name, "h146").unwrap().value,
                    if valid { 17 } else { -99 }
                );
                assert_eq!(
                    game.fns
                        .store
                        .functions
                        .contains_key(&format!("h146:{name}")),
                    valid
                );
                assert_eq!(
                    game.ui
                        .registries
                        .functions
                        .contains(&format!("h146:{name}")),
                    valid
                );
                assert_eq!(
                    game.fns
                        .packs
                        .iter()
                        .any(|p| p.id == format!("file/{name}")),
                    valid
                );
            }
        }
        assert_eq!(game.world.game_time, clock);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod required_tag_tests {
    use super::*;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn real_pack_required_tag_failures_execute_no_partial_functions() {
        use crate::game::{Audio, Options, Settings, load_resources};
        use std::time::{Instant, SystemTime, UNIX_EPOCH};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &crate::game::Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        let dir = std::env::temp_dir().join(format!(
            "horizon-required-tags-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let pack = dir.join("datapacks/h145");
        let write = |path: &str, body: &str| {
            let path = pack.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };
        let (major, minor) = horizon_core::version::DATA_PACK_FORMAT;
        write(
            "pack.mcmeta",
            &serde_json::json!({"pack": {
                "description": "Required dependency test", "pack_format": major,
                "min_format": [major, minor], "max_format": [major, minor],
            }})
            .to_string(),
        );
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            write(
                &format!("data/h145/function/{name}.mcfunction"),
                &format!(
                    "scoreboard players add {name} h145 1\nscoreboard players operation order h145 *= ten h145\nscoreboard players add order h145 {}\n",
                    i + 1,
                ),
            );
        }
        let tags = serde_json::json!({
            "empty": [], "ordered": ["h145:a", "h145:b", "h145:a"],
            "nested": ["#h145:empty", "#h145:ordered", "h145:a"],
            "optional_member": ["h145:a", {"id":"h145:missing","required":false}, "h145:b"],
            "optional_child": ["h145:a", {"id":"#h145:missing","required":false}, "h145:b"],
            "missing_first": ["h145:missing", "h145:a"],
            "missing_last": ["h145:a", "h145:missing"],
            "missing_child": ["h145:a", "#h145:missing"],
            "invalid_child": ["h145:c", "h145:missing"],
            "required_invalid_child": ["h145:a", "#h145:invalid_child", "h145:b"],
            "optional_invalid_child": ["h145:a", {"id":"#h145:invalid_child","required":false}, "h145:b"],
        });
        for (name, values) in tags.as_object().unwrap() {
            write(
                &format!("data/h145/tags/function/{name}.json"),
                &serde_json::json!({"values": values}).to_string(),
            );
        }
        game.world_root = dir.clone();
        game.build_registries();
        game.set_allow_commands(true);
        let run = |game: &mut Game<'_>, line: &str| {
            game.exec = ExecState::default();
            let source = game.player_source();
            if let Err(error) = game.execute_line(line, source) {
                assert!(line.contains("function #h145:"), "{line}: {error:?}");
                assert_eq!(error.key, "commands.function.scheduled.no_functions");
            }
        };
        run(&mut game, "scoreboard objectives add h145 dummy");
        run(&mut game, "scoreboard players set ten h145 10");
        run(&mut game, "scoreboard players set sentinel h145 73");
        let clock = game.world.game_time;
        run(&mut game, "reload");
        assert_eq!(
            game.score.board.score("sentinel", "h145").unwrap().value,
            73
        );
        assert!(game.fns.enabled.contains(&"file/h145".to_owned()));
        assert!(
            game.fns.store.errors.is_empty(),
            "{:?}",
            game.fns.store.errors
        );
        for name in ["a", "b", "c"] {
            assert!(
                game.fns
                    .store
                    .functions
                    .contains_key(&format!("h145:{name}"))
            );
        }
        for name in tags.as_object().unwrap().keys() {
            assert!(
                game.fns
                    .store
                    .function_tags
                    .contains_key(&format!("h145:{name}"))
            );
        }
        let score = |game: &Game<'_>, key: &str| game.score.board.score(key, "h145").unwrap().value;
        let reset = |game: &mut Game<'_>| {
            for key in ["a", "b", "c", "order"] {
                run(game, &format!("scoreboard players set {key} h145 0"));
                assert_eq!(score(game, key), 0);
            }
        };
        // Independent actual functions and valid tag controls run before the defect assertions.
        for (target, expected) in [
            ("h145:a", [1, 0, 0, 1]),
            ("h145:b", [0, 1, 0, 2]),
            ("h145:c", [0, 0, 1, 3]),
            ("#h145:empty", [0, 0, 0, 0]),
            ("#h145:ordered", [1, 1, 0, 12]),
            ("#h145:nested", [1, 1, 0, 12]),
            ("#h145:optional_member", [1, 1, 0, 12]),
            ("#h145:optional_child", [1, 1, 0, 12]),
        ] {
            reset(&mut game);
            run(&mut game, &format!("function {target}"));
            assert_eq!(
                [
                    score(&game, "a"),
                    score(&game, "b"),
                    score(&game, "c"),
                    score(&game, "order")
                ],
                expected,
                "{target}"
            );
        }
        println!(
            "H145 actual pack metadata, functions, empty/nested/dedup, optional-missing and reload controls admitted"
        );
        let mut failures = Vec::new();
        for name in [
            "missing_first",
            "missing_last",
            "missing_child",
            "invalid_child",
            "required_invalid_child",
            "optional_invalid_child",
        ] {
            reset(&mut game);
            run(&mut game, "scoreboard players set result h145 31");
            run(&mut game, "scoreboard players set success h145 17");
            assert_eq!((score(&game, "result"), score(&game, "success")), (31, 17));
            run(
                &mut game,
                &format!(
                    "execute store result score result h145 store success score success h145 run function #h145:{name}"
                ),
            );
            let actual = [
                score(&game, "a"),
                score(&game, "b"),
                score(&game, "c"),
                score(&game, "order"),
            ];
            let expected = if name == "optional_invalid_child" {
                [1, 1, 0, 12]
            } else {
                [0, 0, 0, 0]
            };
            if actual != expected {
                failures.push(format!("{name}: {actual:?} expected {expected:?}"));
            }
            let ids = game.fns.store.tag_functions(&format!("h145:{name}"));
            let expected_ids: Vec<String> = if name == "optional_invalid_child" {
                vec!["h145:a".into(), "h145:b".into()]
            } else {
                Vec::new()
            };
            if ids != expected_ids {
                failures.push(format!(
                    "{name}: DataStore {ids:?} expected {expected_ids:?}"
                ));
            }
        }
        assert_eq!(game.world.game_time, clock);
        drop(game);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(
            failures.is_empty(),
            "required tag partial execution: {failures:#?}"
        );
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn named_predicates_resolve_loaded_references_for_actual_sources() {
        use crate::game::{Audio, Options, Settings, load_resources};
        use std::time::Instant;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &crate::game::Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
        game.build_registries();
        game.set_allow_commands(true);
        let dir = std::env::temp_dir().join(format!(
            "horizon-predicate-reference-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let pack = dir.join("datapacks/references");
        let folder = pack.join("data/h121/predicate");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(pack.join("pack.mcmeta"), r#"{"pack":{"description":"Predicate references","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}"#).unwrap();
        let write = |name: &str, value: serde_json::Value| {
            std::fs::write(folder.join(format!("{name}.json")), value.to_string()).unwrap();
        };
        write(
            "pig",
            serde_json::json!({"type":"minecraft:entity_properties","entity":"this","predicate":{"minecraft:entity_type":"minecraft:pig"}}),
        );
        write(
            "cow",
            serde_json::json!({"type":"minecraft:entity_properties","entity":"this","predicate":{"minecraft:entity_type":"minecraft:cow"}}),
        );
        write(
            "reference",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:pig"]}),
        );
        write(
            "inverted",
            serde_json::json!({"type":"minecraft:inverted","term":"h121:pig"}),
        );
        write(
            "conjunction",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:pig",{"type":"minecraft:inverted","term":"h121:cow"}]}),
        );
        write(
            "both",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:pig","h121:cow"]}),
        );
        write(
            "either",
            serde_json::json!({"type":"minecraft:any_of","terms":["h121:pig","h121:cow"]}),
        );
        write(
            "repeated",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:pig","h121:pig"]}),
        );
        for index in 0..33 {
            let next = if index == 32 {
                "h121:pig".to_owned()
            } else {
                format!("h121:chain_{}", index + 1)
            };
            write(
                &format!("chain_{index}"),
                serde_json::json!({"type":"minecraft:all_of","terms":[next]}),
            );
        }
        game.world_root = dir.clone();
        game.reload_datapacks(false);
        assert!(game.fns.store.predicates.contains_key("h121:reference"));
        assert!(
            game.ui
                .registries
                .predicates
                .contains(&"h121:reference".into())
        );
        let run = |game: &mut Game<'_>, line: &str| {
            game.exec = ExecState::default();
            let source = game.player_source();
            game.execute_line(line, source).unwrap();
        };
        run(&mut game, "scoreboard objectives add h121 dummy");
        let mut actors = Vec::new();
        for (index, kind) in ["pig", "cow"].into_iter().enumerate() {
            let uuid = [121, 20, 30, index as i32 + 1];
            let tag = Tag::compound([
                (
                    "id",
                    Tag::String(format!("minecraft:{kind}").as_str().into()),
                ),
                ("UUID", Tag::IntArray(uuid.to_vec())),
                (
                    "Pos",
                    Tag::List(vec![
                        Tag::Double((4.5 + index as f64 * 4.0).to_bits()),
                        Tag::Double(200.0_f64.to_bits()),
                        Tag::Double(4.5_f64.to_bits()),
                    ]),
                ),
                ("Health", Tag::Float(10.0_f32.to_bits())),
                ("NoAI", Tag::Byte(1)),
                ("NoGravity", Tag::Byte(1)),
            ]);
            let id = game.load_mob(tag, None).unwrap();
            assert_eq!(game.persist.uuids.get(&id), Some(&uuid));
            assert_eq!(
                game.entity_view_of(Target::Mob(id)).kind,
                format!("minecraft:{kind}")
            );
            actors.push((id, save::uuid_text(uuid), kind));
        }
        let clock = game.world.game_time;
        for (_, uuid, kind) in &actors {
            let pig = *kind == "pig";
            for (name, expected) in [
                ("pig", pig),
                ("cow", !pig),
                ("reference", pig),
                ("inverted", !pig),
                ("conjunction", pig),
                ("both", false),
                ("either", true),
                ("repeated", pig),
                ("chain_0", pig),
            ] {
                for sign in ["if", "unless"] {
                    run(&mut game, "scoreboard players set branch h121 -99");
                    run(
                        &mut game,
                        &format!(
                            "execute as {uuid} {sign} predicate h121:{name} run scoreboard players set branch h121 1"
                        ),
                    );
                    // The intended old failure is the real loaded reference on the pig.
                    assert_eq!(
                        game.score.board.score("branch", "h121").unwrap().value,
                        if expected == (sign == "if") { 1 } else { -99 },
                        "{kind} {sign} {name}"
                    );
                }
            }
        }
        let previous = game.command_source;
        let server = game.server_source();
        assert!(server.entity.is_none());
        game.command_source = Some(server);
        assert!(!game.check_predicate("h121:pig").unwrap());
        assert!(!game.check_predicate("h121:reference").unwrap());
        assert!(game.check_predicate("h121:inverted").unwrap());
        assert!(game.check_predicate("h121:absent").is_err());
        game.command_source = previous;
        // Defensive evaluator controls only. Java rejects these graphs at
        // world load, so this does not establish startup/availability parity.
        write(
            "missing",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:absent"]}),
        );
        write(
            "nested_missing",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:missing"]}),
        );
        write(
            "not_missing",
            serde_json::json!({"type":"minecraft:inverted","term":"h121:absent"}),
        );
        write(
            "cycle_a",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:cycle_b"]}),
        );
        write(
            "cycle_b",
            serde_json::json!({"type":"minecraft:all_of","terms":["h121:cycle_a"]}),
        );
        write(
            "not_cycle",
            serde_json::json!({"type":"minecraft:inverted","term":"h121:cycle_a"}),
        );
        for invalid in ["h121:absent", "h121:cycle_a"] {
            for (label, kind, first) in [
                ("any_skip", "minecraft:any_of", "h121:pig"),
                ("any_reach", "minecraft:any_of", "h121:cow"),
                ("all_skip", "minecraft:all_of", "h121:cow"),
                ("all_reach", "minecraft:all_of", "h121:pig"),
            ] {
                write(
                    &format!("{label}_{}", invalid.split(':').next_back().unwrap()),
                    serde_json::json!({"type":kind,"terms":[first,invalid]}),
                );
            }
        }
        let canonical = pack.join("data/minecraft/predicate/h121_canonical.json");
        std::fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        std::fs::write(
            canonical,
            r#"{"type":"minecraft:all_of","terms":["h121_canonical"]}"#,
        )
        .unwrap();
        game.reload_datapacks(false);
        let mut source = game.player_source();
        source.entity = Some(Target::Mob(actors[0].0));
        source.pos = game.target_position(Target::Mob(actors[0].0)).unwrap();
        game.command_source = Some(source);
        for name in [
            "missing",
            "nested_missing",
            "not_missing",
            "cycle_a",
            "not_cycle",
            "any_skip_absent",
            "any_reach_absent",
            "all_skip_absent",
            "all_reach_absent",
            "any_skip_cycle_a",
            "any_reach_cycle_a",
            "all_skip_cycle_a",
            "all_reach_cycle_a",
        ] {
            assert!(
                game.fns
                    .store
                    .predicates
                    .contains_key(&format!("h121:{name}"))
            );
            assert!(
                !game.check_predicate(&format!("h121:{name}")).unwrap(),
                "{name}"
            );
        }
        assert!(!game.check_predicate("minecraft:h121_canonical").unwrap());
        assert!(game.check_predicate("h121:reference").unwrap());
        game.command_source = previous;
        assert_eq!(game.world.game_time, clock);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
