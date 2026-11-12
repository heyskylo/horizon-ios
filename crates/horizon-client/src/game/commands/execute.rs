//! `/execute` (ExecuteCommand): walk the parsed redirect chain, forking and
//! filtering command sources, then run the final command once per source,
//! reporting each result to the `store` callbacks of its source.
use super::{Arg, Context, Dim, Game, R, Target, data::DataRef, err};
use horizon_client::{
    command::{CommandError, vanilla},
    nbt_path::NbtPath,
    text::Text,
};
use horizon_world::{dimension_type::DimensionFacts, nbt::Tag};

/// `CommandSourceStack` fields Horizon models: the executing entity (None for
/// the server), position, rotation, anchor, whether feedback is suppressed
/// and the chain of `store` callbacks.
#[derive(Clone, Copy, Debug)]
pub(in crate::game) struct Source {
    pub entity: Option<Target>,
    pub pos: [f64; 3],
    pub rot: [f32; 2],
    /// EntityAnchorArgument.Anchor: eyes (true) or feet.
    pub eyes: bool,
    /// `withSuppressedOutput`: functions do not print feedback.
    pub silent: bool,
    /// Index of the newest `store` in `ExecState::stores`.
    pub store: Option<u32>,
    /// The level this command source uses. Horizon has only this dimension's
    /// chunks loaded in the single-player client.
    pub dimension: Dim,
    /// The dimension `execute in` named, if any: a server teleports its
    /// players there (`TeleportCommand` into the source's level).
    pub level: Option<super::super::Dim>,
    /// The command block whose output this source is
    /// (`BaseCommandBlock.CloseableCommandBlockSource`).
    pub block: Option<(i32, i32, i32)>,
    /// The command block minecart whose output this source is.
    pub cart: Option<u32>,
}

/// Where `execute store` writes.
#[derive(Clone, Debug)]
pub(in crate::game) enum StoreTarget {
    Score(Vec<String>, String),
    Bossbar(String, bool),
    Data(DataRef, NbtPath, String, f64),
}
#[derive(Clone, Debug)]
pub(in crate::game) struct StoreEntry {
    /// `store result` (true) or `store success`.
    result: bool,
    target: StoreTarget,
    parent: Option<u32>,
}
/// Command execution state shared across nested commands and functions.
#[derive(Default)]
pub(in crate::game) struct ExecState {
    stores: Vec<StoreEntry>,
    /// Commands run since the outermost one began.
    pub commands: u32,
    /// What the last leaf command reports to its `store` callbacks: None for
    /// the default, `Some(None)` for nothing (a function that did not
    /// `return`), `Some(Some((success, result)))` for an explicit value.
    pub callback: Option<Option<(bool, i32)>>,
    /// The innermost function frame's `return`.
    pub returned: Option<Option<i32>>,
    /// Depth of function calls.
    pub function_depth: u32,
    pub prepared_function: Option<super::super::functions::PreparedFunctionCall>,
}

/// A modifier that brings the sources to `max_command_forks` ends the chain
/// (`BuildContexts.ERROR_FORK_LIMIT_REACHED`); the error is only shown when
/// no fork came before it (`CommandSourceStack.handleError`).
fn fork_limit_reached(forked: bool, limit: usize) -> R<i32> {
    if forked {
        Ok(0)
    } else {
        Err(err("command.forkLimit", &[&limit]))
    }
}

/// Java's `(int)`/`(short)`/`(byte)` narrowing of a double.
fn java_int(v: f64) -> i32 {
    v as i32
}

