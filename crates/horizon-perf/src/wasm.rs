//! The browser boundary: a C ABI over JSON (docs/perf-platform.md §7.1).
//!
//! The page calls these exports through `horizon_perf.js`, which sits beside the wasm. Everything
//! a number needs happens here, in the same code the CLI runs, so the web UI and `horizon-perf`
//! cannot disagree. The ABI is deliberately small and has no `wasm-bindgen` dependency:
//!
//! | Export | Does |
//! |---|---|
//! | `hp_alloc(len) -> ptr`, `hp_free(ptr, len)` | memory for the UI to pass bytes in, and to give replies back |
//! | `hp_open(ptr, len) -> reply` | decodes a capture; the reply's JSON holds its `handle` |
//! | `hp_query(handle, req, len) -> reply` | the same op and filters as `horizon-perf query`, as §6.3's envelope |
//! | `hp_summary(handle, req, len) -> reply` | the §5 summary text |
//! | `hp_diff(a, b, req, len) -> reply` | the §6.6 statistics, with the text table |
//! | `hp_export(handle, req, len) -> reply` | pprof, folded or Perfetto bytes |
//! | `hp_close(handle)` | frees a decoded capture |
//!
//! Every reply is a buffer `[kind: u8][len: u32 little endian][payload]` where kind 0 is JSON, 1 is
//! bytes and 2 is text. The caller frees it with `hp_free(ptr, 5 + len)`. A failure is always a
//! kind-0 reply `{"ok": false, "error": {"code": …, "message": …}}`: the code is stable and the UI
//! translates it, the message is for logs and agents.
//!
//! The interface returns data, ids and codes, never English sentences, except `hp_summary` (which
//! is a sentence report by design). UI strings live in `web/perf/lang/`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use horizon_perf_format::v1::Capture;
use serde::{Deserialize, Serialize};

use crate::diff::DiffOptions;
use crate::export::{ExportOptions, Format};
use crate::query::{Loaded, Query};
use crate::read::read_capture;
use crate::request::{self, QueryRequest};
use crate::summary::{Baseline, DEFAULT_BUDGET, SummaryOptions};
use crate::wire::{Op, Select, SideName};
use crate::{Error, ops};

/// The kind byte of a reply buffer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Json = 0,
    Bytes = 1,
    Text = 2,
}

/// One reply to the page, before it becomes a buffer.
pub enum Reply {
    Json(String),
    Text(String),
    Bytes(Vec<u8>),
}

impl Reply {
    /// The buffer the ABI hands out: kind, length, payload.
    pub fn into_buffer(self) -> Vec<u8> {
        let (kind, payload) = match self {
            Reply::Json(text) => (Kind::Json, text.into_bytes()),
            Reply::Text(text) => (Kind::Text, text.into_bytes()),
            Reply::Bytes(bytes) => (Kind::Bytes, bytes),
        };
        let mut out = Vec::with_capacity(5 + payload.len());
        out.push(kind as u8);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }
}

/// A stable failure code and a message for logs; the UI translates the code.
fn failure(code: &str, message: impl Into<String>) -> Reply {
    let reply = FailureReply {
        ok: false,
        error: FailureBody {
            code,
            message: message.into(),
        },
    };
    Reply::Json(serde_json::to_string(&reply).expect("a reply of plain fields serializes"))
}

#[derive(Serialize)]
struct FailureReply<'a> {
    ok: bool,
    error: FailureBody<'a>,
}

#[derive(Serialize)]
struct FailureBody<'a> {
    code: &'a str,
    message: String,
}

#[derive(Serialize)]
struct OpenedReply {
    ok: bool,
    handle: u32,
}

/// The code the reader's error maps to, as §7.3 names them.
fn capture_code(error: &Error) -> &'static str {
    match error {
        Error::FormatTooNew { .. } => "unsupported_format",
        Error::NotGzip | Error::Corrupt(_) => "invalid_capture",
        Error::Unfinished { .. } | Error::Empty => "still_writing",
        _ => "invalid_capture",
    }
}

/// `not_a_capture` for bytes the reader cannot decode, with the reader's message.
fn open_error(error: Error) -> Reply {
    failure(capture_code(&error), error.to_string())
}

thread_local! {
    /// Decoded captures, shared so a query hands one out without copying it.
    static CAPTURES: RefCell<HashMap<u32, Arc<Capture>>> = RefCell::new(HashMap::new());
    static NEXT_HANDLE: Cell<u32> = const { Cell::new(1) };
}

fn capture(handle: u32) -> Result<Arc<Capture>, Reply> {
    CAPTURES.with(|captures| {
        captures
            .borrow()
            .get(&handle)
            .cloned()
            .ok_or_else(|| failure("unknown_handle", format!("no capture loaded as {handle}")))
    })
}

