//! `horizon-mod-registry`: the submission form's service at
//! `playhorizon.dev/api/mods/*` (docs/api/mod-index.md). A creator signs in
//! with GitHub, sends a file and its page; the service runs the file's own
//! checks, uploads it to the registry's `staging` release and opens a pull
//! request with the entry. Reviewers decide; merging publishes. Signed-in
//! players also rate published projects (`ratings`, its only data: sessions
//! are signed cookies, and GitHub holds everything else).
mod archive;
pub mod github;
pub mod page;
pub mod ratings;

use axum::{
    Json, Router,
    body::Body,
    extract::{
        DefaultBodyLimit, FromRequest, Multipart, Path as UrlPath, Query, Request, State,
        multipart::MultipartError,
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use github::GitHub;
use hmac::{Hmac, Mac};
use horizon_mods::index::{
    self, Kind,
    registry::{self, Entry, EntryVersion, Sources},
};
use horizon_release_sign::native_build::{self, Mod, Request as BuildRequest};
use ratings::Ratings;
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How long a sign-in lasts.
const SESSION: u64 = 12 * 3600;
/// Submissions per account per hour, and from everyone.
const PER_ACCOUNT: usize = 6;
const PER_HOUR: usize = 60;
/// The page's JSON.
const MAX_FORM: usize = 64 << 10;
/// A submission's whole body: the file, the page, its images and the form's framing.
const MAX_BODY: usize = index::MAX_FILE as usize + page::MAX_IMAGES + MAX_FORM * 2;
/// Ratings set per account per hour, and by everyone.
const RATINGS_PER_ACCOUNT: usize = 30;
const RATINGS_PER_HOUR: usize = 600;
/// Maximum mods allowed in a single prebuilt request.
const MAX_SET_MODS: usize = 64;
/// Native build requests per address per hour, and from everyone.
const BUILDS_PER_ADDRESS: usize = 12;
const BUILDS_PER_HOUR: usize = 120;
/// Status polls per address per hour, and from everyone: each one costs a GitHub API call.
const POLLS_PER_ADDRESS: usize = 120;
const POLLS_PER_HOUR: usize = 1200;
/// How long a dispatched native build is deduplicated.
const DISPATCH_DEDUP_WINDOW: Duration = Duration::from_secs(15 * 60);
/// How long the ratings' totals are served from memory (and cached by
/// browsers and the launcher).
const TOTALS_FOR: Duration = Duration::from_secs(60);

pub struct Config {
    /// `https://playhorizon.dev`: where the form is, and the only origin it
    /// takes submissions from.
    pub public_url: String,
    /// At least 32 bytes: signs sessions.
    pub session_key: Vec<u8>,
    pub github: GitHub,
    /// `REGISTRY_UPDATE_KEY`: signs an owner update's trailer; None turns the
    /// feature off (every update waits for a reviewer).
    pub update_key: Option<Vec<u8>>,
    /// Where files wait during a submission.
    pub scratch: PathBuf,
    /// The ratings' SQLite file (a volume): `:memory:` keeps them in memory.
    pub database: PathBuf,
}

impl Config {
    /// `REGISTRY_PUBLIC_URL`, `REGISTRY_SESSION_KEY`, `REGISTRY_REPO`,
    /// `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, `GITHUB_BOT_TOKEN`,
    /// `REGISTRY_UPDATE_KEY` (optional), `REGISTRY_DB` (default
    /// `/data/registry.sqlite3`).
    pub fn from_env() -> Result<Config, String> {
        let var = |name: &str| std::env::var(name).map_err(|_| format!("{name} is not set"));
        let key = var("REGISTRY_SESSION_KEY")?;
        if key.len() < 32 {
            return Err("REGISTRY_SESSION_KEY needs at least 32 characters".into());
        }
        Ok(Config {
            public_url: var("REGISTRY_PUBLIC_URL")?
                .trim()
                .trim_end_matches('/')
                .to_owned(),
            session_key: key.into_bytes(),
            github: GitHub::new(
                github::Endpoints::default(),
                std::env::var("REGISTRY_REPO")
                    .unwrap_or_else(|_| "horizon-rust/horizon-mods".into()),
                var("GITHUB_CLIENT_ID")?,
                var("GITHUB_CLIENT_SECRET")?,
                var("GITHUB_BOT_TOKEN")?,
            ),
            update_key: std::env::var("REGISTRY_UPDATE_KEY")
                .ok()
                .filter(|k| !k.is_empty())
                .map(String::into_bytes),
            scratch: std::env::temp_dir().join("horizon-mod-registry"),
            database: std::env::var_os("REGISTRY_DB")
                .map_or_else(|| PathBuf::from("/data/registry.sqlite3"), PathBuf::from),
        })
    }

    fn callback(&self) -> String {
        format!("{}/api/mods/auth/callback", self.public_url)
    }
}

#[derive(Clone)]
pub struct App(Arc<Inner>);

type Windows = Mutex<HashMap<String, VecDeque<Instant>>>;

struct Inner {
    config: Config,
    /// Submissions in the last hour, by account ("" for everyone's).
    recent: Windows,
    /// Ratings set in the last hour, the same way.
    rated: Windows,
    /// Native build requests in the last hour, by client address ("" for everyone's).
    build_windows: Windows,
    /// Deduplication window for recently dispatched native builds (key -> dispatched instant).
    dispatched: Mutex<HashMap<String, Instant>>,
    /// Status polls in the last hour, by client address ("" for everyone's).
    poll_windows: Windows,
    ratings: Mutex<Ratings>,
    /// `GET /api/mods/ratings`' body, and when it was made.
    totals: Mutex<Option<(Instant, String)>>,
}

impl App {
    /// Fails when the ratings' file can't be opened.
    pub fn new(config: Config) -> Result<App, String> {
        let ratings = Ratings::open(&config.database)
            .map_err(|e| format!("{}: {e}", config.database.display()))?;
        Ok(App(Arc::new(Inner {
            config,
            recent: Mutex::default(),
            rated: Mutex::default(),
            build_windows: Mutex::default(),
            dispatched: Mutex::default(),
            poll_windows: Mutex::default(),
            ratings: Mutex::new(ratings),
            totals: Mutex::default(),
        })))
    }
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/mods/auth/login", get(login))
        .route("/api/mods/auth/callback", get(callback))
        .route("/api/mods/auth/logout", post(logout))
        .route("/api/mods/me", get(me))
        .route("/api/mods/ratings", get(totals))
        .route("/api/mods/{id}/rating", get(my_rating).post(rate))
        .route(
            "/api/mods/submit",
            post(submit).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route(
            "/api/native-builds",
            post(request_native_build).get(get_native_build),
        )
        .route(
            "/api/mods/native-builds",
            post(request_native_build).get(get_native_build),
        )
        .with_state(app)
}

/// An error players read in the form.
#[derive(Debug)]
pub struct Refusal(StatusCode, String);

impl Refusal {
    fn new(status: StatusCode, message: impl Into<String>) -> Refusal {
        Refusal(status, message.into())
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(StatusCode::BAD_REQUEST, message)
}

fn github_down(e: github::Failure) -> Refusal {
    eprintln!("horizon-mod-registry: GitHub {}: {}", e.status, e.message);
    if e.timed_out {
        return Refusal::new(
            StatusCode::GATEWAY_TIMEOUT,
            "GitHub is slow right now: try again in a few minutes",
        );
    }
    Refusal::new(
        StatusCode::BAD_GATEWAY,
        "GitHub didn't answer as expected: try again in a minute",
    )
}

// Signed values: `<payload>.<HMAC-SHA256>`, both base64url.

fn sign(key: &[u8], payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(payload.as_bytes());
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

fn unsign(key: &[u8], value: &str) -> Option<String> {
    let (payload, tag) = value.split_once('.')?;
    let payload = String::from_utf8(URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).ok()?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&URL_SAFE_NO_PAD.decode(tag).ok()?).ok()?;
    Some(payload)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn random_hex() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
}

fn set_cookie(name: &str, value: &str, max_age: u64) -> String {
    format!("{name}={value}; Path=/api/mods; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age}")
}

/// The signed-in account: (login, id).
fn session(app: &Inner, headers: &HeaderMap) -> Option<(String, u64)> {
    let payload = unsign(&app.config.session_key, &cookie(headers, "hm_session")?)?;
    let mut parts = payload.split('\n');
    let (login, id, until) = (parts.next()?, parts.next()?, parts.next()?);
    (until.parse::<u64>().ok()? > now()).then(|| (login.to_owned(), id.parse().unwrap_or(0)))
}

/// The form sends this with each submission: a page on another site can't
/// read it, so it can't submit in a signed-in creator's name.
fn csrf(app: &Inner, headers: &HeaderMap) -> Option<String> {
    let session = cookie(headers, "hm_session")?;
    let signed = sign(&app.config.session_key, &format!("csrf\n{session}"));
    Some(signed.rsplit('.').next()?.chars().take(32).collect())
}

#[derive(Deserialize)]
struct Login {
    /// The site's page to come back to: `/mods`, `/mods/submit` (the
    /// default) or `/mods/<id>`.
    back: Option<String>,
}

/// Whether `path` is a registry page a sign-in may come back to.
fn registry_page(path: &str) -> bool {
    match path.strip_prefix("/mods") {
        Some("") => true,
        Some(rest) => rest.strip_prefix('/').is_some_and(index::valid_id) || rest == "/submit",
        None => false,
    }
}

async fn login(State(app): State<App>, Query(q): Query<Login>) -> Response {
    let state = random_hex();
    let back = q
        .back
        .filter(|b| registry_page(b))
        .unwrap_or_else(|| "/mods/submit".into());
    let signed = sign(&app.0.config.session_key, &format!("{state}\n{back}"));
    let url = app
        .0
        .config
        .github
        .authorize_url(&app.0.config.callback(), &state);
    (
        [(header::SET_COOKIE, set_cookie("hm_state", &signed, 600))],
        Redirect::to(&url),
    )
        .into_response()
}

#[derive(Deserialize)]
struct Callback {
    code: String,
    state: String,
}

async fn callback(
    State(app): State<App>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Result<Response, Refusal> {
    let started = cookie(&headers, "hm_state").and_then(|v| unsign(&app.0.config.session_key, &v));
    let Some((_, back)) = started
        .as_deref()
        .and_then(|s| s.split_once('\n'))
        .filter(|(state, _)| *state == q.state)
    else {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            "the sign-in expired or didn't start here: try again",
        ));
    };
    let inner = app.0.clone();
    let (login, id) = tokio::task::spawn_blocking(move || {
        inner
            .config
            .github
            .sign_in(&q.code, &inner.config.callback())
    })
    .await
    .map_err(|_| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed"))?
    .map_err(github_down)?;
    let value = sign(
        &app.0.config.session_key,
        &format!("{login}\n{id}\n{}", now() + SESSION),
    );
    let back = format!("{}{back}", app.0.config.public_url);
    let mut response = Redirect::to(&back).into_response();
    let headers = response.headers_mut();
    headers.append(
        header::SET_COOKIE,
        set_cookie("hm_session", &value, SESSION).parse().unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        set_cookie("hm_state", "", 0).parse().unwrap(),
    );
    Ok(response)
}

async fn logout() -> Response {
    (
        [(header::SET_COOKIE, set_cookie("hm_session", "", 0))],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

async fn me(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Refusal> {
    let (login, _) = session(&app.0, &headers)
        .ok_or_else(|| Refusal::new(StatusCode::UNAUTHORIZED, "not signed in"))?;
    Ok(Json(
        json!({"login": login, "csrf": csrf(&app.0, &headers)}),
    ))
}

fn database(e: rusqlite::Error) -> Refusal {
    eprintln!("horizon-mod-registry: ratings: {e}");
    Refusal::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "the ratings can't be read right now",
    )
}

/// A mean as the pages show it: two decimals.
fn rounded(average: f64) -> f64 {
    (average * 100.0).round() / 100.0
}

/// `GET /api/mods/ratings`: `{"projects": {"<id>": {"average", "count"}}}` for
/// every rated project. Totals only: never who rated.
async fn totals(State(app): State<App>) -> Result<Response, Refusal> {
    let mut cached = app.0.totals.lock().unwrap();
    let body = match cached.as_ref().filter(|(at, _)| at.elapsed() < TOTALS_FOR) {
        Some((_, body)) => body.clone(),
        None => {
            let all = app
                .0
                .ratings
                .lock()
                .unwrap()
                .summaries()
                .map_err(database)?;
            let projects: serde_json::Map<String, serde_json::Value> = all
                .into_iter()
                .map(|(id, s)| (id, json!({"average": rounded(s.average), "count": s.count})))
                .collect();
            let body = json!({ "projects": projects }).to_string();
            *cached = Some((Instant::now(), body.clone()));
            body
        }
    };
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        body,
    )
        .into_response())
}

/// `GET /api/mods/{id}/rating`: the signed-in account's own rating,
/// `{"stars": 1..5}` or `{"stars": null}`.
async fn my_rating(
    State(app): State<App>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<serde_json::Value>, Refusal> {
    let (_, account) = session(&app.0, &headers)
        .ok_or_else(|| Refusal::new(StatusCode::UNAUTHORIZED, "not signed in"))?;
    let stars = app
        .0
        .ratings
        .lock()
        .unwrap()
        .mine(&id, account)
        .map_err(database)?;
    Ok(Json(json!({ "stars": stars })))
}

#[derive(Deserialize)]
struct Rating {
    stars: u8,
}

/// `POST /api/mods/{id}/rating` with `{"stars": 1..5}`: sets or changes the
/// signed-in account's rating of a published project, not its own. Answers
/// the project's new totals and `stars`.
async fn rate(
    State(app): State<App>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
    Json(rating): Json<Rating>,
) -> Result<Json<serde_json::Value>, Refusal> {
    let inner = app.0.clone();
    let (login, account) = signed_in_here(&inner, &headers)?;
    if !(1..=5).contains(&rating.stars) {
        return Err(bad("a rating is 1 to 5 stars"));
    }
    if !index::valid_id(&id) {
        return Err(Refusal::new(StatusCode::NOT_FOUND, "no such project"));
    }
    if !allow(&inner.rated, &login, RATINGS_PER_ACCOUNT, RATINGS_PER_HOUR) {
        return Err(Refusal::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many ratings this hour: try again later",
        ));
    }
    let lookup = inner.clone();
    let wanted = id.clone();
    let entry = tokio::task::spawn_blocking(move || lookup.config.github.entry(&wanted))
        .await
        .map_err(|_| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, "the check crashed"))?
        .map_err(github_down)?;
    let Some((entry, _)) = entry else {
        return Err(Refusal::new(
            StatusCode::NOT_FOUND,
            format!("{id} isn't published in the registry"),
        ));
    };
    if ownership(&entry, &login, account) != Ownership::None {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            "you publish this project: its players rate it",
        ));
    }
    let summary = {
        let ratings = inner.ratings.lock().unwrap();
        ratings
            .rate(&id, account, rating.stars, now())
            .map_err(database)?;
        ratings.summary(&id).map_err(database)?
    };
    *inner.totals.lock().unwrap() = None;
    let (average, count) = summary.map_or((0.0, 0), |s| (rounded(s.average), s.count));
    Ok(Json(
        json!({"stars": rating.stars, "average": average, "count": count}),
    ))
}

/// The page a submission fills in: a project's fields, and the version's.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub license: String,
    /// The code's address.
    #[serde(default)]
    pub source: String,
    /// Refused: the registry no longer follows repositories (every version is a submission).
    /// Kept so an old form's listing gets a clear answer.
    #[serde(default)]
    pub repo: Option<String>,
    /// Refused: icons are uploaded (the `icon` part) and kept in the registry.
    #[serde(default)]
    pub icon: Option<String>,
    /// 1 to 3 of the kind's categories.
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The project's screenshots to keep, in order (`screenshots/<id>/<name>` of its entry);
    /// uploaded ones come after them. Absent: all of them.
    #[serde(default)]
    pub screenshots: Option<Vec<String>>,
    /// A `shaderpack`'s submitter says they made it and its licence lets the registry hand it out.
    #[serde(default)]
    pub own_work: bool,
    /// Changes the page only, with no file ("Edit project").
    #[serde(default)]
    pub edit: bool,
    /// A new project's minisign public key.
    #[serde(default)]
    pub key: Option<String>,
    /// The `.minisig` text.
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub changelog: String,
}

