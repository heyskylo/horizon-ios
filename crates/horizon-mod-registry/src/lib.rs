//! `horizon-mod-registry`: the submission form's service at
//! `playhorizon.dev/api/mods/*` (docs/api/mod-index.md). A creator signs in
//! with GitHub, sends a file and its page; the service runs the file's own
//! checks, uploads it to the registry's `staging` release and opens a pull
//! request with the entry. Reviewers decide; merging publishes. Signed-in
//! players also rate published projects (`ratings`, its only data: sessions
//! are signed cookies, and GitHub holds everything else).
pub mod github;
pub mod ratings;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path as UrlPath, Query, State},
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
/// Ratings set per account per hour, and by everyone.
const RATINGS_PER_ACCOUNT: usize = 30;
const RATINGS_PER_HOUR: usize = 600;
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
    /// Where files wait during a submission.
    pub scratch: PathBuf,
    /// The ratings' SQLite file (a volume): `:memory:` keeps them in memory.
    pub database: PathBuf,
}

impl Config {
    /// `REGISTRY_PUBLIC_URL`, `REGISTRY_SESSION_KEY`, `REGISTRY_REPO`,
    /// `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, `GITHUB_BOT_TOKEN`,
    /// `REGISTRY_DB` (default `/data/registry.sqlite3`).
    pub fn from_env() -> Result<Config, String> {
        let var = |name: &str| std::env::var(name).map_err(|_| format!("{name} is not set"));
        let key = var("REGISTRY_SESSION_KEY")?;
        if key.len() < 32 {
            return Err("REGISTRY_SESSION_KEY needs at least 32 characters".into());
        }
        Ok(Config {
            public_url: var("REGISTRY_PUBLIC_URL")?.trim_end_matches('/').to_owned(),
            session_key: key.into_bytes(),
            github: GitHub::new(
                github::Endpoints::default(),
                std::env::var("REGISTRY_REPO")
                    .unwrap_or_else(|_| "horizon-rust/horizon-mods".into()),
                var("GITHUB_CLIENT_ID")?,
                var("GITHUB_CLIENT_SECRET")?,
                var("GITHUB_BOT_TOKEN")?,
            ),
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
            post(submit).layer(DefaultBodyLimit::max(
                index::MAX_FILE as usize + MAX_FORM * 2,
            )),
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
    if entry.owners.iter().any(|o| o.eq_ignore_ascii_case(&login)) {
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
    pub source: String,
    /// `https://` only; an icon in the repository is a pull request by hand.
    #[serde(default)]
    pub icon: Option<String>,
    /// A new project's minisign public key.
    #[serde(default)]
    pub key: Option<String>,
    /// The `.minisig` text.
    #[serde(default)]
    pub signature: Option<String>,
    pub version: String,
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

/// A change sent by the site's own pages: from its origin, signed in, with
/// the session's CSRF token. The account: (login, id).
fn signed_in_here(app: &Inner, headers: &HeaderMap) -> Result<(String, u64), Refusal> {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if origin != Some(app.config.public_url.as_str()) {
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

async fn submit(
    State(app): State<App>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, Refusal> {
    let inner = app.0.clone();
    let (login, user_id) = signed_in_here(&inner, &headers)?;
    if !allow(&inner.recent, &login, PER_ACCOUNT, PER_HOUR) {
        return Err(Refusal::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many submissions this hour: try again later",
        ));
    }
    let dir = inner.config.scratch.join(random_hex());
    std::fs::create_dir_all(&dir)
        .map_err(|e| Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let result = receive(&mut multipart, &dir).await;
    let result = match result {
        Ok((form, file)) => {
            let inner = inner.clone();
            tokio::task::spawn_blocking(move || {
                publish(&inner.config, &login, user_id, &form, &file)
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

/// Reads the form's two parts: `entry` (JSON) and `file`, streamed to `dir`.
async fn receive(multipart: &mut Multipart, dir: &Path) -> Result<(Form, PathBuf), Refusal> {
    use tokio::io::AsyncWriteExt;
    let (mut form, mut file) = (None, None);
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| bad(e.body_text()))?
    {
        match field.name() {
            Some("entry") => {
                let text = field.text().await.map_err(|e| bad(e.body_text()))?;
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
                while let Some(chunk) = field.chunk().await.map_err(|e| bad(e.body_text()))? {
                    size += chunk.len() as u64;
                    if size > index::MAX_FILE {
                        return Err(bad(format!(
                            "the file is over {} MiB",
                            index::MAX_FILE >> 20
                        )));
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
        (Some(form), Some(file)) => Ok((form, file)),
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

/// The submission, past its checks: the entry written on a new branch and a
/// pull request opened. Returns the pull request's address.
pub fn publish(
    config: &Config,
    login: &str,
    user_id: u64,
    form: &Form,
    file: &Path,
) -> Result<String, Refusal> {
    let gh = &config.github;
    let kind = Kind::from(form.kind.clone());
    registry::open(&kind).map_err(bad)?;
    if !index::valid_id(&form.id) {
        return Err(bad("an id is 1 to 64 of a-z, 0-9 and _"));
    }
    if form
        .icon
        .as_ref()
        .is_some_and(|i| !i.starts_with("https://"))
    {
        return Err(bad("the icon must be an https:// address"));
    }
    if !form.source.trim().starts_with("https://") {
        return Err(bad("the source must be an https:// address of the code"));
    }
    if form.name.trim().is_empty() || form.license.trim().is_empty() {
        return Err(bad("a project needs a name and a licence"));
    }
    // The file's own checks first: a wrong file never reaches GitHub.
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
    let (entry, blob) = match existing {
        None => (
            Entry {
                id: form.id.clone(),
                kind: kind.clone(),
                name: form.name.trim().to_owned(),
                summary: form.summary.trim().to_owned(),
                description: form.description.clone(),
                authors: form.authors.clone(),
                owners: vec![login.to_owned()],
                license: form.license.trim().to_owned(),
                source: form.source.trim().to_owned(),
                icon: form.icon.clone(),
                key: form.key.clone().filter(|k| !k.trim().is_empty()),
                versions: vec![version.clone()],
            },
            None,
        ),
        Some((mut entry, blob)) => {
            if !entry.owners.iter().any(|o| o.eq_ignore_ascii_case(login)) {
                return Err(Refusal::new(
                    StatusCode::FORBIDDEN,
                    format!(
                        "{} is {}'s: only its owners publish its updates",
                        form.id,
                        entry.owners.join(", ")
                    ),
                ));
            }
            if entry.kind != kind {
                return Err(bad(format!("{} is a {}", form.id, entry.kind.name())));
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
            entry.name = form.name.trim().to_owned();
            entry.summary = form.summary.trim().to_owned();
            entry.description = form.description.clone();
            entry.authors = form.authors.clone();
            entry.license = form.license.trim().to_owned();
            entry.source = form.source.trim().to_owned();
            if form.icon.is_some() {
                entry.icon = form.icon.clone();
            }
            entry.versions.insert(0, version.clone());
            (entry, Some(blob))
        }
    };
    // Every check CI runs but the trial run: the file as its kind, its id and
    // version, its signature, the entry's fields. CI runs it all again.
    let files = file.parent().unwrap_or(Path::new("."));
    std::fs::copy(file, files.join(&sha256)).map_err(|e| bad(e.to_string()))?;
    let mut checked = entry.clone();
    checked.versions.truncate(1);
    if checked
        .icon
        .as_ref()
        .is_some_and(|i| !i.starts_with("https://"))
    {
        checked.icon = None;
    }
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
    if !gh.branch(&branch).map_err(github_down)? {
        return Err(Refusal::new(
            StatusCode::CONFLICT,
            format!(
                "{} {} was already submitted: its pull request is open",
                form.id, version.version
            ),
        ));
    }
    let what = if is_new { "Add" } else { "Update" };
    let message = format!(
        "{what} {} {}\n\nSubmitted by @{login} through the form.\n\nCo-authored-by: {login} <{user_id}+{login}@users.noreply.github.com>",
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
        new = if is_new { "new" } else { "update" },
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
    gh.pull(
        &format!("{what} {} {}", form.id, version.version),
        &branch,
        &body,
    )
    .map_err(github_down)
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
}