/// Decodes `bytes` and keeps them under a new handle.
pub fn open(bytes: &[u8]) -> Reply {
    match read_capture(bytes) {
        Ok(capture) => {
            let handle = NEXT_HANDLE.with(|next| {
                let handle = next.get();
                next.set(handle.wrapping_add(1).max(1));
                handle
            });
            CAPTURES.with(|captures| {
                captures.borrow_mut().insert(handle, Arc::new(capture));
            });
            let reply = OpenedReply { ok: true, handle };
            Reply::Json(serde_json::to_string(&reply).expect("a reply of plain fields serializes"))
        }
        Err(error) => open_error(error),
    }
}

/// What a query call asks for: `horizon-perf query`'s op, arguments and filters, plus the name the
/// capture is listed under.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCall {
    pub op: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default)]
    pub filters: QueryRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

impl QueryCall {
    fn name(&self) -> String {
        self.file
            .clone()
            .or_else(|| self.files.first().cloned())
            .unwrap_or_else(|| "capture.hzperf".to_string())
    }

    fn query(&self) -> Result<Query, Reply> {
        let op = Op::from_str(&self.op).map_err(|_| {
            failure(
                "unknown_op",
                format!("unknown query op {}; ops: {}", self.op, Op::answered()),
            )
        })?;
        self.filters
            .clone()
            .into_query(op, self.args.clone())
            .map_err(|e| failure("bad_request", e.0))
    }
}

/// Runs one query over a handle; the reply is §6.3's envelope.
pub fn query(handle: u32, request: &str) -> Reply {
    let call: QueryCall = match serde_json::from_str(request) {
        Ok(call) => call,
        Err(e) => return failure("bad_request", e.to_string()),
    };
    let query = match call.query() {
        Ok(query) => query,
        Err(reply) => return reply,
    };
    let capture = match capture(handle) {
        Ok(capture) => capture,
        Err(reply) => return reply,
    };
    let loaded = [Loaded {
        file: call.name(),
        capture,
    }];
    let answer = ops::query_loaded(&query, &loaded);
    match answer.json() {
        Ok(text) => Reply::Json(text),
        Err(e) => failure("internal", e),
    }
}

/// What `hp_summary` asks for; `baseline` is another loaded handle to compare against.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SideName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slow: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<u32>,
    /// The name the capture is shown under in the report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// The §5 summary of a handle, as text.
pub fn summary(handle: u32, request: &str) -> Reply {
    let call: SummaryCall = match serde_json::from_str(request) {
        Ok(call) => call,
        Err(e) => return failure("bad_request", e.to_string()),
    };
    let side = match request::side(call.side) {
        Ok(side) => side,
        Err(e) => return failure("bad_request", e.0),
    };
    let loaded = match capture(handle) {
        Ok(capture) => capture,
        Err(reply) => return reply,
    };
    let baseline = match call.baseline {
        None => None,
        Some(baseline) => match capture(baseline) {
            Ok(other) => Some(Baseline {
                name: "baseline.hzperf".to_string(),
                capture: (*other).clone(),
            }),
            Err(reply) => return reply,
        },
    };
    let options = SummaryOptions {
        budget_tokens: call.budget.unwrap_or(DEFAULT_BUDGET),
        side,
        thread: call.thread,
        slow: call.slow,
        baseline,
    };
    let name = call.file.unwrap_or_else(|| "capture.hzperf".to_string());
    match ops::summary_capture(&loaded, &name, &options) {
        Ok(text) => Reply::Text(text),
        Err(e) => failure("bad_capture", e),
    }
}

/// What `hp_diff` asks for, as `horizon-perf diff`'s flags.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SideName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// What `hp_diff` returns: the text table and the JSON reply `diff --json` prints, as the MCP tool.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiffToolReply {
    pub text: String,
    pub reply: ops::DiffReply,
}

/// Compares capture B against A (A is the earlier run).
pub fn diff(a: u32, b: u32, request: &str) -> Reply {
    let call: DiffCall = match serde_json::from_str(request) {
        Ok(call) => call,
        Err(e) => return failure("bad_request", e.to_string()),
    };
    let side = match request::side(call.side) {
        Ok(side) => side,
        Err(e) => return failure("bad_request", e.0),
    };
    let (a, b) = match (capture(a), capture(b)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(reply), _) | (_, Err(reply)) => return reply,
    };
    let options = DiffOptions {
        side,
        thread: call.thread,
        min_ms: call.min_ms.unwrap_or(0.0),
        limit: call.limit.unwrap_or(crate::query::DEFAULT_LIMIT),
    };
    match ops::compare_captures(("a.hzperf", &a), ("b.hzperf", &b), &options) {
        Ok(answer) => {
            let reply = DiffToolReply {
                text: answer.text(),
                reply: answer.reply(),
            };
            serde_json::to_string(&reply)
                .map(Reply::Json)
                .unwrap_or_else(|e| failure("internal", e.to_string()))
        }
        Err(e) => failure("bad_capture", e),
    }
}

