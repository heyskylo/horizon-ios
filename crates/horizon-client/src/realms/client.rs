//! The Realms service client: vanilla's `com.mojang.realmsclient.client.RealmsClient` and its
//! `Request`, for the signed-in player's own Microsoft account only. Every request carries
//! what `RealmsClient.execute` adds and nothing more: the `sid`, `user` and `version` cookies
//! and the three version headers. The access token goes only into the `sid` cookie (and the
//! `x-mojangauth` header vanilla adds for the invite-code service); it is never logged, never
//! put in an error and never sent anywhere else.
use horizon_core::version;
use serde::{Deserialize, Deserializer};
use std::time::Duration;

/// `RealmsClient.Environment.PRODUCTION`'s base URL.
pub const DEFAULT_REALMS_HOST: &str = "https://pc.realms.minecraft.net";
/// `RealmsClient.Environment.PRODUCTION`'s `frontendV2Url` (invite codes).
pub const DEFAULT_FRONTEND_V2_HOST: &str = "https://java.frontend.realms.minecraft-services.net";

/// `Request.DEFAULT_CONNECT_TIMEOUT`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// `Request.DEFAULT_READ_TIMEOUT`.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// `RealmsClient.join`: `Request.get(url, 5000, 30000)`.
const JOIN_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Vanilla sets no user agent: `HttpURLConnection` sends the JVM's own, `Java/<version>`.
fn user_agent() -> String {
    format!("Java/{}", version::JAVA_VERSION)
}

/// Gson leaves a field at its default when the JSON has `null`; serde would fail.
fn nullable<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `RealmsServer`: the fields the screens read.
#[derive(Clone, Debug, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RealmServer {
    #[serde(default, deserialize_with = "nullable")]
    pub id: i64,
    #[serde(default)]
    pub remote_subscription_id: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub motd: String,
    #[serde(default, deserialize_with = "nullable")]
    pub state: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(rename = "ownerUUID", default)]
    pub owner_uuid: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub expired: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub expired_trial: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub days_left: i32,
    #[serde(default, deserialize_with = "nullable")]
    pub world_type: String,
    #[serde(default, deserialize_with = "nullable")]
    pub active_slot: i32,
    #[serde(default)]
    pub minigame_name: Option<String>,
    #[serde(default)]
    pub minigame_id: Option<i32>,
    #[serde(default)]
    pub minigame_image: Option<String>,
}

impl RealmServer {
    /// `RealmsServer.State.OPEN` and not expired: the Play button's condition.
    pub fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case("OPEN") && !self.expired
    }
    /// `Minecraft.isLocalPlayer(server.ownerUUID)`.
    pub fn owned_by(&self, uuid: u128) -> bool {
        self.owner_uuid
            .as_deref()
            .and_then(horizon_client_render::auth::parse_uuid)
            == Some(uuid)
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
struct RealmServerList {
    #[serde(default, deserialize_with = "nullable")]
    servers: Vec<RealmServer>,
}

/// `RealmsJoinInformation`: where to connect, and the world's resource pack.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealmAddress {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub resource_pack_url: Option<String>,
    #[serde(default)]
    pub resource_pack_hash: Option<String>,
}

/// `PendingInvite`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingInviteDto {
    #[serde(default, deserialize_with = "nullable")]
    pub invitation_id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub world_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub world_owner_name: String,
    #[serde(default)]
    pub world_owner_uuid: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub date: i64,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct PendingInvitesResponse {
    #[serde(default, deserialize_with = "nullable")]
    invites: Vec<PendingInviteDto>,
}

/// `Subscription`: the realm's own, as `RealmsClient.subscriptionFor` reads it.
#[derive(Clone, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// `Subscription.startDate`: epoch milliseconds.
    #[serde(default, deserialize_with = "nullable")]
    pub start_date: i64,
    #[serde(default, deserialize_with = "nullable")]
    pub days_left: i32,
    /// `Subscription.SubscriptionType`: `NORMAL` or `RECURRING`.
    #[serde(rename = "type", default, deserialize_with = "nullable")]
    pub kind: String,
}

