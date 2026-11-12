use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const CONTENT_TYPE: &str = "application/vnd.horizon.perf";

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dedicated,
    Integrated,
    Client,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CaptureSummary {
    pub kind: Kind,
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

/// Why a pin failed: the upload is gone from the server (expired or deleted), or anything else.
enum PinError {
    Gone(String),
    Other(String),
}

impl From<String> for PinError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl PinError {
    fn message(self) -> String {
        match self {
            Self::Gone(m) | Self::Other(m) => m,
        }
    }
}

fn response(result: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, String> {
    response_status(result).map_err(|(_, message)| message)
}

fn response_status(
    result: Result<ureq::Response, ureq::Error>,
) -> Result<ureq::Response, (Option<u16>, String)> {
    result.map_err(|error| match error {
        ureq::Error::Status(status, response) => {
            let message = match serde_json::from_reader::<_, ErrorReply>(
                response.into_reader().take(16 * 1024),
            ) {
                Ok(reply) => format!(
                    "upload API {status}: {} ({})",
                    reply.error.message, reply.error.code
                ),
                Err(_) => format!("upload API answered HTTP {status}"),
            };
            (Some(status), message)
        }
        other => (None, other.to_string()),
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

pub fn fetch(link: &str) -> Result<Vec<u8>, String> {
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
    Ok(bytes)
}

pub fn upload(endpoint: &str, bytes: &[u8], share: bool) -> Result<UploadReply, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("the capture exceeds 64 MiB".into());
    }
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Err("not a capture: a .hzperf file is gzip".into());
    }
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
        pin(endpoint, &mut upload).map_err(PinError::message)?;
    }
    Ok(upload)
}

fn pin(mut endpoint: Url, upload: &mut UploadReply) -> Result<(), PinError> {
    endpoint.set_path(&format!("/api/perf/captures/{}/share", upload.id));
    let result = response_status(
        agent()
            .post(endpoint.as_str())
            .set("Authorization", &format!("Bearer {}", upload.delete_token))
            .call(),
    )
    .map_err(|(status, message)| match status {
        Some(404 | 410) => PinError::Gone(message),
        _ => PinError::Other(message),
    })?;
    let shared: ShareReply =
        serde_json::from_reader(result.into_reader().take(64 * 1024)).map_err(|e| e.to_string())?;
    if shared.id != upload.id || !shared.shared {
        return Err(PinError::Other(
            "the share reply did not match the uploaded capture".into(),
        ));
    }
    let link = url(&shared.url)?;
    if link.origin() != endpoint.origin() || link.path() != format!("/perf/c/{}", upload.id) {
        return Err(PinError::Other(
            "the share link did not match the upload endpoint".into(),
        ));
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

/// The capture's fresh upload and its receipt, written over any old one; the receipt is private to its owner.
fn upload_with_receipt(
    endpoint: &Url,
    path: &Path,
    receipt_path: &Path,
) -> Result<Receipt, String> {
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
    let fresh = receipt_path.with_extension("json.new");
    let _ = std::fs::remove_file(&fresh);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let saved = options
        .open(&fresh)
        .map_err(|e| e.to_string())
        .and_then(|file| serde_json::to_writer(file, &receipt).map_err(|e| e.to_string()))
        .and_then(|()| std::fs::rename(&fresh, receipt_path).map_err(|e| e.to_string()));
    if let Err(e) = saved {
        let _ = std::fs::remove_file(&fresh);
        return Err(format!("could not save upload ownership: {e}"));
    }
    Ok(receipt)
}

/// Uploads and pins the capture at `path`, reusing the upload its receipt names; an upload the server no longer has is made again and its receipt replaced.
pub fn share_saved(endpoint: &str, path: &Path) -> Result<UploadReply, String> {
    let mut endpoint = url(endpoint)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    let receipt_path = path.with_extension("hzperf.share.json");
    let (mut receipt, reused) = match std::fs::File::open(&receipt_path) {
        Ok(file) => (
            serde_json::from_reader::<_, Receipt>(file.take(64 * 1024))
                .map_err(|e| e.to_string())?,
            true,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (upload_with_receipt(&endpoint, path, &receipt_path)?, false)
        }
        Err(e) => return Err(e.to_string()),
    };
    if receipt.endpoint != endpoint.as_str() {
        return Err("the saved upload belongs to another endpoint".into());
    }
    match pin(endpoint.clone(), &mut receipt.upload) {
        Ok(()) => {}
        Err(PinError::Gone(_)) if reused => {
            receipt = upload_with_receipt(&endpoint, path, &receipt_path)?;
            pin(endpoint, &mut receipt.upload).map_err(PinError::message)?;
        }
        Err(e) => return Err(e.message()),
    }
    Ok(receipt.upload)
}
