//! Open and Share for saved performance captures (`.hzperf`): the Horizon layer's side of
//! `horizon_platform::hooks::perf::Captures`, registered by `horizon-official`. Saving is the
//! engine's and needs none of this; nothing here reads telemetry consent.
#![forbid(unsafe_code)]

mod local;
mod share;

use std::path::Path;
use std::sync::OnceLock;

use horizon_platform::hooks::{Hooks, perf::Captures};

pub use local::{Offer, offer};
pub use share::{CaptureSummary, UploadReply, fetch, share_saved, upload};

/// The captures hook, for `horizon_platform::hooks::register`.
pub static HOOKS: PerfShare = PerfShare;

pub struct PerfShare;

fn env_or(var: &str, default: &str) -> String {
    match std::env::var(var) {
        Ok(url) if !url.trim().is_empty() => url.trim().trim_end_matches('/').to_owned(),
        _ => default.to_owned(),
    }
}

/// The viewer page (`HORIZON_PERF_VIEWER` overrides it).
pub fn viewer() -> &'static str {
    static VIEWER: OnceLock<String> = OnceLock::new();
    VIEWER.get_or_init(|| env_or("HORIZON_PERF_VIEWER", horizon_addresses::PERF_VIEWER))
}

/// The upload API (`HORIZON_PERF_CAPTURES` overrides it).
pub fn captures_url() -> String {
    env_or("HORIZON_PERF_CAPTURES", horizon_addresses::PERF_CAPTURES)
}

/// The system browser on `url`.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    open::that_detached(url).map_err(|e| e.to_string())
}

impl Captures for PerfShare {
    fn viewer(&self) -> &str {
        viewer()
    }

    fn open(&self, file: &Path) -> Result<(), String> {
        let offer = offer(file, viewer())?;
        open_in_browser(&offer.url).inspect_err(|_| offer.cancel())
    }

    fn share(&self, file: &Path) -> Result<String, String> {
        share_saved(&captures_url(), file).map(|reply| reply.url)
    }
}

impl Hooks for PerfShare {
    fn captures(&self) -> Option<&dyn Captures> {
        Some(self)
    }
}
