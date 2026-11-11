//! The host's public IPv4 address, from a STUN Binding request (RFC 8489)
//! to public STUN servers. Only the address is used: the port a STUN server
//! sees is a UDP mapping, while the game listens on TCP.
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket},
    time::Duration,
};

/// [`horizon_addresses::STUN_SERVERS`]. Configurable with `HORIZON_STUN_SERVERS` (a comma list;
/// `none` turns STUN off).
pub const DEFAULT_SERVERS: &[&str] = horizon_addresses::STUN_SERVERS;

const MAGIC: u32 = 0x2112_A442;

/// The servers to ask: `HORIZON_STUN_SERVERS`, else the defaults.
pub fn servers_from_env() -> Vec<String> {
    match std::env::var("HORIZON_STUN_SERVERS") {
        Ok(v) => parse_servers(&v),
        Err(_) => DEFAULT_SERVERS.iter().map(|s| (*s).to_owned()).collect(),
    }
}
/// A comma list; empty or `none` means no STUN.
pub fn parse_servers(list: &str) -> Vec<String> {
    if list.trim().eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn request(id: &[u8; 12]) -> [u8; 20] {
    let mut m = [0u8; 20];
    m[0..2].copy_from_slice(&0x0001u16.to_be_bytes());
    m[4..8].copy_from_slice(&MAGIC.to_be_bytes());
    m[8..20].copy_from_slice(id);
    m
}

/// The mapped address in a Binding success response for transaction `id`.
pub fn parse_response(m: &[u8], id: &[u8; 12]) -> Option<SocketAddr> {
    if m.len() < 20 || m[0..2] != [0x01, 0x01] || m[4..8] != MAGIC.to_be_bytes() || &m[8..20] != id
    {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([m[2], m[3]]));
    let attrs = m.get(20..20 + len)?;
    let mut i = 0;
    let mut plain = None;
    while i + 4 <= attrs.len() {
        let kind = u16::from_be_bytes([attrs[i], attrs[i + 1]]);
        let alen = usize::from(u16::from_be_bytes([attrs[i + 2], attrs[i + 3]]));
        let value = attrs.get(i + 4..i + 4 + alen)?;
        let xor = kind == 0x0020;
        if (xor || kind == 0x0001) && value.len() >= 8 {
            let mut port = u16::from_be_bytes([value[2], value[3]]);
            if xor {
                port ^= (MAGIC >> 16) as u16;
            }
            let ip = match value[1] {
                1 => {
                    let mut o: [u8; 4] = value[4..8].try_into().ok()?;
                    if xor {
                        for (b, k) in o.iter_mut().zip(MAGIC.to_be_bytes()) {
                            *b ^= k;
                        }
                    }
                    IpAddr::V4(Ipv4Addr::from(o))
                }
                2 if value.len() >= 20 => {
                    let mut o: [u8; 16] = value[4..20].try_into().ok()?;
                    if xor {
                        let mut key = MAGIC.to_be_bytes().to_vec();
                        key.extend_from_slice(id);
                        for (b, k) in o.iter_mut().zip(key) {
                            *b ^= k;
                        }
                    }
                    IpAddr::V6(Ipv6Addr::from(o))
                }
                _ => return None,
            };
            let found = SocketAddr::new(ip, port);
            if xor {
                return Some(found);
            }
            plain = Some(found);
        }
        i += 4 + alen.div_ceil(4) * 4;
    }
    plain
}

/// Ask one server (resolved to IPv4), retrying twice.
pub fn query(server: &str, timeout: Duration) -> io::Result<SocketAddr> {
    let target = server
        .to_socket_addrs()?
        .find(SocketAddr::is_ipv4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no IPv4 address"))?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(timeout / 3))?;
    let mut id = [0u8; 12];
    crate::fill_random(&mut id)?;
    let mut buf = [0u8; 576];
    for _ in 0..3 {
        socket.send_to(&request(&id), target)?;
        // One deadline an attempt: the read timeout alone restarts with every datagram, and a
        // socket kept fed would hold the query (and the host's setup waiting on it) for good.
        let until = std::time::Instant::now() + timeout / 3;
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            socket.set_read_timeout(Some(left))?;
            match socket.recv_from(&mut buf) {
                Ok((n, from)) if from == target => {
                    if let Some(a) = parse_response(&buf[..n], &id) {
                        return Ok(a);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "no STUN answer"))
}

/// The public IPv4 address from the first server that answers.
pub fn public_ip(servers: &[String], timeout: Duration) -> Option<IpAddr> {
    servers
        .iter()
        .find_map(|s| query(s, timeout).ok())
        .map(|a| a.ip())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A server that never answers the query but keeps the socket fed with other datagrams.
    #[test]
    fn a_query_kept_fed_with_noise_still_ends() {
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server = silent.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 576];
            if let Ok((_, from)) = silent.recv_from(&mut buf) {
                for _ in 0..200 {
                    let _ = silent.send_to(b"noise", from);
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        });
        let started = std::time::Instant::now();
        assert!(query(&server.to_string(), Duration::from_millis(300)).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    /// A STUN server answering with XOR-MAPPED-ADDRESS = `answer`.
    pub(crate) fn mock(answer: SocketAddr) -> SocketAddr {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = s.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 576];
            while let Ok((n, from)) = s.recv_from(&mut buf) {
                if n < 20 {
                    continue;
                }
                let id: [u8; 12] = buf[8..20].try_into().unwrap();
                let _ = s.send_to(&response(&id, answer), from);
            }
        });
        addr
    }
    pub(crate) fn response(id: &[u8; 12], a: SocketAddr) -> Vec<u8> {
        let IpAddr::V4(ip) = a.ip() else { panic!() };
        let mut attr = vec![0x00, 0x20, 0x00, 0x08, 0x00, 0x01];
        attr.extend_from_slice(&(a.port() ^ (MAGIC >> 16) as u16).to_be_bytes());
        let o = ip.octets();
        let k = MAGIC.to_be_bytes();
        attr.extend((0..4).map(|i| o[i] ^ k[i]));
        let mut m = vec![0x01, 0x01];
        m.extend_from_slice(&(attr.len() as u16).to_be_bytes());
        m.extend_from_slice(&MAGIC.to_be_bytes());
        m.extend_from_slice(id);
        m.extend(attr);
        m
    }

    #[test]
    fn asks_and_reads_the_mapped_address() {
        let answer: SocketAddr = "203.0.113.9:40000".parse().unwrap();
        let server = mock(answer);
        assert_eq!(
            query(&server.to_string(), Duration::from_secs(2)).unwrap(),
            answer
        );
        // The first server that answers wins; dead ones are skipped.
        let dead = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .to_string();
        assert_eq!(
            public_ip(&[dead, server.to_string()], Duration::from_millis(300)),
            Some(answer.ip())
        );
    }

    #[test]
    fn responses_are_checked() {
        let id = [7u8; 12];
        let a: SocketAddr = "198.51.100.4:1234".parse().unwrap();
        let good = response(&id, a);
        assert_eq!(parse_response(&good, &id), Some(a));
        assert_eq!(parse_response(&good, &[8u8; 12]), None);
        let mut error = good.clone();
        error[1] = 0x11;
        assert_eq!(parse_response(&error, &id), None);
        assert_eq!(parse_response(&good[..10], &id), None);
    }

    #[test]
    fn server_lists() {
        assert_eq!(parse_servers("none"), Vec::<String>::new());
        assert_eq!(parse_servers(" a:1 , b:2,"), vec!["a:1", "b:2"]);
    }
}
