//! Headless command runner for `tools/test_command_exec_parity.py`: with
//! `HORIZON_COMMAND_SCRIPT` and `HORIZON_COMMAND_OUTPUT` set, the opened world
//! runs on its server thread with a connected, headless client. Each script
//! line is a JSON operation: `cmd` runs a
//! command as the player (Commands.performPrefixedCommand) and records every
//! system message it produced, as styled spans, with the results reported to
//! the player's `CommandResultCallback`; `tick` advances the game; `settle`
//! waits until the terrain and entities around the player are loaded;
//! `use_item` dispatches the main-hand server packet; `map_state` reads saved map metadata;
//! `use_block` uses the main hand at a block top;
//! `break` destroys a block as the server player; `portal` sends the player
//! through the nether portal it stands in; `falling` counts physical falling
//! blocks by dimension, block and gravity.
use super::Game;
use horizon_client::{Result, text::Text};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// A message as `[text, style]` runs with adjacent equal styles merged; the
/// style lists colour, bold, italic, underlined, strikethrough, obfuscated
/// and the suggested command.
pub(super) fn spans(text: &Text) -> Value {
    let mut out: Vec<(String, Value)> = Vec::new();
    for span in &text.spans {
        if span.text.is_empty() {
            continue;
        }
        let s = &span.style;
        let style = json!([
            s.color,
            s.bold == Some(true),
            s.italic == Some(true),
            s.underlined == Some(true),
            s.strikethrough == Some(true),
            s.obfuscated == Some(true),
        ]);
        match out.last_mut() {
            Some((t, last)) if *last == style => t.push_str(&span.text),
            _ => out.push((span.text.clone(), style)),
        }
    }
    Value::Array(out.into_iter().map(|(t, s)| json!([t, s])).collect())
}