/// Why a Realms call failed (`RealmsServiceException` and its `RealmsError`). None of them
/// holds a request header: the text a screen shows can't contain the token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RealmsError {
    /// `CustomError.connectivityError`: no answer (the transport error, which names the URL).
    Connectivity(String),
    /// `AuthenticationError`: a 401, with its `WWW-Authenticate` header.
    Authentication(String),
    /// `RetryCallException`: 503 or 277, with the `Retry-After` seconds (5 when missing).
    Retry(u64),
    /// `RealmsError.parse`: any other answer, with the body's `errorCode`, `reason` and
    /// `errorMsg` (or the raw body).
    Service {
        status: u16,
        code: Option<i32>,
        reason: Option<String>,
        message: Option<String>,
    },
    /// An answer whose JSON doesn't read.
    Parse,
    /// `GetServerDetailsTask`'s `TimeoutException`: still busy after 40 tries.
    Timeout,
}

impl RealmsError {
    /// `RealmsError.errorCode`.
    pub fn code(&self) -> i32 {
        match self {
            Self::Connectivity(_) => 500,
            Self::Authentication(_) => 401,
            Self::Retry(_) => 503,
            Self::Service { status, code, .. } => code.unwrap_or(i32::from(*status)),
            Self::Parse | Self::Timeout => 500,
        }
    }
    /// `RealmsError.errorMessage`, as `RealmsGenericErrorScreen` shows it after the
    /// `mco.errorMessage.realmsService.realmsError` line.
    pub fn text(&self) -> String {
        let no_details = || crate::lang::get("mco.errorMessage.noDetails");
        let detail = match self {
            Self::Connectivity(e) => crate::lang::tr(
                "mco.errorMessage.realmsService.connectivity",
                &[e as &dyn std::fmt::Display],
            ),
            Self::Authentication(header) if !header.is_empty() => header.clone(),
            Self::Authentication(_) => no_details(),
            Self::Retry(_) => crate::lang::get("mco.errorMessage.retry"),
            Self::Service { status: 429, .. } => crate::lang::get("mco.errorMessage.serviceBusy"),
            Self::Service {
                code,
                reason,
                message,
                ..
            } => {
                let code_key = code.map(|c| format!("mco.errorMessage.{c}"));
                let reason_key = reason.as_ref().map(|r| format!("mco.errorReason.{r}"));
                match (code_key, reason_key) {
                    (Some(k), _) if crate::lang::has(&k) => crate::lang::get(&k),
                    (_, Some(k)) if crate::lang::has(&k) => crate::lang::get(&k),
                    _ => message.clone().unwrap_or_else(no_details),
                }
            }
            Self::Parse | Self::Timeout => crate::lang::get("mco.errorMessage.connectionFailure"),
        };
        let code = self.code();
        format!(
            "{} {detail}",
            crate::lang::tr(
                "mco.errorMessage.realmsService.realmsError",
                &[&code as &dyn std::fmt::Display]
            )
        )
    }
    /// `RealmsError.parse` of a non-2xx answer.
    fn from_answer(status: u16, body: &str, html: bool) -> Self {
        let body = body.trim();
        let service = |code, reason, message| Self::Service {
            status,
            code,
            reason,
            message,
        };
        if status == 429 || body.is_empty() {
            return service(None, None, None);
        }
        if html {
            // CustomError.htmlPayload: the text between <body> and </body>.
            let inner = body
                .find("<body>")
                .zip(body.find("</body>"))
                .filter(|(a, b)| b > a)
                .map(|(a, b)| body[a + 6..b].trim().to_owned());
            return service(None, None, inner);
        }
        if let Ok(serde_json::Value::Object(o)) = serde_json::from_str::<serde_json::Value>(body) {
            let text = |k: &str| o.get(k).and_then(|v| v.as_str()).map(str::to_owned);
            let code = o
                .get("errorCode")
                .and_then(serde_json::Value::as_i64)
                .and_then(|c| i32::try_from(c).ok());
            let (reason, message) = (text("reason"), text("errorMsg"));
            if code.is_some() || reason.is_some() || message.is_some() {
                return service(code, reason, message);
            }
        }
        // ErrorWithRawPayload.
        service(None, None, Some(body.chars().take(256).collect()))
    }
}

