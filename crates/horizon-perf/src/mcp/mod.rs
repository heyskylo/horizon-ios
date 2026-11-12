//! `horizon-perf mcp` (docs/perf-platform.md §8): the Model Context Protocol over stdin and stdout, one JSON-RPC message per line.

mod schema;
pub mod tools;

use std::io::{BufRead, Write};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub use schema::{JsonType, Schema};
use tools::Outcome;
pub use tools::ToolName;

use crate::cli::Env;
use crate::wire::vocab;

const INSTRUCTIONS: &str = "Read Horizon performance captures (.hzperf). Start with perf_summary: it ends with the exact drill-down queries. perf_query takes the ids the summary prints. perf_follow reads a capture that is still being written: pass back the cursor it returns.";

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i32 = -32700;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;

vocab! {
    /// The JSON-RPC methods this server answers.
    Method {
        Initialize = "initialize",
        Initialized = "notifications/initialized",
        Ping = "ping",
        ToolsList = "tools/list",
        ToolsCall = "tools/call",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Number(i64),
    Text(String),
}

#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    id: Option<RpcId>,
    method: String,
    #[serde(default)]
    params: Option<Box<RawValue>>,
}

#[derive(Serialize)]
struct Success<T: Serialize> {
    jsonrpc: &'static str,
    id: Option<RpcId>,
    result: T,
}

#[derive(Serialize)]
struct Failure {
    jsonrpc: &'static str,
    id: Option<RpcId>,
    error: RpcError,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Deserialize, Default)]
struct InitializeParams {
    #[serde(default, rename = "protocolVersion")]
    protocol_version: Option<String>,
}

#[derive(Serialize)]
struct ToolsCapability {}

#[derive(Serialize)]
struct Capabilities {
    tools: ToolsCapability,
}

#[derive(Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: &'static str,
    capabilities: Capabilities,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
    instructions: &'static str,
}

#[derive(Serialize)]
struct ToolInfo {
    name: ToolName,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Schema,
}

#[derive(Serialize)]
struct ToolList {
    tools: Vec<ToolInfo>,
}

#[derive(Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Option<Box<RawValue>>,
}

vocab! {
    ContentKind { Text = "text" }
}

#[derive(Serialize)]
struct Content {
    #[serde(rename = "type")]
    kind: ContentKind,
    text: String,
}

#[derive(Serialize)]
struct CallResult {
    content: Vec<Content>,
    #[serde(rename = "isError")]
    is_error: bool,
}

/// The tools with their descriptions and argument schemas, as `tools/list` returns them.
fn tool_list() -> ToolList {
    ToolList {
        tools: ToolName::ALL
            .iter()
            .map(|&name| ToolInfo {
                name,
                description: name.description(),
                input_schema: schema::input(name),
            })
            .collect(),
    }
}

fn line<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("a reply always serializes")
}

fn success<T: Serialize>(id: Option<RpcId>, result: T) -> String {
    line(&Success {
        jsonrpc: "2.0",
        id,
        result,
    })
}

fn failure(id: Option<RpcId>, code: i32, message: String) -> String {
    line(&Failure {
        jsonrpc: "2.0",
        id,
        error: RpcError { code, message },
    })
}

fn initialize(params: Option<&RawValue>) -> InitializeResult {
    let asked = params
        .and_then(|p| serde_json::from_str::<InitializeParams>(p.get()).ok())
        .unwrap_or_default()
        .protocol_version;
    let protocol_version = PROTOCOL_VERSIONS
        .iter()
        .find(|v| Some(**v) == asked.as_deref())
        .unwrap_or(&PROTOCOL_VERSIONS[0]);
    InitializeResult {
        protocol_version,
        capabilities: Capabilities {
            tools: ToolsCapability {},
        },
        server_info: ServerInfo {
            name: "horizon-perf",
            version: env!("CARGO_PKG_VERSION"),
        },
        instructions: INSTRUCTIONS,
    }
}

fn call(id: Option<RpcId>, params: Option<&RawValue>, env: &Env) -> String {
    let Some(params) = params.and_then(|p| serde_json::from_str::<CallParams>(p.get()).ok()) else {
        return failure(id, INVALID_PARAMS, "tools/call needs a name".into());
    };
    let Ok(tool) = ToolName::from_str(&params.name) else {
        return failure(id, INVALID_PARAMS, format!("unknown tool {}", params.name));
    };
    let arguments = params.arguments.as_deref().map_or("{}", RawValue::get);
    let arguments = if arguments == "null" { "{}" } else { arguments };
    match tools::call(tool, arguments, env) {
        Ok(Outcome { text, is_error }) => success(
            id,
            CallResult {
                content: vec![Content {
                    kind: ContentKind::Text,
                    text,
                }],
                is_error,
            },
        ),
        Err(tools::ParamError(message)) => failure(id, INVALID_PARAMS, message),
    }
}

/// The reply to one request line, or none for a notification.
pub fn handle(text: &str, env: &Env) -> Option<String> {
    let request: Request = match serde_json::from_str(text) {
        Ok(request) => request,
        Err(e) => return Some(failure(None, PARSE_ERROR, format!("parse error: {e}"))),
    };
    let method = Method::from_str(&request.method).ok();
    let id = request.id?;
    let params = request.params.as_deref();
    Some(match method {
        Some(Method::Initialize) => success(Some(id), initialize(params)),
        Some(Method::Ping) => success(Some(id), Empty {}),
        Some(Method::ToolsList) => success(Some(id), tool_list()),
        Some(Method::ToolsCall) => call(Some(id), params, env),
        Some(Method::Initialized) | None => failure(
            Some(id),
            METHOD_NOT_FOUND,
            format!("method not found: {}", request.method),
        ),
    })
}

/// Answers one request per line of `input` until it ends.
pub fn serve(input: &mut dyn BufRead, out: &mut dyn Write, env: &Env) -> Result<(), String> {
    let mut text = String::new();
    loop {
        text.clear();
        if input.read_line(&mut text).map_err(|e| e.to_string())? == 0 {
            return Ok(());
        }
        if text.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(text.trim(), env) {
            writeln!(out, "{reply}").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
}