impl Game<'_> {
    /// The source a command typed by the player starts from.
    pub(in crate::game) fn player_source(&self) -> Source {
        Source {
            // The server console has no entity (CommandSourceStack of the server).
            entity: self.player.is_some().then_some(Target::Player),
            pos: self.source_position(),
            rot: self.source_rotation(),
            eyes: false,
            silent: false,
            store: None,
            dimension: self.dim,
            level: None,
            block: None,
            cart: None,
        }
    }
    /// The server as a source (function ticks and schedules): the world spawn.
    pub(in crate::game) fn server_source(&self) -> Source {
        Source {
            entity: None,
            pos: self.server_position(),
            rot: [0.0, 0.0],
            eyes: false,
            silent: true,
            store: None,
            dimension: self.dim,
            level: None,
            block: None,
            cart: None,
        }
    }
    pub(in crate::game) fn target_rotation(&self, t: Target) -> [f32; 2] {
        match t {
            Target::Player => [self.yaw, self.pitch],
            Target::Guest(conn) => self
                .command_guest(conn)
                .map_or([0.0; 2], |g| [g.yaw, g.pitch]),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or([0.0; 2], |m| [m.yaw, m.head_pitch]),
            Target::Stand(id) => self
                .armor_stands
                .iter()
                .find(|a| a.id == id)
                .map_or([0.0; 2], |a| [a.yaw, 0.0]),
            Target::Boat(id) => self
                .boats
                .iter()
                .find(|b| b.id == id)
                .map_or([0.0; 2], |b| [b.yaw, b.pitch]),
            Target::Cart(id) => self
                .carts
                .iter()
                .find(|c| c.id == id)
                .map_or([0.0; 2], |c| [c.yaw, c.pitch]),
            Target::Fangs(id) => self
                .fangs
                .list
                .iter()
                .find(|f| f.id == id)
                .map_or([0.0; 2], |f| [f.yaw, 0.0]),
            Target::Other(id) => self
                .others
                .iter()
                .find(|o| o.id == id)
                .map_or([0.0; 2], |o| [o.yaw, o.pitch]),
            _ => [0.0; 2],
        }
    }
    pub(in crate::game) fn target_eye_height(&self, t: Target) -> f64 {
        match t {
            Target::Player => self
                .player
                .as_ref()
                .map_or(1.62, |p| f64::from(p.dimensions.eye_height)),
            Target::Guest(conn) => self.command_guest(conn).map_or(1.62, |g| g.eye_height()),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or(0.0, |m| m.eye_y() - m.position.y),
            // ArmorStand.getDefaultDimensions: a marker's are empty, a small
            // stand's are BABY_DIMENSIONS, whose eye height is 0.9875F.
            Target::Stand(id) => match self.armor_stands.iter().find(|a| a.id == id) {
                Some(a) if a.marker => 0.0,
                Some(a) if a.small => f64::from(0.9875f32),
                _ => self.type_eye_height(&self.target_type(t)),
            },
            // Entity.getEyeHeight: the type's dimensions.
            _ => self.type_eye_height(&self.target_type(t)),
        }
    }
    /// `EntityType.getDimensions().eyeHeight()` from `entity-types.json`,
    /// read once: a float, widened as `Entity.getEyeY` does.
    fn type_eye_height(&self, ty: &str) -> f64 {
        static EYE_HEIGHTS: std::sync::OnceLock<std::collections::HashMap<String, f64>> =
            std::sync::OnceLock::new();
        EYE_HEIGHTS
            .get_or_init(|| {
                std::fs::read(self.options.resources.join("entity-types.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .and_then(|v| {
                        v.as_object().map(|o| {
                            o.iter()
                                .filter_map(|(k, t)| {
                                    Some((k.clone(), f64::from(t["eye_height"].as_f64()? as f32)))
                                })
                                .collect()
                        })
                    })
                    .unwrap_or_default()
            })
            .get(ty)
            .copied()
            .unwrap_or(0.0)
    }
    /// EntityAnchorArgument.Anchor.apply.
    pub(super) fn anchor_position(&self, s: &Source) -> [f64; 3] {
        match s.entity {
            Some(t) if s.eyes => [s.pos[0], s.pos[1] + self.target_eye_height(t), s.pos[2]],
            _ => s.pos,
        }
    }
    /// Evaluate `f` with `source` as the command source, in the source's
    /// level (`CommandSourceStack.getLevel`): a server puts that level in
    /// place for it.
    pub(in crate::game) fn with_source<T>(
        &mut self,
        source: Source,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.command_source.replace(source);
        let back = self.enter_command_level(source.dimension);
        let out = f(self);
        if let Some(back) = back {
            self.enter_command_level(back);
        }
        self.command_source = previous;
        out
    }

    // ---- running commands -----------------------------------------------

    /// Run one command line with `source` (no error reporting): the entry
    /// for functions and `execute run`.
    pub(in crate::game) fn execute_line(&mut self, line: &str, source: Source) -> R<i32> {
        let d = vanilla::game_dispatcher_with_features(&self.level.enabled_features);
        let parse = d.parse(line, &self.ui.registries);
        let (ctx, _) = parse.executable()?;
        let ctx = ctx.clone();
        let root = parse.context.clone();
        self.with_source(source, |g| g.run_chain(&root, &ctx))
    }
    /// Run a parsed command (`root` is the first context; `ctx` the one with
    /// the executable node): `execute` and `return` walk their own chains.
    pub(in crate::game) fn run_chain(&mut self, root: &Context, ctx: &Context) -> R<i32> {
        self.exec.commands += 1;
        let limit = self.ui.rules.int("max_command_sequence_length").max(1) as u32;
        if self.exec.commands > limit {
            return Err(err("command.failed", &[]));
        }
        match root.literals.first().map(String::as_str) {
            Some("execute") => self.run_execute(root),
            Some("return") if root.child.is_some() => self.run_return_run(root),
            _ => {
                let id = ctx.command.unwrap_or("");
                self.run_leaf(id, ctx)
            }
        }
    }
    /// Run a leaf command and report its outcome to the `store` callbacks.
    pub(in crate::game) fn run_leaf(&mut self, id: &str, ctx: &Context) -> R<i32> {
        self.exec.callback = None;
        let store = self.command_source.and_then(|s| s.store);
        let result = self.execute_command(id, ctx);
        let custom = self.exec.callback.take();
        let outcome = match (&result, custom) {
            (_, Some(c)) => c,
            (Ok(v), None) => Some((true, *v)),
            (Err(_), None) => Some((false, 0)),
        };
        if let Some((success, value)) = outcome {
            self.deliver(store, success, value);
        }
        result
    }
    /// `CommandResultCallback`s of a source's `store` chain, oldest first.
    fn deliver(&mut self, store: Option<u32>, success: bool, value: i32) {
        // The outermost source's own CommandResultCallback (harness record).
        if self.exec.function_depth == 0
            && let Some(results) = self.cmds.results.as_mut()
        {
            results.push((success, value));
        }
        let mut chain = Vec::new();
        let mut at = store;
        while let Some(i) = at {
            let Some(e) = self.exec.stores.get(i as usize) else {
                break;
            };
            chain.push(e.clone());
            at = e.parent;
        }
        for e in chain.into_iter().rev() {
            let v = if e.result { value } else { i32::from(success) };
            self.store_value(&e.target, v);
        }
    }
    fn store_value(&mut self, target: &StoreTarget, value: i32) {
        match target {
            StoreTarget::Score(holders, objective) => {
                for h in holders {
                    self.score.board.set(h, objective, value);
                }
            }
            StoreTarget::Bossbar(id, is_value) => self.set_bossbar_value(id, *is_value, value),
            StoreTarget::Data(r, path, kind, scale) => {
                let Ok(mut data) = self.data_of(r) else {
                    return;
                };
                let v = f64::from(value) * scale;
                let tag = match kind.as_str() {
                    "int" => Tag::Int(java_int(v)),
                    "float" => Tag::Float((v as f32).to_bits()),
                    "short" => Tag::Short(java_int(v) as i16),
                    "long" => Tag::Long(v as i64),
                    "double" => Tag::Double(v.to_bits()),
                    _ => Tag::Byte(java_int(v) as i8),
                };
                if path.set(&mut data, &tag).is_ok() {
                    let _ = self.set_data_of(r, data);
                }
            }
        }
    }

    /// ExecuteCommand: `ctx` is the context holding the `execute` literal.
    pub(in crate::game) fn run_execute(&mut self, ctx: &Context) -> R<i32> {
        // BuildContexts: a modifier may not bring the sources to this many.
        let fork_limit = self.ui.rules.int("max_command_forks") as usize;
        let mut sources = vec![self.command_source.unwrap_or_else(|| self.player_source())];
        let mut c = ctx;
        // A fork modifier (as, at, positioned as, rotated as, facing entity,
        // on, a non-terminal if/unless) makes later errors silent
        // (CommandSourceStack.handleError with `forked`).
        let mut forked = false;
        // Nested execute/conditional/other modifier variants retain the old path.
        let mut function_batch_path = ctx.range.0 <= 1 && self.exec.function_depth == 0;
        let mut function_as_seen = false;
        loop {
            let word = c
                .literals
                .iter()
                .map(String::as_str)
                .find(|w| *w != "execute")
                .unwrap_or("");
            match word {
                "run" => {
                    let Some(command) = c.child.as_deref() else {
                        return Err(err("command.unknown.command", &[]));
                    };
                    let last = command.last_child();
                    if last.command.is_none() {
                        return Err(err("command.unknown.command", &[]));
                    }
                    let mut prepared = if function_batch_path
                        && function_as_seen
                        && command.literals.first().map(String::as_str) == Some("function")
                    {
                        self.prepare_function_batch(last, &sources)
                            .map(Vec::into_iter)
                    } else {
                        None
                    };
                    let mut total = 0;
                    let many = sources.len() > 1;
                    let mut failure = None;
                    for s in sources {
                        let result = if let Some(tasks) = prepared.as_mut() {
                            self.exec.prepared_function = tasks.next();
                            let result = self.with_source(s, |g| g.run_chain(command, last));
                            // Includes a command-cost rejection before the leaf and
                            // a return/error after the slot was consumed.
                            self.exec.prepared_function = None;
                            result
                        } else {
                            self.with_source(s, |g| g.run_chain(command, last))
                        };
                        match result {
                            Ok(n) => total += n,
                            Err(_) if forked => {}
                            // Several sources: report each failure and carry on.
                            Err(e) if many => self.command_failure(e.message),
                            Err(e) => failure = Some(e),
                        }
                        if self.exec.returned.is_some() {
                            break;
                        }
                    }
                    return match failure {
                        Some(e) => Err(e),
                        None => Ok(total),
                    };
                }
                "if" | "unless" => {
                    function_batch_path = false;
                    let want = word == "if";
                    if c.child.is_none() {
                        return self.run_conditional_terminal(c, sources, want, forked);
                    }
                    forked = true;
                    let mut kept = Vec::new();
                    for s in sources {
                        let (keep, _) = self.with_source(s, |g| g.condition(c, want))?;
                        if kept.len() + usize::from(keep) >= fork_limit {
                            return fork_limit_reached(forked, fork_limit);
                        }
                        if keep {
                            kept.push(s);
                        }
                    }
                    sources = kept;
                }
                _ => {
                    if word == "as" {
                        function_batch_path &= !function_as_seen;
                        function_as_seen = true;
                    } else if word != "store" {
                        function_batch_path = false;
                    }
                    forked |= match word {
                        "as" | "at" | "on" => true,
                        "positioned" | "rotated" => c.has("as"),
                        "facing" => c.has("entity"),
                        _ => false,
                    };
                    let mut next = Vec::new();
                    for s in sources {
                        let new = self.with_source(s, |g| g.modify(word, c, s))?;
                        if next.len() + new.len() >= fork_limit {
                            return fork_limit_reached(forked, fork_limit);
                        }
                        next.extend(new);
                    }
                    sources = next;
                }
            }
            match c.child.as_deref() {
                Some(child) => c = child,
                None => return Err(err("command.unknown.command", &[])),
            }
        }
    }
    /// A conditional that ends the chain runs as a command: a test per source.
    /// `CommandSourceStack.handleError`: error delivery depends on the fork flag,
    /// including a fork that yielded only one source; success feedback is unchanged.
    fn run_conditional_terminal(
        &mut self,
        c: &Context,
        sources: Vec<Source>,
        want: bool,
        forked: bool,
    ) -> R<i32> {
        let numeric = ["entity", "data", "blocks", "items", "slots"]
            .iter()
            .any(|k| c.has(k));
        let many = sources.len() > 1;
        let mut total = 0;
        let mut failure: Option<CommandError> = None;
        for s in sources {
            self.exec.callback = None;
            let (keep, count) = match self.with_source(s, |g| g.condition(c, want)) {
                Ok(v) => v,
                Err(e) => {
                    self.deliver(s.store, false, 0);
                    if !forked {
                        if many {
                            self.command_failure(e.message);
                        } else {
                            failure = Some(e);
                        }
                    }
                    continue;
                }
            };
            if keep {
                let n = if want && numeric { count } else { 1 };
                let message = if want && numeric {
                    Text::translate(
                        "commands.execute.conditional.pass_count",
                        &[Text::literal(count.to_string())],
                    )
                } else {
                    Text::translate("commands.execute.conditional.pass", &[])
                };
                self.with_source(s, |g| g.feedback(message));
                self.deliver(s.store, true, n);
                total += n;
            } else {
                self.deliver(s.store, false, 0);
                let e = if !want && numeric {
                    err("commands.execute.conditional.fail_count", &[&count])
                } else {
                    err("commands.execute.conditional.fail", &[])
                };
                if !forked {
                    if many {
                        self.command_failure(e.message);
                    } else {
                        failure = Some(e);
                    }
                }
            }
        }
        match failure {
            Some(e) => Err(e),
            None => Ok(total),
        }
    }
    /// `return run <command>`: run it and return its result from the frame.
    fn run_return_run(&mut self, root: &Context) -> R<i32> {
        let Some(command) = root.child.as_deref() else {
            return Err(err("command.unknown.command", &[]));
        };
        let last = command.last_child();
        let result = self.run_chain(command, last);
        // The command's own callbacks do not fire; the function frame gets
        // its result instead.
        match result {
            Ok(v) => self.finish_return(Some(v)),
            Err(e) => {
                self.finish_return(None);
                self.command_failure(e.message);
            }
        }
        self.exec.callback = Some(None);
        Ok(0)
    }
    /// `Frame.returnSuccess`/`returnFailure` then `discard`.
    pub(in crate::game) fn finish_return(&mut self, value: Option<i32>) {
        self.exec.returned = Some(value);
    }

    /// The sources one modifier produces from `s`.
    fn modify(&mut self, word: &str, c: &Context, s: Source) -> R<Vec<Source>> {
        let targets = |g: &mut Self| -> R<Vec<Target>> {
            // An empty selection forks to nothing rather than failing.
            match c.get("targets") {
                Some(Arg::Entity(sel)) => {
                    let sel = sel.clone();
                    Ok(g.select(&sel))
                }
                _ => Ok(Vec::new()),
            }
        };
        Ok(match word {
            "as" => targets(self)?
                .into_iter()
                .map(|t| Source {
                    entity: Some(t),
                    ..s
                })
                .collect(),
            "at" => targets(self)?
                .into_iter()
                .filter_map(|t| {
                    // CommandSourceStack.withLevel(entity.level()).
                    let level = self.target_level(t);
                    Some(Source {
                        pos: self.target_position(t)?,
                        rot: self.target_rotation(t),
                        dimension: level,
                        level: (level != self.dim).then_some(level),
                        ..s
                    })
                })
                .collect(),
            "positioned" if c.has("as") => targets(self)?
                .into_iter()
                .filter_map(|t| {
                    // withPosition keeps the anchor (only `positioned <pos>`
                    // resets it to the feet).
                    Some(Source {
                        pos: self.target_position(t)?,
                        ..s
                    })
                })
                .collect(),
            "positioned" if c.has("over") => {
                let Some(Arg::Str(kind)) = c.get("heightmap") else {
                    return Ok(vec![s]);
                };
                let (x, z) = (s.pos[0].floor() as i32, s.pos[2].floor() as i32);
                if !self.world.chunks.contains_key(&(x >> 4, z >> 4)) {
                    return Err(err("argument.pos.unloaded", &[]));
                }
                let y = self.heightmap_at(kind, x, z);
                vec![Source {
                    pos: [s.pos[0], f64::from(y), s.pos[2]],
                    ..s
                }]
            }
            "positioned" => {
                let pos = self
                    .position_arg(c, "pos")
                    .ok_or_else(|| err("argument.pos.missing.double", &[]))?;
                vec![Source {
                    pos,
                    eyes: false,
                    ..s
                }]
            }
            "rotated" if c.has("as") => targets(self)?
                .into_iter()
                .map(|t| Source {
                    rot: self.target_rotation(t),
                    ..s
                })
                .collect(),
            "rotated" => {
                let rot = match c.get("rot") {
                    Some(Arg::Coords(r)) => r.rotation(s.rot),
                    _ => s.rot,
                };
                vec![Source { rot, ..s }]
            }
            "facing" if c.has("entity") => {
                let eyes = matches!(c.get("anchor"), Some(Arg::Anchor(true)));
                targets(self)?
                    .into_iter()
                    .filter_map(|t| {
                        let p = self.target_position(t)?;
                        let y = if eyes {
                            p[1] + self.target_eye_height(t)
                        } else {
                            p[1]
                        };
                        Some(self.facing(s, [p[0], y, p[2]]))
                    })
                    .collect()
            }
            "facing" => {
                let to = self
                    .position_arg(c, "pos")
                    .ok_or_else(|| err("argument.pos.missing.double", &[]))?;
                vec![self.facing(s, to)]
            }
            "align" => {
                let Some(Arg::Str(axes)) = c.get("axes") else {
                    return Ok(vec![s]);
                };
                let mut pos = s.pos;
                for (i, axis) in ['x', 'y', 'z'].into_iter().enumerate() {
                    if axes.contains(axis) {
                        pos[i] = pos[i].floor();
                    }
                }
                vec![Source { pos, ..s }]
            }
            "anchored" => vec![Source {
                eyes: matches!(c.get("anchor"), Some(Arg::Anchor(true))),
                ..s
            }],
            "in" => {
                // CommandSourceStack.withLevel scales the source position;
                // absolute coordinates passed to /tp are unaffected.
                let Some(Arg::Resource(dim)) = c.get("dimension") else {
                    return Ok(vec![s]);
                };
                let Some(dimension) = self.known_level(dim) else {
                    return Err(err("argument.dimension.invalid", &[dim]));
                };
                let scale = DimensionFacts::teleportation_scale(
                    self.dimension_type_of(s.dimension),
                    self.dimension_type_of(dimension),
                );
                vec![Source {
                    pos: [s.pos[0] * scale, s.pos[1], s.pos[2] * scale],
                    dimension,
                    level: Some(dimension),
                    ..s
                }]
            }
            "summon" => {
                let Some(Arg::Resource(kind)) = c.get("entity").cloned() else {
                    return Ok(vec![s]);
                };
                let target = self.summon_with_nbt(&kind, s.pos, "{}")?;
                let Some(t) = target else {
                    return Err(err("commands.summon.failed", &[]));
                };
                vec![Source {
                    entity: Some(t),
                    ..s
                }]
            }
            "on" => self.relation(c, s),
            "store" => vec![self.store_source(c, s)?],
            _ => return Err(err("command.unknown.command", &[])),
        })
    }
    /// `ExecuteCommand.createRelationOperations`: the entities `on <relation>` reaches.
    fn relation(&mut self, c: &Context, s: Source) -> Vec<Source> {
        let Some(entity) = s.entity else {
            return Vec::new();
        };
        let found: Vec<Target> = match c.literals.last().map(String::as_str) {
            Some("owner") => match entity {
                // OwnableEntity.getOwner → EntityReference.getLivingEntity:
                // a marker or a display with that UUID is not an owner.
                Target::Mob(id) => self
                    .mobs
                    .iter()
                    .find(|m| m.id == id)
                    .and_then(|m| m.extra.owner)
                    // A guest owner too, not only the player in place.
                    .and_then(|u| {
                        self.target_by_uuid(horizon_client::lan::playerdata::ints_uuid(u))
                    })
                    .filter(|owner| {
                        matches!(
                            owner,
                            Target::Player
                                | Target::Guest(_)
                                | Target::Mob(_)
                                | Target::Stand(_)
                                | Target::Dragon(_)
                        )
                    })
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            },
            // Leashable.getLeashHolder: the server's resolved holder, not a
            // saved UUID that is still waiting for tickLeash to restore it.
            Some("leasher") => {
                let leash =
                    match entity {
                        Target::Mob(id) => self.mobs.iter().find(|m| m.id == id).and_then(|m| {
                            m.extra.leash.map(|holder| (holder, m.extra.leash_player))
                        }),
                        Target::Boat(id) => self
                            .boats
                            .iter()
                            .find(|b| b.id == id)
                            .and_then(|b| b.leash.map(|holder| (holder, b.leash_player))),
                        _ => None,
                    };
                leash
                    .and_then(|(holder, player)| match holder {
                        // The player who tied it (the host's when unrecorded).
                        horizon_client::mobs::leash::Holder::Player => {
                            let uuid = player.or(self.level.player_uuid)?;
                            self.target_by_uuid(horizon_client::lan::playerdata::ints_uuid(uuid))
                        }
                        horizon_client::mobs::leash::Holder::Mob(i) => Some(Target::Mob(i)),
                        horizon_client::mobs::leash::Holder::Boat(_)
                        | horizon_client::mobs::leash::Holder::Knot(_) => None,
                    })
                    .into_iter()
                    .collect()
            }
            // TraceableEntity.getOwner: who shot a projectile, who threw an
            // item, who summoned a vex.
            Some("origin") => match entity {
                Target::Projectile(id) => self
                    .creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| {
                        // Projectile.getOwner resolves EntityReference<Entity>,
                        // including loaded owners that cannot be a Who.
                        let Some(owner) = p.owner else {
                            return p.owner_uuid.and_then(|uuid| {
                                self.target_by_uuid(horizon_client::lan::playerdata::ints_uuid(
                                    uuid,
                                ))
                            });
                        };
                        match (owner, p.guest) {
                            // A guest's shot: `Who::Player` is them.
                            (horizon_client::mobs::Who::Player, Some(conn))
                                if self.lan_acting == Some(conn) =>
                            {
                                Some(Target::Player)
                            }
                            (horizon_client::mobs::Who::Player, Some(conn)) => {
                                Some(Target::Guest(conn))
                            }
                            (horizon_client::mobs::Who::Player, None) => Some(Target::Player),
                            (horizon_client::mobs::Who::Mob(id), _) => Some(Target::Mob(id)),
                        }
                    })
                    .into_iter()
                    .collect(),
                Target::Item(id) => self
                    .dropped
                    .iter()
                    .find(|d| d.id == id)
                    .and_then(|d| d.thrower)
                    .and_then(|words| {
                        let uuid = words
                            .iter()
                            .fold(0u128, |all, w| all << 32 | u128::from(*w as u32));
                        self.target_by_uuid(uuid)
                    })
                    .into_iter()
                    .collect(),
                Target::Mob(id) => self
                    .mobs
                    .iter()
                    .find(|m| m.id == id && m.kind.id == "minecraft:vex")
                    .map(|m| m.extra.counter2 as u32)
                    .filter(|owner| *owner != 0)
                    .map(Target::Mob)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            },
            Some("vehicle") => self.ride_vehicle(entity).into_iter().collect(),
            Some("passengers") => {
                let mut passengers = self.player_passengers(entity);
                // Direct runtime children only. Complete mount insertion order is
                // not modeled; retain the existing mob collection order here.
                if let Target::Mob(id) | Target::Boat(id) = entity {
                    passengers.extend(
                        self.mobs
                            .iter()
                            .filter(|m| m.extra.mount == Some(id) && m.alive() && !m.removed)
                            .map(|m| Target::Mob(m.id)),
                    );
                }
                passengers
            }
            Some("controller") => match entity {
                Target::Mob(id) if self.mobs.iter().any(|m| m.id == id && m.rider.is_some()) => {
                    vec![Target::Player]
                }
                _ => Vec::new(),
            },
            Some("target") | Some("attacker") => {
                let attacker = c.literals.last().is_some_and(|l| l == "attacker");
                match entity {
                    Target::Mob(id) => self
                        .mobs
                        .iter()
                        .find(|m| m.id == id)
                        .and_then(|m| {
                            if attacker {
                                m.ai.last_hurt_by
                            } else {
                                m.ai.target
                            }
                        })
                        .map(|w| match w {
                            horizon_client::mobs::Who::Player => {
                                // LivingEntity.getLastHurtByMob is the player
                                // who hit it, not whichever client is reading it.
                                match self.last_hurt_by_player.get(&id) {
                                    Some(Some(conn))
                                        if attacker && self.lan_acting != Some(*conn) =>
                                    {
                                        Target::Guest(*conn)
                                    }
                                    _ => Target::Player,
                                }
                            }
                            horizon_client::mobs::Who::Mob(i) => Target::Mob(i),
                        })
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        found
            .into_iter()
            .filter(|t| self.target_position(*t).is_some())
            .map(|t| Source {
                entity: Some(t),
                ..s
            })
            .collect()
    }
    /// The players among `Entity.getPassengers` of `vehicle`: the player in
    /// place when it rides it, then the other players of the server riding it.
    /// A server's players are its guests, so a datapack running with no player
    /// in place still finds them.
    fn player_passengers(&self, vehicle: Target) -> Vec<Target> {
        let id = match vehicle {
            Target::Mob(id) | Target::Boat(id) | Target::Cart(id) => id,
            _ => return Vec::new(),
        };
        let in_place = self.player.is_some() && self.riding == Some(id);
        let guests = self.lan.iter().flat_map(|host| {
            host.hub
                .guests
                .iter()
                .filter(move |g| {
                    g.placed
                        && Some(g.conn) != self.lan_acting
                        && host.hub.here(g)
                        && g.vehicle == Some(id)
                })
                .map(|g| Target::Guest(g.conn))
        });
        in_place
            .then_some(Target::Player)
            .into_iter()
            .chain(guests)
            .collect()
    }
    /// `ServerLevel.getEntity(UUID)`: any entity of the level in place, not
    /// only mobs (displays, armor stands, items...). The map is keyed by id,
    /// so a killed entity and a later summon of the same UUID can both be
    /// in it; the one the level still holds is the entity.
    pub(in crate::game) fn target_with_uuid(&self, uuid: [i32; 4]) -> Option<Target> {
        if is_player_in_place(uuid, self.lan_acting_uuid, self.level.player_uuid) {
            return Some(Target::Player);
        }
        self.persist.uuids.iter().find_map(|(&id, found)| {
            if *found != uuid {
                return None;
            }
            self.level_entity(i32::try_from(id).ok()?)
        })
    }
    /// The highest block of a heightmap type at a column (`Level.getHeight`).
    fn heightmap_at(&self, kind: &str, x: i32, z: i32) -> i32 {
        let top = self.world.top_y();
        for y in (self.world.bottom_y()..top).rev() {
            let state = self.world.block(x, y, z);
            if state == self.world.air {
                continue;
            }
            let facts = self.res.facts().get(state);
            let blocks = facts.legacy_solid || facts.fluid != horizon_client::blocks::Fluid::None;
            let leaves = self
                .pack
                .assets
                .registry
                .state(state)
                .is_some_and(|s| s.name.ends_with("_leaves"));
            let found = match kind {
                "ocean_floor" => facts.legacy_solid,
                "motion_blocking" => blocks,
                "motion_blocking_no_leaves" => blocks && !leaves,
                _ => true,
            };
            if found {
                return y + 1;
            }
        }
        self.world.bottom_y()
    }
    /// A `store` modifier: a new callback in the source's chain.
    fn store_source(&mut self, c: &Context, s: Source) -> R<Source> {
        let result = c.has("result");
        let target = if c.has("score") {
            let holders = self.score_holder_names(c, "targets", true)?;
            let Some(Arg::Str(objective)) = c.get("objective") else {
                return Err(err("command.failed", &[]));
            };
            if self.score.board.objective(objective).is_none() {
                return Err(err("arguments.objective.notFound", &[objective]));
            }
            StoreTarget::Score(holders, objective.clone())
        } else if c.has("bossbar") {
            let Some(Arg::Resource(id)) = c.get("id") else {
                return Err(err("command.failed", &[]));
            };
            if !self.bossbar_exists(id) {
                return Err(err("commands.bossbar.unknown", &[id]));
            }
            StoreTarget::Bossbar(id.clone(), c.has("value"))
        } else {
            let r = self.data_ref(c, 0)?;
            let path = match c.get("path") {
                Some(Arg::Str(p)) => {
                    NbtPath::parse(p).map_err(horizon_client::data_cmd::path_error)?
                }
                _ => return Err(err("command.failed", &[])),
            };
            let kind = c
                .literals
                .iter()
                .find(|l| {
                    matches!(
                        l.as_str(),
                        "int" | "float" | "short" | "long" | "double" | "byte"
                    )
                })
                .cloned()
                .unwrap_or_else(|| "int".into());
            let scale = match c.get("scale") {
                Some(Arg::Double(v)) => *v,
                _ => 1.0,
            };
            StoreTarget::Data(r, path, kind, scale)
        };
        self.exec.stores.push(StoreEntry {
            result,
            target,
            parent: s.store,
        });
        Ok(Source {
            store: Some(self.exec.stores.len() as u32 - 1),
            ..s
        })
    }
    /// CommandSourceStack.facing: rotation from the anchor point to `to`.
    fn facing(&self, s: Source, to: [f64; 3]) -> Source {
        let from = self.anchor_position(&s);
        let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
        let horizontal = (dx * dx + dz * dz).sqrt();
        let pitch = (-(dy.atan2(horizontal).to_degrees())) as f32;
        let yaw = (dz.atan2(dx).to_degrees() - 90.0) as f32;
        Source {
            rot: [yaw.rem_euclid(360.0), pitch],
            ..s
        }
    }

    // ---- conditions ----------------------------------------------------------

    /// Whether the source stays for `if`/`unless` (`want`), and the count the
    /// numeric conditions report.
    fn condition(&mut self, c: &Context, want: bool) -> R<(bool, i32)> {
        if c.has("function") {
            return self.condition_function(c, want);
        }
        let (matched, count) = self.condition_matches(c)?;
        Ok((matched == want, count))
    }
    /// `FallthroughTask.execute`: a fallthrough supplies failure/zero to the
    /// function-condition continuation. Preserve explicit-result decisions
    /// and direct function callbacks.
    fn condition_function(&mut self, c: &Context, want: bool) -> R<(bool, i32)> {
        let Some(Arg::Resource(name)) = c.get("name").cloned() else {
            return Ok((false, 0));
        };
        let results = self.call_function_ref(&name, None)?;
        let keep = results.iter().any(|r| r.is_some_and(|v| (v != 0) == want));
        let fallthrough = !results.is_empty() && results.iter().all(Option::is_none);
        Ok((keep || (!want && fallthrough), 0))
    }
    /// `ExecuteCommand.isChunkLoaded`: an entity-ticking loading status and
    /// completed entity loading, rather than merely a retained lit column.
    fn command_chunk_loaded(&self, x: i32, z: i32) -> bool {
        self.tickets.levels().loaded_entity_ticking(x, z)
            && self.world.chunks.contains_key(&(x, z))
            && self.streaming.is_lit(x, z)
            // Streaming queues the saved tags before making the column resident;
            // spawn_loaded_entities applies them before this chunk's entities run.
            && !self.world.loaded_entities.iter().any(|(key, _)| *key == (x, z))
    }

    fn condition_matches(&mut self, c: &Context) -> R<(bool, i32)> {
        let flag = |b: bool| (b, i32::from(b));
        if c.has("data") {
            let r = self.data_ref_named(c, 0, "source")?;
            let data = self.data_of(&r)?;
            let Some(Arg::Str(p)) = c.get("path") else {
                return Ok((false, 0));
            };
            let path = NbtPath::parse(p).map_err(horizon_client::data_cmd::path_error)?;
            let n = path.count(&data) as i32;
            return Ok((n > 0, n));
        }
        if c.has("items") || c.has("slots") {
            return self.condition_items(c);
        }
        if c.has("entity") {
            let Some(Arg::Entity(sel)) = c.get("entities").cloned() else {
                return Ok((false, 0));
            };
            let n = self.select(&sel).len() as i32;
            return Ok((n > 0, n));
        }
        if c.has("loaded") {
            let p = self
                .coords(c, "pos")
                .map(|co| co.block(self.source_position(), self.source_rotation()));
            let dimension = self.command_source.map_or(self.dim, |s| s.dimension);
            return Ok(flag(
                dimension == self.dim
                    && p.is_some_and(|p| self.command_chunk_loaded(p[0] >> 4, p[2] >> 4)),
            ));
        }
        if c.has("dimension") {
            let dimension = self.command_source.map_or(self.dim, |s| s.dimension);
            return Ok(flag(matches!(c.get("dimension"), Some(Arg::Resource(d))
                if d.as_str() == dimension.id())));
        }
        if c.has("block") {
            let p = self.block_pos_arg(c, "pos")?;
            let Some(Arg::Block(b)) = c.get("block").cloned() else {
                return Ok((false, 0));
            };
            return Ok(flag(self.block_matches(&b, p)));
        }
        if c.has("biome") {
            let p = self.block_pos_arg(c, "pos")?;
            let Some(Arg::Resource(want_biome)) = c.get("biome").cloned() else {
                return Ok((false, 0));
            };
            let name = self.biome_name_at_block(p);
            return Ok(flag(
                name.is_some_and(|n| self.biome_matches(&want_biome, &n)),
            ));
        }
        if c.has("score") {
            return self.condition_score(c).map(flag);
        }
        if c.has("blocks") {
            return self.condition_blocks(c);
        }
        if c.has("range") && c.has("id") {
            return self.condition_stopwatch(c).map(flag);
        }
        if c.has("predicate") {
            let Some(Arg::Resource(id)) = c.get("predicate").cloned() else {
                return Ok((false, 0));
            };
            return Ok(flag(self.check_predicate(&id)?));
        }
        Ok((false, 0))
    }
    fn condition_stopwatch(&mut self, c: &Context) -> R<bool> {
        let Some(Arg::Resource(id)) = c.get("id").cloned() else {
            return Ok(false);
        };
        let Some(Arg::DoubleRange((lo, hi))) = c.get("range").cloned() else {
            return Ok(false);
        };
        let Some(t) = self.score.stopwatches.elapsed_seconds(&id) else {
            return Err(err("commands.stopwatch.does_not_exist", &[&id]));
        };
        Ok(lo.is_none_or(|l| t >= l) && hi.is_none_or(|h| t <= h))
    }
    fn condition_score(&mut self, c: &Context) -> R<bool> {
        let target = self.score_holder_names(c, "target", false)?;
        let Some(Arg::Str(to)) = c.get("targetObjective").cloned() else {
            return Ok(false);
        };
        if self.score.board.objective(&to).is_none() {
            return Err(err("arguments.objective.notFound", &[&to]));
        }
        let Some(a) = target
            .first()
            .and_then(|t| self.score.board.score(t, &to))
            .map(|s| s.value)
        else {
            return Ok(false);
        };
        if c.has("matches") {
            let Some(Arg::IntRange((lo, hi))) = c.get("range") else {
                return Ok(false);
            };
            return Ok(lo.is_none_or(|l| a >= l) && hi.is_none_or(|h| a <= h));
        }
        let source = self.score_holder_names(c, "source", false)?;
        let Some(Arg::Str(so)) = c.get("sourceObjective").cloned() else {
            return Ok(false);
        };
        if self.score.board.objective(&so).is_none() {
            return Err(err("arguments.objective.notFound", &[&so]));
        }
        let Some(b) = source
            .first()
            .and_then(|t| self.score.board.score(t, &so))
            .map(|s| s.value)
        else {
            return Ok(false);
        };
        let op = c.literals.last().map_or("", String::as_str);
        Ok(match op {
            "=" => a == b,
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            _ => false,
        })
    }
    /// `if blocks`: the region at `start..end` equals the one at `destination`.
    fn condition_blocks(&mut self, c: &Context) -> R<(bool, i32)> {
        let (a, b, d) = (
            self.block_pos_arg(c, "start")?,
            self.block_pos_arg(c, "end")?,
            self.block_pos_arg(c, "destination")?,
        );
        let masked = c.has("masked");
        let lo = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let hi = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        let area = i64::from(hi[0] - lo[0] + 1)
            * i64::from(hi[1] - lo[1] + 1)
            * i64::from(hi[2] - lo[2] + 1);
        if area > 32768 {
            return Err(err("commands.execute.blocks.toobig", &[&32768, &area]));
        }
        let offset = [d[0] - lo[0], d[1] - lo[1], d[2] - lo[2]];
        let mut count = 0;
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let src = self.world.block(x, y, z);
                    if masked && src == self.world.air {
                        continue;
                    }
                    let (dx, dy, dz) = (x + offset[0], y + offset[1], z + offset[2]);
                    if src != self.world.block(dx, dy, dz) {
                        return Ok((false, 0));
                    }
                    let (se, de) = (
                        self.world.block_entities.get(&(x, y, z)),
                        self.world.block_entities.get(&(dx, dy, dz)),
                    );
                    if let Some(se) = se {
                        let name = self.data_block_name([x, y, z]);
                        let Some(de) = de else {
                            return Ok((false, 0));
                        };
                        let st = horizon_client::save::encode_block_entity((0, 0, 0), se, &name);
                        let dt = horizon_client::save::encode_block_entity((0, 0, 0), de, &name);
                        if st != dt {
                            return Ok((false, 0));
                        }
                    }
                    count += 1;
                }
            }
        }
        Ok((true, count))
    }
    /// The biome name at a block position.
    pub(in crate::game) fn biome_name_at_block(&self, p: [i32; 3]) -> Option<String> {
        let chunk = self.world.chunks.get(&(p[0] >> 4, p[2] >> 4))?;
        // The world's table (`world_biomes!`): a packed biome's id is past
        // the built-in list, and `Biomes::get` would clamp it to the last
        // vanilla one.
        Some(
            world_biomes!(self)
                .get(chunk.biome(p[0], p[1], p[2]))
                .name
                .clone(),
        )
    }
    /// A biome id or `#tag` matched against a biome name.
    pub(super) fn biome_matches(&self, wanted: &str, biome: &str) -> bool {
        match wanted.strip_prefix('#') {
            Some(tag) => self.biome_tag_has(tag, biome),
            None => wanted == biome,
        }
    }
    /// `#tag`: the biome tags the game's files and the world's packs define,
    /// nested tags expanded; an unknown tag falls back to the game's built-in
    /// table.
    fn biome_tag_has(&self, tag: &str, biome: &str) -> bool {
        let entries = self.tag_entries("worldgen/biome", tag);
        entries.iter().any(|id| id == biome)
            || (entries.is_empty() && self.res.mob_data().biome_has(tag, biome))
    }
}