fn take(window: &mut VecDeque<Instant>, max: usize) -> bool {
    while window
        .front()
        .is_some_and(|t| t.elapsed() > Duration::from_secs(3600))
    {
        window.pop_front();
    }
    window.len() < max
}

/// The origin a URL names: `scheme://host[:port]`, without a path. An
/// `Origin` is a page's origin and never carries a path, while
/// `REGISTRY_PUBLIC_URL` may: the form's pages are below the site's root.
fn url_origin(url: &str) -> &str {
    let host = url.find("://").map_or(0, |scheme| scheme + 3);
    url[host..]
        .find('/')
        .map_or(url, |path| &url[..host + path])
}

/// Whether a change comes from the site's own pages: its `Origin` is the
/// origin of `REGISTRY_PUBLIC_URL`, which is what its pages are. A page on
/// another site can't send its origin, so this is a whole origin's match,
/// case folded the way a host name is.
fn from_the_site(public_url: &str, origin: Option<&str>) -> bool {
    origin.is_some_and(|origin| origin.eq_ignore_ascii_case(url_origin(public_url)))
}

/// A change sent by the site's own pages: from its origin, signed in, with
/// the session's CSRF token. The account: (login, id).
fn signed_in_here(app: &Inner, headers: &HeaderMap) -> Result<(String, u64), Refusal> {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if !from_the_site(&app.config.public_url, origin) {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            "send it from the registry's pages",
        ));
    }
    let account = session(app, headers)
        .ok_or_else(|| Refusal::new(StatusCode::UNAUTHORIZED, "sign in with GitHub first"))?;
    let sent = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());
    if sent.is_none() || sent.map(str::to_owned) != csrf(app, headers) {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            "reload the page and try again",
        ));
    }
    Ok(account)
}

