//! How a host can be reached, worked out on its own: its interface
//! addresses (LAN, VPNs such as Tailscale or ZeroTier, IPv6), a port opened
//! on the router (PCP / NAT-PMP / UPnP), and its public address from STUN.
//! The endpoints go into the invite code; the verdict tells the host what
//! friends outside its network can expect, and what to do when it's nothing.
use crate::portmap::{self, Mapping};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

/// Where the troubleshooting lives (shown to hosts nobody can reach).
pub const HELP_URL: &str =
    "https://github.com/horizon-rust/horizon/blob/main/docs/play-with-friends.md#troubleshooting";

#[derive(Clone, Debug)]
pub struct Options {
    /// Ask the router to open the port.
    pub map_port: bool,
    pub stun: Vec<String>,
    /// Offer 127.0.0.1 too (tests with both ends on one machine).
    pub loopback: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Reach {
    pub port: u16,
    pub local: Vec<IpAddr>,
    pub mapping: Option<Mapping>,
    pub mapping_error: Option<String>,
    pub public_ip: Option<IpAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The router opened the port (method name).
    Mapped(&'static str),
    /// This machine has the public address itself.
    Public,
    /// Only the LAN and VPNs reach it; `cgnat` when the router is itself
    /// behind another NAT (the provider's, or a second router), so port
    /// forwarding on it can't help.
    LocalOnly { cgnat: bool },
}

/// 100.64.0.0/10 (carrier-grade NAT, also Tailscale's range) or private.
fn shared(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private() || (o[0] == 100 && (64..128).contains(&o[1]))
        }
        IpAddr::V6(_) => false,
    }
}

