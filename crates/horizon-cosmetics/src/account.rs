//! What the game asks the service about its own account: whether it owns a cosmetic (`GET
//! /v1/me`: no other player's cosmetics show for one that owns none, unless the player asks), and
//! the one setting it changes itself, "Show my cosmetics" (`PUT /v1/visible`).
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

/// A session of the service's own for the account `name`; `join` tells Mojang that account joins
/// the server id the service chose.
fn sign_in(
    service: &impl Call,
    name: &str,
    join: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, String> {
    let start = service.call("POST", "/v1/auth/start", None, &json!({}))?;
    let server_id = start["server_id"].as_str().ok_or("no server id")?;
    join(server_id)?;
    let finish = service.call(
        "POST",
        "/v1/auth/finish",
        None,
        &json!({ "server_id": server_id, "name": name }),
    )?;
    Ok(finish["session"].as_str().ok_or("no session")?.to_owned())
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
                "/v1/me" if s == Some("svc") => Ok(json!({"owned": self.owned})),
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
}
