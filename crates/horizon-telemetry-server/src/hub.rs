//! The hub's votes: a 👍 on a roadmap item at playhorizon.dev/hub, and the helpers the hub's other
//! endpoints (`intake`) share. roadmap/README.md documents them for Kira and the website.
//!
//! - `POST /hub/vote`: a 👍 on a roadmap item, one per browser (`voter`, a random id the page makes and
//!   keeps); `"up": false` takes it back. Answers the item's count.
//! - `GET /hub/votes`: every item's count, cached a minute.
//!
//! Stored: an item id and the browser's random id per vote. Never an IP address: the rate limits key their
//! buckets by a hash of it with a key drawn at start, in memory.
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use rusqlite::params;
use serde_json::{Map, Value, json};

use crate::store::Store;
use crate::{App, json_response, peer, take};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS hub_votes (item TEXT NOT NULL, voter TEXT NOT NULL, PRIMARY KEY (item, voter)) WITHOUT ROWID;
";

/// Items with votes, at most: a vote for a new id past this is refused (ids are only checked for shape).
const MAX_ITEMS: i64 = 1000;
/// The largest body accepted: a report's fields at their limits, as JSON, fit.
const MAX_BODY: usize = 24 * 1024;
const VOTES_FOR: Duration = Duration::from_secs(60);

/// The hub's buckets: keyed by a keyed hash of the address, never the address itself.
pub type HashedBuckets = HashMap<u64, (f64, Instant)>;

