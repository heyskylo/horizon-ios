//! The running game's tools in the launcher's MCP, so an agent needs one server
//! (docs/agent-control.md): the selected version's `horizon-ctl mcp` runs as a child, its tools
//! are offered as `game_<name>`, and calls go to it unchanged. `play` starts the game with
//! the launcher's own control endpoint; a game the player started with Agent Control on answers
//! on the game's default endpoint instead. The child talks to whichever of them answers.
use horizon_launcher::backend;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::time::Duration;

pub const PREFIX: &str = "game_";

/// horizon-ctl tools the launcher covers itself: `play` starts the game.
const HIDDEN: &[&str] = &["launch"];

const CTL: &str = if cfg!(windows) {
    "horizon-ctl.exe"
} else {
    "horizon-ctl"
};

/// The control endpoint of a game started by `play`: a Unix socket, or on Windows the
/// file naming a localhost port and its token.
pub fn socket_path() -> PathBuf {
    backend::data_dir().join(if cfg!(unix) {
        "game-control.sock"
    } else {
        "game-control.json"
    })
}

/// The game arguments that put it under the agent's control.
pub fn control_args() -> Vec<String> {
    vec![
        "--control-socket".into(),
        socket_path().display().to_string(),
    ]
}

/// The endpoint a game listens on when nothing names one, which is where Agent Control opens it.
/// The launcher runs whichever version is installed, so this is a contract with every game
/// rather than shared code: `control::default_path` in horizon-client, and `horizon-ctl help`.
fn default_endpoint(
    control: Option<OsString>,
    runtime: Option<OsString>,
    temp: PathBuf,
) -> PathBuf {
    if let Some(path) = control {
        return path.into();
    }
    if !cfg!(unix) {
        return temp.join("horizon-control.json");
    }
    runtime
        .map_or(temp, PathBuf::from)
        .join("horizon-control.sock")
}

/// The endpoint to talk to: the game `play` started, else the game answering on the default
/// endpoint. With neither it stays the launcher's, the one `play` will open.
fn choose_endpoint(own: PathBuf, default: PathBuf) -> PathBuf {
    if !answers(&own) && answers(&default) {
        default
    } else {
        own
    }
}

fn current_endpoint() -> PathBuf {
    choose_endpoint(
        socket_path(),
        default_endpoint(
            std::env::var_os("HORIZON_CONTROL"),
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::temp_dir(),
        ),
    )
}

/// Whether a game answers on an endpoint. The file alone says nothing: a crashed game leaves it.
fn answers(path: &Path) -> bool {
    #[cfg(unix)]
    if path.extension().is_none_or(|e| e != "json") {
        return std::os::unix::net::UnixStream::connect(path).is_ok();
    }
    tcp_answers(path).unwrap_or(false)
}

/// The file names a localhost port and its token (`control::connect` in horizon-client): a game
/// accepts that token, and whatever took over the port of a game long gone does not.
fn tcp_answers(path: &Path) -> Option<bool> {
    let endpoint: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let port = u16::try_from(endpoint["port"].as_u64()?).ok()?;
    let token = endpoint["token"].as_str()?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    writeln!(&stream, "auth {token}").ok()?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply).ok()?;
    Some(reply.contains("\"ok\""))
}

/// Whether the game `play` started answers yet.
pub fn listening() -> bool {
    answers(&socket_path())
}

pub struct GameTools {
    /// The installed version this `horizon-ctl` belongs to.
    tag: String,
    /// The endpoint it talks to.
    endpoint: PathBuf,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    tools: Vec<Value>,
}

impl GameTools {
    /// Start `horizon-ctl mcp` of an installed version and read its tools.
    pub fn start(tag: &str) -> Result<Self, String> {
        Self::start_on(tag, current_endpoint())
    }

