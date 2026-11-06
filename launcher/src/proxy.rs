//! The proxy every download and request goes through, when the player needs one: a school's or an
//! office's, or a country where a host is unreachable. The Settings page's Proxy row (kept as
//! `proxy` in `settings.json`) wins; without it the usual `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`
//! and `NO_PROXY` variables apply, so a launcher started from a shell that has them needs nothing.
//!
//! What goes through it: Horizon's releases and the launcher's own update, Minecraft's files and
//! Java from Mojang, Modrinth, CurseForge, Horizon's mod registry, pictures, Microsoft sign-in,
//! Tailscale's API, and the jars a modpack import downloads. A local address never does, and the
//! Servers page's calls to a game server's API (`servers.rs`) build their agent without it: they
//! are on loopback and asked every second.
//!
//! Which hosts are reached directly, and how an address is read, are `horizon-port-pack`'s rules
//! (`crates/horizon-port-pack/src/net.rs`): the jars an import fetches are downloaded there, and
//! the same hosts have to be left alone on both sides. This module adds what is the launcher's
//! own — the setting in `settings.json`, kept in memory so a request doesn't read the file — and
//! hands the address to the parts that build their agents.
use crate::backend::Settings;
use std::sync::Mutex;

/// The launcher's setting, as `announce` last read it, or None before it has: a request shouldn't
/// read `settings.json` every time, and an import fetches hundreds of jars.
static SET: Mutex<Option<Option<String>>> = Mutex::new(None);

/// The proxy address to reach `url` through: the launcher's setting, else the environment's, unless
/// `url` is a local address or `NO_PROXY` names its host.
pub fn spec_for(url: &str) -> Option<String> {
    let scheme = horizon_port_pack::net::scheme_of(url)?;
    if horizon_port_pack::net::direct(url) {
        return None;
    }
    setting().or_else(|| horizon_port_pack::net::env_for(scheme))
}

/// The proxy to reach `url` through: `spec_for`'s address, read as one.
pub fn for_url(url: &str) -> Option<ureq::Proxy> {
    spec_for(url).and_then(|spec| horizon_port_pack::net::parse(&spec))
}

/// `builder`, reaching `url` through the proxy when one is set. For the parts that ask for a few
/// addresses; the ones that fetch file after file keep their agent instead (`keep`).
pub fn agent(builder: ureq::AgentBuilder, url: &str) -> ureq::AgentBuilder {
    match for_url(url) {
        Some(proxy) => builder.proxy(proxy),
        None => builder,
    }
}

/// Why `spec` can't be used as a proxy, or None when it can (empty means none).
pub fn refuse(spec: &str) -> Option<String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    // `ureq` takes `http://` without a host, which would leave a download failing with nothing
    // said about the proxy.
    if horizon_port_pack::net::parse(spec).is_none()
        || horizon_port_pack::net::host_of(spec).is_none_or(str::is_empty)
    {
        return Some(format!(
            "{} isn't a proxy address. Write it as http://host:port or socks5://host:port.",
            horizon_port_pack::net::redacted(spec)
        ));
    }
    None
}

/// Tells the parts that build their own agents (`horizon-port-pack`) which proxy to use, and keeps
/// the setting for the launcher's own requests. Called with what `settings.json` holds: at start,
/// and whenever the player changes the setting.
///
/// The environment's is left to them: they read it per address (`HTTPS_PROXY` for an https jar,
/// `HTTP_PROXY` for an http one), the way the launcher does for its own.
pub fn announce(setting: Option<String>) {
    let spec = setting
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty());
    *held() = Some(spec.clone());
    horizon_port_pack::net::set_proxy(spec);
}

/// The launcher's setting: read from `settings.json` the first time it is needed and kept after
/// that, so a download doesn't read the file once per file.
fn setting() -> Option<String> {
    let mut set = held();
    match set.clone() {
        Some(kept) => kept,
        None => {
            let read = written();
            *set = Some(read.clone());
            read
        }
    }
}

/// The launcher's setting as it is written down, or None when there is none.
fn written() -> Option<String> {
    Settings::load()
        .proxy
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
}

/// The setting's lock, even after a thread panicked while holding it: an address lost to a panic
/// is worse than an old one kept.
fn held() -> std::sync::MutexGuard<'static, Option<Option<String>>> {
    SET.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_address_takes_no_proxy() {
        assert_eq!(spec_for("http://127.0.0.1:8130/api/v1/status"), None);
        assert_eq!(spec_for("http://localhost/api/v1/status"), None);
    }

    /// An address the launcher knows nothing about is one nothing is written for either.
    #[test]
    fn a_relative_address_has_no_scheme_to_read_a_proxy_for() {
        assert_eq!(spec_for("data/a.jar"), None);
    }

    #[test]
    fn a_proxy_address_is_checked_before_it_is_kept() {
        assert_eq!(refuse(""), None);
        assert_eq!(refuse("  "), None);
        assert_eq!(refuse("http://127.0.0.1:7890"), None);
        assert_eq!(refuse("127.0.0.1:7890"), None);
        assert_eq!(refuse("http://user:pass@proxy.local:3128"), None);
        assert!(refuse("http://").is_some());
        assert_eq!(refuse("socks5://127.0.0.1:7891"), None);
        assert_eq!(refuse("socks5h://user:pass@127.0.0.1:7891"), None);
        assert!(refuse("socks5://").is_some());
    }

    /// The reason is shown under the field and printed by `horizon-launcher settings set`.
    #[test]
    fn a_refused_address_keeps_its_credentials_to_itself() {
        let why = refuse("socks3://user:secret@127.0.0.1:7891").unwrap();
        assert!(why.starts_with("socks3://***@127.0.0.1:7891 "), "{why}");
        let why = refuse("http://user:secret@").unwrap();
        assert!(!why.contains("secret"), "{why}");
    }
}