/// A roadmap item id's shape: lowercase words joined by dashes (tools/roadmap.py), at most 48 characters.
pub fn item_id(id: &str) -> bool {
    (1..=48).contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A voter id's shape: what `crypto.randomUUID()` and similar make.
fn voter_id(id: &str) -> bool {
    (16..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Store {
    /// Adds or takes back a vote; the item's count after it. None when the item would be one too many.
    pub fn vote(&mut self, item: &str, voter: &str, up: bool) -> rusqlite::Result<Option<i64>> {
        let tx = self.db.transaction()?;
        if up {
            let known = tx
                .prepare("SELECT 1 FROM hub_votes WHERE item = ?1 LIMIT 1")?
                .exists([item])?;
            let items: i64 =
                tx.query_row("SELECT count(DISTINCT item) FROM hub_votes", [], |r| {
                    r.get(0)
                })?;
            if !known && items >= MAX_ITEMS {
                return Ok(None);
            }
            tx.execute(
                "INSERT OR IGNORE INTO hub_votes (item, voter) VALUES (?1, ?2)",
                params![item, voter],
            )?;
        } else {
            tx.execute(
                "DELETE FROM hub_votes WHERE item = ?1 AND voter = ?2",
                params![item, voter],
            )?;
        }
        let count = tx.query_row(
            "SELECT count(*) FROM hub_votes WHERE item = ?1",
            [item],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(Some(count))
    }

    /// Every item's count.
    pub fn votes(&self) -> rusqlite::Result<Map<String, Value>> {
        let mut statement = self
            .db
            .prepare("SELECT item, count(*) FROM hub_votes GROUP BY item ORDER BY item")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.map(|row| row.map(|(item, n)| (item, Value::from(n))))
            .collect()
    }
}

impl App {
    pub(crate) fn hashed(&self, req: &Request) -> u64 {
        self.hub_key.hash_one(self.client(req.headers(), peer(req)))
    }

    /// Takes a token from the address's hub bucket (votes and reads of the counts).
    fn allow_hub(&self, req: &Request) -> bool {
        take(
            &self.hub_buckets,
            self.config.hub_per_hour,
            self.hashed(req),
        )
    }

    /// The hub's share of `maintain`: idle buckets are forgotten.
    pub(crate) fn forget_hub_buckets(&self) {
        for buckets in [&self.hub_buckets, &self.hub_post_buckets] {
            buckets
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|_, (_, at)| at.elapsed() < Duration::from_secs(3600));
        }
    }
}

/// A JSON answer to the website: its origin may read it, nothing caches it.
pub(crate) fn answer(app: &App, code: StatusCode, body: Value) -> Response {
    let mut response = json_response(app, body.to_string(), 0);
    *response.status_mut() = code;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(crate) fn refuse(app: &App, code: StatusCode, message: &str) -> Response {
    answer(app, code, json!({ "error": message }))
}

/// A browser's preflight for the JSON posts: the website may post them.
pub async fn preflight(State(app): State<Arc<App>>) -> Response {
    let mut response = answer(&app, StatusCode::NO_CONTENT, Value::Null);
    *response.body_mut() = axum::body::Body::empty();
    let headers = response.headers_mut();
    headers.remove(header::CONTENT_TYPE);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    response
}

pub(crate) async fn json_body(req: Request) -> Result<Value, &'static str> {
    let bytes = axum::body::to_bytes(req.into_body(), MAX_BODY)
        .await
        .map_err(|_| "too large")?;
    serde_json::from_slice(&bytes).map_err(|_| "send JSON")
}

/// `POST /hub/vote` `{"item", "voter", "up"}`: 200 `{"item", "votes"}`.
pub async fn vote(State(app): State<Arc<App>>, req: Request) -> Response {
    if !app.allow_hub(&req) {
        return refuse(&app, StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    let body = match json_body(req).await {
        Ok(body) => body,
        Err(e) => return refuse(&app, StatusCode::BAD_REQUEST, e),
    };
    let item = body["item"].as_str().unwrap_or("");
    let voter = body["voter"].as_str().unwrap_or("");
    if !item_id(item) || !voter_id(voter) {
        return refuse(&app, StatusCode::BAD_REQUEST, "an item id and a voter id");
    }
    let up = body["up"].as_bool().unwrap_or(true);
    // SQLite runs on the blocking pool, never on the two async workers (crate::blocking).
    let (item, voter) = (item.to_owned(), voter.to_owned());
    let counted = {
        let item = item.clone();
        match crate::blocking(&app, move |app| app.store().vote(&item, &voter, up)).await {
            Ok(counted) => counted,
            Err(_) => return refuse(&app, StatusCode::SERVICE_UNAVAILABLE, "try again later"),
        }
    };
    match counted {
        Ok(Some(votes)) => {
            *app.votes_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
            answer(
                &app,
                StatusCode::OK,
                json!({ "item": item, "votes": votes }),
            )
        }
        Ok(None) => refuse(&app, StatusCode::BAD_REQUEST, "no such item"),
        Err(e) => {
            eprintln!("horizon-telemetry-server: could not store a vote: {e}");
            refuse(&app, StatusCode::SERVICE_UNAVAILABLE, "try again later")
        }
    }
}

/// `GET /hub/votes`: `{"votes": {"<item>": n}}`, made again at most every minute.
pub async fn votes(State(app): State<Arc<App>>, req: Request) -> Response {
    if !app.allow_hub(&req) {
        return refuse(&app, StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    let cached = app
        .votes_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|(at, _)| at.elapsed() < VOTES_FOR)
        .map(|(_, json)| json.clone());
    let json = match cached {
        Some(json) => json,
        // Counted on the blocking pool, with the cache's lock released: a handler waiting for the
        // store holds nothing another one needs (crate::blocking).
        None => match crate::blocking(&app, |app| app.store().votes()).await {
            Err(_) => return refuse(&app, StatusCode::SERVICE_UNAVAILABLE, "try again later"),
            Ok(Ok(counts)) => {
                let json = json!({ "votes": counts }).to_string();
                *app.votes_cache.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((Instant::now(), json.clone()));
                json
            }
            Ok(Err(e)) => {
                eprintln!("horizon-telemetry-server: could not read the votes: {e}");
                return refuse(&app, StatusCode::SERVICE_UNAVAILABLE, "try again later");
            }
        },
    };
    json_response(&app, json, VOTES_FOR.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_checked_for_shape() {
        assert!(item_id("smooth-frames") && item_id("v2"));
        for bad in ["", "-a", "a-", "a--b", "Smooth", "a b", &"a".repeat(49)] {
            assert!(!item_id(bad), "{bad}");
        }
        assert!(voter_id("0b7c3c9e-3f0a-4d8e-9a39-4c1f5a6b7d21") && !voter_id("short"));
    }
}
