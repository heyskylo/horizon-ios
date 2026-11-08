use std::fs;
use std::io::Write;

use serde::Deserialize;

use super::{CliError, Env, failed, parse, resolve};
use crate::read;

const UPLOAD_FLAGS: [super::Flag; 1] = [super::Flag::Dir];

#[derive(Deserialize)]
struct UploadReply {
    id: String,
    delete_token: String,
}

#[derive(Deserialize)]
struct ShareReply {
    url: String,
}

fn base_url() -> String {
    std::env::var("PERF_UPLOAD_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:3000".to_string())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("horizon-perf")
        .build()
}

#[allow(clippy::collapsible_if)]
fn err_from(e: ureq::Error) -> CliError {
    match e {
        ureq::Error::Status(status, response) => {
            let body = response.into_string().unwrap_or_default();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(err) = value.get("error") {
                    let code = err
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let message = err
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    return failed(format!("{status} {code}: {message}"));
                }
            }
            failed(format!("{status}: {body}"))
        }
        other => failed(other),
    }
}

pub(super) fn run(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &UPLOAD_FLAGS)?;
    let Some(file) = parsed.one_file()? else {
        return Err(CliError::Usage("share needs a FILE".into()));
    };
    let dir = parsed.flags.get(&super::Flag::Dir).map(String::as_str);
    let path = resolve(Some(file), dir, env, true)?;

    let bytes = fs::read(&path).map_err(failed)?;
    read::read_capture(&bytes).map_err(failed)?;

    let base = base_url();
    let upload_url = format!("{}/api/perf/captures", base.trim_end_matches('/'));

    let response = agent()
        .post(&upload_url)
        .set("content-type", "application/vnd.horizon.perf")
        .send_bytes(&bytes)
        .map_err(err_from)?;
    let value: serde_json::Value =
        serde_json::from_str(&response.into_string().map_err(failed)?).map_err(failed)?;
    let upload: UploadReply =
        serde_json::from_value(value).map_err(failed)?;

    let share_url = format!(
        "{}/api/perf/captures/{}/share",
        base.trim_end_matches('/'),
        upload.id
    );
    let response = agent()
        .post(&share_url)
        .set("authorization", &format!("Bearer {}", upload.delete_token))
        .call()
        .map_err(err_from)?;
    let value: serde_json::Value =
        serde_json::from_str(&response.into_string().map_err(failed)?).map_err(failed)?;
    let shared: ShareReply = serde_json::from_value(value).map_err(failed)?;

    writeln!(out, "{}", shared.url).map_err(failed)?;
    writeln!(out, "{}", upload.delete_token).map_err(failed)?;
    Ok(())
}