/// Whether `uuid` is the player in place (`Target::Player`): during a guest's turn that is the guest, not the
/// level's own player (the world owner), whom `Level.getEntity(UUID)` finds only while they are in the level.
fn is_player_in_place(
    uuid: [i32; 4],
    acting: Option<u128>,
    level_player: Option<[i32; 4]>,
) -> bool {
    match acting {
        Some(guest) => horizon_client::lan::playerdata::ints_uuid(uuid) == guest,
        None => Some(uuid) == level_player,
    }
}

#[cfg(test)]
mod tests {
    use super::is_player_in_place;
    use horizon_client::lan::playerdata::ints_uuid;

    const OWNER: [i32; 4] = [0, 0, 0, 12];
    const GUEST: [i32; 4] = [0, 0, 0, 13];

    /// A owns the world, B is a guest: the sequence of one command each way.
    #[test]
    fn the_owners_uuid_is_not_the_guest_in_place() {
        // A's own command, or the single-player one: no guest in place, the level's player is.
        assert!(is_player_in_place(OWNER, None, Some(OWNER)));
        assert!(!is_player_in_place(GUEST, None, Some(OWNER)));
        // B's turn: only B is in place, the owner's UUID names someone else.
        let b = Some(ints_uuid(GUEST));
        assert!(is_player_in_place(GUEST, b, Some(OWNER)));
        assert!(!is_player_in_place(OWNER, b, Some(OWNER)));
        // A's own turn as a guest of an integrated server.
        let a = Some(ints_uuid(OWNER));
        assert!(is_player_in_place(OWNER, a, Some(OWNER)));
        assert!(!is_player_in_place(GUEST, a, Some(OWNER)));
        // No level player at all (a dedicated server): nobody is in place without a guest.
        assert!(!is_player_in_place(OWNER, None, None));
    }
}