/// The client for one signed-in account.
#[derive(Clone)]
pub struct RealmsClient {
    base_url: String,
    frontend_url: String,
    player_name: String,
    player_uuid: u128,
    access_token: String,
}

impl std::fmt::Debug for RealmsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealmsClient")
            .field("base_url", &self.base_url)
            .field("player_name", &self.player_name)
            .field("access_token", &"<hidden>")
            .finish()
    }
}

impl RealmsClient {
    pub fn new(
        base_url: &str,
        frontend_url: &str,
        session: &horizon_client_render::auth::AuthSession,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            frontend_url: frontend_url.trim_end_matches('/').to_owned(),
            player_name: session.name.clone(),
            player_uuid: session.uuid,
            access_token: session.access_token().to_owned(),
        }
    }
    /// The production service.
    pub fn production(session: &horizon_client_render::auth::AuthSession) -> Self {
        Self::new(DEFAULT_REALMS_HOST, DEFAULT_FRONTEND_V2_HOST, session)
    }

    /// `RealmsClient.execute`'s cookies (`User.getSessionId`, `getName`,
    /// `WorldVersion.name()`) and `Request.addVersionHeaders`, in that order.
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "Cookie",
                format!(
                    "sid=token:{}:{:032x};user={};version={}",
                    self.access_token,
                    self.player_uuid,
                    self.player_name,
                    version::VANILLA_VERSION
                ),
            ),
            ("Client-Ref", version::VERSION_ID.to_owned()),
            (
                "X-NetworkProtocolVersion",
                version::PROTOCOL_VERSION.to_string(),
            ),
            ("Is-Prerelease", (!version::STABLE).to_string()),
        ]
    }

    fn request(&self, method: &str, url: &str, read_timeout: Duration) -> ureq::Request {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(read_timeout)
            .user_agent(&user_agent())
            // A redirect must not carry the session cookie to another host.
            .redirects(0)
            .build();
        self.headers()
            .into_iter()
            .fold(agent.request(method, url), |r, (k, v)| r.set(k, &v))
    }

    /// `RealmsClient.execute`: the body of a 2xx answer, else the error.
    fn execute(request: ureq::Request, body: Option<&str>) -> Result<String, RealmsError> {
        let result = match body {
            Some(body) => request.send_string(body),
            None => request.call(),
        };
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            // The transport error names the URL and the cause, never a header.
            Err(ureq::Error::Transport(e)) => return Err(RealmsError::Connectivity(e.to_string())),
        };
        let status = response.status();
        if status == 503 || status == 277 {
            let pause = response
                .header("Retry-After")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(5);
            return Err(RealmsError::Retry(pause));
        }
        let auth =
            (status == 401).then(|| response.header("WWW-Authenticate").unwrap_or("").to_owned());
        let html = response.content_type().starts_with("text/html");
        let text = response
            .into_string()
            .map_err(|e| RealmsError::Connectivity(e.to_string()))?;
        if (200..300).contains(&status) {
            return Ok(text);
        }
        if let Some(header) = auth {
            return Err(RealmsError::Authentication(header));
        }
        Err(RealmsError::from_answer(status, &text, html))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url)
    }

    /// `RealmsClient.listRealms` (a release client: no `listUserWorldsOfType`).
    pub fn list_worlds(&self) -> Result<Vec<RealmServer>, RealmsError> {
        let json = Self::execute(self.request("GET", &self.url("worlds"), READ_TIMEOUT), None)?;
        serde_json::from_str::<RealmServerList>(&json)
            .map(|l| l.servers)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.join`: `worlds/v1/<id>/join/pc`.
    pub fn join_world(&self, realm_id: i64) -> Result<RealmAddress, RealmsError> {
        let url = self.url(&format!("worlds/v1/{realm_id}/join/pc"));
        let json = Self::execute(self.request("GET", &url, JOIN_READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.pendingInvites`.
    pub fn pending_invites(&self) -> Result<Vec<PendingInviteDto>, RealmsError> {
        let json = Self::execute(
            self.request("GET", &self.url("invites/pending"), READ_TIMEOUT),
            None,
        )?;
        serde_json::from_str::<PendingInvitesResponse>(&json)
            .map(|r| r.invites)
            .map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.acceptInvitation`.
    pub fn accept_invite(&self, invitation_id: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/accept/{}", path_segment(invitation_id)?));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.rejectInvitation`.
    pub fn reject_invite(&self, invitation_id: &str) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/reject/{}", path_segment(invitation_id)?));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.uninviteMyselfFrom`: leaves someone else's Realm.
    pub fn uninvite_myself(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("invites/{realm_id}"));
        Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None).map(drop)
    }

    /// `RealmsClient.open`.
    pub fn open_realm(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/open"));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.close`.
    pub fn close_realm(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/close"));
        Self::execute(self.request("PUT", &url, READ_TIMEOUT), Some("")).map(drop)
    }

    /// `RealmsClient.subscriptionFor`: the configuration screen's subscription tab.
    pub fn subscription_for(&self, realm_id: i64) -> Result<Subscription, RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}/subscription"));
        let json = Self::execute(self.request("GET", &url, READ_TIMEOUT), None)?;
        serde_json::from_str(&json).map_err(|_| RealmsError::Parse)
    }

    /// `RealmsClient.deleteRealm` (`Request.delete`).
    pub fn delete_realm(&self, realm_id: i64) -> Result<(), RealmsError> {
        let url = self.url(&format!("worlds/{realm_id}"));
        Self::execute(self.request("DELETE", &url, READ_TIMEOUT), None).map(drop)
    }

    /// `RealmsClient.redeemInviteCode`: the invite-code service, with the token in
    /// `x-mojangauth` as `frontendV2Request` sets it.
    pub fn redeem_invite_code(&self, code: &str) -> Result<(), RealmsError> {
        let code = path_segment(code.trim())?;
        let url = format!("{}/api/v1.0/invite/codes/use/{code}", self.frontend_url);
        let request = self
            .request("POST", &url, READ_TIMEOUT)
            .set("x-mojangauth", &self.access_token)
            .set("Accept", "application/json");
        Self::execute(request, Some("")).map(drop)
    }
}

