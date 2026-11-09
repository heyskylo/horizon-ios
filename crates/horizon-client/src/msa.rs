//! Microsoft account sign-in inside the game, from the title screen: the OAuth device-code flow, then Xbox
//! Live, XSTS, the Minecraft services, the ownership check and the profile (docs/microsoft-account.md). The
//! same requests the phone's SwiftUI launcher made before the game took them over. The Azure app is Horizon's
//! official public application (`HORIZON_MSA_CLIENT_ID` at build time overrides it); the refresh token is kept in the game's home,
//! readable by its owner only, so the next start signs in again without a code. Tokens are never logged.
use crate::auth::{self, AuthSession};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// The Azure app the game signs in with (an empty `HORIZON_MSA_CLIENT_ID`, an unset CI variable, keeps it).
pub const CLIENT_ID: &str = match option_env!("HORIZON_MSA_CLIENT_ID") {
    Some(id) if !id.is_empty() => id,
    _ => "331e08f8-172d-43b9-835a-3108c2fcdbf3",
};
const SCOPE: &str = "XboxLive.signin offline_access";
const DEVICE_CODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
/// The file in the game's home that keeps the account between starts.
const FILE: &str = "microsoft-account.json";

/// Where a sign-in stands, for the account screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    SignedOut,
    /// The player enters `code` at `link` in a browser.
    Code {
        code: String,
        link: String,
    },
    Working(String),
    SignedIn(String),
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::SignedOut);
/// Bumped by every start and sign-out: a sign-in thread whose number is no longer current stops quietly.
static ATTEMPT: AtomicU64 = AtomicU64::new(0);

pub fn state() -> State {
    STATE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn set(attempt: u64, state: State) -> bool {
    let current = ATTEMPT.load(Ordering::SeqCst) == attempt;
    if current {
        *STATE.lock().unwrap_or_else(PoisonError::into_inner) = state;
    }
    current
}

/// Sign in with a new code, replacing any sign-in under way.
pub fn sign_in(home: &Path) {
    let attempt = ATTEMPT.fetch_add(1, Ordering::SeqCst) + 1;
    set(
        attempt,
        State::Working("Asking Microsoft for a code...".into()),
    );
    let home = home.to_owned();
    spawn(attempt, move || {
        let microsoft = device_code(attempt)?;
        finish(attempt, &home, microsoft)
    });
}

/// The account a last start kept, signed in again with its refresh token. Nothing without one.
pub fn restore(home: &Path) {
    let Some(refresh) = read(home) else { return };
    let attempt = ATTEMPT.fetch_add(1, Ordering::SeqCst) + 1;
    set(attempt, State::Working("Signing in...".into()));
    let home = home.to_owned();
    spawn(attempt, move || {
        let microsoft = refresh_token(&refresh).map_err(|e| format!("Sign in again: {e}"))?;
        finish(attempt, &home, microsoft)
    });
}

/// Play offline again, forgetting the kept account.
pub fn sign_out(home: &Path) {
    let attempt = ATTEMPT.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = std::fs::remove_file(home.join(FILE));
    auth::clear_current();
    set(attempt, State::SignedOut);
}

fn spawn(attempt: u64, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("horizon-microsoft-sign-in".into())
        .spawn(move || {
            let state = match work() {
                Ok(name) => State::SignedIn(name),
                Err(error) => {
                    log::warn!("Horizon: Microsoft sign-in failed: {error}");
                    State::Failed(error)
                }
            };
            set(attempt, state);
        });
}

type Answer = serde_json::Value;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
}

/// The JSON object a request answered, error statuses included (their `status` added); `None` for no answer.
fn send(request: ureq::Request, body: Option<(&str, String)>) -> Result<Answer, String> {
    let result = match body {
        Some((kind, body)) => request
            .set("Content-Type", kind)
            .set("Accept", "application/json")
            .send_string(&body),
        None => request.call(),
    };
    let (status, response) = match result {
        Ok(response) => (response.status(), response),
        Err(ureq::Error::Status(status, response)) => (status, response),
        Err(ureq::Error::Transport(_)) => {
            return Err("No connection to the sign-in service.".into());
        }
    };
    let mut answer: Answer = response
        .into_string()
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Answer::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    if !(200..300).contains(&status) {
        answer["status"] = status.into();
    }
    Ok(answer)
}

