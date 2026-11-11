//! The invite code that needs no server: the host's reachable endpoints and
//! the session secret, in Crockford base32 groups with a checksum, e.g.
//! `2A7KQ-M3XPD-...`. A guest dials the endpoints directly; when the host
//! also uses a relay, the code carries the relay's address and the short
//! code too, so the guest can fall back to it without any setting.
//!
//! Layout (bytes, then base32): version (2), secret (10), the direct port
//! (u16), entries, then the first two bytes of SHA-256 over all of it.
//! Entries: `0x04` IPv4 on the direct port, `0x05` IPv4 and its own port
//! (a router mapping), `0x06`/`0x07` the same for IPv6, `0x10` a relay:
//! address length, address, then the short code's 40 bits.
use crate::code::{ALPHABET, Code, HALF};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const VERSION: u8 = 2;
pub const SECRET: usize = 10;
/// Endpoints a code carries at most.
pub const MAX_ENDPOINTS: usize = 8;
const GROUP: usize = 5;

/// The relay a host also registered with: where, and its short code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayRef {
    pub address: String,
    pub code: Code,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Invite {
    pub secret: [u8; SECRET],
    /// The host's direct socket: LAN, VPN and IPv6 addresses use this port.
    pub port: u16,
    pub endpoints: Vec<SocketAddr>,
    pub relay: Option<RelayRef>,
}
/// The secret stays out of logs.
impl std::fmt::Debug for Invite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invite")
            .field("port", &self.port)
            .field("endpoints", &self.endpoints)
            .field("relay", &self.relay.as_ref().map(|r| &r.address))
            .finish_non_exhaustive()
    }
}

/// What a player pasted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Eight symbols: a relay's short code (needs that relay).
    Short(Code),
    Long(Invite),
}

/// Why a code didn't parse, for the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeError {
    Characters,
    Typo,
    Version,
}
impl std::fmt::Display for CodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Characters => {
                "That isn't an invite code: codes are letters and digits in groups."
            }
            Self::Typo => "The code has a typo: copy it again from your friend's message.",
            Self::Version => {
                "This invite comes from another version of Horizon. Update both games."
            }
        })
    }
}

fn symbol(c: char) -> Option<u8> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    ALPHABET
        .iter()
        .position(|&a| char::from(a) == c)
        .map(|i| i as u8)
}

fn encode32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

