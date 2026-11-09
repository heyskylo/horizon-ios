//! Play with Friends, joining side: the engine's `friend:` addresses
//! (`horizon_client::net::tunnel`) read as invite codes, and joined through the
//! tunnel horizon-friends opens. Registered once at startup (entry.rs).
use horizon_client::net::tunnel::{self, Opened, Progress, Resolver};
use horizon_friends::{
    PathMode,
    guest::{self, GuestConfig},
    invite::{Parsed, parse},
};
use horizon_protocol::{GAME_VERSION, PROTOCOL_VERSION};
use std::net::SocketAddr;

struct Invites;

impl Resolver for Invites {
    fn is_code(&self, code: &str, full: bool) -> bool {
        match parse(code) {
            Some(Parsed::Long(_)) => true,
            Some(Parsed::Short(_)) => !full,
            None => false,
        }
    }

    fn nameplate(&self, code: &str) -> Option<String> {
        match parse(code) {
            Some(Parsed::Short(c)) => Some(c.nameplate),
            _ => None,
        }
    }

    /// The relay is kept beside the addresses: two hosts on other networks can
    /// share a private address and the fixed direct port.
    fn public_part(&self, code: &str) -> String {
        match parse(code) {
            Some(Parsed::Short(code)) => code.nameplate,
            Some(Parsed::Long(invite)) => {
                let relay = invite
                    .relay
                    .map(|r| format!("{}/{}", r.address, r.code.nameplate));
                let endpoints = invite.endpoints.iter().map(SocketAddr::to_string);
                endpoints.chain(relay).collect::<Vec<_>>().join(",")
            }
            None => String::new(),
        }
    }

    fn relay_from_env(&self) -> Option<String> {
        horizon_friends::relay_url()
    }

    fn open(
        &self,
        code: &str,
        relay: Option<String>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Opened, String> {
        progress(Progress::Finding);
        let tunnel = guest::join(&GuestConfig {
            code: code.to_owned(),
            relay,
            protocol: PROTOCOL_VERSION,
            game: GAME_VERSION.to_owned(),
            path: PathMode::from_env(),
        })
        .map_err(|e| e.to_string())?;
        progress(Progress::Connected {
            direct: tunnel.direct,
        });
        let loopback = tunnel.serve_loopback().map_err(|e| e.to_string())?;
        Ok(Opened {
            port: loopback.port,
            token: loopback.token.to_vec(),
        })
    }
}

/// Lets `friend:` addresses join (the first registration stays).
pub(crate) fn register() {
    // Both name the same addresses: the engine's join and the codes players paste.
    debug_assert_eq!(tunnel::PREFIX, horizon_friends::ADDRESS_PREFIX);
    let _ = tunnel::register(Box::new(Invites));
}

#[cfg(test)]
mod tests {
    use horizon_client::net::{FRIEND, is_invite, shown_address};
    use horizon_friends::{code::Code, invite::Invite};

    #[test]
    fn invites_are_read_and_their_secret_never_shown() {
        super::register();
        let invite = Invite {
            secret: *b"0123456789",
            port: 25580,
            endpoints: vec!["192.168.1.20:25580".parse().unwrap()],
            relay: None,
        }
        .encode();
        // Direct Connection takes a full invite; a short code needs Join a Friend.
        assert!(is_invite(&invite, true));
        let short = Code {
            nameplate: "K7QM".into(),
            secret: "3XPD".into(),
        }
        .to_string();
        assert!(is_invite(&short, false));
        assert!(!is_invite(&short, true));
        // A short code could be a host name, so Direct Connection keeps host names.
        for host in ["localhost", "example.org:25566", "abcdefgh", "[::1]:25568"] {
            assert!(!is_invite(host, true), "{host}");
        }
        assert_eq!(shown_address(&format!("{FRIEND}{invite}")), "Friend's world");
        assert_eq!(
            shown_address(&format!("{FRIEND}{short}")),
            "Friend's world (K7QM-****)"
        );
        assert_eq!(
            shown_address("mc.example.com:25565"),
            "mc.example.com:25565"
        );
    }
}