    fn start_on(tag: &str, endpoint: PathBuf) -> Result<Self, String> {
        let ctl = backend::version_dir(tag).join(CTL);
        if !ctl.exists() {
            return Err(format!("{tag} has no {CTL}"));
        }
        let mut child = horizon_launcher::process::command(&ctl)
            .arg("--socket")
            .arg(&endpoint)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start {}: {e}", ctl.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut tools = Self {
            tag: tag.to_owned(),
            endpoint,
            child,
            stdin,
            stdout,
            next_id: 0,
            tools: Vec::new(),
        };
        tools.rpc(
            // The child is whichever `horizon-ctl` the player has installed, so open with the
            // handshake revision: older versions do not know the stateless one.
            "initialize",
            json!({ "protocolVersion": horizon_mcp::LEGACY, "capabilities": {},
                    "clientInfo": { "name": "horizon-launcher", "version": backend::VERSION } }),
        )?;
        tools.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        let listed = tools.rpc("tools/list", json!({}))?;
        tools.tools = prefixed(&listed["tools"]);
        Ok(tools)
    }

    /// The game's tools, named `game_<name>`.
    pub fn tools(&self) -> &[Value] {
        &self.tools
    }

    /// Call a `game_` tool; the result is horizon-ctl's (text and image content). The game can
    /// start, stop or turn Agent Control on between two calls, and an installed horizon-ctl keeps
    /// the endpoint it started with, so another endpoint means another child.
    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let inner = name.strip_prefix(PREFIX).ok_or("not a game tool")?;
        let endpoint = current_endpoint();
        if endpoint != self.endpoint {
            *self = Self::start_on(&self.tag, endpoint)?;
        }
        let result = self.rpc("tools/call", json!({ "name": inner, "arguments": args }))?;
        Ok(explain_unreachable(result))
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{message}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("horizon-ctl stopped: {e}"))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let mut line = String::new();
        loop {
            line.clear();
            match self.stdout.read_line(&mut line) {
                Ok(0) | Err(_) => return Err("horizon-ctl stopped".into()),
                Ok(_) => {}
            }
            let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if reply["id"] != id {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return Err(error["message"].as_str().unwrap_or("error").to_owned());
            }
            return Ok(reply["result"].clone());
        }
    }
}

impl Drop for GameTools {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// horizon-ctl's tools as the launcher offers them.
fn prefixed(tools: &Value) -> Vec<Value> {
    tools
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| !HIDDEN.contains(&t["name"].as_str().unwrap_or_default()))
        .map(|t| {
            let mut t = t.clone();
            t["name"] = format!("{PREFIX}{}", t["name"].as_str().unwrap_or_default()).into();
            t["description"] = format!(
                "In the running game (start it with play): {}",
                t["description"].as_str().unwrap_or_default()
            )
            .into();
            t
        })
        .collect()
}