/// Interface names of containers and VMs, whose addresses nobody else reaches.
fn virtual_interface(name: &str) -> bool {
    [
        "docker", "br-", "veth", "virbr", "vboxnet", "vmnet", "lxc", "cni", "podman",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

/// This machine's addresses worth offering, best first: private IPv4 (the
/// LAN), shared IPv4 (Tailscale's 100.64/10, CGNAT), other IPv4, one global
/// IPv6. No link-local or unique-local; loopback on request.
pub fn interface_addresses(loopback: bool) -> Vec<IpAddr> {
    let mut found: Vec<(u8, IpAddr)> = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        if virtual_interface(&i.name) {
            continue;
        }
        let ip = i.ip();
        let rank = match ip {
            IpAddr::V4(v4) if v4.is_loopback() => {
                if loopback {
                    6
                } else {
                    continue;
                }
            }
            IpAddr::V4(v4) if v4.is_link_local() || v4.is_unspecified() => continue,
            IpAddr::V4(v4) if v4.is_private() => 0,
            v4 @ IpAddr::V4(_) if shared(v4) => 1,
            IpAddr::V4(_) => 2,
            IpAddr::V6(v6) if v6.is_loopback() || v6.is_unspecified() => continue,
            // fe80::/10.
            IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80 => continue,
            // fc00::/7: VPNs that use it (Tailscale) also give an IPv4 address.
            IpAddr::V6(v6) if v6.segments()[0] & 0xfe00 == 0xfc00 => continue,
            IpAddr::V6(_) => 3,
        };
        if !found.iter().any(|(_, a)| *a == ip) {
            found.push((rank, ip));
        }
    }
    found.sort_by_key(|(r, _)| *r);
    // One global IPv6 address is enough (temporary addresses multiply
    // them), and one address per /24 (Wi-Fi and Ethernet on the same LAN).
    let mut out: Vec<IpAddr> = Vec::new();
    for (_, ip) in found {
        let same = out.iter().any(|o| match (o, ip) {
            (IpAddr::V6(_), IpAddr::V6(_)) => true,
            (IpAddr::V4(a), IpAddr::V4(b)) => a.octets()[..3] == b.octets()[..3],
            _ => false,
        });
        if !same {
            out.push(ip);
        }
    }
    out
}

impl Reach {
    /// Find out (takes a few seconds: router discovery and STUN).
    pub fn discover(port: u16, options: &Options) -> Self {
        let local = interface_addresses(options.loopback);
        let stun = {
            let servers = options.stun.clone();
            std::thread::spawn(move || crate::stun::public_ip(&servers, Duration::from_secs(2)))
        };
        let (mapping, mapping_error) = if options.map_port {
            match local.iter().find_map(|ip| match ip {
                IpAddr::V4(v4) if v4.is_private() => Some(*v4),
                _ => None,
            }) {
                Some(client) => {
                    let targets = portmap::Targets::discover(Duration::from_millis(1500));
                    match portmap::map(&targets, client, port, portmap::LEASE) {
                        Ok(m) => (Some(m), None),
                        Err(e) => (None, Some(e)),
                    }
                }
                None => (
                    None,
                    Some("no private IPv4 address (no router to ask)".into()),
                ),
            }
        } else {
            (None, Some("port mapping is off".into()))
        };
        let public_ip = stun.join().ok().flatten();
        Self {
            port,
            local,
            mapping,
            mapping_error,
            public_ip,
        }
    }

    /// The router's public side, with the WAN address from STUN when the
    /// router didn't give one.
    fn mapped_endpoint(&self) -> Option<SocketAddr> {
        let m = self.mapping.as_ref()?;
        let ip = if m.external.ip().is_unspecified() {
            self.public_ip?
        } else {
            m.external.ip()
        };
        Some(SocketAddr::new(ip, m.external.port()))
    }

    pub fn verdict(&self) -> Verdict {
        if let Some(m) = &self.mapping {
            let wan = m.external.ip();
            // The router's own WAN side is shared or private, or STUN sees
            // another address: a second NAT upstream.
            let behind = (!wan.is_unspecified() && shared(wan))
                || self
                    .public_ip
                    .is_some_and(|p| !wan.is_unspecified() && p != wan);
            if !behind {
                return Verdict::Mapped(m.method.name());
            }
            return Verdict::LocalOnly { cgnat: true };
        }
        if let Some(p) = self.public_ip
            && self.local.contains(&p)
        {
            return Verdict::Public;
        }
        Verdict::LocalOnly { cgnat: false }
    }

    /// The code's endpoints, best first: the router mapping (or this
    /// machine's own public address), then the public address on the
    /// direct port (works once the port is forwarded by hand), then the
    /// interface addresses.
    pub fn endpoints(&self) -> Vec<SocketAddr> {
        let mut out = Vec::new();
        let mut push = |a: SocketAddr| {
            if !out.contains(&a) {
                out.push(a);
            }
        };
        if let Some(m) = self.mapped_endpoint() {
            push(m);
        }
        if let Some(p) = self.public_ip {
            push(SocketAddr::new(p, self.port));
        }
        for ip in &self.local {
            push(SocketAddr::new(*ip, self.port));
        }
        out.truncate(crate::invite::MAX_ENDPOINTS);
        out
    }

    /// The LAN address a port forward should point at.
    pub fn lan_ip(&self) -> Option<Ipv4Addr> {
        self.local.iter().find_map(|ip| match ip {
            IpAddr::V4(v4) if v4.is_private() => Some(*v4),
            _ => None,
        })
    }

    /// What the host reads under the code.
    pub fn summary(&self) -> String {
        match self.verdict() {
            Verdict::Mapped(method) => {
                let at = self.mapped_endpoint().map(|a| format!(" ({a})")).unwrap_or_default();
                format!("Friends anywhere can join: your router opened the port with {method}{at}.")
            }
            Verdict::Public => format!(
                "Friends anywhere can join: this computer has a public address (TCP port {} must not be blocked by a firewall).",
                self.port
            ),
            Verdict::LocalOnly { cgnat: true } => "Friends on your network or VPN can join. From the internet they can't: \
                 your router is itself behind another NAT (your provider's carrier-grade NAT, CGNAT, or a second router), \
                 so opening its port isn't enough. Use a relay or a VPN such as Tailscale or ZeroTier."
                .to_owned(),
            Verdict::LocalOnly { cgnat: false } => {
                let forward = match self.lan_ip() {
                    Some(ip) => format!("forward TCP port {} to {ip}", self.port),
                    None => format!("forward TCP port {}", self.port),
                };
                format!(
                    "Friends on your network or VPN can join. For friends on the internet: turn on UPnP on your router, \
                     or {forward}, or use a relay, or a VPN such as Tailscale or ZeroTier (the code already holds \
                     your VPN address)."
                )
            }
        }
    }

    /// Delete the router mapping.
    pub fn release(&mut self) {
        if let Some(m) = self.mapping.take() {
            m.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach(public: Option<&str>, local: &[&str]) -> Reach {
        Reach {
            port: 25580,
            local: local.iter().map(|s| s.parse().unwrap()).collect(),
            mapping: None,
            mapping_error: Some("no router answered".into()),
            public_ip: public.map(|s| s.parse().unwrap()),
        }
    }

    #[test]
    fn endpoints_put_the_public_side_first() {
        let r = reach(
            Some("203.0.113.5"),
            &["192.168.1.20", "100.72.51.8", "2001:db8::20"],
        );
        assert_eq!(
            r.endpoints(),
            [
                "203.0.113.5:25580",
                "192.168.1.20:25580",
                "100.72.51.8:25580",
                "[2001:db8::20]:25580"
            ]
            .map(|s| s.parse::<SocketAddr>().unwrap())
        );
        assert_eq!(r.verdict(), Verdict::LocalOnly { cgnat: false });
        let s = r.summary();
        assert!(
            s.contains("forward TCP port 25580 to 192.168.1.20") && s.contains("Tailscale"),
            "{s}"
        );
    }

    #[test]
    fn a_public_address_on_this_machine() {
        let r = reach(Some("203.0.113.5"), &["203.0.113.5"]);
        assert_eq!(r.verdict(), Verdict::Public);
        assert_eq!(
            r.endpoints(),
            vec!["203.0.113.5:25580".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn a_router_mapping_and_what_is_behind_it() {
        use crate::portmap::{Mapping, Method};
        let mut r = reach(Some("203.0.113.5"), &["192.168.1.20"]);
        r.mapping = Some(Mapping::granted(
            Method::Upnp,
            "203.0.113.5:40123".parse().unwrap(),
        ));
        assert_eq!(r.verdict(), Verdict::Mapped("UPnP"));
        assert_eq!(
            r.endpoints()[0],
            "203.0.113.5:40123".parse::<SocketAddr>().unwrap()
        );
        assert!(r.summary().contains("UPnP (203.0.113.5:40123)"));
        // The router didn't say its address: STUN's is used.
        r.mapping = Some(Mapping::granted(
            Method::NatPmp,
            "0.0.0.0:40123".parse().unwrap(),
        ));
        assert_eq!(
            r.endpoints()[0],
            "203.0.113.5:40123".parse::<SocketAddr>().unwrap()
        );
        // The router's WAN side is in the carrier's shared range: CGNAT.
        r.mapping = Some(Mapping::granted(
            Method::Pcp,
            "100.80.1.2:40123".parse().unwrap(),
        ));
        assert_eq!(r.verdict(), Verdict::LocalOnly { cgnat: true });
        assert!(r.summary().contains("CGNAT"));
        // STUN sees another address than the router's: a second NAT.
        r.mapping = Some(Mapping::granted(
            Method::Pcp,
            "198.51.100.1:40123".parse().unwrap(),
        ));
        assert_eq!(r.verdict(), Verdict::LocalOnly { cgnat: true });
    }

    #[test]
    fn shared_ranges() {
        assert!(shared("100.72.51.8".parse().unwrap()));
        assert!(shared("10.1.2.3".parse().unwrap()));
        assert!(!shared("100.128.0.1".parse().unwrap()));
        assert!(!shared("8.8.8.8".parse().unwrap()));
        assert!(
            virtual_interface("docker0")
                && !virtual_interface("tailscale0")
                && !virtual_interface("en0")
        );
    }

    #[test]
    fn interface_addresses_skip_loopback_unless_asked() {
        assert!(interface_addresses(false).iter().all(|a| !a.is_loopback()));
        assert!(interface_addresses(true).contains(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }
}