fn form(url: &str, fields: &[(&str, &str)]) -> Result<Answer, String> {
    let body = fields
        .iter()
        .map(|(k, v)| format!("{k}={}", percent(v)))
        .collect::<Vec<_>>()
        .join("&");
    send(
        agent().post(url),
        Some(("application/x-www-form-urlencoded", body)),
    )
}

fn json(url: &str, body: Answer) -> Result<Answer, String> {
    send(
        agent().post(url),
        Some(("application/json", body.to_string())),
    )
}

fn get(url: &str, token: &str) -> Result<Answer, String> {
    send(
        agent()
            .get(url)
            .set("Authorization", &format!("Bearer {token}")),
        None,
    )
}

/// RFC 3986 unreserved characters as they are, every other byte `%XX`.
fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn text<'a>(answer: &'a Answer, key: &str) -> Option<&'a str> {
    answer.get(key).and_then(Answer::as_str)
}

/// Steps 1 and 2: a code for the player to enter, then the wait for them to do it. The Microsoft access and
/// refresh tokens.
fn device_code(attempt: u64) -> Result<(String, String), String> {
    let start = form(DEVICE_CODE, &[("client_id", CLIENT_ID), ("scope", SCOPE)])?;
    let (Some(device), Some(code), Some(link)) = (
        text(&start, "device_code"),
        text(&start, "user_code"),
        text(&start, "verification_uri").filter(|l| l.starts_with("https://")),
    ) else {
        return Err("Microsoft didn't give a sign-in code.".into());
    };
    if !set(
        attempt,
        State::Code {
            code: code.into(),
            link: link.into(),
        },
    ) {
        return Err("cancelled".into());
    }
    let mut interval = start["interval"].as_u64().unwrap_or(5).max(1);
    let deadline =
        Instant::now() + Duration::from_secs(start["expires_in"].as_u64().unwrap_or(900));
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(interval));
        if ATTEMPT.load(Ordering::SeqCst) != attempt {
            return Err("cancelled".into());
        }
        let answer = form(
            TOKEN,
            &[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", CLIENT_ID),
                ("device_code", device),
            ],
        )?;
        match text(&answer, "error") {
            None => {
                return match (
                    text(&answer, "access_token"),
                    text(&answer, "refresh_token"),
                ) {
                    (Some(access), Some(refresh)) => Ok((access.into(), refresh.into())),
                    _ => Err("Microsoft's answer had no token.".into()),
                };
            }
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            Some("authorization_declined") => return Err("The sign-in was declined.".into()),
            Some("expired_token") => return Err("The code expired. Try again.".into()),
            Some(_) => return Err("Microsoft refused the sign-in.".into()),
        }
    }
    Err("The code expired. Try again.".into())
}

fn refresh_token(refresh: &str) -> Result<(String, String), String> {
    let answer = form(
        TOKEN,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh),
            ("scope", SCOPE),
        ],
    )?;
    let access = text(&answer, "access_token").ok_or("Microsoft wants you to sign in again.")?;
    Ok((
        access.into(),
        text(&answer, "refresh_token").unwrap_or(refresh).into(),
    ))
}

