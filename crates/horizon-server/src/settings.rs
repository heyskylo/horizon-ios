//! `horizon-server config`: read server.toml's settings, and change them the way the server checks them
//! (docs/server.md, section 4). A running server takes what vanilla can change live through its console
//! (difficulty, the default game mode, the whitelist) and through its management settings (the message of the
//! day, the player limit) at once; the rest applies at the next start.

use crate::{api::Client, config, engine};
use serde_json::{Value, json};
use std::path::Path;

/// `config [--json]` and `config set KEY=VALUE... [--json]`.
pub fn command(dir: &Path, args: &[String]) -> Result<(), String> {
    let as_json = args.iter().any(|a| a == "--json");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json")
        .collect();
    match args.as_slice() {
        [] => show(dir, as_json),
        ["set", changes @ ..] if !changes.is_empty() => {
            let outcome = set(dir, changes)?;
            if as_json {
                println!("{}", outcome.to_json());
            } else {
                print!("{}", outcome.describe());
            }
            Ok(())
        }
        _ => Err(
            "config takes no arguments, or `set KEY=VALUE...` (for example: config set world.difficulty=hard)"
                .into(),
        ),
    }
}

fn show(dir: &Path, as_json: bool) -> Result<(), String> {
    let table = shown(dir)?;
    if as_json {
        println!("{table}");
        return Ok(());
    }
    for (section, values) in table.as_object().into_iter().flatten() {
        for (key, value) in values.as_object().into_iter().flatten() {
            println!("{section}.{key} = {value}");
        }
    }
    Ok(())
}

/// What `config set` did.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Every setting written to server.toml.
    pub saved: Vec<String>,
    /// Those the running server took at once.
    pub applied: Vec<String>,
    /// Those that wait for the game's next start, while it runs.
    pub restart: Vec<String>,
    pub running: bool,
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        json!({
            "saved": self.saved,
            "applied": self.applied,
            "restart": self.restart,
            "running": self.running,
        })
    }

    fn describe(&self) -> String {
        if !self.running {
            return format!(
                "Saved {}: the server uses them from its next start.\n",
                self.saved.join(", ")
            );
        }
        let mut out = String::new();
        if !self.applied.is_empty() {
            out += &format!("Applied now: {}\n", self.applied.join(", "));
        }
        if !self.restart.is_empty() {
            out += &format!(
                "Saved; restart the server to apply: {} (`horizon-server restart`)\n",
                self.restart.join(", ")
            );
        }
        out
    }
}

/// One `KEY=VALUE`: the section, the field and the value as server.toml types it.
pub fn parse_change(
    current: &toml::Table,
    change: &str,
) -> Result<(String, String, toml::Value), String> {
    let (key, text) = change
        .split_once('=')
        .ok_or_else(|| format!("{change:?}: write KEY=VALUE, like world.difficulty=hard"))?;
    let (section, field) = key
        .trim()
        .split_once('.')
        .ok_or_else(|| format!("{key:?}: name a setting as section.name, like server.port"))?;
    let text = text.trim();
    let value = if section == "properties" {
        config::loose_value(text)
    } else {
        let like = current
            .get(section)
            .and_then(|s| s.get(field))
            .ok_or_else(|| {
                format!("{key} is not a setting (`horizon-server config` lists them)")
            })?;
        match like {
            toml::Value::Boolean(_) => toml::Value::Boolean(match text {
                "true" | "on" | "yes" => true,
                "false" | "off" | "no" => false,
                _ => return Err(format!("{key} is on or off: use true or false")),
            }),
            toml::Value::Integer(_) => toml::Value::Integer(
                text.parse()
                    .map_err(|_| format!("{key} is a number, not {text:?}"))?,
            ),
            _ => toml::Value::String(text.to_owned()),
        }
    };
    if let (Some(range), toml::Value::Integer(n)) = (range(section, field), &value)
        && !range.contains(n)
    {
        return Err(format!(
            "{key} is {n}: use {} to {}",
            range.start(),
            range.end()
        ));
    }
    Ok((section.to_owned(), field.to_owned(), value))
}

/// The values vanilla uses for the numbers it clamps (`ChunkMap` keeps both distances within 2 to 32).
fn range(section: &str, field: &str) -> Option<std::ops::RangeInclusive<i64>> {
    match (section, field) {
        ("server", "view_distance" | "simulation_distance") => Some(2..=32),
        ("server", "max_view_distance") => Some(0..=32),
        ("server", "max_players") => Some(1..=1000),
        ("backups", "keep") => Some(1..=1000),
        _ => None,
    }
}

/// The control line that applies a setting to a running game, if there is a way to: a console command, or the
/// management setting (`serversettings/motd/set`, `serversettings/max_players/set`) that the management
/// protocol uses; `Some("")` for the settings the supervisor itself reads every time (backups, updates).
pub fn live_line(section: &str, field: &str, value: &toml::Value) -> Option<String> {
    let setting = |name: &str, value: Value| {
        let request = json!({ "method": format!("serversettings/{name}/set"), "value": value });
        // Connection 0: no RPC client made this change, the command line did.
        Some(format!("management 0 {request}"))
    };
    match (section, field, value) {
        ("server", "motd", toml::Value::String(text)) => setting("motd", json!(text)),
        ("server", "max_players", toml::Value::Integer(n)) => setting("max_players", json!(n)),
        _ => live_command(section, field, value).map(|line| {
            if line.is_empty() {
                line
            } else {
                format!("console {line}")
            }
        }),
    }
}