/// What `hp_export` asks for: the format name alone, or the format and `export`'s flags.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportCall {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SideName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Select>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slow: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ExportRequest {
    Name(String),
    Options(ExportCall),
}

impl ExportRequest {
    fn call(self) -> ExportCall {
        match self {
            ExportRequest::Name(kind) => ExportCall {
                kind,
                ..Default::default()
            },
            ExportRequest::Options(call) => call,
        }
    }
}

/// The bytes of an export of a handle.
pub fn export(handle: u32, request: &str) -> Reply {
    let request = match serde_json::from_str::<ExportRequest>(request) {
        Ok(request) => request,
        Err(e) => return failure("bad_request", e.to_string()),
    };
    let call = request.call();
    let format = match Format::from_str(&call.kind) {
        Ok(format) => format,
        Err(_) => return failure("unknown_format", Format::spellings()),
    };
    let side = match request::side(call.side) {
        Ok(side) => side,
        Err(e) => return failure("bad_request", e.0),
    };
    let capture = match capture(handle) {
        Ok(capture) => capture,
        Err(reply) => return reply,
    };
    let options = ExportOptions {
        side,
        thread: call.thread,
        select: call.select.unwrap_or(Select::Cpu),
        slow: call.slow,
    };
    match ops::export_capture(&capture, format, &options) {
        Ok(bytes) => Reply::Bytes(bytes),
        Err(e) => failure("no_export", e),
    }
}

/// Forgets a handle.
pub fn close(handle: u32) {
    CAPTURES.with(|captures| {
        captures.borrow_mut().remove(&handle);
    });
}

/// Forgets every handle: what a test starts from.
#[cfg(test)]
pub fn close_all() {
    CAPTURES.with(|captures| captures.borrow_mut().clear());
}

// ---------------------------------------------------------------------------------------------
// The C ABI. Every call is a thin wrapper: it copies the caller's bytes in, runs the typed
// function above, and hands back a reply buffer the caller frees.
// ---------------------------------------------------------------------------------------------

fn input<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        Some(&[])
    } else if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

fn text<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    std::str::from_utf8(input(ptr, len)?).ok()
}

/// Hands a reply buffer to the caller; `hp_free(ptr, 5 + payload_len)` gives it back.
fn reply_buffer(reply: Reply) -> *mut u8 {
    let buffer = reply.into_buffer().into_boxed_slice();
    Box::into_raw(buffer) as *mut u8
}

fn bad_pointer() -> Reply {
    failure("bad_argument", "the pointer is null")
}

/// Bytes the caller writes input into.
#[unsafe(no_mangle)]
pub extern "C" fn hp_alloc(len: usize) -> *mut u8 {
    let buffer = vec![0u8; len].into_boxed_slice();
    Box::into_raw(buffer) as *mut u8
}

/// Frees a buffer from `hp_alloc` or a reply.
///
/// # Safety
///
/// `ptr` must come from `hp_alloc` or a reply with exactly that length, and must not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hp_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

/// Decodes the capture in `[ptr, ptr+len)`; the reply's JSON holds its `handle`.
///
/// # Safety
///
/// `ptr` must be valid for `len` bytes, from `hp_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hp_open(ptr: *const u8, len: usize) -> *mut u8 {
    match input(ptr, len) {
        Some(bytes) => reply_buffer(open(bytes)),
        None => reply_buffer(bad_pointer()),
    }
}

/// Runs a query request (`QueryCall` JSON) against `handle`.
///
/// # Safety
///
/// `req` must be valid for `len` bytes, from `hp_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hp_query(handle: u32, req: *const u8, len: usize) -> *mut u8 {
    match text(req, len) {
        Some(request) => reply_buffer(query(handle, request)),
        None => reply_buffer(bad_pointer()),
    }
}

/// The summary of `handle` as text.
///
/// # Safety
///
/// `req` must be valid for `len` bytes, from `hp_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hp_summary(handle: u32, req: *const u8, len: usize) -> *mut u8 {
    match text(req, len) {
        Some(request) => reply_buffer(summary(handle, request)),
        None => reply_buffer(bad_pointer()),
    }
}

/// The statistics of B against A, as `{text, reply}`.
///
/// # Safety
///
/// `req` must be valid for `len` bytes, from `hp_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hp_diff(a: u32, b: u32, req: *const u8, len: usize) -> *mut u8 {
    match text(req, len) {
        Some(request) => reply_buffer(diff(a, b, request)),
        None => reply_buffer(bad_pointer()),
    }
}

/// The bytes of an export of `handle`.
///
/// # Safety
///
/// `req` must be valid for `len` bytes, from `hp_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hp_export(handle: u32, req: *const u8, len: usize) -> *mut u8 {
    match text(req, len) {
        Some(request) => reply_buffer(export(handle, request)),
        None => reply_buffer(bad_pointer()),
    }
}

/// Forgets a decoded capture.
#[unsafe(no_mangle)]
pub extern "C" fn hp_close(handle: u32) {
    close(handle);
}