/// Counts one more for `login` and for everyone, unless either is at its
/// hourly limit.
fn allow(windows: &Windows, login: &str, per_account: usize, per_hour: usize) -> bool {
    let mut windows = windows.lock().unwrap();
    let account = login.to_lowercase();
    let ok = take(windows.entry(String::new()).or_default(), per_hour)
        && take(windows.entry(account.clone()).or_default(), per_account);
    if ok {
        let at = Instant::now();
        windows.entry(String::new()).or_default().push_back(at);
        windows.entry(account).or_default().push_back(at);
    }
    ok
}

/// The answer to a file that is too big.
fn too_big() -> Refusal {
    Refusal::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("the file is over {} MiB", index::MAX_FILE >> 20),
    )
}

/// A form that broke off: over the body's limit, or not a form.
fn torn(e: MultipartError) -> Refusal {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        too_big()
    } else {
        bad(e.body_text())
    }
}

/// Reads and drops the rest of a refused submission, up to its limit. A
/// browser still sending the file when the answer comes reports a network
/// error instead of the answer, so the answer waits for the upload's end.
async fn drain(body: Body) {
    use http_body_util::BodyExt;
    let mut body = http_body_util::Limited::new(body, MAX_BODY);
    while let Some(Ok(_)) = body.frame().await {}
}