/// Sends the running game one control line on its endpoint, and says whether it took it.
pub fn ask_game(dir: &Path, line: &str) -> bool {
    engine::Control::connect(dir)
        .and_then(|mut game| game.ask(line))
        .is_ok_and(|reply| reply.get("error").is_none())
}

/// The console command that applies a setting to a running game, if vanilla has one; `Some("")` for the
/// settings the supervisor itself reads every time (backups, updates).
pub fn live_command(section: &str, field: &str, value: &toml::Value) -> Option<String> {
    let text = match value {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    match (section, field) {
        ("world", "difficulty") => Some(format!("difficulty {text}")),
        ("world", "game_mode") => Some(format!("defaultgamemode {text}")),
        ("server", "whitelist") => Some(format!(
            "whitelist {}",
            if value.as_bool() == Some(true) {
                "on"
            } else {
                "off"
            }
        )),
        ("backups" | "updates", _) => Some(String::new()),
        _ => None,
    }
}

pub fn set(dir: &Path, changes: &[&str]) -> Result<Outcome, String> {
    let client = Client::open(dir)?;
    let up = client.as_ref().is_some_and(|c| {
        c.call("GET", "/api/v1/status", None)
            .is_ok_and(|s| s["phase"] == "running")
    });
    let run = |line: &str| up && ask_game(dir, line);
    apply(dir, changes, client.is_some().then_some(&run))
}

/// Writes the changes to server.toml, then sends a running server (`live` sends the game a control line and
/// says whether it took it) what it can take at once.
pub fn apply(
    dir: &Path,
    changes: &[&str],
    live: Option<&dyn Fn(&str) -> bool>,
) -> Result<Outcome, String> {
    let current = toml::Table::try_from(config::Config::load(dir)?).map_err(|e| e.to_string())?;
    let parsed = changes
        .iter()
        .map(|c| parse_change(&current, c))
        .collect::<Result<Vec<_>, _>>()?;
    config::update_all(dir, &parsed)?;
    let mut outcome = Outcome {
        saved: parsed.iter().map(|(s, f, _)| format!("{s}.{f}")).collect(),
        ..Outcome::default()
    };
    let Some(live) = live else {
        return Ok(outcome);
    };
    outcome.running = true;
    for (section, field, value) in &parsed {
        let key = format!("{section}.{field}");
        let unchanged = current.get(section).and_then(|s| s.get(field)) == Some(value);
        match live_line(section, field, value) {
            _ if unchanged => {}
            Some(line) if line.is_empty() => outcome.applied.push(key),
            Some(line) if live(&line) => outcome.applied.push(key),
            _ => outcome.restart.push(key),
        }
    }
    Ok(outcome)
}

/// The settings the control API may change: none of `[engine]` (which program runs), `[panel]` and `[api]`
/// (who can reach the server), `[discord]` (its webhook is a secret), nor the vanilla keys that open another remote console.
pub fn changeable_remotely(key: &str) -> bool {
    let (section, field) = key.split_once('.').unwrap_or((key, ""));
    !matches!(section, "engine" | "panel" | "api" | "discord")
        && !(section == "properties"
            && (field.starts_with("rcon.")
                || field.starts_with("management-server-")
                || matches!(field, "enable-rcon" | "enable-query" | "query.port")))
}

/// The settings as JSON, the API's token and the Discord webhook hidden.
pub fn shown(dir: &Path) -> Result<Value, String> {
    let mut table = toml::Table::try_from(config::Config::load(dir)?).map_err(|e| e.to_string())?;
    for (section, field) in [("api", "token"), ("discord", "webhook")] {
        if let Some(secret) = table
            .get_mut(section)
            .and_then(|a| a.get_mut(field))
            .filter(|t| t.as_str() != Some(""))
        {
            *secret = toml::Value::String("(set)".into());
        }
    }
    serde_json::to_value(&table).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> toml::Table {
        toml::Table::try_from(config::Config::default()).unwrap()
    }

    #[test]
    fn changes_take_the_settings_types() {
        let t = table();
        assert_eq!(
            parse_change(&t, "server.max_players=8").unwrap(),
            (
                "server".into(),
                "max_players".into(),
                toml::Value::Integer(8)
            )
        );
        assert_eq!(
            parse_change(&t, "server.whitelist=off").unwrap().2,
            toml::Value::Boolean(false)
        );
        assert_eq!(
            parse_change(&t, "server.motd= Friends only ").unwrap().2,
            toml::Value::String("Friends only".into())
        );
        assert_eq!(
            parse_change(&t, "properties.spawn-protection=0").unwrap(),
            (
                "properties".into(),
                "spawn-protection".into(),
                toml::Value::Integer(0)
            )
        );
        for (bad, says) in [
            ("server.view_distance=40", "use 2 to 32"),
            ("server.max_players=many", "is a number"),
            ("server.online_mode=maybe", "use true or false"),
            ("server.colour=red", "is not a setting"),
            ("difficulty=hard", "section.name"),
            ("world.difficulty", "KEY=VALUE"),
        ] {
            let e = parse_change(&t, bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    #[test]
    fn vanilla_changes_these_live() {
        let s = |v: &str| toml::Value::String(v.into());
        assert_eq!(
            live_command("world", "difficulty", &s("hard")).as_deref(),
            Some("difficulty hard")
        );
        assert_eq!(
            live_command("world", "game_mode", &s("creative")).as_deref(),
            Some("defaultgamemode creative")
        );
        assert_eq!(
            live_command("server", "whitelist", &toml::Value::Boolean(false)).as_deref(),
            Some("whitelist off")
        );
        assert_eq!(
            live_command("backups", "every", &s("1h")).as_deref(),
            Some("")
        );
        assert_eq!(
            live_command("server", "view_distance", &toml::Value::Integer(8)),
            None
        );
    }

    #[test]
    fn the_message_and_the_player_limit_go_to_the_management_settings() {
        let s = |v: &str| toml::Value::String(v.into());
        assert_eq!(
            live_line("server", "motd", &s("Friends \"only\"")).as_deref(),
            Some(r#"management 0 {"method":"serversettings/motd/set","value":"Friends \"only\""}"#)
        );
        assert_eq!(
            live_line("server", "max_players", &toml::Value::Integer(8)).as_deref(),
            Some(r#"management 0 {"method":"serversettings/max_players/set","value":8}"#)
        );
        // The console ones keep their command, the supervisor's own settings need none.
        assert_eq!(
            live_line("world", "difficulty", &s("hard")).as_deref(),
            Some("console difficulty hard")
        );
        assert_eq!(live_line("backups", "every", &s("1h")).as_deref(), Some(""));
        assert_eq!(
            live_line("server", "view_distance", &toml::Value::Integer(8)),
            None
        );
    }

    #[test]
    fn a_running_server_is_sent_only_what_changed() {
        let dir = std::env::temp_dir().join(format!("horizon-server-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(config::FILE), config::TEMPLATE).unwrap();
        let sent = std::cell::RefCell::new(Vec::new());
        let game = |line: &str| {
            sent.borrow_mut().push(line.to_owned());
            true
        };
        let outcome = apply(
            &dir,
            &[
                "server.motd=Back at noon",
                "server.max_players=5",
                "server.view_distance=9",
            ],
            Some(&game),
        )
        .unwrap();
        assert_eq!(outcome.applied, ["server.motd", "server.max_players"]);
        assert_eq!(outcome.restart, ["server.view_distance"]);
        assert_eq!(
            *sent.borrow(),
            [
                r#"management 0 {"method":"serversettings/motd/set","value":"Back at noon"}"#,
                r#"management 0 {"method":"serversettings/max_players/set","value":5}"#,
            ]
        );
        // The same values again change nothing, so nothing is sent.
        sent.borrow_mut().clear();
        let outcome = apply(
            &dir,
            &["server.motd=Back at noon", "server.max_players=5"],
            Some(&game),
        )
        .unwrap();
        assert!(outcome.applied.is_empty() && outcome.restart.is_empty());
        assert!(sent.borrow().is_empty());
        // A game that refuses leaves the setting for the next start.
        let refuses = |_: &str| false;
        let outcome = apply(&dir, &["server.motd=Closed"], Some(&refuses)).unwrap();
        assert_eq!(outcome.restart, ["server.motd"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stopped_server_saves_and_keeps_comments() {
        let dir =
            std::env::temp_dir().join(format!("horizon-server-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(config::FILE), config::TEMPLATE).unwrap();
        let outcome = set(
            &dir,
            &[
                "world.difficulty=hard",
                "server.max_players=5",
                "backups.every=0",
            ],
        )
        .unwrap();
        assert_eq!(
            outcome,
            Outcome {
                saved: vec![
                    "world.difficulty".into(),
                    "server.max_players".into(),
                    "backups.every".into()
                ],
                ..Outcome::default()
            }
        );
        let text = std::fs::read_to_string(dir.join(config::FILE)).unwrap();
        assert!(
            text.contains("difficulty = \"hard\"          # peaceful, easy, normal or hard"),
            "{text}"
        );
        let cfg = config::Config::load(&dir).unwrap();
        assert_eq!(
            (cfg.server.max_players, cfg.backups.every.as_str()),
            (5, "0")
        );
        // Checked as the server checks it: nothing is written.
        assert!(set(&dir, &["world.difficulty=impossible"]).is_err());
        assert!(set(&dir, &["world.name=a/b", "server.port=1"]).is_err());
        assert_eq!(config::Config::load(&dir).unwrap(), cfg);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
