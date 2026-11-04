//! Microsoft sessions handed over by the launcher; offline split profiles belong to the instance.
use horizon_client::auth::AuthSession;
use std::sync::Arc;

/// The launcher's other signed-in accounts (`HORIZON_ACCOUNT_SESSIONS`: name, uuid and a session
/// file each), for splitscreen players to sign in with: read at start beside `--session-file`
/// (`read_now`), as the files go when read and are refused once a couple of minutes old.
pub(super) fn accounts() -> Vec<Arc<AuthSession>> {
    READ.get_or_init(read).0.clone()
}

static READ: std::sync::OnceLock<(Vec<Arc<AuthSession>>, Vec<String>)> = std::sync::OnceLock::new();

pub(super) fn unavailable() -> Vec<String> {
    READ.get_or_init(read).1.clone()
}

/// Read (and so delete) the session files now, before the game loads.
pub(in crate::game) fn read_now() {
    READ.get_or_init(read);
}

fn read() -> (Vec<Arc<AuthSession>>, Vec<String>) {
    let Ok(json) = std::env::var("HORIZON_ACCOUNT_SESSIONS") else {
        return Default::default();
    };
    parse(&json)
}

fn parse(json: &str) -> (Vec<Arc<AuthSession>>, Vec<String>) {
    let list: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    let mut sessions = Vec::new();
    let mut unavailable = Vec::new();
    for a in list {
        // Every file listed is read (and so deleted) first, whatever else the entry says:
        // nothing with a token stays on disk.
        let session = a
            .get("file")
            .and_then(|v| v.as_str())
            .and_then(|file| AuthSession::load(std::path::Path::new(file)).ok());
        let Some(name) = a.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        if !horizon_client::split_profiles::valid_name(name) {
            continue;
        }
        match session {
            Some(session)
                if session.name.eq_ignore_ascii_case(name)
                    && a.get("uuid")
                        .and_then(|v| v.as_str())
                        .and_then(horizon_client::auth::parse_uuid)
                        == Some(session.uuid) =>
            {
                sessions.push(Arc::new(session))
            }
            _ => {
                // Reasons are deliberately fixed text: never log session contents or tokens.
                log::warn!(
                    "Horizon: splitscreen: {name} is offline; sign in again in the launcher."
                );
                unavailable.push(name.to_owned());
            }
        }
    }
    (sessions, unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatched_account_sessions_are_consumed_and_refused() {
        let dir = std::env::temp_dir().join(format!("hz-split-account-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.json");
        std::fs::write(&file, r#"{"name":"Ann","uuid":"00000000000000000000000000000001","access_token":"unused","user_type":"msa"}"#).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let json = serde_json::json!([{"name":"Ann", "uuid":"00000000000000000000000000000002", "file":file}]).to_string();
        let (sessions, unavailable) = parse(&json);
        assert!(sessions.is_empty());
        assert_eq!(unavailable, ["Ann"]);
        assert!(
            !file.exists(),
            "session files are read once even when mismatched"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_listed_session_file_is_consumed() {
        let dir = std::env::temp_dir().join(format!("hz-split-consumed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |n: &str| {
            let file = dir.join(format!("session-{n}.json"));
            std::fs::write(&file, r#"{"name":"Bea","uuid":"00000000000000000000000000000002","access_token":"not-a-real-token","user_type":"msa"}"#).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            file
        };
        let (bad_name, no_name, good) = (file("a"), file("b"), file("c"));
        let uuid = "00000000000000000000000000000002";
        let json = serde_json::json!([
            {"name": "not a name!", "uuid": uuid, "file": bad_name},
            {"uuid": uuid, "file": no_name},
            {"name": "Bea", "uuid": uuid, "file": good},
        ])
        .to_string();
        let (sessions, unavailable) = parse(&json);
        assert_eq!(sessions.len(), 1);
        assert_eq!((sessions[0].name.as_str(), sessions[0].uuid), ("Bea", 2));
        assert!(unavailable.is_empty());
        for f in [bad_name, no_name, good] {
            assert!(!f.exists(), "{} left on disk", f.display());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