/// An id or code that goes into a URL path: vanilla builds those URLs from the server's own
/// ids; anything else (a typed code with `/` or `?`) would reach another path.
fn path_segment(s: &str) -> Result<&str, RealmsError> {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(s)
    } else {
        Err(RealmsError::Service {
            status: 400,
            code: None,
            reason: None,
            message: Some(crate::lang::get("mco.selectServer.joinCode.invalid")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client_render::auth::AuthSession;
    use std::io::{Read, Write};

    const TOKEN: &str = "eyJsecret.token.value";
    const UUID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

    /// One HTTP exchange on a local port: the request as the server read it.
    fn serve_once(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            stream.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        (base, handle)
    }

    fn client(base: &str) -> RealmsClient {
        RealmsClient::new(base, base, &AuthSession::new("Steve", UUID, TOKEN))
    }

    fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    #[test]
    fn requests_send_the_session_cookie_only_and_errors_never_hold_the_token() {
        let (base, server) = serve_once(
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"Realms\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let error = client(&base).list_worlds().unwrap_err();
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /worlds HTTP/1.1"), "{request}");
        // RealmsClient.execute: sid = User.getSessionId (token:<token>:<undashed uuid>), user, version.
        assert_eq!(
            header(&request, "Cookie"),
            Some(
                "sid=token:eyJsecret.token.value:0123456789abcdef0123456789abcdef;user=Steve;version=26.3"
            )
        );
        assert_eq!(header(&request, "Authorization"), None, "{request}");
        assert_eq!(header(&request, "Client-Ref"), Some("26.3"));
        assert_eq!(header(&request, "X-NetworkProtocolVersion"), Some("777"));
        assert_eq!(header(&request, "Is-Prerelease"), Some("false"));
        assert_eq!(header(&request, "User-Agent"), Some("Java/25"));
        // The token appears once in the whole request: in the cookie.
        assert_eq!(request.matches(TOKEN).count(), 1);
        assert_eq!(
            error,
            RealmsError::Authentication("Bearer realm=\"Realms\"".into())
        );
        assert!(!error.text().contains(TOKEN));
        assert!(!format!("{error:?}").contains(TOKEN));
        assert!(!format!("{:?}", client(&base)).contains(TOKEN));
    }

    #[test]
    fn a_refused_connection_names_no_header() {
        // A port nothing listens on: the transport error.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let error = client(&format!("http://127.0.0.1:{port}"))
            .join_world(7)
            .unwrap_err();
        assert!(matches!(error, RealmsError::Connectivity(_)), "{error:?}");
        assert!(!error.text().contains(TOKEN));
    }

    #[test]
    fn busy_answers_ask_for_a_retry_after_their_delay() {
        let (base, server) = serve_once(
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 3\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(client(&base).join_world(7), Err(RealmsError::Retry(3)));
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("GET /worlds/v1/7/join/pc ")
        );
    }

    #[test]
    fn the_join_answer_reads_vanillas_field_names() {
        let (base, server) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"address\":\"1.2.3.4:25565\",\"resourcePackUrl\":\"https://x.test/p.zip\",\"resourcePackHash\":\"abc\",\"sessionRegionData\":null}",
        );
        let address = client(&base).join_world(42).unwrap();
        server.join().unwrap();
        assert_eq!(
            address,
            RealmAddress {
                address: Some("1.2.3.4:25565".into()),
                resource_pack_url: Some("https://x.test/p.zip".into()),
                resource_pack_hash: Some("abc".into()),
            }
        );
    }

    #[test]
    fn an_invite_code_with_a_path_in_it_is_refused_before_any_request() {
        // Nothing listens: a request would fail as Connectivity, not as the invalid code.
        let error = client("http://127.0.0.1:9")
            .redeem_invite_code("abc/../worlds")
            .unwrap_err();
        assert!(
            matches!(error, RealmsError::Service { status: 400, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn server_list_reads_camel_case_and_nulls() {
        let raw = r#"{"servers":[{"id":12345,"remoteSubscriptionId":"sub","name":"Test Realm",
            "motd":null,"state":"OPEN","owner":"Alex","ownerUUID":"0123456789abcdef0123456789abcdef",
            "daysLeft":25,"expired":false,"expiredTrial":false,"worldType":"NORMAL","activeSlot":2,
            "minigameName":null}]}"#;
        let list: RealmServerList = serde_json::from_str(raw).unwrap();
        let server = &list.servers[0];
        assert_eq!(
            (server.id, server.days_left, server.active_slot),
            (12345, 25, 2)
        );
        assert_eq!(server.motd, "");
        assert!(server.is_open());
        assert!(server.owned_by(UUID));
        assert!(!server.owned_by(UUID + 1));
    }

    #[test]
    fn error_bodies_read_as_vanilla_parses_them() {
        assert_eq!(
            RealmsError::from_answer(403, r#"{"errorCode":6002,"errorMsg":"tos"}"#, false),
            RealmsError::Service {
                status: 403,
                code: Some(6002),
                reason: None,
                message: Some("tos".into())
            }
        );
        assert_eq!(
            RealmsError::from_answer(500, r#"{"errorCode":6006}"#, false).code(),
            6006
        );
        assert_eq!(
            RealmsError::from_answer(502, "<html><body> Bad gateway </body></html>", true),
            RealmsError::Service {
                status: 502,
                code: None,
                reason: None,
                message: Some("Bad gateway".into())
            }
        );
        assert_eq!(RealmsError::from_answer(404, "", false).code(), 404);
    }
}