impl Game<'_> {
    /// Pump the real client's connection, without predicting movement: the
    /// oracle's player stays where commands put it. Errors cross the server
    /// boundary through Integrated::stop, including script and save failures.
    pub(super) fn wait_command_harness(&mut self) -> Result<()> {
        let server = self.integrated.take().ok_or("command harness: no server")?;
        let client_config = std::env::var_os("HORIZON_HARNESS_CLIENT_CONFIG");
        let mut wrote_config = false;
        while !server.stopped() {
            self.poll_stationary_client()?;
            if !wrote_config
                && let Some(path) = &client_config
                && let Some(remote) = self.remote.as_ref().filter(|r| r.logged_in && r.joined)
            {
                std::fs::write(
                    path,
                    json!({
                        "view_distance": remote.view_distance,
                        "simulation_distance": remote.simulation_distance,
                        "render_setting": self.settings.render_distance,
                        "simulation_setting": self.settings.simulation_distance,
                        "logged_in": remote.logged_in, "joined": remote.joined,
                    })
                    .to_string(),
                )?;
                wrote_config = true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        server.stop()
    }

    /// The owner's authoritative body and level, using the same swaps as a
    /// server command. Always put them back, also when a script operation fails.
    fn harness_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let owner = self
            .serving
            .as_ref()
            .ok_or("command harness: not a server")?
            .owner;
        let mut host = self.lan.take().ok_or("command harness: no hub")?;
        let guest = host.hub.guests.iter().find(|g| g.uuid == owner && g.placed);
        let Some(guest) = guest else {
            self.lan = Some(host);
            return Err("command harness: owner is not connected".into());
        };
        let (conn, dim, back) = (guest.conn, super::Dim::from_id(&guest.dimension), self.dim);
        if let Err(error) = self.enter_level(Some(&mut host), dim) {
            self.lan = Some(host);
            return Err(error);
        }
        let swap = self
            .enter_guest(&mut host, conn, back)
            .expect("the owner is not already acting");
        self.lan = Some(host);
        let result = f(self);
        let mut host = self.lan.take().expect("the operation puts the hub back");
        self.leave_guest(&mut host, swap);
        host.hub.sync_all_effects();
        self.run_level_teleports(&mut host);
        let restored = self.enter_level(Some(&mut host), back);
        self.lan = Some(host);
        restored?;
        result
    }

    /// The connected owner's authoritative block/crafting menu, without
    /// client prediction. Only its own slots are counted (not the inventory).
    fn harness_owner_menu(&mut self) -> Result<(u32, usize, i32)> {
        let owner = self.serving.as_ref().ok_or("menu state: no server")?.owner;
        self.in_owner_level(|g| {
            let host = g.lan.as_ref().ok_or("menu state: no hub")?;
            let guest = host
                .hub
                .guests
                .iter()
                .find(|p| p.uuid == owner && p.placed)
                .ok_or("menu state: owner is not connected")?;
            if let Some(menu) = &guest.menu {
                return Ok((guest.conn, menu.size, menu.window));
            }
            if let Some((window, _, menu, _)) = host
                .play
                .get(&guest.conn)
                .and_then(|p| p.crafting.table.as_ref())
            {
                return Ok((guest.conn, menu.slot_count() - 36, *window));
            }
            if let Some(station) = host.play.get(&guest.conn).and_then(|p| p.station.as_ref()) {
                return Ok((guest.conn, station.menu.slot_count() - 36, station.window));
            }
            Ok((guest.conn, 0, 0))
        })
    }

    /// The player's portal process finished in the nether portal it stands
    /// in (the oracle's player is not ticked, so neither is the wait).
    fn harness_portal(&mut self) -> Result<()> {
        let (conn, plan) = self.harness_owner(|g| {
            let p = g
                .player
                .as_ref()
                .ok_or("command harness: portal without a player")?;
            let entry = horizon_core::position::BlockPos::containing(p.position);
            let block = g.world.block(entry.x, entry.y, entry.z);
            if &*g.res.facts().get(block).name != "minecraft:nether_portal" {
                return Err("command harness: the player is not in a nether portal".into());
            }
            let plan = g.portal_plan(
                entry,
                p.position,
                p.dimensions.width,
                p.dimensions.height,
                true,
            );
            Ok((
                g.lan_acting.ok_or("command harness: no acting owner")?,
                plan,
            ))
        })?;
        // Start travel after restoring the owner's body: change_level updates
        // its hub record, which leave_guest would otherwise overwrite.
        self.in_owner_level(|g| {
            let mut host = g.lan.take().ok_or("command harness: no hub")?;
            g.start_server_trip(&mut host, conn, plan.dim, Some(plan), false);
            g.lan = Some(host);
            Ok(())
        })
    }
    /// Load terrain until the player stands in lit chunks.
    fn harness_settle(&mut self, deadline: Instant, required: Option<&Value>) -> Result<()> {
        loop {
            self.update_world();
            // The oracle's settle takes its tickets at once (getChunk runs the
            // DistanceManager updates); a tick would refresh these too, but a
            // settle runs none, and `/execute if loaded` reads them.
            self.update_chunk_tickets();
            if let Some(p) = &self.player {
                let (cx, cz) = (
                    (p.position.x.floor() as i32) >> 4,
                    (p.position.z.floor() as i32) >> 4,
                );
                // Finish requested terrain before inspecting it, so worker
                // timing cannot change which command positions are loaded.
                if (std::env::var_os("HORIZON_HARNESS_TERRAIN_ONLY").is_some()
                    || !self.streaming.busy())
                    && required.and_then(Value::as_array).map_or_else(
                        || {
                            (-1..=1).all(|dz| {
                                (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz))
                            })
                        },
                        |chunks| {
                            chunks.iter().all(|chunk| {
                                chunk[0]
                                    .as_i64()
                                    .zip(chunk[1].as_i64())
                                    .is_some_and(|(x, z)| self.streaming.is_lit(x as i32, z as i32))
                            })
                        },
                    )
                {
                    // The oracle's settle also waits for the entities saved
                    // in those chunks (areEntitiesActuallyLoadedAndTicking):
                    // after a dimension change they come back from the save.
                    self.spawn_loaded_entities();
                    self.world.entities_pending.clear();
                    return Ok(());
                }
            }
            if std::env::var_os("HORIZON_HARNESS_DEBUG").is_some() {
                eprintln!(
                    "settle: player {:?} center {:?} chunks {} pending {}",
                    self.player.as_ref().map(|p| p.position),
                    self.streaming.center,
                    self.world.chunks.len(),
                    self.streaming.pending()
                );
            }
            if Instant::now() > deadline {
                return Err("command harness: terrain did not load".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    pub(super) fn run_command_harness(&mut self, script: &Path, output: &Path) -> Result<()> {
        let deadline = Instant::now()
            + Duration::from_secs(
                std::env::var("HORIZON_HARNESS_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(600),
            );
        // Join/configuration uses the normal hub, but never advances the world.
        while !self
            .lan
            .as_ref()
            .is_some_and(|h| h.hub.guests.iter().any(|g| g.placed && g.loaded))
        {
            self.tick_server(true)?;
            if Instant::now() > deadline {
                return Err("command harness: owner did not connect".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.harness_owner(|g| g.harness_settle(deadline, None))?;
        if let Some(path) = std::env::var_os("HORIZON_FISHING_TICK_REFERENCE") {
            let rows = self.harness_owner(|g| Ok(g.fishing_tick_reference()))?;
            std::fs::write(path, serde_json::to_vec(&rows)?)?;
        }
        if let Some(path) = std::env::var_os("HORIZON_FISHING_BOBBING_REFERENCE") {
            let rows = self.harness_owner(|g| Ok(g.fishing_bobbing_reference()))?;
            std::fs::write(path, serde_json::to_vec(&rows)?)?;
        }
        if let Some(path) = std::env::var_os("HORIZON_FISHING_FALLBACK_REFERENCE") {
            let rows = self.harness_owner(|g| Ok(g.fishing_fallback_reference()))?;
            std::fs::write(path, serde_json::to_vec(&rows)?)?;
        }
        let mut out = String::new();
        for line in std::fs::read_to_string(script)?.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let op: Value = serde_json::from_str(line)?;
            match op["op"].as_str().unwrap_or("") {
                "cmd" => {
                    let command = op["cmd"].as_str().unwrap_or("");
                    self.chat_capture = Some(Vec::new());
                    self.cmds.results = Some(Vec::new());
                    self.harness_owner(|g| {
                        let previous = g.command_source;
                        if op["source"].as_str() == Some("console") {
                            // Actual server source; permission-only `as_console`
                            // otherwise keeps the connected owner's entity.
                            g.command_source = Some(super::commands::execute::Source {
                                silent: false,
                                ..g.server_source()
                            });
                        }
                        g.as_console(|g| {
                            g.run_command(command.strip_prefix('/').unwrap_or(command))
                        });
                        g.command_source = previous;
                        Ok(())
                    })?;
                    let messages = self.chat_capture.take().unwrap_or_default();
                    let results = self.cmds.results.take().unwrap_or_default();
                    if op["record"].as_bool() != Some(false) || op["trace"].as_bool() == Some(true)
                    {
                        let record = json!({
                            "cmd": command,
                            "messages": messages.iter().map(spans).collect::<Vec<_>>(),
                            "results": results.iter().map(|(s, v)| json!([s, v])).collect::<Vec<_>>(),
                        });
                        if op["trace"].as_bool() == Some(true) {
                            eprintln!("command trace: {record}");
                        }
                        if op["record"].as_bool() != Some(false) {
                            out.push_str(&record.to_string());
                            out.push('\n');
                        }
                    }
                }
                "projectile_saved_type" => {
                    let uuid = u128::from_str_radix(
                        &op["uuid"]
                            .as_str()
                            .ok_or("projectile type: missing UUID")?
                            .replace('-', ""),
                        16,
                    )?;
                    let state = self.harness_owner(|g| {
                        let Some(super::commands::Target::Projectile(id)) = g.target_by_uuid(uuid) else {
                            return Err("projectile type: missing loaded projectile".into());
                        };
                        let actual = *g.persist.uuids.get(&id).ok_or("projectile type: missing actual UUID")?;
                        if horizon_client::lan::playerdata::ints_uuid(actual) != uuid {
                            return Err("projectile type: UUID mismatch".into());
                        }
                        let p = g.creatures.projectiles.iter().find(|p| p.id == id)
                            .ok_or("projectile type: missing model")?;
                        // Read the live save adapter's type, not the generic selector alias.
                        // No Duration value is supplied by this admission observation.
                        let saved = horizon_client::entity_nbt::arrow_to_nbt(p, actual, None, None, None)
                            .ok_or("projectile type: not an actual arrow")?;
                        Ok(json!({"uuid":actual,"kind":horizon_client::entity_nbt::kind(&saved)
                            .ok_or("projectile type: missing saved type")?,
                            "pos":[p.position.x,p.position.y,p.position.z],"no_gravity":p.no_gravity}))
                    })?;
                    out.push_str(&json!({"cmd":format!("#projectile_saved_type {}",op["label"].as_str().unwrap_or("")),
                        "messages":[],"results":[],"state":state}).to_string());
                    out.push('\n');
                }
                "raid_state" => {
                    let state = self.harness_owner(|g| g.raid_fixture_state(&op))?;
                    let row = json!({"cmd":format!("#raid_state {} {}",op["label"].as_str().unwrap_or(""),op["id"]),
                        "messages":[],"results":[],"state":state});
                    out.push_str(&row.to_string());
                    out.push('\n');
                }
                "mob_anger" => {
                    // Metadata-state equality through the production writer, not packet delivery.
                    let uuid = u128::from_str_radix(
                        &op["uuid"]
                            .as_str()
                            .ok_or("anger state: missing UUID")?
                            .replace('-', ""),
                        16,
                    )?;
                    let index = op["index"].as_u64().ok_or("anger state: missing index")? as u8;
                    let state = self.harness_owner(|g| {
                        let Some(super::commands::Target::Mob(id)) = g.target_by_uuid(uuid) else {
                            return Err("anger state: UUID is not a loaded mob".into());
                        };
                        let actual = g
                            .persist
                            .uuids
                            .get(&id)
                            .copied()
                            .ok_or("anger state: no saved UUID")?;
                        if horizon_client::lan::playerdata::ints_uuid(actual) != uuid {
                            return Err("anger state: UUID mismatch".into());
                        }
                        let m = g
                            .mobs
                            .iter()
                            .find(|m| m.id == id && m.alive() && !m.removed)
                            .ok_or("anger state: missing live actor")?;
                        if Some(m.kind.id.as_str()) != op["kind"].as_str() {
                            return Err("anger state: wrong actor type".into());
                        }
                        let hub = &g.lan.as_ref().ok_or("anger state: no connected hub")?.hub;
                        let (mut meta, mut defaults) = (Vec::new(), Vec::new());
                        super::lan::mob_meta(
                            m,
                            hub,
                            None,
                            g.world.game_time,
                            &mut meta,
                            &mut defaults,
                        );
                        let entry = meta
                            .iter()
                            .find(|e| e.0 == index)
                            .or_else(|| defaults.iter().find(|e| e.0 == index))
                            .ok_or("anger state: metadata entry/default absent")?;
                        if entry.1 != horizon_protocol::entity_data::serializer::LONG {
                            return Err("anger state: metadata serializer is not LONG".into());
                        }
                        let horizon_protocol::entity_data::Value::Long(value) = &entry.2 else {
                            return Err("anger state: metadata value is not long".into());
                        };
                        Ok(json!({"uuid":actual,"kind":m.kind.id,"index":index,
                            "serializer":entry.1,"value":value,"game_time":g.world.game_time}))
                    })?;
                    out.push_str(
                        &json!({"cmd":format!("#mob_anger {}",op["label"].as_str().unwrap_or("")),
                        "messages":[],"results":[],"state":state})
                        .to_string(),
                    );
                    out.push('\n');
                }
                "server_distances" => {
                    let state = self.harness_owner(|g| {
                        let hub = &g.lan.as_ref().ok_or("no connected hub")?.hub;
                        Ok([hub.config.view_distance, hub.config.simulation_distance])
                    })?;
                    out.push_str(
                        &json!({"cmd": "#server_distances", "messages": [],
                        "results": state.map(|n| json!([true, n]))})
                        .to_string(),
                    );
                    out.push('\n');
                }
                "chunk_ready" => {
                    let [x, z] =
                        std::array::from_fn(|i| op["chunk"][i].as_i64().unwrap_or(0) as i32);
                    let state = self.harness_owner(|g| {
                        let resident = g.world.chunks.contains_key(&(x, z));
                        Ok([
                            resident,
                            g.streaming.is_lit(x, z),
                            resident
                                && !g
                                    .world
                                    .loaded_entities
                                    .iter()
                                    .any(|(key, _)| *key == (x, z)),
                        ])
                    })?;
                    out.push_str(
                        &json!({"cmd": format!("#chunk_ready {x} {z}"), "messages": [],
                        "results": state.map(|ready| json!([true, i32::from(ready)]))})
                        .to_string(),
                    );
                    out.push('\n');
                }
                "wait_chunks" => {
                    let targets: Vec<(i32, i32)> = op["chunks"]
                        .as_array()
                        .ok_or("missing chunks")?
                        .iter()
                        .map(|p| {
                            (
                                p[0].as_i64().unwrap_or(0) as i32,
                                p[1].as_i64().unwrap_or(0) as i32,
                            )
                        })
                        .collect();
                    self.harness_owner(|g| {
                        loop {
                            g.update_world();
                            g.spawn_loaded_entities();
                            if targets.iter().all(|&(x, z)| {
                                g.world.chunks.contains_key(&(x, z))
                                    && g.streaming.is_lit(x, z)
                                    && !g
                                        .world
                                        .loaded_entities
                                        .iter()
                                        .any(|(key, _)| *key == (x, z))
                            }) {
                                // The oracle waits for `areEntitiesLoaded` too.
                                g.world.entities_pending.clear();
                                break;
                            }
                            if Instant::now() > deadline {
                                return Err("initial target readiness timed out".into());
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Ok(())
                    })?;
                }
                "weather_state" => {
                    // Saved state guards, separate from effective-weather predicates.
                    let state = self.harness_owner(|g| {
                        let w = g.world.weather;
                        Ok([
                            i32::from(w.raining),
                            i32::from(w.thundering),
                            w.clear_time,
                            w.rain_time,
                            w.thunder_time,
                        ])
                    })?;
                    let record = json!({
                        "cmd": format!("#weather_state {}", op["label"].as_str().unwrap_or("")),
                        "messages": [],
                        "results": state.iter().map(|value| json!([true, value])).collect::<Vec<_>>(),
                    });
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "mobs" => {
                    // Observe authoritative AI state without a control-socket race.
                    let record = self.harness_owner(|g| Ok(g.control_mobs(128.0)))?;
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "player_loop" => {
                    let ticks = op["n"].as_u64().unwrap_or(1);
                    let count = |g: &mut Self| {
                        Ok(g.stats
                            .values
                            .get("minecraft:custom")
                            .and_then(|m| m.get("minecraft:play_time"))
                            .copied()
                            .unwrap_or(0))
                    };
                    let before = self.harness_owner(count)?;
                    self.chat_capture = Some(Vec::new());
                    for _ in 0..ticks {
                        self.tick_server(false)?;
                    }
                    let messages = self.chat_capture.take().unwrap_or_default();
                    let delta = self.harness_owner(count)? - before;
                    if delta != ticks as i32 {
                        return Err(format!(
                            "player loop did not advance play_time: {delta}/{ticks}"
                        )
                        .into());
                    }
                    let record = json!({"cmd": format!("#player_loop {}",op["label"].as_str().unwrap_or("")), "messages": messages.iter().map(spans).collect::<Vec<_>>(), "results": [[true,delta]]});
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "player_state" => {
                    let state = self.harness_owner(|g| {
                        let p = g.player.as_ref().ok_or("player state: no actor")?;
                        let mut loaded = Vec::new();
                        let mut done = std::collections::BTreeMap::new();
                        for a in g.adv.tree.list.iter().filter(|a| a.id.starts_with("h311:")) {
                            loaded.push(a.id.clone()); done.insert(a.id.clone(),g.adv.progress.done(a));
                        }
                        loaded.sort();
                        Ok(json!({"health":p.health.health(),"poison_ticks":g.potions.effects.list.iter().find(|e|e.id=="poison").map_or(0,|e|e.duration),"loaded":loaded,"done":done}))
                    })?;
                    let record = json!({"cmd":format!("#player_state {}",op["label"].as_str().unwrap_or("")),"messages":[],"results":[],"state":state});
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "tick" => {
                    self.chat_capture = Some(Vec::new());
                    let paced_start = Instant::now();
                    for i in 0..op["n"].as_u64().unwrap_or(1) {
                        // The oracle's profile resolver answers at once; the
                        // game's asks the services (none here: they refuse at
                        // once) on a thread, which a tick can outrun.
                        let asked = Instant::now();
                        while self.profile_fetches_pending()
                            && asked.elapsed() < Duration::from_secs(10)
                        {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        self.tick_server(false)?;
                        if let Some(ms) = op["pace_ms"].as_u64() {
                            std::thread::sleep(
                                Duration::from_millis(ms * (i + 1))
                                    .saturating_sub(paced_start.elapsed()),
                            );
                        }
                    }
                    if op["pace_ms"].is_number() {
                        eprintln!(
                            "harness paced wait: {} ticks, {} ms",
                            op["n"],
                            paced_start.elapsed().as_millis()
                        );
                    }
                    let messages = self.chat_capture.take().unwrap_or_default();
                    if op["record"].as_bool() == Some(true) {
                        let record = json!({
                            "cmd": "#tick",
                            "messages": messages.iter().map(spans).collect::<Vec<_>>(),
                            "results": [],
                        });
                        out.push_str(&record.to_string());
                        out.push('\n');
                    }
                }
                "settle" => {
                    // Finish portal arrivals using connection-only passes too.
                    while !self.server_trips.is_empty() {
                        self.tick_server(true)?;
                        if Instant::now() > deadline {
                            return Err("command harness: portal did not arrive".into());
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    self.harness_owner(|g| g.harness_settle(deadline, op.get("required_chunks")))?;
                }
                "use_item" if op.get("sequence").is_some() => {
                    // Exercise the connected owner's normal server packet route.
                    let sequence = op["sequence"].as_i64().unwrap_or(0) as i32;
                    let yaw = op["yaw"].as_f64().unwrap_or(0.0) as f32;
                    let pitch = op["pitch"].as_f64().unwrap_or(0.0) as f32;
                    let owner = self
                        .serving
                        .as_ref()
                        .ok_or("command harness: no server")?
                        .owner;
                    self.in_owner_level(|g| -> Result<()> {
                        let conn = g
                            .lan
                            .as_ref()
                            .and_then(|h| {
                                h.hub
                                    .guests
                                    .iter()
                                    .find(|guest| guest.uuid == owner && guest.placed)
                                    .map(|guest| guest.conn)
                            })
                            .ok_or("command harness: owner is not connected")?;
                        let mut host = g.lan.take().ok_or("command harness: no hub")?;
                        g.lan_guest_packet(
                            &mut host,
                            conn,
                            horizon_protocol::packets::play::Serverbound::UseItem(
                                horizon_protocol::packets::play::UseItem {
                                    hand: 0,
                                    yaw,
                                    pitch,
                                    sequence,
                                },
                            ),
                        );
                        let ack = host
                            .hub
                            .guest(conn)
                            .is_some_and(|g| g.ack_up_to.get() >= sequence);
                        g.lan = Some(host);
                        if !ack {
                            return Err("command harness: UseItem was not acknowledged".into());
                        }
                        Ok(())
                    })?;
                }
                // Java produced the fixture file before this world was opened; map_state loads it.
                "crafter_max_map_fixture" => {}
                "boat_interact" => {
                    let uuid = u128::from_str_radix(
                        &op["uuid"]
                            .as_str()
                            .ok_or("boat click: missing UUID")?
                            .replace('-', ""),
                        16,
                    )?;
                    self.in_owner_level(|g| -> Result<()> {
                        let id = g
                            .boat_by_uuid(horizon_client::lan::playerdata::uuid_ints(uuid))
                            .ok_or("boat click: missing live boat")?;
                        let owner = g.serving.as_ref().ok_or("boat click: no server")?.owner;
                        let conn = g
                            .lan
                            .as_ref()
                            .and_then(|h| {
                                h.hub
                                    .guests
                                    .iter()
                                    .find(|p| p.uuid == owner && p.placed)
                                    .map(|p| p.conn)
                            })
                            .ok_or("boat click: owner not connected")?;
                        let mut host = g.lan.take().ok_or("boat click: no hub")?;
                        g.lan_guest_packet(
                            &mut host,
                            conn,
                            horizon_protocol::packets::play::Serverbound::Interact(
                                horizon_protocol::packets::play::Interact {
                                    id: id as i32,
                                    hand: 0,
                                    location: [0.0; 3],
                                    secondary: false,
                                },
                            ),
                        );
                        g.lan = Some(host);
                        Ok(())
                    })?;
                }
                "mob_interact" => {
                    // The owner's real Interact packet on a live mob, by UUID.
                    let uuid = u128::from_str_radix(
                        &op["uuid"]
                            .as_str()
                            .ok_or("mob click: missing UUID")?
                            .replace('-', ""),
                        16,
                    )?;
                    self.in_owner_level(|g| -> Result<()> {
                        let id = g
                            .mob_by_uuid(horizon_client::lan::playerdata::uuid_ints(uuid))
                            .filter(|id| g.mobs.iter().any(|m| m.id == *id && !m.removed))
                            .ok_or("mob click: missing live mob")?;
                        let owner = g.serving.as_ref().ok_or("mob click: no server")?.owner;
                        let conn = g
                            .lan
                            .as_ref()
                            .and_then(|h| {
                                h.hub
                                    .guests
                                    .iter()
                                    .find(|p| p.uuid == owner && p.placed)
                                    .map(|p| p.conn)
                            })
                            .ok_or("mob click: owner not connected")?;
                        let mut host = g.lan.take().ok_or("mob click: no hub")?;
                        g.lan_guest_packet(
                            &mut host,
                            conn,
                            horizon_protocol::packets::play::Serverbound::Interact(
                                horizon_protocol::packets::play::Interact {
                                    id: id as i32,
                                    hand: 0,
                                    location: [0.0; 3],
                                    secondary: false,
                                },
                            ),
                        );
                        g.lan = Some(host);
                        Ok(())
                    })?;
                }
                "use_item" => {
                    let owner = self
                        .serving
                        .as_ref()
                        .ok_or("command harness: no server")?
                        .owner;
                    self.in_owner_level(|g| -> Result<()> {
                        let conn = g
                            .lan
                            .as_ref()
                            .and_then(|h| {
                                h.hub
                                    .guests
                                    .iter()
                                    .find(|guest| guest.uuid == owner && guest.placed)
                                    .map(|guest| guest.conn)
                            })
                            .ok_or("command harness: owner is not connected")?;
                        let mut host = g.lan.take().ok_or("command harness: no hub")?;
                        g.lan_guest_packet(
                            &mut host,
                            conn,
                            horizon_protocol::packets::play::Serverbound::UseItem(
                                horizon_protocol::packets::play::UseItem {
                                    hand: 0,
                                    sequence: 0,
                                    yaw: 0.0,
                                    pitch: 0.0,
                                },
                            ),
                        );
                        g.lan = Some(host);
                        Ok(())
                    })?;
                }
                "frame_map_state" => {
                    // Read saved-map association and live decorations, not network delivery.
                    let uuid = u128::from_str_radix(
                        &op["uuid"]
                            .as_str()
                            .ok_or("frame map: missing UUID")?
                            .replace('-', ""),
                        16,
                    )?;
                    let map_id = op["id"].as_i64().ok_or("frame map: missing map id")? as i32;
                    let state = self.harness_owner(|g| {
                        let Some(super::commands::Target::Frame(id)) = g.target_by_uuid(uuid) else {
                            return Err("frame map: UUID is not a live frame".into());
                        };
                        let actual = *g.persist.uuids.get(&id).ok_or("frame map: no UUID")?;
                        let frame = g.item_frames.iter().find(|f| f.id == id)
                            .ok_or("frame map: missing frame")?.clone();
                        if horizon_client::lan::playerdata::ints_uuid(actual) != uuid {
                            return Err("frame map: UUID mismatch".into());
                        }
                        let map = g.map(map_id).ok_or("frame map: no backed map")?;
                        let saved = map.to_nbt();
                        let data = saved.get("data").ok_or("frame map: no saved data compound")?;
                        let records = match data.get("frames") {
                            None => &[][..], // the CODEC omits an empty list
                            Some(horizon_world::nbt::Tag::List(list)) => list.as_slice(),
                            _ => return Err("frame map: frames is not a list".into()),
                        };
                        let record = records.iter().find(|r|
                            horizon_client::save::number(r.get("entity_id")) == Some(i64::from(id)));
                        let pos = record.map(|r| match r.get("pos") {
                            Some(horizon_world::nbt::Tag::IntArray(pos)) => Ok(pos.clone()),
                            _ => Err("frame map: invalid saved position"),
                        }).transpose()?;
                        let rotation = record.map(|r|
                            horizon_client::save::number(r.get("rotation"))
                                .ok_or("frame map: missing rotation")).transpose()?;
                        Ok(json!({"id":map_id,"uuid":actual,"kind":frame.kind(),"pos":frame.pos,
                            "center":map.center,"scale":map.scale,"dimension":map.dimension,
                            "saved":record.is_some(),"saved_pos":pos,"saved_rotation":rotation,
                            "decoration":map.decorations.iter().any(|d|d.key==super::maps::Key::Frame(id as i32)),
                            "frame_records":records.len(),
                            "frame_decorations":map.decorations.iter().filter(|d|d.sprite=="map/decorations/frame").count()}))
                    })?;
                    out.push_str(
                        &json!({"cmd":format!("#frame_map {}",op["label"].as_str().unwrap_or("")),
                        "messages":[],"results":[],"state":state})
                        .to_string(),
                    );
                    out.push('\n');
                }
                "map_carried" => {
                    self.harness_owner(|g| {
                        g.tick_maps();
                        Ok(())
                    })?;
                }
                "craft_menu_click" | "craft_menu_state" | "craft_menu_close" => {
                    let owner = self.serving.as_ref().ok_or("no server")?.owner;
                    let state = self.in_owner_level(|g| -> Result<Value> {
                        let conn = g.lan.as_ref().and_then(|h| h.hub.guests.iter()
                            .find(|p| p.uuid == owner && p.placed).map(|p| p.conn))
                            .ok_or("craft fixture: no connected owner")?;
                        let mut host = g.lan.take().ok_or("no hub")?;
                        let result = (|| -> Result<Value> {
                            let (window, revision, _, _) = host.play.get(&conn)
                                .and_then(|p| p.crafting.table.as_ref())
                                .ok_or("craft fixture: crafting menu not open")?;
                            let (window, revision) = (*window, *revision);
                            if op["op"] == "craft_menu_click" {
                                // Revision last sent by the real table-content sync, never a fixed 0.
                                eprintln!("craft fixture click: window={window}, revision={revision}");
                                g.lan_guest_packet(&mut host, conn,
                                    horizon_protocol::packets::play::Serverbound::ContainerClick(
                                        horizon_protocol::packets::play::ContainerClick {
                                            container: window, state: revision,
                                            slot: op["slot"].as_i64().ok_or("click slot")? as i16,
                                            button: op["button"].as_i64().unwrap_or(0) as i8,
                                            input: op["input"].as_i64().unwrap_or(0) as i32,
                                            changed: Vec::new(), carried: None,
                                        }));
                                return Ok(Value::Null);
                            }
                            if op["op"] == "craft_menu_close" {
                                g.lan_guest_packet(&mut host, conn,
                                    horizon_protocol::packets::play::Serverbound::ContainerClose(
                                        horizon_protocol::packets::play::ContainerClose { container: window }));
                                return Ok(Value::Null);
                            }
                            let (_, _, menu, _) = host.play.get(&conn)
                                .and_then(|p| p.crafting.table.as_ref()).unwrap();
                            if menu.kind() != horizon_client::menu::Kind::Crafting {
                                return Err("craft fixture: wrong menu kind".into());
                            }
                            let guest = host.hub.guest(conn).ok_or("no guest")?;
                            let stack = |s: Option<&horizon_client::items::ItemStack>| {
                                s.map(|s| json!([&*s.item, s.count,
                                    s.data().and_then(|d| d.map_id),
                                    s.data().and_then(|d| d.map_post_processing)]))
                            };
                            // Table's ten slots plus the owner's live inventory in menu order.
                            let slots: Vec<_> = (0..10).map(|i| stack(menu.slot(i)))
                                .chain((9..36).chain(0..9).map(|i| stack(guest.inventory[i].as_ref())))
                                .collect();
                            Ok(json!({"window": window, "kind": "minecraft:crafting",
                                "slots": slots, "carried": stack(guest.carried.as_ref()).unwrap_or_else(|| json!([]))}))
                        })();
                        g.lan = Some(host);
                        result
                    })?;
                    if op["op"] == "craft_menu_state" {
                        let record = json!({"cmd": format!("#craft_menu {}", op["label"].as_str().unwrap_or("")),
                            "state": state, "messages": [], "results": []});
                        out.push_str(&record.to_string());
                        out.push('\n');
                    }
                }
                "map_files" => {
                    let state = self.harness_owner(|g| {
                        use horizon_world::nbt::Tag;
                        g.save_maps();
                        let directory = g.world_root.join("data/minecraft/maps");
                        let maps: Vec<_> = op["ids"].as_array().ok_or("map IDs")?.iter().map(|id| {
                            let path = directory.join(format!("{}.dat", id.as_i64().unwrap()));
                            horizon_client::save::read_dat(&path).map(|root| {
                                let data = root.get("data").unwrap_or(&root);
                                let number = |key| match data.get(key) {
                                    Some(Tag::Int(v)) => *v, Some(Tag::Byte(v)) => i32::from(*v), _ => 0,
                                };
                                let colors = match data.get("colors") {
                                    Some(Tag::ByteArray(bytes)) => Some(bytes.len()), _ => None,
                                };
                                json!({"scale": number("scale"), "locked": number("locked") != 0,
                                    "center": [number("xCenter"), number("zCenter")],
                                    "dimension": horizon_client::save::text(data.get("dimension")),
                                    "byte_array_length": colors})
                            })
                        }).collect();
                        let last = horizon_client::save::read_dat(&directory.join("last_id.dat"))
                            .and_then(|tag| match tag.get("data").and_then(|d| d.get("map")) {
                                Some(Tag::Int(v)) => Some(*v), _ => None,
                            });
                        Ok(json!({"maps": maps, "last_id": last}))
                    })?;
                    out.push_str(
                        &json!({"cmd": "#map_files", "state": state,
                        "messages": [], "results": []})
                        .to_string(),
                    );
                    out.push('\n');
                }
                "map_state" => {
                    let id = op["id"].as_i64().ok_or("command harness: map id")? as i32;
                    let value = self.harness_owner(|g| {
                        Ok(match g.map(id) {
                            Some(m) => {
                                let mut value = format!(
                                    "scale={},locked={},center={},{}",
                                    m.scale, m.locked, m.center[0], m.center[1]
                                );
                                if op["red_x"].as_bool().unwrap_or(false) {
                                    let count = m
                                        .decorations
                                        .iter()
                                        .filter(|d| d.sprite == "map/decorations/red_x")
                                        .count();
                                    value.push_str(&format!(",red_x={count}"));
                                }
                                value
                            }
                            None => "missing".to_owned(),
                        })
                    })?;
                    let record = json!({"cmd": format!("#map_state {id}"),
                        "messages": [spans(&Text::literal(value))], "results": []});
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "light" => {
                    let pos = std::array::from_fn::<_, 3, _>(|i| {
                        op["pos"][i].as_i64().unwrap_or(0) as i32
                    });
                    let levels = self.harness_owner(|g| {
                        use horizon_client::mobs::spawn_rules::SpawnLevel;
                        let level = g.natural_level();
                        Ok([
                            level.block_light(pos[0], pos[1], pos[2]),
                            level.sky_light(pos[0], pos[1], pos[2]),
                        ])
                    })?;
                    let record = json!({"cmd": format!("#light {}", op["pos"]),
                        "messages": [], "results": levels.map(|v| (true, v))});
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "merchant_saved_stats" => {
                    let (state, raw) = self.harness_owner(|g| {
                        g.load_stats();
                        if !g.stats.loaded { return Err("merchant stats not loaded".into()); }
                        g.save_stats();
                        let uuid = g.level.player_uuid.ok_or("merchant stats UUID")?;
                        let path = super::stats::player_json(&g.world_root, "stats", uuid);
                        let raw = std::fs::read_to_string(path)?;
                        let saved: Value = serde_json::from_str(&raw)?;
                        let value = |ty: &str, key: &str| saved["stats"][ty][key].as_i64().unwrap_or(0);
                        Ok((json!({"uuid":uuid,"books":value("minecraft:crafted","minecraft:bookshelf"),
                            "glass":value("minecraft:crafted","minecraft:glass"),
                            "trades":value("minecraft:custom","minecraft:traded_with_villager")}), raw))
                    })?;
                    let label = op["label"].as_str().unwrap_or("");
                    std::fs::write(
                        output.with_file_name(format!("native-merchant-stats-{label}.json")),
                        raw,
                    )?;
                    out.push_str(
                        &json!({"cmd":format!("#merchant_saved_stats {label}"),
                        "messages":[],"results":[],"state":state})
                        .to_string(),
                    );
                    out.push('\n');
                }
                "merchant_action" | "merchant_state" => {
                    let observe = op["op"] == "merchant_state";
                    let state = self.in_owner_level(|g| -> Result<Value> {
                        let mut host = g.lan.take().ok_or("merchant oracle: no hub")?;
                        let result = (|| -> Result<Value> {
                            let owner = g
                                .serving
                                .as_ref()
                                .ok_or("merchant oracle: no server")?
                                .owner;
                            let conn = host
                                .hub
                                .guests
                                .iter()
                                .find(|p| p.uuid == owner && p.placed)
                                .ok_or("merchant oracle: owner not connected")?
                                .conn;
                            if observe {
                                return Ok(g
                                    .harness_merchant(&host, conn)
                                    .ok_or("merchant oracle: no live menu")?
                                    .2);
                            }
                            use horizon_protocol::packets::play::{self, Serverbound as S};
                            let packet = match op["action"].as_str().unwrap_or("") {
                                "open" => {
                                    let uuid = u128::from_str_radix(
                                        op["uuid"]
                                            .as_str()
                                            .ok_or("merchant oracle: UUID")?
                                            .replace('-', "")
                                            .as_str(),
                                        16,
                                    )?;
                                    let Some(super::commands::Target::Mob(id)) =
                                        g.target_by_uuid(uuid)
                                    else {
                                        return Err("merchant oracle: missing live mob".into());
                                    };
                                    S::Interact(play::Interact {
                                        id: id as i32,
                                        hand: 0,
                                        location: [0.0; 3],
                                        secondary: false,
                                    })
                                }
                                "select" => S::SelectTrade(play::SelectTrade {
                                    item: op["index"].as_i64().unwrap_or(0) as i32,
                                }),
                                "click" => {
                                    let (container, state, _) = g
                                        .harness_merchant(&host, conn)
                                        .ok_or("merchant oracle: no menu")?;
                                    S::ContainerClick(play::ContainerClick {
                                        container,
                                        state,
                                        slot: op["slot"].as_i64().ok_or("merchant oracle: slot")?
                                            as i16,
                                        button: op["button"].as_i64().unwrap_or(0) as i8,
                                        input: op["input"].as_i64().unwrap_or(0) as i32,
                                        changed: Vec::new(),
                                        carried: None,
                                    })
                                }
                                "close" => S::ContainerClose(play::ContainerClose {
                                    container: g
                                        .harness_merchant(&host, conn)
                                        .ok_or("merchant oracle: no menu")?
                                        .0,
                                }),
                                _ => return Err("merchant oracle: unknown action".into()),
                            };
                            eprintln!("merchant action receipt: action={} owner={owner:032x} conn={conn} menu={:?} clock={}",
                                op["action"], g.harness_merchant(&host, conn).map(|(w, r, _)| (w, r)), g.world.game_time);
                            g.lan_guest_packet(&mut host, conn, packet);
                            Ok(Value::Null)
                        })();
                        g.lan = Some(host);
                        result
                    })?;
                    if observe {
                        out.push_str(&json!({"cmd":format!("#merchant_state {}",op["label"].as_str().unwrap_or("")),
                            "messages":[],"results":[],"state":state}).to_string());
                        out.push('\n');
                    }
                }
                "menu_state" => {
                    let (_, own_slots, _) = self.harness_owner_menu()?;
                    let record = json!({"cmd": format!("#menu_state {}", op["label"].as_str().unwrap_or("")),
                        "messages": [], "results": [], "state": {"own_slots": own_slots}});
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "close_menu" => {
                    let (conn, _, window) = self.harness_owner_menu()?;
                    self.in_owner_level(|g| -> Result<()> {
                        let mut host = g.lan.take().ok_or("close menu: no hub")?;
                        g.lan_guest_packet(
                            &mut host,
                            conn,
                            horizon_protocol::packets::play::Serverbound::ContainerClose(
                                horizon_protocol::packets::play::ContainerClose {
                                    container: window,
                                },
                            ),
                        );
                        g.lan = Some(host);
                        Ok(())
                    })?;
                }
                "use_block" => {
                    // Exercise the connected owner's normal server packet route.
                    let pos = std::array::from_fn(|i| op["pos"][i].as_i64().unwrap_or(0) as i32);
                    let sequence = op["sequence"].as_i64().unwrap_or(0) as i32;
                    let face = op["face"].as_i64().unwrap_or(1) as i32;
                    let owner = self
                        .serving
                        .as_ref()
                        .ok_or("command harness: no server")?
                        .owner;
                    self.in_owner_level(|g| -> Result<()> {
                        let conn = g
                            .lan
                            .as_ref()
                            .and_then(|h| {
                                h.hub
                                    .guests
                                    .iter()
                                    .find(|guest| guest.uuid == owner && guest.placed)
                                    .map(|guest| guest.conn)
                            })
                            .ok_or("command harness: owner is not connected")?;
                        let mut host = g.lan.take().ok_or("command harness: no hub")?;
                        g.lan_guest_packet(
                            &mut host,
                            conn,
                            horizon_protocol::packets::play::Serverbound::UseItemOn(
                                horizon_protocol::packets::play::UseItemOn {
                                    hand: 0,
                                    pos,
                                    face,
                                    cursor: [0.5, 1.0, 0.5],
                                    inside: false,
                                    world_border: false,
                                    sequence,
                                },
                            ),
                        );
                        let acknowledged = host
                            .hub
                            .guest(conn)
                            .is_some_and(|p| p.ack_up_to.get() >= sequence);
                        g.lan = Some(host);
                        if !acknowledged {
                            return Err("use block: owner packet was not acknowledged".into());
                        }
                        Ok(())
                    })?;
                }
                "crafter_tick" => {
                    let [x, y, z] =
                        std::array::from_fn(|i| op["pos"][i].as_i64().unwrap_or(0) as i32);
                    self.harness_owner(|g| {
                        let state = g.world.block(x, y, z);
                        let name = &*g.res.facts().get(state).name;
                        if name != "minecraft:crafter" && name != "minecraft:dispenser" {
                            return Err(
                                "command harness: crafter tick without a crafter or dispenser"
                                    .into(),
                            );
                        }
                        g.dispense_from(horizon_core::position::BlockPos::new(x, y, z), state)
                    })?;
                }
                "sync_randoms" => {
                    // docs/parity-workflow.md "Randoms that repeat". The script
                    // runs on the server thread, whose uniquifier this sets.
                    let field = |key: &str| {
                        op[key]
                            .as_i64()
                            .ok_or_else(|| format!("sync_randoms: no {key}"))
                    };
                    let (uniquifier, entity_id, level_seed) = (
                        field("uniquifier")?,
                        field("entity_id")?,
                        field("level_seed")?,
                    );
                    horizon_client::random_source::set_thread_uniquifier(uniquifier)?;
                    self.harness_owner(|g| {
                        g.next_entity = u32::try_from(entity_id)?;
                        g.world.random = horizon_core::random::LegacyRandom::new(level_seed);
                        Ok(())
                    })?;
                }
                "break" => {
                    let pos = std::array::from_fn(|i| op["pos"][i].as_i64().unwrap_or(0) as i32);
                    self.harness_owner(|g| g.break_block(pos))?;
                }
                "portal" => self.harness_portal()?,
                "destroy_progress" => {
                    let pos = std::array::from_fn(|i| op["pos"][i].as_i64().unwrap_or(0) as i32);
                    let bits = self.harness_owner(|g| {
                        if let Some(player) = g.player.as_mut() {
                            player.ground.on_ground = true;
                        }
                        Ok(g.destroy_progress(pos).to_bits())
                    })?;
                    let record = json!({
                        "cmd": "#destroy_progress",
                        "messages": [],
                        "results": [[true, bits]],
                    });
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                "falling" => {
                    // Physical falling blocks are not command-selector targets
                    // yet. Inspect the actual level rather than that unrelated gap.
                    let dimension = op["dimension"].as_str().unwrap_or("minecraft:overworld");
                    let block = op["block"].as_str().unwrap_or("minecraft:sand");
                    let no_gravity = op["no_gravity"].as_bool().unwrap_or(false);
                    let back = self.dim;
                    self.enter_level(None, super::Dim::from_id(dimension))?;
                    let facts = self.res.facts();
                    let matches = |f: &super::FallingBlock| {
                        &*facts.get(f.state).name == block && f.no_gravity == no_gravity
                    };
                    let count = self.falling.iter().filter(|f| matches(f)).count();
                    if op["discard"].as_bool() == Some(true) {
                        self.falling.retain(|f| !matches(f));
                    }
                    self.enter_level(None, back)?;
                    let record = json!({
                        "cmd": format!("#falling {dimension} {block} {no_gravity}"),
                        "messages": [],
                        "results": [[true, count]],
                    });
                    out.push_str(&record.to_string());
                    out.push('\n');
                }
                other => return Err(format!("command harness: unknown op {other}").into()),
            }
        }
        std::fs::write(output, out)?;
        Ok(())
    }
}