/// The rest of a form, past a part that was refused.
async fn drain_form(multipart: &mut Multipart) {
    while let Ok(Some(mut field)) = multipart.next_field().await {
        while let Ok(Some(_)) = field.chunk().await {}
    }
}

async fn submit(
    State(app): State<App>,
    request: Request,
) -> Result<Json<serde_json::Value>, Refusal> {
    let inner = app.0.clone();
    let (parts, body) = request.into_parts();
    let checked = signed_in_here(&inner, &parts.headers).and_then(|account| {
        let length = parts.headers.get(header::CONTENT_LENGTH);
        if length.and_then(|l| l.to_str().ok()?.parse::<usize>().ok()) > Some(MAX_BODY) {
            return Err(too_big());
        }
        if !allow(&inner.recent, &account.0, PER_ACCOUNT, PER_HOUR) {
            return Err(Refusal::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too many submissions this hour: try again later",
            ));
        }
        Ok(account)
    });
    let (login, user_id) = match checked {
        Ok(account) => account,
        Err(refusal) => {
            // Too big: what's past the limit would be cut anyway.
            if refusal.0 != StatusCode::PAYLOAD_TOO_LARGE {
                drain(body).await;
            }
            return Err(refusal);
        }
    };
    let mut multipart = Multipart::from_request(Request::from_parts(parts, body), &())
        .await
        .map_err(|e| bad(e.body_text()))?;
    let dir = inner.config.scratch.join(random_hex());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        drain_form(&mut multipart).await;
        return Err(Refusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        ));
    }
    let result = receive(&mut multipart, &dir).await;
    if result.is_err() {
        drain_form(&mut multipart).await;
    }
    let result = match result {
        Ok((form, file, images)) => {
            let inner = inner.clone();
            tokio::task::spawn_blocking(move || match &file {
                Some(file) => publish(&inner.config, &login, user_id, &form, file, &images),
                None => page::edit(&inner.config, &login, user_id, &form, &images),
            })
            .await
            .unwrap_or_else(|_| {
                Err(Refusal::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the check crashed",
                ))
            })
        }
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_dir_all(&dir);
    result.map(|url| Json(json!({"pull_request": url})))
}