#[cfg(test)]
mod passenger_relation_tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    use horizon_core::position::BlockPos;
    use horizon_world::nbt::Tag;

    fn actor(i: usize) -> String {
        format!("00000096-0000-0002-0000-{i:012x}")
    }
    fn value(game: &Game<'_>, i: usize, objective: &str) -> i32 {
        game.score.board.score(&actor(i), objective).unwrap().value
    }
    fn vehicle(game: &mut Game<'_>, child: usize, parent: usize) {
        assert!(game.run_command("scoreboard players set #r h150u 91"));
        assert!(game.run_command("scoreboard players set #s h150u 17"));
        assert!(game.run_command(&format!(
            "execute store result score #r h150u store success score #s h150u as {} on vehicle if entity @s[nbt={{UUID:[I;150,2,0,{parent}]}}]", actor(child)
        )));
        assert_eq!(game.score.board.score("#r", "h150u").unwrap().value, 1);
        assert_eq!(game.score.board.score("#s", "h150u").unwrap().value, 1);
    }
    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn h150_actual_registered_passengers_select_direct_live_mob_and_boat_children() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        for objective in ["h150u", "h150ur", "h150us"] {
            assert!(game.run_command(&format!("scoreboard objectives add {objective} dummy")));
        }
        let clock = game.world.game_time;
        // 1->2->3 is a nested mob tree; 4->5 a boat; 6->7,8 two direct children.
        let kinds = [
            "cow", "pig", "chicken", "oak_boat", "cow", "cow", "pig", "chicken", "pig", "cow",
        ];
        let mut ids = Vec::new();
        for (offset, kind) in kinds.into_iter().enumerate() {
            let i = offset + 1;
            let extra = if kind == "oak_boat" {
                ""
            } else {
                ",NoAI:1b,Health:1f"
            };
            assert!(game.run_command(&format!(
                "summon {kind} 8.5 200 8.5 {{UUID:[I;150,2,0,{i}],Tags:[\"h150u\"],NoGravity:1b,Silent:1b,Motion:[0d,0d,0d]{extra}}}"
            )));
            let words = [150, 2, 0, i as i32];
            let id = *game
                .persist
                .uuids
                .iter()
                .find(|(_, u)| **u == words)
                .unwrap()
                .0;
            let target = if kind == "oak_boat" {
                Target::Boat(id)
            } else {
                Target::Mob(id)
            };
            assert_eq!(game.target_type(target), format!("minecraft:{kind}"));
            assert_eq!(game.target_position(target), Some([8.5, 200.0, 8.5]));
            let saved = game.entity_nbt(target).unwrap();
            assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(words.to_vec())));
            if kind != "oak_boat" {
                assert_eq!(saved.get("Health"), Some(&Tag::Float(1f32.to_bits())));
                assert_eq!(saved.get("NoAI"), Some(&Tag::Byte(1)));
            }
            assert!(game.world.loaded(8, 8));
            assert!(game.run_command("execute if block 8 199 8 stone"));
            // Membership must come from runtime links; no retained Passengers body remains.
            game.persist.raw.remove(&id);
            ids.push(id);
        }
        for (child, parent) in [(2, 1), (3, 2), (5, 4), (7, 6), (8, 6)] {
            assert!(game.run_command(&format!("ride {} mount {}", actor(child), actor(parent))));
            assert_eq!(
                game.mobs
                    .iter()
                    .find(|m| m.id == ids[child - 1])
                    .unwrap()
                    .extra
                    .mount,
                Some(ids[parent - 1])
            );
            vehicle(&mut game, child, parent);
        }
        let mut failures = Vec::new();
        for (phase, parent, depth, selected) in [
            ("direct", 1, 1, vec![2]),
            ("nested", 1, 2, vec![3]),
            ("boat", 4, 1, vec![5]),
            ("multiple", 6, 1, vec![7, 8]),
            ("empty", 9, 1, vec![]),
        ] {
            for (objective, n) in [("h150u", 0), ("h150ur", 91), ("h150us", 17)] {
                for i in 1..=10 {
                    assert!(game.run_command(&format!(
                        "scoreboard players set {} {objective} {n}",
                        actor(i)
                    )));
                }
            }
            let nested = if depth == 2 { " on passengers" } else { "" };
            game.cmds.results = Some(Vec::new());
            game.chat_capture = Some(Vec::new());
            assert!(game.run_command(&format!(
                "execute as {} on passengers{nested} store result score @s h150ur store success score @s h150us run scoreboard players add @s h150u 1", actor(parent)
            )));
            let callbacks = game.cmds.results.take().unwrap();
            let messages: Vec<_> = game
                .chat_capture
                .take()
                .unwrap()
                .iter()
                .map(|t| t.plain())
                .collect();
            println!("H150 actual {phase}: callbacks={callbacks:?}, feedback={messages:?}");
            let expected = vec![(true, 1); selected.len()];
            if selected.is_empty() {
                assert_eq!(callbacks, expected, "independent empty parent callback");
            } else if callbacks != expected {
                failures.push(format!("{phase} callbacks {callbacks:?} != {expected:?}"));
            }
            for i in 1..=10 {
                let yes = selected.contains(&i);
                let got = (
                    value(&game, i, "h150u"),
                    value(&game, i, "h150ur"),
                    value(&game, i, "h150us"),
                );
                let want = if yes { (1, 1, 1) } else { (0, 91, 17) };
                if phase == "empty" || !yes {
                    assert_eq!(
                        got, want,
                        "independent empty/unrelated/no-descendant control: {phase} {i}"
                    );
                } else if got != want {
                    failures.push(format!("{phase} {i}: {got:?} != {want:?}"));
                }
            }
        }
        assert!(game.run_command(&format!("ride {} dismount", actor(2))));
        assert_eq!(
            game.mobs
                .iter()
                .find(|m| m.id == ids[1])
                .unwrap()
                .extra
                .mount,
            None
        );
        vehicle(&mut game, 3, 2);
        for phase in ["dismounted", "remounted"] {
            if phase == "remounted" {
                assert!(game.run_command(&format!("ride {} mount {}", actor(2), actor(1))));
                vehicle(&mut game, 2, 1);
            }
            for (objective, n) in [("h150u", 0), ("h150ur", 91), ("h150us", 17)] {
                for i in 1..=10 {
                    assert!(game.run_command(&format!(
                        "scoreboard players set {} {objective} {n}",
                        actor(i)
                    )));
                }
            }
            assert!(game.run_command(&format!(
                "execute as {} on passengers store result score @s h150ur store success score @s h150us run scoreboard players add @s h150u 1", actor(1)
            )));
            for i in [1, 2, 3, 10] {
                let got = (
                    value(&game, i, "h150u"),
                    value(&game, i, "h150ur"),
                    value(&game, i, "h150us"),
                );
                let want = if phase == "remounted" && i == 2 {
                    (1, 1, 1)
                } else {
                    (0, 91, 17)
                };
                if phase == "dismounted" || i != 2 {
                    assert_eq!(
                        got, want,
                        "independent dismount/unrelated control: {phase} {i}"
                    );
                } else if got != want {
                    failures.push(format!("{phase} {i}: {got:?} != {want:?}"));
                }
            }
        }
        assert_eq!(game.world.game_time, clock, "zero actual observation ticks");
        assert!(
            failures.is_empty(),
            "actual registered direct passenger branch/callback/store failures: {failures:?}"
        );
    }
}

