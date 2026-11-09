//! `HORIZON_TICK_SCRIPT=PATH` (tools/test_wire_stream_parity.py): a server's client plays a tick-indexed
//! script, the one the vanilla client plays in `VanillaTickScript.java`, so the two clients' serverbound
//! streams can be compared tick by tick. The harness writes the scenario to PATH (JSON `{"name", "ticks",
//! "steps": {"TICK": [LINE...]}}`, LINE a horizon-ctl line: `key w down`, `mouse left click`, `look 90 0`,
//! `click-slot 3`, `close`), then creates PATH with the extension `go`. From the next client tick the script
//! plays: its first tick sends `/wire-mark`, which marks it in the capture, then each tick's lines act
//! before the tick runs, as a player's input in the frame before it; when its ticks have run, the client
//! writes the scenario's name to PATH with the extension `done`.
use super::Game;
use super::event_loop::EventLoopCtl;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default)]
pub(super) struct TickScript {
    /// PATH, when the variable is set.
    path: Option<PathBuf>,
    running: Option<Running>,
}

struct Running {
    name: String,
    ticks: u32,
    steps: BTreeMap<u32, Vec<String>>,
    tick: u32,
}

impl TickScript {
    pub(super) fn from_env() -> Self {
        Self {
            path: std::env::var_os("HORIZON_TICK_SCRIPT").map(PathBuf::from),
            running: None,
        }
    }
    /// Whether a scenario is playing.
    pub(super) fn playing(&self) -> bool {
        self.running.is_some()
    }
    /// Whether the variable is set (the frame asks before every tick).
    pub(super) fn armed(&self) -> bool {
        self.path.is_some()
    }
}

/// The scenario at `text`: its name, its length in ticks and each tick's lines.
fn parse(text: &str) -> Option<Running> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut steps = BTreeMap::new();
    for (tick, lines) in json.get("steps")?.as_object()? {
        let lines = lines
            .as_array()?
            .iter()
            .filter_map(|l| l.as_str().map(str::to_owned))
            .collect();
        steps.insert(tick.parse().ok()?, lines);
    }
    Some(Running {
        name: json.get("name")?.as_str()?.to_owned(),
        ticks: u32::try_from(json.get("ticks")?.as_u64()?).ok()?,
        steps,
        tick: 0,
    })
}

impl Game<'_> {
    /// Before a client tick: start the scenario the harness handed over, or play this tick's lines.
    pub(super) fn tick_script_step(&mut self, event_loop: &dyn EventLoopCtl) {
        let Some(path) = self.tick_script.path.clone() else {
            return;
        };
        if self.tick_script.running.is_none() {
            let go = path.with_extension("go");
            if self.remote.is_none() || !go.exists() {
                return;
            }
            let _ = std::fs::remove_file(&go);
            let Some(script) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t)) else {
                log::warn!("Horizon: tick script {} is not a scenario", path.display());
                return;
            };
            self.tick_script.running = Some(script);
            self.send_chat("/wire-mark");
        }
        let Some(running) = self.tick_script.running.as_mut() else {
            return;
        };
        let tick = running.tick;
        running.tick += 1;
        let lines = running.steps.remove(&tick).unwrap_or_default();
        let finished = (running.tick > running.ticks).then(|| running.name.clone());
        for line in lines {
            if !self.tick_script_click(&line) {
                self.replay_line(&line, event_loop);
            }
        }
        if let Some(name) = finished {
            self.tick_script.running = None;
            let _ = std::fs::write(path.with_extension("done"), name);
        }
    }
}

impl Game<'_> {
    /// `click-slot SLOT [right] [shift]` without a renderer: horizon-ctl's version clicks where the
    /// slot is drawn, and `--headless` draws nothing. The vanilla driver clicks the menu itself
    /// (`MultiPlayerGameMode.handleContainerInput`), and so does this.
    fn tick_script_click(&mut self, line: &str) -> bool {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (Some(&"click-slot"), Some(Ok(slot))) =
            (words.first(), words.get(1).map(|w| w.parse()))
        else {
            return false;
        };
        let input = if words.contains(&"shift") {
            horizon_client::menu::Input::QuickMove
        } else {
            horizon_client::menu::Input::Pickup
        };
        self.remote_menu_ops(&[(Some(slot), i32::from(words.contains(&"right")), input)]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scenario_names_its_ticks_and_their_lines() {
        let s = parse(r#"{"name":"walk","ticks":40,"steps":{"0":["key w down"],"20":["key w up","look 90 0"]}}"#)
            .unwrap();
        assert_eq!((s.name.as_str(), s.ticks), ("walk", 40));
        assert_eq!(s.steps[&20], ["key w up", "look 90 0"]);
        assert!(parse(r#"{"name":"x","ticks":1,"steps":{"a":[]}}"#).is_none());
    }
}
