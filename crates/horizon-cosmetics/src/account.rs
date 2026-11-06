//! What the game asks the service about its own account: whether it owns a cosmetic (`GET
//! /v1/me`: no other player's cosmetics show for one that owns none, unless the player asks), and
//! the settings it changes itself, "Show my cosmetics" (`PUT /v1/visible`) and "Show me as a
//! Horizon player" (`POST /v1/presence`, also sent every `PRESENCE_EVERY` while on), and the
//! emote its player plays (`POST /v1/emote`, with a session kept for it: `Kept`).
//!
//! The game signs in the way an online-mode server checks a player: the service hands out a
//! one-time server id, the game tells Mojang's session server it joins it (`join`, with the
//! player's own Minecraft session), and the service asks Mojang. The Minecraft token goes to
//! Mojang only; the service's session lives for this one call and is never kept.
use serde_json::{Value, json};

/// A request to the service: `method` on `path`, with the service's session if any.
pub trait Call {
    fn call(
        &self,
        method: &str,
        path: &str,
        session: Option<&str>,
        body: &Value,
    ) -> Result<Value, String>;
}

impl Call for crate::Http {
    fn call(
        &self,
        method: &str,
        path: &str,
        session: Option<&str>,
        body: &Value,
    ) -> Result<Value, String> {
        let mut request = self
            .agent()
            .request(method, &format!("{}{path}", self.base()))
            .set("Content-Type", "application/json");
        if let Some(s) = session {
            request = request.set("Authorization", &format!("Bearer {s}"));
        }
        let bytes = crate::Http::read(request.send_bytes(body.to_string().as_bytes()))?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }
}

/// How often a running game tells the service its player plays Horizon (the service's
/// `PRESENCE_EVERY`; it forgets them some hours after the last time).
pub const PRESENCE_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

/// "Show me as a Horizon player": listed, other Horizon players see the mark by `name`'s name for
/// a while (sent again every `PRESENCE_EVERY`); not listed, the service forgets it at once.
pub fn set_listed(
    service: &impl Call,
    name: &str,
    join: impl FnOnce(&str) -> Result<(), String>,
    listed: bool,
) -> Result<bool, String> {
    let session = sign_in(service, name, join)?;
    let answer = service.call(
        "POST",
        "/v1/presence",
        Some(&session),
        &json!({ "listed": listed }),
    )?;
    answer["listed"].as_bool().ok_or_else(|| "no answer".into())
}