#[cfg(test)]
mod terminal_fork_tests {
    use super::Game;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    fn capture(game: &mut Game<'_>, command: &str) -> (Vec<String>, Vec<(bool, i32)>) {
        game.chat_capture = Some(Vec::new());
        game.cmds.results = Some(Vec::new());
        game.as_console(|g| g.run_command(command));
        let messages = game
            .chat_capture
            .take()
            .unwrap()
            .iter()
            .map(|t| t.plain())
            .collect();
        (messages, game.cmds.results.take().unwrap())
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn terminal_conditions_keep_the_fork_state_and_callbacks() {
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
        options.game_dir = Some(std::env::current_dir().unwrap().join("game"));
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
        let mut chunk = Chunk::empty(0, 0);
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone.0);
            }
        }
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("scoreboard objectives add h147 dummy"));
        assert!(game.run_command("scoreboard players set zero h147 0"));
        for n in [1, 2] {
            assert!(game.run_command(&format!("summon pig {} 200 4.5 {{UUID:[I;147,0,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Tags:[\"h147\"]}}", n as f64 + 0.5)));
            assert!(
                game.mobs
                    .iter()
                    .any(|m| game.persist.uuids.get(&m.id) == Some(&[147, 0, 0, n])
                        && m.kind.id == "minecraft:pig"
                        && m.health == 10.0)
            );
        }
        let time = game.world.game_time;
        let a = "00000093-0000-0000-0000-000000000001";
        let positive = capture(
            &mut game,
            &format!("execute as {a} if score zero h147 matches 0"),
        );
        assert_eq!(positive.0.len(), 1);
        assert_eq!(positive.1, vec![(true, 1)]);
        let direct = capture(&mut game, "execute if score zero h147 matches 1");
        assert_eq!(direct.0.len(), 1);
        assert_eq!(direct.1, vec![(false, 0)]);
        for tail in [
            format!("as {a} if score zero h147 matches 1"),
            format!("at {a} if block ~ ~-1 ~ air"),
            format!("as {a} if score zero h147_missing matches 1"),
        ] {
            assert!(game.run_command("scoreboard players set result h147 -99"));
            assert!(game.run_command("scoreboard players set success h147 -99"));
            let (messages, callbacks) = capture(
                &mut game,
                &format!(
                    "execute store result score result h147 store success score success h147 {tail}"
                ),
            );
            assert_eq!(callbacks, vec![(false, 0)]);
            assert_eq!(
                game.score.board.score("result", "h147").map(|s| s.value),
                Some(0)
            );
            assert_eq!(
                game.score.board.score("success", "h147").map(|s| s.value),
                Some(0)
            );
            assert!(messages.is_empty(), "forked terminal error: {messages:?}");
        }
        let many = capture(
            &mut game,
            "execute as @e[type=pig,tag=h147] if score zero h147 matches 1",
        );
        assert!(many.0.is_empty());
        assert_eq!(many.1, vec![(false, 0), (false, 0)]);
        let missing = capture(&mut game, "execute if score zero h147_missing matches 1");
        assert_eq!(missing.0.len(), 1);
        assert_eq!(missing.1, vec![(false, 0)]);
        let empty = capture(
            &mut game,
            "execute as @e[tag=h147_none] if score zero h147 matches 1",
        );
        assert!(empty.0.is_empty() && empty.1.is_empty());
        assert_eq!(game.world.game_time, time);
    }
}