/// Reads the form's parts: `entry` (JSON), `file`, streamed to `dir`, and the images, `icon`
/// and up to 8 `screenshot`s.
async fn receive(
    multipart: &mut Multipart,
    dir: &Path,
) -> Result<(Form, Option<PathBuf>, page::Uploads), Refusal> {
    use tokio::io::AsyncWriteExt;
    let (mut form, mut file) = (None, None);
    let mut images = page::Uploads::default();
    while let Some(mut field) = multipart.next_field().await.map_err(torn)? {
        match field.name() {
            Some(part @ ("icon" | "screenshot")) => {
                let part = part.to_owned();
                let name = field.file_name().unwrap_or("").to_owned();
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(torn)? {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > page::MAX_IMAGE {
                        return Err(bad(format!("{part} {name:?} is too big")));
                    }
                }
                images.add(&part, &name, bytes).map_err(bad)?;
            }
            Some("entry") => {
                let text = field.text().await.map_err(torn)?;
                if text.len() > MAX_FORM {
                    return Err(bad("the page's text is too long"));
                }
                form = Some(serde_json::from_str::<Form>(&text).map_err(|e| bad(e.to_string()))?);
            }
            Some("file") => {
                let path = dir.join("upload");
                let mut out = tokio::fs::File::create(&path)
                    .await
                    .map_err(|e| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                let mut size = 0u64;
                while let Some(chunk) = field.chunk().await.map_err(torn)? {
                    size += chunk.len() as u64;
                    if size > index::MAX_FILE {
                        return Err(too_big());
                    }
                    out.write_all(&chunk).await.map_err(|e| {
                        Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                    })?;
                }
                out.flush().await.ok();
                file = Some(path);
            }
            _ => {}
        }
    }
    match (form, file) {
        (Some(form), _) if form.repo.is_some() => Err(bad(
            "the registry doesn't follow repositories: submit each version's file here, so it is checked and reviewed",
        )),
        (Some(form), None) if form.edit => Ok((form, None, images)),
        (Some(form), Some(file)) if !form.edit => Ok((form, Some(file), images)),
        _ => Err(bad("send the page and the file")),
    }
}

/// `YYYY-MM-DD` of a Unix time (UTC).
fn date(secs: u64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A key for submission tags, separate from the signed-cookie format. Keeping
/// the domains distinct means a public tag can never be repackaged as a
/// session cookie.
fn submission_key(config: &Config) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(&config.session_key).expect("any key length");
    mac.update(b"submission-tag-v1");
    mac.finalize().into_bytes().into()
}

/// What a submission branch's commit carries to say the form wrote it for
/// `login`: signed with the service's submission key, so a retry by the same
/// account may start that branch again.
fn submission_tag(config: &Config, branch: &str, login: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(&submission_key(config)).expect("32 bytes");
    mac.update(format!("submission\n{branch}\n{}", login.to_lowercase()).as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn submission_tag_valid(config: &Config, branch: &str, login: &str, tag: &str) -> bool {
    let Ok(tag) = URL_SAFE_NO_PAD.decode(tag) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(&submission_key(config)).expect("32 bytes");
    mac.update(format!("submission\n{branch}\n{}", login.to_lowercase()).as_bytes());
    mac.verify_slice(&tag).is_ok()
}

/// How a signed-in account relates to a project's owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    /// Its numeric ID is one of `owner_ids`: the account that listed it (or was added), whatever
    /// its login is now.
    ById,
    /// The entry records no IDs (from before they were recorded) and its login is listed: an owner
    /// as far as anyone can tell, but its updates always wait for a reviewer.
    ByLoginOnly,
    None,
}

/// Who `login` (GitHub account `user_id`) is to `entry`. When the entry has IDs, only they count: a
/// login listed in `owners` but held by another account is someone who claimed it after a rename.
pub fn ownership(entry: &Entry, login: &str, user_id: u64) -> Ownership {
    if !entry.owner_ids.is_empty() {
        return if entry.owner_ids.contains(&user_id) {
            Ownership::ById
        } else {
            Ownership::None
        };
    }
    if entry.owners.iter().any(|o| o.eq_ignore_ascii_case(login)) {
        Ownership::ByLoginOnly
    } else {
        Ownership::None
    }
}

/// The trailer of an owner update's pull request: a comment holding what the
/// update is and an HMAC-SHA256 (hex, with `key`) over it, which the registry
/// repository's `owner-update` workflow verifies with the same secret
/// (`scripts/owner_update.py` there builds the same message). v2 adds the
/// submitter's numeric GitHub ID, which the workflow checks against the
/// entry's `owner_ids` on main.
pub fn update_trailer(
    key: &[u8],
    id: &str,
    version: &str,
    sha256: &str,
    file: &str,
    submitter: &str,
    submitter_id: u64,
) -> String {
    let message =
        format!("owner-update-v2\n{id}\n{version}\n{sha256}\n{file}\n{submitter}\n{submitter_id}");
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(message.as_bytes());
    let hmac: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!(
        "<!-- owner-update\nformat: v2\nid: {id}\nversion: {version}\nsha256: {sha256}\nfile: {file}\nsubmitter: {submitter}\nsubmitter_id: {submitter_id}\nhmac: {hmac}\n-->"
    )
}

/// The submission, past its checks: the entry written on a new branch and a
/// pull request opened. Returns the pull request's address.
pub fn publish(
    config: &Config,
    login: &str,
    user_id: u64,
    form: &Form,
    file: &Path,
    images: &page::Uploads,
) -> Result<String, Refusal> {
    let gh = &config.github;
    let kind = Kind::from(form.kind.clone());
    registry::open(&kind).map_err(bad)?;
    if !index::valid_id(&form.id) {
        return Err(bad("an id is 1 to 64 of a-z, 0-9 and _"));
    }
    page::check_form(form, &kind).map_err(bad)?;
    if !form.source.trim().starts_with("https://") {
        return Err(bad("the source must be an https:// address of the code"));
    }
    if form.name.trim().is_empty() || form.license.trim().is_empty() {
        return Err(bad("a project needs a name and a licence"));
    }
    // The file's own checks first: a wrong file never reaches GitHub.
    archive::check(file).map_err(bad)?;
    let facts = index::inspect(&kind, file).map_err(bad)?;
    if let Some(id) = facts.id.as_ref().filter(|id| **id != form.id) {
        return Err(bad(format!("the file's id is {id:?}, not {:?}", form.id)));
    }
    if let Some(v) = facts
        .version
        .as_ref()
        .filter(|v| **v != form.version.trim())
    {
        return Err(bad(format!(
            "the file says version {v:?}, not {:?}",
            form.version.trim()
        )));
    }
    let sha256 = index::sha256_file(file).map_err(bad)?;
    let extension = kind.extension().unwrap_or("zip");
    let version = EntryVersion {
        version: form.version.trim().to_owned(),
        published: date(now()),
        game: form.game.trim().to_owned(),
        file: format!("{}-{}.{extension}", form.id, form.version.trim()),
        sha256: sha256.clone(),
        signature: form.signature.clone().filter(|s| !s.trim().is_empty()),
        changelog: form.changelog.clone(),
    };
    let existing = gh.entry(&form.id).map_err(github_down)?;
    let is_new = existing.is_none();
    if is_new {
        page::check_new(form, images).map_err(bad)?;
    }
    let turns_native =
        kind == Kind::NativeMod && existing.as_ref().is_some_and(|(e, _)| e.kind == Kind::Mod);
    // An owner update: an owner's new version and nothing else of the entry
    // changes (key and owners included).
    let mut owner_update = false;
    let mut matched = Ownership::ById;
    let (entry, blob) = match existing {
        None => (
            Entry {
                id: form.id.clone(),
                repo: None,
                kind: kind.clone(),
                name: form.name.trim().to_owned(),
                summary: form.summary.trim().to_owned(),
                description: form.description.clone(),
                authors: form.authors.clone(),
                owners: vec![login.to_owned()],
                owner_ids: vec![user_id],
                license: form.license.trim().to_owned(),
                source: form.source.trim().to_owned(),
                icon: None,
                categories: form.categories.clone(),
                tags: form.tags.clone(),
                screenshots: Vec::new(),
                key: form.key.clone().filter(|k| !k.trim().is_empty()),
                versions: vec![version.clone()],
            },
            None,
        ),
        Some((mut entry, blob)) => {
            matched = ownership(&entry, login, user_id);
            if matched == Ownership::None {
                return Err(Refusal::new(
                    StatusCode::FORBIDDEN,
                    format!(
                        "{} is {}'s: only its owners publish its updates",
                        form.id,
                        entry.owners.join(", ")
                    ),
                ));
            }
            if let Some(repo) = &entry.repo {
                return Err(Refusal::new(
                    StatusCode::CONFLICT,
                    format!(
                        "{} is listed by its repository: publish a GitHub release in https://github.com/{repo} and it shows up within the hour",
                        form.id
                    ),
                ));
            }
            // A mod may turn native once, never back (`registry::review`,
            // docs/native-mods.md): its `.hmod` versions stay published.
            if entry.kind != kind {
                if (&entry.kind, &kind) != (&Kind::Mod, &Kind::NativeMod) {
                    return Err(bad(format!(
                        "{} is a {}, not a {}: a project's kind never changes (only a mod may turn native-mod, once)",
                        form.id,
                        entry.kind.name(),
                        kind.name()
                    )));
                }
                entry.kind = kind.clone();
            }
            if entry.versions.iter().any(|v| v.version == version.version) {
                return Err(Refusal::new(
                    StatusCode::CONFLICT,
                    format!(
                        "{} {} is already published: give this file a new version",
                        form.id, version.version
                    ),
                ));
            }
            let key = form.key.clone().filter(|k| !k.trim().is_empty());
            if key.is_some() && key != entry.key {
                return Err(bad(
                    "changing a project's key is a pull request by hand, reviewed on its own",
                ));
            }
            let before = entry.clone();
            entry.name = form.name.trim().to_owned();
            entry.summary = form.summary.trim().to_owned();
            entry.description = form.description.clone();
            entry.authors = form.authors.clone();
            entry.license = form.license.trim().to_owned();
            entry.source = form.source.trim().to_owned();
            entry.categories = form.categories.clone();
            entry.tags = form.tags.clone();
            entry.versions.insert(0, version.clone());
            let (mut a, mut b) = (entry.clone(), before);
            a.versions.clear();
            b.versions.clear();
            // A mod turning native is reviewed by hand, never an owner update.
            // Only an owner known by ID publishes on their own: a login match alone
            // could be someone who claimed the login after a rename.
            owner_update = matched == Ownership::ById
                && a == b
                && images.is_empty()
                && !turns_native
                && (entry.key.is_none() || version.signature.is_some())
                && version
                    .version
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-+_".contains(&c));
            (entry, Some(blob))
        }
    };
    let mut entry = entry;
    let writes = page::place(&mut entry, form, images).map_err(bad)?;
    page::check_entry(&entry).map_err(bad)?;
    // Every check CI runs but the trial run: the file as its kind, its id and
    // version, its signature, the entry's fields. CI runs it all again.
    let files = file.parent().unwrap_or(Path::new("."));
    std::fs::copy(file, files.join(&sha256)).map_err(|e| bad(e.to_string()))?;
    let mut checked = entry.clone();
    checked.versions.truncate(1);
    // The images were checked by `page`; the registry's own files aren't in this scratch folder.
    checked.icon = None;
    checked.screenshots.clear();
    let project = registry::project(
        &checked,
        &Sources {
            registry: files,
            files,
            release: index::FILE_HOST,
            site: "https://example.invalid/",
            generated: String::new(),
        },
    )
    .map_err(|errors| bad(errors.join("; ")))?;
    let text = entry.to_toml().map_err(bad)?;
    gh.stage(&format!("{sha256}.{extension}"), file)
        .map_err(github_down)?;
    let branch = format!("submit/{}-{}", form.id, version.version);
    // Its commit's record of whose it is: only this service can write it,
    // where a commit's text or author can be anyone's who writes to the repository.
    let submission = submission_tag(config, &branch, login);
    if !gh.branch(&branch).map_err(github_down)? {
        // The branch is there: a pull request, someone else's submission, or
        // this submitter's own that GitHub broke off (a timeout past the
        // branch's creation). Only the last is taken up again, from main: the
        // branch holds no commit yet, or only those this form wrote for them.
        if let Some(url) = gh.open_pull(&branch).map_err(github_down)? {
            return Err(Refusal::new(
                StatusCode::CONFLICT,
                format!(
                    "{} {} was already submitted: its pull request is open ({url})",
                    form.id, version.version
                ),
            ));
        }
        let commits = gh.commits(&branch).map_err(github_down)?;
        // Every commit the form wrote (the entry, then its images) carries the tag; one without
        // it is someone else's.
        let theirs = commits.iter().all(|m| {
            m.lines().any(|line| {
                line.trim()
                    .strip_prefix("Submission:")
                    .is_some_and(|tag| submission_tag_valid(config, &branch, login, tag.trim()))
            })
        });
        if !theirs {
            return Err(Refusal::new(
                StatusCode::CONFLICT,
                format!(
                    "{} {} was already submitted by someone else: give this file a new version",
                    form.id, version.version
                ),
            ));
        }
        gh.reset(&branch).map_err(github_down)?;
    }
    let what = if is_new { "Add" } else { "Update" };
    let message = format!(
        "{what} {} {}\n\nSubmitted by @{login} through the form.\n\nSubmission: {submission}\nCo-authored-by: {login} <{user_id}+{login}@users.noreply.github.com>",
        form.id, version.version
    );
    gh.put(
        &format!("mods/{}.toml", form.id),
        &text,
        &branch,
        blob.as_deref(),
        &message,
    )
    .map_err(github_down)?;
    page::write(gh, &writes, &branch, &message)?;
    let v = &project.versions[0];
    let capabilities: Vec<String> = v
        .capabilities
        .iter()
        .map(|c| format!("- may {}", c.describe()))
        .collect();
    let requires: Vec<String> = v
        .requires
        .iter()
        .map(|r| format!("- needs {} {}", r.id, r.version))
        .collect();
    let body = format!(
        "<!-- submitted-by: {login} -->\n🤖 Submitted through [{site}]({public}/mods/submit) by @{login}. The form opens the pull request with \
         the registry's token: neither Ludovic nor a reviewer wrote it.\n\n\
         | | |\n|---|---|\n| Project | `{id}` ({kind}, {new}) |\n| Version | {version} for {game} |\n| File | `{file}`, {size} bytes, SHA-256 `{sha256}` |\n| Signed | {signed} |\n| Source | {source} |\n\n{lists}\n\n\
         The checks below run the file's own checks again and, for a mod, a trial run in the sandbox. \
         Reviewers: read the source and the checks' report before you approve.",
        public = config.public_url,
        site = config
            .public_url
            .split_once("://")
            .map_or(config.public_url.as_str(), |(_, host)| host),
        id = form.id,
        kind = kind.name(),
        new = if is_new {
            "new"
        } else if turns_native {
            "update: turns native, its `.hmod` versions stay published"
        } else {
            "update"
        },
        version = v.version,
        game = v.game,
        file = v.file.name,
        size = v.file.size,
        signed = if entry.key.is_some() {
            "yes, with the project's key"
        } else {
            "no"
        },
        source = entry.source,
        lists = [capabilities, requires].concat().join("\n"),
    );
    let body = if matched == Ownership::ByLoginOnly {
        format!(
            "{body}\n\n⚠️ This entry records no owner IDs, so @{login} (GitHub ID {user_id}) was matched by login only. \
             Check it is the same account that listed the project (a renamed owner's login can be claimed by anyone), \
             then add `owner_ids = [{user_id}]` to the entry."
        )
    } else {
        body
    };
    let trailer = config
        .update_key
        .as_deref()
        .filter(|_| owner_update)
        .map(|key| {
            update_trailer(
                key,
                &form.id,
                &version.version,
                &sha256,
                &v.file.name,
                login,
                user_id,
            )
        });
    let body = match &trailer {
        Some(t) => format!("{body}\n\n{t}"),
        None => body,
    };
    let (url, number) = gh
        .pull(
            &format!("{what} {} {}", form.id, version.version),
            &branch,
            &body,
        )
        .map_err(github_down)?;
    if trailer.is_some() {
        // The pull request is open: a label that fails only leaves it to a reviewer.
        if let Err(e) = gh.label(number, "owner-update") {
            eprintln!("labelling {url}: {} {}", e.status, e.message);
        }
    }
    Ok(url)
}

/// The client's address as the proxy in front of the service saw it: `X-Real-IP`, else the
/// last `X-Forwarded-For` hop (the one the proxy appended). The first hop is whatever the
/// client sent, so a limit keyed on it could be dodged by changing the header.
fn client_ip(headers: &HeaderMap) -> String {
    let real = headers.get("x-real-ip").and_then(|v| v.to_str().ok());
    let forwarded = || {
        headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .last()
    };
    real.or_else(forwarded)
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anonymous".to_string())
}

fn is_hex_str(s: &str, len: usize) -> bool {
    s.len() == len && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_target_triple(s: &str) -> bool {
    matches!(
        s,
        "x86_64-unknown-linux-gnu"
            | "x86_64-pc-windows-gnu"
            | "x86_64-pc-windows-msvc"
            | "aarch64-pc-windows-msvc"
            | "aarch64-apple-darwin"
            | "x86_64-apple-darwin"
    )
}

#[derive(Debug, Deserialize)]
pub struct NativeBuildRequest {
    pub target: String,
    pub base: String,
    pub mods: Vec<NativeBuildMod>,
}

#[derive(Debug, Deserialize)]
pub struct NativeBuildMod {
    pub id: String,
    pub version: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
pub struct NativeBuildQuery {
    pub target: String,
    pub base: String,
    pub key: String,
}

async fn request_native_build(
    State(app): State<App>,
    headers: HeaderMap,
    Json(payload): Json<NativeBuildRequest>,
) -> Result<Json<serde_json::Value>, Refusal> {
    let inner = app.0.clone();
    let ip = client_ip(&headers);
    if !allow(
        &inner.build_windows,
        &ip,
        BUILDS_PER_ADDRESS,
        BUILDS_PER_HOUR,
    ) {
        return Err(Refusal::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many native build requests this hour: try again later",
        ));
    }

    if payload.mods.is_empty() {
        return Err(bad("a native build request needs at least one mod"));
    }
    if payload.mods.len() > MAX_SET_MODS {
        return Err(bad(format!(
            "at most {MAX_SET_MODS} mods per native build request"
        )));
    }

    let mods: Vec<Mod> = payload
        .mods
        .into_iter()
        .map(|m| Mod {
            id: m.id,
            version: m.version,
            sha256: m.sha256,
        })
        .collect();

    let request = BuildRequest::new(&payload.target, &payload.base, mods).map_err(bad)?;

    let key = request.key();
    let tag = native_build::release_tag(&request.base);
    let archive_name = native_build::archive(&key, &request.target);
    let sig_name = native_build::signature(&key, &request.target);

    let gh = inner.config.github.clone();
    let tag_clone = tag.clone();
    let archive_clone = archive_name.clone();

    let exists =
        tokio::task::spawn_blocking(move || gh.release_asset_exists(&tag_clone, &archive_clone))
            .await
            .map_err(|e| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(github_down)?;

    let base_url = format!(
        "https://github.com/{}/releases/download/{}",
        inner.config.github.repo, tag
    );
    let archive_url = format!("{base_url}/{archive_name}");
    let sig_url = format!("{base_url}/{sig_name}");

    if exists {
        return Ok(Json(json!({
            "status": "ready",
            "key": key,
            "tag": tag,
            "archive": archive_name,
            "signature": sig_name,
            "url": archive_url,
            "sig_url": sig_url,
        })));
    }

    // Verify all requested mods exist in the registry, match version and sha256, and are not revoked
    let mods_to_check = request.mods.clone();
    let gh = inner.config.github.clone();
    tokio::task::spawn_blocking(move || -> Result<(), Refusal> {
        for m in &mods_to_check {
            let entry = gh.entry(&m.id).map_err(github_down)?;
            let Some((entry, _blob)) = entry else {
                return Err(bad(format!("mod {:?} is not in the registry", m.id)));
            };
            if entry.kind != Kind::NativeMod {
                return Err(bad(format!("mod {:?} is not a native mod", m.id)));
            }
            let version = entry.versions.iter().find(|v| v.version == m.version);
            let Some(v) = version else {
                return Err(bad(format!(
                    "mod {:?} has no version {:?}",
                    m.id, m.version
                )));
            };
            if !v.sha256.eq_ignore_ascii_case(&m.sha256) {
                return Err(bad(format!(
                    "file SHA-256 mismatch for {:?}@{}: expected {}, got {}",
                    m.id, m.version, v.sha256, m.sha256
                )));
            }
        }
        let revoked = gh.revoked().map_err(github_down)?;
        if let Some(m) = mods_to_check.iter().find(|m| {
            revoked.iter().any(|r| {
                r.id == m.id
                    && r.sha256
                        .as_ref()
                        .is_none_or(|h| h.eq_ignore_ascii_case(&m.sha256))
            })
        }) {
            return Err(bad(format!("{}@{} was taken down", m.id, m.version)));
        }
        Ok(())
    })
    .await
    .map_err(|e| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;

    // Check dedup window
    let recent = {
        let dispatched = inner.dispatched.lock().unwrap();
        dispatched
            .get(&key)
            .is_some_and(|at| at.elapsed() < DISPATCH_DEDUP_WINDOW)
    };

    if !recent {
        let gh = inner.config.github.clone();
        let target = request.target.clone();
        let base = request.base.clone();
        let mods_str = request
            .mods
            .iter()
            .map(|m| format!("{}@{}", m.id, m.version))
            .collect::<Vec<_>>()
            .join(" ");

        tokio::task::spawn_blocking(move || {
            gh.dispatch_native_build("main", &target, &base, &mods_str)
        })
        .await
        .map_err(|e| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(github_down)?;

        inner
            .dispatched
            .lock()
            .unwrap()
            .insert(key.clone(), Instant::now());
    }

    Ok(Json(json!({
        "status": "queued",
        "key": key,
        "tag": tag,
        "archive": archive_name,
        "signature": sig_name,
        "url": archive_url,
        "sig_url": sig_url,
    })))
}

async fn get_native_build(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<NativeBuildQuery>,
) -> Result<Json<serde_json::Value>, Refusal> {
    if !allow(
        &app.0.poll_windows,
        &client_ip(&headers),
        POLLS_PER_ADDRESS,
        POLLS_PER_HOUR,
    ) {
        return Err(Refusal::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many native build polls this hour: try again later",
        ));
    }
    if !is_hex_str(&query.base, 40) {
        return Err(bad(format!(
            "{:?} is not a commit (40 hex digits)",
            query.base
        )));
    }
    if !is_target_triple(&query.target) {
        return Err(bad(format!(
            "{:?} is not a valid target triple",
            query.target
        )));
    }
    if !is_hex_str(&query.key, 64) {
        return Err(bad(format!(
            "{:?} is not a request key (64 hex digits)",
            query.key
        )));
    }

    let tag = native_build::release_tag(&query.base);
    let archive_name = native_build::archive(&query.key, &query.target);
    let sig_name = native_build::signature(&query.key, &query.target);

    let gh = app.0.config.github.clone();
    let tag_clone = tag.clone();
    let archive_clone = archive_name.clone();

    let exists =
        tokio::task::spawn_blocking(move || gh.release_asset_exists(&tag_clone, &archive_clone))
            .await
            .map_err(|e| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(github_down)?;

    let base_url = format!(
        "https://github.com/{}/releases/download/{}",
        app.0.config.github.repo, tag
    );
    let archive_url = format!("{base_url}/{archive_name}");
    let sig_url = format!("{base_url}/{sig_name}");

    let status = if exists { "ready" } else { "queued" };
    Ok(Json(json!({
        "status": status,
        "key": query.key,
        "tag": tag,
        "archive": archive_name,
        "signature": sig_name,
        "url": archive_url,
        "sig_url": sig_url,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_signatures() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_792_800_000), "2026-10-24");
        let key = b"0123456789abcdef0123456789abcdef";
        let signed = sign(key, "alice\n7\n99");
        assert_eq!(unsign(key, &signed).as_deref(), Some("alice\n7\n99"));
        assert_eq!(unsign(b"another key, another key, another", &signed), None);
        let forged = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode("mallory\n8\n99"),
            signed.split_once('.').unwrap().1
        );
        assert_eq!(unsign(key, &forged), None);
    }

    #[test]
    fn submission_tags_are_not_session_cookies() {
        let key = b"0123456789abcdef0123456789abcdef";
        let config = Config {
            public_url: "https://example.invalid".into(),
            session_key: key.to_vec(),
            github: GitHub::new(
                github::Endpoints::default(),
                "owner/repo".into(),
                "client".into(),
                "secret".into(),
                "token".into(),
            ),
            update_key: None,
            scratch: PathBuf::from("/tmp/horizon-mod-registry-test"),
            database: PathBuf::from(":memory:"),
        };
        let branch = "submit/example-1.0.0";
        let login = "12345678901";
        let tag = submission_tag(&config, branch, login);
        assert!(submission_tag_valid(&config, branch, login, &tag));

        let payload = format!("submission\n{branch}\n{login}");
        let cookie = format!("{}.{}", URL_SAFE_NO_PAD.encode(payload), tag);
        assert_eq!(unsign(&config.session_key, &cookie), None);
    }

    #[test]
    fn a_page_is_known_by_its_origin() {
        // An `Origin` has no path, though REGISTRY_PUBLIC_URL may: the site's
        // pages are below its root (#6159).
        assert_eq!(
            url_origin("https://playhorizon.dev/mods"),
            "https://playhorizon.dev"
        );
        assert_eq!(
            url_origin("https://playhorizon.dev/mods/submit/"),
            "https://playhorizon.dev"
        );
        assert_eq!(
            url_origin("https://playhorizon.dev"),
            "https://playhorizon.dev"
        );
        assert_eq!(
            url_origin("http://localhost:8080/mods"),
            "http://localhost:8080"
        );
        // The configured site's origin, path or no path, and a host name's
        // case folded as a browser folds it.
        assert!(from_the_site(
            "https://playhorizon.dev/mods",
            Some("https://playhorizon.dev")
        ));
        assert!(from_the_site(
            "https://playhorizon.dev/hub/",
            Some("https://PlayHorizon.dev")
        ));
        // The scheme, the host and the port are all the origin's.
        assert!(!from_the_site(
            "https://playhorizon.dev",
            Some("http://playhorizon.dev")
        ));
        assert!(!from_the_site(
            "https://playhorizon.dev",
            Some("https://playhorizon.dev:8443")
        ));
        // Another site's page, a page with no origin to give, and a name
        // that only starts like the host.
        assert!(!from_the_site(
            "https://playhorizon.dev",
            Some("https://evil.test")
        ));
        assert!(!from_the_site("https://playhorizon.dev", None));
        assert!(!from_the_site(
            "https://playhorizon.dev",
            Some("https://playhorizon.dev.evil.test")
        ));
        assert!(!from_the_site("::", Some("null")));
    }

    #[test]
    fn native_build_request_and_key_computation() {
        let req = NativeBuildRequest {
            target: "x86_64-unknown-linux-gnu".into(),
            base: "0123456789abcdef0123456789abcdef01234567".into(),
            mods: vec![
                NativeBuildMod {
                    id: "zoom".into(),
                    version: "1.0.0".into(),
                    sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .into(),
                },
                NativeBuildMod {
                    id: "minimap".into(),
                    version: "2.1.0".into(),
                    sha256: "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                        .into(),
                },
            ],
        };
        let mods: Vec<Mod> = req
            .mods
            .into_iter()
            .map(|m| Mod {
                id: m.id,
                version: m.version,
                sha256: m.sha256,
            })
            .collect();
        let built = BuildRequest::new(&req.target, &req.base, mods).expect("valid request");
        assert_eq!(built.mods[0].id, "minimap");
        assert_eq!(built.mods[1].id, "zoom");
        let key = built.key();
        assert_eq!(key.len(), 64);
        assert_eq!(
            native_build::release_tag(&req.base),
            "native-builds-0123456789ab"
        );
        let archive = native_build::archive(&key, &req.target);
        assert!(archive.starts_with(&key[..16]));
        assert!(archive.ends_with("-x86_64-unknown-linux-gnu.tar.gz"));
        let sig = native_build::signature(&key, &req.target);
        assert!(sig.starts_with(&key[..16]));
        assert!(sig.ends_with("-x86_64-unknown-linux-gnu.sig"));
    }

    #[test]
    fn a_client_cannot_pick_its_own_rate_limit_address() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "1.2.3.4, 203.0.113.9".parse().unwrap());
        assert_eq!(client_ip(&headers), "203.0.113.9");
        headers.insert("x-real-ip", "198.51.100.7".parse().unwrap());
        assert_eq!(client_ip(&headers), "198.51.100.7");
        assert_eq!(client_ip(&HeaderMap::new()), "anonymous");
    }

    #[test]
    fn native_build_query_validation() {
        assert!(is_hex_str("0123456789abcdef0123456789abcdef01234567", 40));
        assert!(!is_hex_str("0123456789abcdef", 40));
        assert!(!is_hex_str("0123456789abcdef0123456789abcdef0123456g", 40));
        assert!(is_target_triple("x86_64-unknown-linux-gnu"));
        assert!(is_target_triple("x86_64-pc-windows-gnu"));
        assert!(is_target_triple("aarch64-apple-darwin"));
        assert!(!is_target_triple("riscv64gc-unknown-linux-gnu"));
    }
}
