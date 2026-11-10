use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const CONTENT_TYPE: &str = "application/vnd.horizon.perf";
pub const DEFAULT_UPLOAD_URL: &str = "https://playsunrise.net/api/perf/captures";

#[derive(Debug, Deserialize, Serialize)]
pub struct CaptureSummary {
    pub kind: crate::wire::KindName,
    pub seconds: f64,
    pub version: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UploadReply {
    pub id: String,
    pub url: String,
    pub delete_token: String,
    pub shared: bool,
    pub expires_utc: String,
    pub bytes: u64,
    pub summary: CaptureSummary,
}

#[derive(Debug, Deserialize)]
struct ShareReply {
    id: String,
    url: String,
    shared: bool,
    expires_utc: String,
}

#[derive(Debug, Deserialize)]
struct ErrorReply {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent("Horizon/0.1")
        .timeout(Duration::from_secs(60))
        .redirects(0)
        .build()
}

fn response(result: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, String> {
    result.map_err(|error| match error {
        ureq::Error::Status(status, response) => {
            match serde_json::from_reader::<_, ErrorReply>(response.into_reader().take(16 * 1024)) {
                Ok(reply) => format!(
                    "upload API {status}: {} ({})",
                    reply.error.message, reply.error.code
                ),
                Err(_) => format!("upload API answered HTTP {status}"),
            }
        }
        other => other.to_string(),
    })
}

fn url(text: &str) -> Result<Url, String> {
    let url = Url::parse(text).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("capture URLs must use HTTP or HTTPS without credentials".into());
    }
    Ok(url)
}

pub fn download(link: &str) -> Result<Vec<u8>, String> {
    let mut target = url(link)?;
    let id = target
        .path()
        .strip_prefix("/perf/c/")
        .or_else(|| target.path().strip_prefix("/api/perf/captures/"))
        .ok_or("give a /perf/c/<id> capture link")?
        .trim_end_matches('/')
        .to_string();
    if id.len() != 10
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    {
        return Err("invalid capture id".into());
    }
    target.set_path(&format!("/api/perf/captures/{id}"));
    target.set_query(None);
    target.set_fragment(None);
    let reply = response(agent().get(target.as_str()).call())?;
    let content_type = reply
        .header("Content-Type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if content_type != CONTENT_TYPE && content_type != "application/octet-stream" {
        return Err("the link did not return a capture".into());
    }
    let mut bytes = Vec::new();
    reply
        .into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("the capture exceeds 64 MiB".into());
    }
    crate::read_capture(&bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

pub fn upload(endpoint: &str, bytes: &[u8], share: bool) -> Result<UploadReply, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("the capture exceeds 64 MiB".into());
    }
    crate::read_capture(bytes).map_err(|e| e.to_string())?;
    let mut endpoint = url(endpoint)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    let result = response(
        agent()
            .post(endpoint.as_str())
            .set("Content-Type", CONTENT_TYPE)
            .send_bytes(bytes),
    )?;
    let mut upload: UploadReply =
        serde_json::from_reader(result.into_reader().take(64 * 1024)).map_err(|e| e.to_string())?;
    if share {
        pin(endpoint, &mut upload)?;
    }
    Ok(upload)
}

fn pin(mut endpoint: Url, upload: &mut UploadReply) -> Result<(), String> {
    endpoint.set_path(&format!("/api/perf/captures/{}/share", upload.id));
    let result = response(
        agent()
            .post(endpoint.as_str())
            .set("Authorization", &format!("Bearer {}", upload.delete_token))
            .call(),
    )?;
    let shared: ShareReply =
        serde_json::from_reader(result.into_reader().take(64 * 1024)).map_err(|e| e.to_string())?;
    if shared.id != upload.id || !shared.shared {
        return Err("the share reply did not match the uploaded capture".into());
    }
    let link = url(&shared.url)?;
    if link.origin() != endpoint.origin() || link.path() != format!("/perf/c/{}", upload.id) {
        return Err("the share link did not match the upload endpoint".into());
    }
    upload.url = shared.url;
    upload.shared = true;
    upload.expires_utc = shared.expires_utc;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    endpoint: String,
    upload: UploadReply,
}

pub fn share_saved(endpoint: &str, path: &Path) -> Result<UploadReply, String> {
    let mut endpoint = url(endpoint)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    let receipt_path = path.with_extension("hzperf.share.json");
    let mut receipt: Receipt = match std::fs::File::open(&receipt_path) {
        Ok(file) => serde_json::from_reader(file.take(64 * 1024)).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let receipt = Receipt {
                endpoint: endpoint.to_string(),
                upload: upload(endpoint.as_str(), &bytes, false)?,
            };
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options
                .open(&receipt_path)
                .map_err(|e| format!("could not save upload ownership: {e}"))?;
            serde_json::to_writer(file, &receipt).map_err(|e| e.to_string())?;
            receipt
        }
        Err(e) => return Err(e.to_string()),
    };
    if receipt.endpoint != endpoint.as_str() {
        return Err("the saved upload belongs to another endpoint".into());
    }
    pin(endpoint, &mut receipt.upload)?;
    Ok(receipt.upload)
}