fn decode32(symbols: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(symbols.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &v in symbols {
        acc = ((acc << 5) | u32::from(v)) & 0xFFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // Padding bits must be zero (a canonical encoding).
    (acc & ((1 << bits) - 1) == 0).then_some(out)
}

fn checksum(bytes: &[u8]) -> [u8; 2] {
    let h = Sha256::digest(bytes);
    [h[0], h[1]]
}

/// The 40 bits of a short code (20 per half).
fn short_bits(code: &Code) -> Option<u64> {
    let mut v = 0u64;
    for c in code.nameplate.chars().chain(code.secret.chars()) {
        v = (v << 5) | u64::from(symbol(c)?);
    }
    Some(v)
}
fn short_from_bits(v: u64) -> Code {
    let s: String = (0..2 * HALF)
        .rev()
        .map(|i| char::from(ALPHABET[((v >> (5 * i)) & 31) as usize]))
        .collect();
    let (n, sec) = s.split_at(HALF);
    Code {
        nameplate: n.to_owned(),
        secret: sec.to_owned(),
    }
}

impl Invite {
    /// A fresh secret from the OS random generator.
    pub fn new(
        port: u16,
        endpoints: Vec<SocketAddr>,
        relay: Option<RelayRef>,
    ) -> std::io::Result<Self> {
        let mut secret = [0u8; SECRET];
        crate::fill_random(&mut secret)?;
        Ok(Self {
            secret,
            port,
            endpoints,
            relay,
        })
    }
    /// The SPAKE2 password of direct sessions.
    pub fn secret_text(&self) -> String {
        encode32(&self.secret)
    }
    fn bytes(&self) -> Vec<u8> {
        let mut b = vec![VERSION];
        b.extend_from_slice(&self.secret);
        b.extend_from_slice(&self.port.to_be_bytes());
        for e in self.endpoints.iter().take(MAX_ENDPOINTS) {
            let own = e.port() != self.port;
            match e.ip() {
                IpAddr::V4(ip) => {
                    b.push(if own { 0x05 } else { 0x04 });
                    b.extend_from_slice(&ip.octets());
                }
                IpAddr::V6(ip) => {
                    b.push(if own { 0x07 } else { 0x06 });
                    b.extend_from_slice(&ip.octets());
                }
            }
            if own {
                b.extend_from_slice(&e.port().to_be_bytes());
            }
        }
        if let Some(r) = &self.relay
            && let Some(bits) = short_bits(&r.code)
            && r.address.len() <= 255
        {
            b.push(0x10);
            b.push(r.address.len() as u8);
            b.extend_from_slice(r.address.as_bytes());
            b.extend_from_slice(&bits.to_be_bytes()[3..]);
        }
        let sum = checksum(&b);
        b.extend_from_slice(&sum);
        b
    }
    /// The code to share, in groups of five.
    pub fn encode(&self) -> String {
        let text = encode32(&self.bytes());
        text.as_bytes()
            .chunks(GROUP)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("-")
    }
    fn decode(bytes: &[u8]) -> Result<Self, CodeError> {
        if bytes.len() < 1 + SECRET + 2 + 2 {
            return Err(CodeError::Characters);
        }
        let (body, sum) = bytes.split_at(bytes.len() - 2);
        if checksum(body) != sum {
            return Err(CodeError::Typo);
        }
        if body[0] != VERSION {
            return Err(CodeError::Version);
        }
        let mut secret = [0u8; SECRET];
        secret.copy_from_slice(&body[1..1 + SECRET]);
        let port = u16::from_be_bytes([body[1 + SECRET], body[2 + SECRET]]);
        let mut rest = &body[3 + SECRET..];
        let mut take = |n: usize| -> Result<&[u8], CodeError> {
            if rest.len() < n {
                return Err(CodeError::Version);
            }
            let (a, b) = rest.split_at(n);
            rest = b;
            Ok(a)
        };
        let mut endpoints = Vec::new();
        let mut relay = None;
        while let Ok(tag) = take(1) {
            let tag = tag[0];
            match tag {
                0x04..=0x07 => {
                    let ip = if tag < 0x06 {
                        let o: [u8; 4] = take(4)?.try_into().unwrap();
                        IpAddr::V4(Ipv4Addr::from(o))
                    } else {
                        let o: [u8; 16] = take(16)?.try_into().unwrap();
                        IpAddr::V6(Ipv6Addr::from(o))
                    };
                    let p = if tag & 1 == 1 {
                        let p = take(2)?;
                        u16::from_be_bytes([p[0], p[1]])
                    } else {
                        port
                    };
                    // As `encode` writes them: a crafted code can't make the guest dial thousands.
                    if endpoints.len() == MAX_ENDPOINTS {
                        return Err(CodeError::Version);
                    }
                    endpoints.push(SocketAddr::new(ip, p));
                }
                0x10 => {
                    let len = usize::from(take(1)?[0]);
                    let address =
                        String::from_utf8(take(len)?.to_vec()).map_err(|_| CodeError::Version)?;
                    let b = take(5)?;
                    let bits = b.iter().fold(0u64, |v, &x| (v << 8) | u64::from(x));
                    relay = Some(RelayRef {
                        address,
                        code: short_from_bits(bits),
                    });
                }
                _ => return Err(CodeError::Version),
            }
        }
        Ok(Self {
            secret,
            port,
            endpoints,
            relay,
        })
    }
}

/// Read what a player pasted: a short relay code or a full invite.
pub fn parse_detailed(text: &str) -> Result<Parsed, CodeError> {
    let mut symbols = Vec::with_capacity(text.len());
    for c in text.chars() {
        if c.is_whitespace() || c == '-' {
            continue;
        }
        symbols.push(symbol(c).ok_or(CodeError::Characters)?);
    }
    if symbols.len() == 2 * HALF {
        let s: String = symbols
            .iter()
            .map(|&v| char::from(ALPHABET[v as usize]))
            .collect();
        return Code::parse(&s)
            .map(Parsed::Short)
            .ok_or(CodeError::Characters);
    }
    let bytes = decode32(&symbols).ok_or(CodeError::Typo)?;
    Invite::decode(&bytes).map(Parsed::Long)
}
pub fn parse(text: &str) -> Option<Parsed> {
    parse_detailed(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_with_more_endpoints_than_encode_writes_is_refused() {
        // `bytes` writes at most MAX_ENDPOINTS: built by hand past it, with a valid checksum.
        let with = |n: usize| {
            let mut b = vec![VERSION];
            b.extend_from_slice(b"0123456789");
            b.extend_from_slice(&25580u16.to_be_bytes());
            for i in 0..n {
                b.push(0x04);
                b.extend_from_slice(&[10, 0, 0, i as u8]);
            }
            let sum = checksum(&b);
            b.extend_from_slice(&sum);
            Invite::decode(&b)
        };
        assert_eq!(with(MAX_ENDPOINTS).unwrap().endpoints.len(), MAX_ENDPOINTS);
        assert!(with(MAX_ENDPOINTS + 1).is_err());
    }

    fn sample() -> Invite {
        Invite {
            secret: *b"0123456789",
            port: 25580,
            endpoints: vec![
                "203.0.113.5:41234".parse().unwrap(),
                "192.168.1.20:25580".parse().unwrap(),
                "100.72.51.8:25580".parse().unwrap(),
                "[2001:db8::20]:25580".parse().unwrap(),
            ],
            relay: None,
        }
    }

    #[test]
    fn base32_round_trips() {
        for n in 0..40 {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let text = encode32(&bytes);
            let symbols: Vec<u8> = text.chars().map(|c| symbol(c).unwrap()).collect();
            assert_eq!(decode32(&symbols).unwrap(), bytes);
        }
    }

    #[test]
    fn codes_round_trip_and_stay_pasteable() {
        let invite = sample();
        let code = invite.encode();
        assert!(
            code.split('-')
                .all(|g| g.len() <= GROUP && g.chars().all(|c| ALPHABET.contains(&(c as u8))))
        );
        // A home with a router mapping, a LAN, a tailnet and IPv6: 79 symbols
        // in 16 groups; without IPv6, 52.
        assert_eq!(code.replace('-', "").len(), 79, "{code}");
        let mut v4 = invite.clone();
        v4.endpoints.pop();
        assert_eq!(v4.encode().replace('-', "").len(), 52);
        assert_eq!(parse(&code), Some(Parsed::Long(invite.clone())));
        // Case, spaces and look-alike letters don't matter.
        let sloppy = code.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(parse(&sloppy), Some(Parsed::Long(invite)));
    }

    #[test]
    fn a_relay_rides_along() {
        let mut invite = sample();
        invite.relay = Some(RelayRef {
            address: "relay.example.org:7000".into(),
            code: Code::parse("K7QM-3XPD").unwrap(),
        });
        let Some(Parsed::Long(back)) = parse(&invite.encode()) else {
            panic!()
        };
        assert_eq!(back.relay, invite.relay);
        assert_eq!(back.endpoints, invite.endpoints);
    }

    #[test]
    fn typos_are_caught() {
        let code = sample().encode();
        let mut caught = 0;
        let symbols: Vec<char> = code.chars().collect();
        for i in (0..symbols.len()).filter(|&i| symbols[i] != '-') {
            let mut changed = symbols.clone();
            changed[i] = if changed[i] == 'Z' { 'Y' } else { 'Z' };
            let text: String = changed.into_iter().collect();
            if parse_detailed(&text).is_err() {
                caught += 1;
            }
        }
        // Every single-symbol change is refused (the checksum misses one in 65536).
        assert_eq!(caught, symbols.iter().filter(|&&c| c != '-').count());
        // A dropped group too.
        let shorter: String = code.split('-').skip(1).collect::<Vec<_>>().join("-");
        assert!(parse(&shorter).is_none());
    }

    #[test]
    fn short_codes_and_garbage() {
        assert_eq!(
            parse("k7qm-3xpd"),
            Some(Parsed::Short(Code::parse("K7QM-3XPD").unwrap()))
        );
        assert_eq!(parse_detailed("hello world!"), Err(CodeError::Characters));
        assert!(parse("").is_none());
        // Another version is told apart from a typo.
        let mut b = sample().bytes();
        b[0] = 3;
        let n = b.len() - 2;
        let sum = checksum(&b[..n]);
        b[n..].copy_from_slice(&sum);
        assert_eq!(parse_detailed(&encode32(&b)), Err(CodeError::Version));
    }

    #[test]
    fn debug_hides_the_secret() {
        assert!(!format!("{:?}", sample()).contains("0123456789"));
    }
}