/// Steps 3 to 7: Xbox Live, XSTS, Minecraft, ownership and the profile; then the game plays as the account,
/// and the refresh token is kept. The profile's name.
fn finish(
    attempt: u64,
    home: &Path,
    (access, refresh): (String, String),
) -> Result<String, String> {
    set(attempt, State::Working("Signing in to Xbox Live...".into()));
    let xbl = json(
        "https://user.auth.xboxlive.com/user/authenticate",
        serde_json::json!({
            "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={access}")},
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }),
    )?;
    let (Some(xbl_token), Some(uhs)) = (
        text(&xbl, "Token"),
        xbl.pointer("/DisplayClaims/xui/0/uhs")
            .and_then(Answer::as_str),
    ) else {
        return Err("Xbox Live didn't sign you in.".into());
    };
    let xsts = json(
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        serde_json::json!({
            "Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl_token]},
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }),
    )?;
    if let Some(code) = xsts["XErr"].as_u64() {
        return Err(xbox_error(code));
    }
    let xsts_token = text(&xsts, "Token").ok_or("Xbox Live refused the sign-in.")?;
    set(attempt, State::Working("Signing in to Minecraft...".into()));
    let minecraft = json(
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        serde_json::json!({"identityToken": format!("XBL3.0 x={uhs};{xsts_token}")}),
    )?;
    let Some(token) = text(&minecraft, "access_token") else {
        let unapproved = minecraft["status"].as_u64() == Some(403)
            || text(&minecraft, "errorMessage").is_some_and(|m| m.contains("Invalid app"));
        return Err(if unapproved {
            "This app is not approved by Mojang yet.".into()
        } else {
            "Minecraft refused the sign-in.".into()
        });
    };
    let store = get(
        "https://api.minecraftservices.com/entitlements/mcstore",
        token,
    )?;
    let owned = store["items"].as_array().is_some_and(|items| {
        items.iter().any(|i| {
            matches!(
                text(i, "name"),
                Some("game_minecraft" | "product_minecraft")
            )
        })
    });
    if !owned {
        return Err("This account doesn't own Minecraft: Java Edition.".into());
    }
    let profile = get("https://api.minecraftservices.com/minecraft/profile", token)?;
    let (Some(name), Some(uuid)) = (
        text(&profile, "name"),
        text(&profile, "id").and_then(auth::parse_uuid),
    ) else {
        return Err("Create your Minecraft profile on minecraft.net first.".into());
    };
    if ATTEMPT.load(Ordering::SeqCst) != attempt {
        return Err("cancelled".into());
    }
    auth::set_current(AuthSession::new(name, uuid, token));
    if let Err(e) = write(home, &refresh) {
        log::warn!("Horizon: could not keep the Microsoft account: {e}");
    }
    log::info!("Horizon: signed in to Microsoft as {name}");
    Ok(name.into())
}

fn xbox_error(code: u64) -> String {
    match code {
        2_148_916_233 => {
            "This Microsoft account has no Xbox profile yet: create one at xbox.com.".into()
        }
        2_148_916_235 => "Xbox Live isn't available in your country.".into(),
        2_148_916_236 | 2_148_916_237 => {
            "This account needs adult verification on xbox.com.".into()
        }
        2_148_916_238 => {
            "This is a child account: an adult has to add it to a Microsoft family.".into()
        }
        _ => format!("Xbox Live refused the sign-in ({code})."),
    }
}

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

fn read(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path(home)).ok()?;
    let value: Answer = serde_json::from_str(&text).ok()?;
    text_owned(&value, "refresh_token")
}

fn text_owned(value: &Answer, key: &str) -> Option<String> {
    text(value, key)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// The refresh token, in a file only its owner can read.
fn write(home: &Path, refresh: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(home)?;
    let body = serde_json::json!({ "refresh_token": refresh }).to_string();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path(home))?, body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_encode_like_url_session() {
        assert_eq!(
            percent("XboxLive.signin offline_access"),
            "XboxLive.signin%20offline_access"
        );
        assert_eq!(percent("a-b_c.d~e/f:"), "a-b_c.d~e%2Ff%3A");
    }

    #[test]
    fn the_refresh_token_round_trips_and_signing_out_forgets_it() {
        let home = std::env::temp_dir().join(format!("horizon-msa-{}", std::process::id()));
        write(&home, "secret").unwrap();
        assert_eq!(read(&home).as_deref(), Some("secret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path(&home)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        sign_out(&home);
        assert_eq!(read(&home), None);
        assert_eq!(state(), State::SignedOut);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn xbox_errors_say_what_to_do() {
        assert!(xbox_error(2_148_916_233).contains("xbox.com"));
        assert!(xbox_error(1).contains("(1)"));
    }
}