/// A game tool called while no game listens says how to start one from here.
fn explain_unreachable(mut result: Value) -> Value {
    let unreachable = result["isError"] == true
        && result["content"].as_array().is_some_and(|c| {
            c.iter().any(|block| {
                block["text"]
                    .as_str()
                    .and_then(|t| serde_json::from_str::<Value>(t).ok())
                    .is_some_and(|v| v["kind"] == "connection")
            })
        });
    if unreachable {
        result["content"] = json!([{ "type": "text", "text": json!({
            "error": "The game is not running under agent control. Start it with play, then game_wait_ready, or ask the player to turn on Agent Control in the game they started (Options > Horizon Settings > AI Agent).",
            "kind": "state",
        }).to_string() }]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A folder for one test's endpoint files.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-game-endpoint-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_endpoint(path: &Path, port: u16, token: &str) {
        let endpoint = json!({ "transport": "tcp", "port": port, "token": token });
        std::fs::write(path, endpoint.to_string()).unwrap();
    }

    /// An endpoint a game listens on: the game's half of the token exchange.
    fn live_endpoint(path: &Path) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        write_endpoint(path, listener.local_addr().unwrap().port(), "secret");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let reply = if line.trim() == "auth secret" {
                    r#"{"ok":true}"#
                } else {
                    r#"{"error":"bad or missing token: the first line must be auth TOKEN"}"#
                };
                let _ = writeln!(&stream, "{reply}");
            }
        });
    }

    /// What a crashed game leaves: the file, and nothing on its port.
    fn stale_endpoint(path: &Path) {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        write_endpoint(path, port, "secret");
    }

    #[test]
    fn a_game_the_launcher_started_comes_before_the_default_endpoint() {
        let dir = scratch("both");
        let (own, default) = (dir.join("game-control.json"), dir.join("default.json"));
        live_endpoint(&own);
        live_endpoint(&default);
        assert_eq!(choose_endpoint(own.clone(), default), own);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// #9465: a game started from its window, with Agent Control turned on.
    #[test]
    fn with_no_game_from_the_launcher_the_default_endpoint_is_used() {
        let dir = scratch("fallback");
        let (own, default) = (dir.join("game-control.json"), dir.join("default.json"));
        live_endpoint(&default);
        assert_eq!(choose_endpoint(own.clone(), default.clone()), default);
        // A crashed game's file on the launcher's endpoint hides nothing.
        stale_endpoint(&own);
        assert_eq!(choose_endpoint(own, default.clone()), default);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn with_no_game_answering_the_launchers_endpoint_stays() {
        let dir = scratch("none");
        let (own, default) = (dir.join("game-control.json"), dir.join("default.json"));
        assert_eq!(choose_endpoint(own.clone(), default.clone()), own);
        stale_endpoint(&default);
        assert_eq!(choose_endpoint(own.clone(), default), own);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A program that took over the port of a game long gone is not a game, and neither is a
    /// file that names no port.
    #[test]
    fn an_endpoint_that_refuses_its_token_does_not_answer() {
        let dir = scratch("refused");
        let refused = dir.join("refused.json");
        live_endpoint(&refused);
        let port = serde_json::from_str::<Value>(&std::fs::read_to_string(&refused).unwrap())
            .unwrap()["port"]
            .as_u64()
            .unwrap();
        assert!(answers(&refused));
        write_endpoint(&refused, port as u16, "another");
        assert!(!answers(&refused));
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{}").unwrap();
        assert!(!answers(&broken));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_default_endpoint_is_the_games() {
        let temp = PathBuf::from("temp");
        let runtime = || Some(OsString::from("run"));
        assert_eq!(
            default_endpoint(Some("mine.sock".into()), runtime(), temp.clone()),
            PathBuf::from("mine.sock")
        );
        if cfg!(unix) {
            assert_eq!(
                default_endpoint(None, runtime(), temp.clone()),
                Path::new("run").join("horizon-control.sock")
            );
            assert_eq!(
                default_endpoint(None, None, temp.clone()),
                temp.join("horizon-control.sock")
            );
        } else {
            assert_eq!(
                default_endpoint(None, runtime(), temp.clone()),
                temp.join("horizon-control.json")
            );
        }
    }

    #[test]
    fn offers_the_game_tools_prefixed_without_launch() {
        let listed = json!([
            { "name": "launch", "description": "start", "inputSchema": {} },
            { "name": "click_slot", "description": "Click a slot", "inputSchema": { "type": "object" } },
        ]);
        let tools = prefixed(&listed);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "game_click_slot");
        assert!(tools[0]["description"]
            .as_str()
            .unwrap()
            .ends_with("Click a slot"));
        assert_eq!(tools[0]["inputSchema"], json!({ "type": "object" }));
    }

    #[test]
    fn a_connection_error_says_to_launch_the_client() {
        let text = json!({ "error": "cannot connect", "kind": "connection" }).to_string();
        let result = explain_unreachable(
            json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
        );
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("with play"));
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Agent Control"));
        let other = json!({ "content": [{ "type": "text", "text": "{\"error\":\"no world\"}" }], "isError": true });
        assert_eq!(explain_unreachable(other.clone()), other);
    }
}