/// A session of the service's own for the account `name`; `join` tells Mojang that account joins
/// the server id the service chose.
fn sign_in(
    service: &impl Call,
    name: &str,
    join: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, String> {
    let mut join = Some(join);
    sign_in_for(service, name, &mut |id: &str| {
        join.take().ok_or("joined twice")?(id)
    })
    .map(|(session, _)| session)
}

/// `sign_in`, with how long the session lives (seconds; an hour if the service didn't say).
fn sign_in_for(
    service: &impl Call,
    name: &str,
    join: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(String, u64), String> {
    let start = service.call("POST", "/v1/auth/start", None, &json!({}))?;
    let server_id = start["server_id"].as_str().ok_or("no server id")?;
    join(server_id)?;
    let finish = service.call(
        "POST",
        "/v1/auth/finish",
        None,
        &json!({ "server_id": server_id, "name": name }),
    )?;
    let session = finish["session"].as_str().ok_or("no session")?.to_owned();
    Ok((session, finish["expires_in"].as_u64().unwrap_or(3600)))
}

/// The service's session for an emote's start and stop, kept for the time the service gave less a
/// minute: emotes come often, Mojang's `join` shouldn't each time.
#[derive(Default)]
pub struct Kept {
    session: Option<(String, std::time::Instant)>,
}

impl Kept {
    /// `method` on `path` with `body` as the account `name`, signing in first if no session is
    /// kept; a session the service no longer knows (401) is dropped and the call made once more
    /// after a sign-in.
    pub fn call(
        &mut self,
        service: &impl Call,
        name: &str,
        mut join: impl FnMut(&str) -> Result<(), String>,
        method: &str,
        path: &str,
        body: &Value,
    ) -> Result<Value, String> {
        for attempt in 0..2 {
            let fresh = self
                .session
                .as_ref()
                .filter(|(_, until)| std::time::Instant::now() < *until);
            let session = match fresh {
                Some((s, _)) => s.clone(),
                None => {
                    let (session, seconds) = sign_in_for(service, name, &mut join)?;
                    let keep = std::time::Duration::from_secs(seconds.saturating_sub(60));
                    self.session = Some((session.clone(), std::time::Instant::now() + keep));
                    session
                }
            };
            match service.call(method, path, Some(&session), body) {
                Err(e) if e == "HTTP 401" && attempt == 0 => self.session = None,
                answer => return answer,
            }
        }
        Err("HTTP 401".into())
    }
}

/// Tells the service the player plays the owned emote `emote` now (`None`: stopped); only while
/// they show themselves as a Horizon player (else the service answers 409).
pub fn play_emote(
    kept: &mut Kept,
    service: &impl Call,
    name: &str,
    join: impl FnMut(&str) -> Result<(), String>,
    emote: Option<&str>,
) -> Result<(), String> {
    kept.call(
        service,
        name,
        join,
        "POST",
        "/v1/emote",
        &json!({ "emote": emote }),
    )
    .map(|_| ())
}

/// An emote the account owns: its item and its names by locale (`en_us`, `fr_fr`...).
pub type OwnedEmote = (crate::Item, std::collections::BTreeMap<String, String>);

/// The emotes the account `name` owns (`GET /v1/me`'s `emotes`: id, file hash, names), their
/// files read with `get` (`/v1/items/<id>?hash=`). A file that fails to come or to parse, or
/// isn't the emote it says, is left out.
pub fn owned_emotes(
    kept: &mut Kept,
    service: &impl Call,
    name: &str,
    join: impl FnMut(&str) -> Result<(), String>,
    get: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<OwnedEmote>, String> {
    let me = kept.call(service, name, join, "GET", "/v1/me", &Value::Null)?;
    let mut emotes = Vec::new();
    for e in me["emotes"].as_array().into_iter().flatten() {
        let (Some(id), Some(hash)) = (e["id"].as_str(), e["hash"].as_str()) else {
            continue;
        };
        let item = get(&format!("/v1/items/{id}?hash={hash}"))
            .ok()
            .and_then(|bytes| crate::Item::parse(&bytes).ok())
            .filter(|i| i.slot == crate::Slot::Emote && i.id == id);
        let Some(item) = item else {
            continue;
        };
        let names = e["names"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect();
        emotes.push((item, names));
    }
    Ok(emotes)
}

/// Whether the account `name` owns at least one cosmetic (signed in as `set_visible` does).
pub fn owns(
    service: &impl Call,
    name: &str,
    join: impl FnOnce(&str) -> Result<(), String>,
) -> Result<bool, String> {
    let session = sign_in(service, name, join)?;
    let me = service.call("GET", "/v1/me", Some(&session), &Value::Null)?;
    let owned = me["owned"].as_array().ok_or("no answer")?;
    Ok(!owned.is_empty())
}

/// Turns "Show my cosmetics" on or off for the account `name`; `join` tells Mojang that account
/// joins the server id the service chose. Returns what the service now keeps.
pub fn set_visible(
    service: &impl Call,
    name: &str,
    join: impl FnOnce(&str) -> Result<(), String>,
    visible: bool,
) -> Result<bool, String> {
    let session = sign_in(service, name, join)?;
    let session = session.as_str();
    let me = service.call(
        "PUT",
        "/v1/visible",
        Some(session),
        &json!({ "visible": visible }),
    )?;
    me["visible"].as_bool().ok_or_else(|| "no answer".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// The service, as far as this call goes: what it was sent, and whether the id was joined.
    #[derive(Default)]
    struct Service {
        sent: RefCell<Vec<(String, String, Option<String>, Value)>>,
        joined: RefCell<Option<String>>,
        /// What `GET /v1/me` lists as owned.
        owned: Vec<&'static str>,
    }

    impl Call for Service {
        fn call(
            &self,
            method: &str,
            path: &str,
            s: Option<&str>,
            body: &Value,
        ) -> Result<Value, String> {
            self.sent.borrow_mut().push((
                method.into(),
                path.into(),
                s.map(Into::into),
                body.clone(),
            ));
            match path {
                "/v1/auth/start" => Ok(json!({"server_id": "id-1"})),
                "/v1/auth/finish" if self.joined.borrow().as_deref() == Some("id-1") => {
                    Ok(json!({"session": "svc"}))
                }
                "/v1/auth/finish" => Err("HTTP 401".into()),
                "/v1/visible" if s == Some("svc") => Ok(json!({"visible": body["visible"]})),
                "/v1/me" if s == Some("svc") => Ok(json!({
                    "owned": self.owned,
                    "emotes": [
                        {"id": "wave", "hash": "h1", "names": {"en_us": "Wave", "fr_fr": "Coucou"}},
                        {"id": "gone", "hash": "h1", "names": {}},
                        {"id": "hat", "hash": "h1", "names": {}},
                    ],
                })),
                "/v1/presence" if s == Some("svc") => Ok(json!({"listed": body["listed"]})),
                "/v1/emote" if s == Some("svc") => Ok(json!({"emote": body["emote"]})),
                _ => Err("HTTP 401".into()),
            }
        }
    }

    #[test]
    fn hiding_goes_through_mojang_and_sends_only_the_services_session() {
        let service = Service::default();
        let shown = set_visible(
            &service,
            "Alex",
            |id| {
                *service.joined.borrow_mut() = Some(id.to_owned());
                Ok(())
            },
            false,
        );
        assert_eq!(shown, Ok(false));
        let sent = service.sent.borrow();
        let paths: Vec<&str> = sent.iter().map(|(_, p, ..)| p.as_str()).collect();
        assert_eq!(paths, ["/v1/auth/start", "/v1/auth/finish", "/v1/visible"]);
        assert_eq!(sent[1].3["name"], "Alex");
        // Only the last call carries a session, and it is the service's own.
        let sessions: Vec<Option<&str>> = sent.iter().map(|(.., s, _)| s.as_deref()).collect();
        assert_eq!(sessions, [None, None, Some("svc")]);
    }

    #[test]
    fn ownership_is_whether_the_accounts_own_list_has_anything() {
        fn joined(service: &Service) -> impl FnOnce(&str) -> Result<(), String> + '_ {
            move |id| {
                *service.joined.borrow_mut() = Some(id.to_owned());
                Ok(())
            }
        }
        let none = Service::default();
        assert_eq!(owns(&none, "Alex", joined(&none)), Ok(false));
        let founder = Service {
            owned: vec!["founder_visor"],
            ..Service::default()
        };
        assert_eq!(owns(&founder, "Alex", joined(&founder)), Ok(true));
        let sent = founder.sent.borrow();
        assert_eq!(
            sent.last()
                .map(|c| (c.0.as_str(), c.1.as_str(), c.2.as_deref())),
            Some(("GET", "/v1/me", Some("svc")))
        );
        drop(sent);
        // Mojang refusing the join: no answer, so the game keeps the vanilla look.
        assert!(owns(&founder, "Alex", |_| Err("403".into())).is_err());
    }

    #[test]
    fn mojang_refusing_stops_before_the_service_is_asked_to_trust_it() {
        let service = Service::default();
        let refused = set_visible(&service, "Alex", |_| Err("403".into()), false);
        assert_eq!(refused, Err("403".into()));
        assert_eq!(service.sent.borrow().len(), 1);
        // A join Mojang never saw: the service refuses the id.
        assert!(set_visible(&service, "Alex", |_| Ok(()), false).is_err());
    }

    #[test]
    fn emotes_sign_in_once_and_again_only_when_the_session_is_gone() {
        let service = Service::default();
        let joins = std::cell::Cell::new(0);
        let join = |id: &str| {
            joins.set(joins.get() + 1);
            *service.joined.borrow_mut() = Some(id.to_owned());
            Ok(())
        };
        let mut kept = Kept::default();
        for emote in [Some("wave_hello"), None, Some("wave_hello")] {
            play_emote(&mut kept, &service, "Alex", join, emote).unwrap();
        }
        assert_eq!(joins.get(), 1);
        let sent = service.sent.borrow();
        let emotes: Vec<(&str, Option<&str>, &Value)> = sent
            .iter()
            .filter(|c| c.1 == "/v1/emote")
            .map(|c| (c.0.as_str(), c.2.as_deref(), &c.3))
            .collect();
        assert_eq!(emotes.len(), 3);
        assert_eq!(
            emotes[0],
            ("POST", Some("svc"), &json!({"emote": "wave_hello"}))
        );
        assert_eq!(emotes[1].2, &json!({"emote": null}));
        drop(sent);
        // The service forgot the session (a restart): one more sign-in, then the call.
        kept.session = Some((
            "stale".into(),
            std::time::Instant::now() + std::time::Duration::from_secs(60),
        ));
        play_emote(&mut kept, &service, "Alex", join, Some("wave_hello")).unwrap();
        assert_eq!(joins.get(), 2);
    }

    #[test]
    fn the_wheel_gets_the_owned_emotes_files_and_names() {
        let service = Service::default();
        let join = |id: &str| {
            *service.joined.borrow_mut() = Some(id.to_owned());
            Ok(())
        };
        let emote = r#"{"format": 1, "id": "wave", "slot": "emote", "parts": [],
            "emote": {"length": 20, "tracks": [{"part": "right_arm", "keys": [{"t": 0}, {"t": 10, "rotation": [0, 0, 90]}]}]}}"#;
        let asked = RefCell::new(Vec::new());
        let get = |path: &str| {
            asked.borrow_mut().push(path.to_owned());
            match path {
                "/v1/items/wave?hash=h1" => Ok(emote.as_bytes().to_vec()),
                // A file that says it is the emote `wave`, asked for as `hat`.
                "/v1/items/hat?hash=h1" => Ok(emote.as_bytes().to_vec()),
                _ => Err("HTTP 404".to_owned()),
            }
        };
        let mut kept = Kept::default();
        let owned = owned_emotes(&mut kept, &service, "Alex", join, get).unwrap();
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].0.id, "wave");
        assert_eq!(owned[0].1["fr_fr"], "Coucou");
        assert_eq!(asked.borrow().len(), 3);
        let sent = service.sent.borrow();
        assert_eq!(
            sent.last()
                .map(|c| (c.0.as_str(), c.1.as_str(), c.2.as_deref())),
            Some(("GET", "/v1/me", Some("svc")))
        );
    }

    #[test]
    fn leaving_the_list_signs_in_the_same_way() {
        let service = Service::default();
        let join = |id: &str| {
            *service.joined.borrow_mut() = Some(id.to_owned());
            Ok(())
        };
        assert_eq!(set_listed(&service, "Alex", join, false), Ok(false));
        let sent = service.sent.borrow();
        assert_eq!(sent[2].1, "/v1/presence");
        assert_eq!(
            (sent[2].2.as_deref(), &sent[2].3),
            (Some("svc"), &json!({"listed": false}))
        );
    }
}
