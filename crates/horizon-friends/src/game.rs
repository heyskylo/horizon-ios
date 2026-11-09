//! Play with Friends in the game, joining side (the `game` feature): the engine's `friend:`
//! addresses (`horizon_platform::hooks::tunnel`) read as invite codes, and joined through this
//! crate's tunnel. `horizon-official` registers [`HOOKS`]; the game's own executable has none, so
//! there no invite code joins.
use crate::{
    PathMode,
    guest::{self, GuestConfig},
    invite::{Parsed, parse},
};
use horizon_platform::{
    hooks::{
        Hooks,
        tunnel::{Opened, Progress, Resolver},
    },
    version::{PROTOCOL_VERSION, VANILLA_VERSION},
};
use std::net::SocketAddr;

/// What `horizon-official` registers: Play with Friends' hooks.
pub static HOOKS: Friends = Friends;

pub struct Friends;

impl Hooks for Friends {
    fn tunnel(&self) -> Option<&dyn Resolver> {
        Some(&Invites)
    }
}

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
        crate::relay_url()
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
            game: VANILLA_VERSION.to_owned(),
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

#[cfg(test)]
mod tests {
    use super::{Invites, Resolver};
    use horizon_platform::hooks::tunnel;
    use crate::{
        code::Code,
        invite::{self, Invite},
    };

    fn short(nameplate: &str, secret: &str) -> Code {
        Code {
            nameplate: nameplate.into(),
            secret: secret.into(),
        }
    }

    #[test]
    fn the_engine_and_the_codes_name_the_same_addresses() {
        assert_eq!(tunnel::PREFIX, crate::ADDRESS_PREFIX);
    }

    #[test]
    fn invites_are_read_and_their_secret_never_shown() {
        let invite = Invite {
            secret: *b"0123456789",
            port: 25580,
            endpoints: vec!["192.168.1.20:25580".parse().unwrap()],
            relay: None,
        }
        .encode();
        // Direct Connection takes a full invite; a short code needs Join a Friend.
        assert!(Invites.is_code(&invite, true));
        let short = short("K7QM", "3XPD").to_string();
        assert!(Invites.is_code(&short, false));
        assert!(!Invites.is_code(&short, true));
        // A short code could be a host name, so Direct Connection keeps host names.
        for host in ["localhost", "example.org:25566", "abcdefgh", "[::1]:25568"] {
            assert!(!Invites.is_code(host, true), "{host}");
        }
        // Only a short code's name may be shown; a full invite is all secret.
        assert_eq!(Invites.nameplate(&invite), None);
        assert_eq!(Invites.nameplate(&short).as_deref(), Some("K7QM"));
    }

    #[test]
    fn friends_worlds_are_told_apart_by_their_hosts_never_by_their_secrets() {
        const PORT: u16 = 50000;
        const RELAY: &str = "relay.example.com:443";
        let invite = |secret: u8, host: &str, relay: Option<Code>| {
            let code = Invite {
                secret: [secret; invite::SECRET],
                port: PORT,
                endpoints: vec![format!("{host}:{PORT}").parse().unwrap()],
                relay: relay.map(|code| invite::RelayRef {
                    address: RELAY.into(),
                    code,
                }),
            }
            .encode();
            Invites.public_part(&code)
        };
        assert_ne!(
            invite(1, "192.168.1.5", None),
            invite(1, "192.168.1.6", None),
            "two friends"
        );
        assert_ne!(
            invite(1, "192.168.1.5", Some(short("ABCD", "WXYZ"))),
            invite(1, "192.168.1.5", Some(short("EFGH", "WXYZ"))),
            "two friends on other networks behind the same private address"
        );
        assert_eq!(
            invite(1, "192.168.1.5", Some(short("ABCD", "WXYZ"))),
            invite(2, "192.168.1.5", Some(short("ABCD", "QRST"))),
            "the invite's and the relay code's secrets take no part"
        );
        let typed = |code: Code| Invites.public_part(&code.to_string());
        assert_eq!(typed(short("ABCD", "WXYZ")), typed(short("ABCD", "QRST")));
        assert_eq!(typed(short("ABCD", "WXYZ")), "ABCD");
    }
}
