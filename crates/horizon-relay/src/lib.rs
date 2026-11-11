//! Play with friends' relay. Hosts register and get a nameplate (the
//! relay-visible half of an invite code); guests resolve it; the relay
//! pairs each guest's connection with a fresh one from the host and copies
//! bytes between them. Those bytes are the end-to-end encrypted session
//! (docs/play-with-friends.md): the relay can't read them, only count them.
//! State is in memory; nothing about players is logged.
pub mod limits;

use horizon_friends::{
    code,
    wire::{self, ErrorCode, FromRelay, ToRelay},
};
use limits::{Bucket, Limiter, Rates};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Config {
    pub max_hosts: usize,
    pub max_bridges: usize,
    pub max_pending_per_host: usize,
    /// Connections open at once, of every kind (each is a thread).
    pub max_connections: usize,
    /// Bytes a second each way per relayed stream.
    pub session_rate: f64,
    /// Bytes a second for everything relayed.
    pub total_rate: f64,
    pub rates: Rates,
    /// Count limits per client address (off when every client comes from
    /// the same proxy address).
    pub per_address: bool,
    /// Unknown codes a minute across all clients, and the burst.
    pub global_unknown_per_min: f64,
    pub global_unknown_burst: f64,
    /// Read a PROXY protocol v1 line before anything else.
    pub proxy_protocol: bool,
    /// The load balancers allowed to send that line (`RELAY_PROXY_FROM`, addresses or CIDR ranges).
    /// A connection from anywhere else is closed unanswered: its line would be a client choosing its
    /// own address. The relay refuses to start with `proxy_protocol` and none listed ([`Config::check`]).
    pub proxy_from: Vec<Net>,
    pub idle: Duration,
    pub ping_every: Duration,
    pub host_timeout: Duration,
    pub first_line: Duration,
    pub attach_wait: Duration,
    pub ended_kept: Duration,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_hosts: 1000,
            max_bridges: 200,
            max_pending_per_host: 8,
            max_connections: 4096,
            session_rate: 1024.0 * 1024.0,
            total_rate: 100.0 * 1_000_000.0 / 8.0,
            rates: Rates::default(),
            per_address: true,
            global_unknown_per_min: 120.0,
            global_unknown_burst: 30.0,
            proxy_protocol: false,
            proxy_from: Vec::new(),
            idle: Duration::from_secs(120),
            ping_every: Duration::from_secs(20),
            host_timeout: Duration::from_secs(60),
            first_line: Duration::from_secs(10),
            attach_wait: Duration::from_secs(10),
            ended_kept: Duration::from_secs(15 * 60),
        }
    }
}
impl Config {
    /// The defaults with the `RELAY_*` environment variables applied: an error for a setting the relay
    /// can't run with as given ([`Config::check`]).
    pub fn from_env() -> Result<Self, String> {
        fn var<T: std::str::FromStr>(name: &str) -> Option<T> {
            std::env::var(name).ok()?.trim().parse().ok()
        }
        let mut c = Self::default();
        if let Some(v) = var("RELAY_MAX_HOSTS") {
            c.max_hosts = v;
        }
        if let Some(v) = var("RELAY_MAX_HOSTS_PER_ADDRESS") {
            c.rates.hosts_per_address = v;
        }
        if let Some(v) = var("RELAY_MAX_OPEN_PER_ADDRESS") {
            c.rates.open_per_address = v;
        }
        if let Some(v) = var("RELAY_MAX_BRIDGES") {
            c.max_bridges = v;
        }
        if let Some(v) = var("RELAY_MAX_PENDING_PER_HOST") {
            c.max_pending_per_host = v;
        }
        if let Some(v) = var("RELAY_MAX_CONNECTIONS") {
            c.max_connections = v;
        }
        if let Some(v) = var::<f64>("RELAY_SESSION_KBPS") {
            c.session_rate = v * 1024.0;
        }
        if let Some(v) = var::<f64>("RELAY_TOTAL_MBPS") {
            c.total_rate = v * 1_000_000.0 / 8.0;
        }
        if let Some(v) = var::<f64>("RELAY_GLOBAL_UNKNOWN_PER_MIN") {
            c.global_unknown_per_min = v;
        }
        if let Some(v) = var::<f64>("RELAY_NAMEPLATE_UNKNOWN_PER_MIN") {
            c.rates.nameplate_unknown_per_min = v;
        }
        if let Some(v) = var::<f64>("RELAY_NAMEPLATE_UNKNOWN_BURST") {
            c.rates.nameplate_unknown_burst = v;
        }
        if let Some(v) = var::<u8>("RELAY_PER_ADDRESS_LIMITS") {
            c.per_address = v != 0;
        }
        if let Some(v) = var::<u8>("RELAY_PROXY_PROTOCOL") {
            c.proxy_protocol = v != 0;
        }
        if let Ok(list) = std::env::var("RELAY_PROXY_FROM") {
            c.proxy_from = Net::parse_list(&list).map_err(|e| format!("RELAY_PROXY_FROM: {e}"))?;
        }
        c.check()?;
        Ok(c)
    }

    /// Why the relay must not start with these settings, if it mustn't.
    pub fn check(&self) -> Result<(), String> {
        if self.proxy_protocol && self.proxy_from.is_empty() {
            return Err("RELAY_PROXY_PROTOCOL=1 needs RELAY_PROXY_FROM: the load balancers' addresses or \
                        ranges (e.g. 10.0.0.0/8). Without it any client could send a PROXY line and choose \
                        the address its limits count."
                .into());
        }
        Ok(())
    }
}

/// An address or a CIDR range: `10.1.2.3`, `10.0.0.0/8`, `fd00::/8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Net {
    address: std::net::IpAddr,
    prefix: u8,
}
impl Net {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (address, prefix) = match text.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (text, None),
        };
        let address: std::net::IpAddr = address
            .parse()
            .map_err(|_| format!("{text:?} is not an address or a range"))?;
        let bits = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => bits,
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= bits)
                .ok_or_else(|| format!("{text:?}: the prefix is 0 to {bits}"))?,
        };
        Ok(Self { address, prefix })
    }

    /// A comma-separated list; empty entries are skipped, a bad one is an error.
    pub fn parse_list(text: &str) -> Result<Vec<Self>, String> {
        text.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(Self::parse)
            .collect()
    }

    pub fn contains(&self, ip: std::net::IpAddr) -> bool {
        use std::net::IpAddr;
        // An IPv4 client of a dual-stack listener arrives as ::ffff:a.b.c.d.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 => v4,
        };
        match (self.address, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// What the relay reports (and logs): counts only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub hosts: usize,
    /// Bridges open, and joins matched with a host and waiting for its attach (each holds one of
    /// `max_bridges` from the match on).
    pub bridges: usize,
    pub registrations: u64,
    pub joins: u64,
    pub unknown: u64,
    pub rate_limited: u64,
    pub relayed_bytes: u64,
}
impl Counts {
    pub fn json(&self) -> String {
        format!(
            "{{\"ok\":true,\"hosts\":{},\"bridges\":{},\"registrations\":{},\"joins\":{},\"unknown\":{},\"rate_limited\":{},\"relayed_bytes\":{}}}",
            self.hosts,
            self.bridges,
            self.registrations,
            self.joins,
            self.unknown,
            self.rate_limited,
            self.relayed_bytes
        )
    }
}

/// What the host's control connection sends next.
enum Out {
    Line(FromRelay),
    /// The host stopped answering or went away.
    Closed,
}
struct HostEntry {
    token: String,
    protocol: i32,
    game: String,
    to_host: Sender<Out>,
    pending: usize,
}
struct Pending {
    nameplate: String,
    attach: SyncSender<(TcpStream, SocketAddr)>,
}
#[derive(Default)]
struct Inner {
    hosts: HashMap<String, HostEntry>,
    ended: HashMap<String, Instant>,
    pending: HashMap<u64, Pending>,
}

struct State {
    config: Config,
    inner: Mutex<Inner>,
    limiter: Mutex<Limiter>,
    global_unknown: Mutex<Bucket>,
    total: Mutex<Bucket>,
    bridges: AtomicUsize,
    connections: AtomicUsize,
    registrations: AtomicU64,
    joins: AtomicU64,
    unknown: AtomicU64,
    rate_limited: AtomicU64,
    relayed_bytes: AtomicU64,
    stop: AtomicBool,
}

/// The state behind `mutex`, even if a connection's thread panicked holding it: one bad connection
/// must not make every later one panic too.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone)]
pub struct Relay {
    state: Arc<State>,
}

fn random_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("OS random generator");
    u64::from_le_bytes(b)
}
fn random_token() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS random generator");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Relay {
    pub fn new(config: Config) -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(State {
                limiter: Mutex::new(Limiter::new(config.rates.clone(), config.per_address)),
                global_unknown: Mutex::new(Bucket::new(
                    config.global_unknown_per_min / 60.0,
                    config.global_unknown_burst,
                    now,
                )),
                total: Mutex::new(Bucket::new(config.total_rate, config.total_rate / 4.0, now)),
                config,
                inner: Mutex::new(Inner::default()),
                bridges: AtomicUsize::new(0),
                connections: AtomicUsize::new(0),
                registrations: AtomicU64::new(0),
                joins: AtomicU64::new(0),
                unknown: AtomicU64::new(0),
                rate_limited: AtomicU64::new(0),
                relayed_bytes: AtomicU64::new(0),
                stop: AtomicBool::new(false),
            }),
        }
    }
    /// A relay on a free loopback port, served on a thread (tests).
    pub fn spawn_local(config: Config) -> io::Result<(Self, SocketAddr, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let relay = Self::new(config);
        let r = relay.clone();
        let t = std::thread::spawn(move || r.serve(listener));
        Ok((relay, address, t))
    }
    pub fn counts(&self) -> Counts {
        counts(&self.state)
    }
    pub fn stop(&self) {
        self.state.stop.store(true, Ordering::Relaxed);
    }
    /// Accept connections until [`Relay::stop`].
    pub fn serve(&self, listener: TcpListener) {
        let _ = listener.set_nonblocking(true);
        while !self.state.stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    // Beyond the cap the connection is closed unanswered.
                    if self.state.connections.fetch_add(1, Ordering::Relaxed)
                        >= self.state.config.max_connections
                    {
                        self.state.connections.fetch_sub(1, Ordering::Relaxed);
                        continue;
                    }
                    // Given back when the thread ends, a panic included, or with the closure
                    // when the thread can't start.
                    let slot = ConnSlot(self.state.clone());
                    let _ =
                        std::thread::Builder::new()
                            .name("relay-conn".into())
                            .spawn(move || {
                                let _ = stream.set_nonblocking(false);
                                let _ = handle(&slot.0, stream, peer);
                                drop(slot);
                            });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

fn counts(s: &State) -> Counts {
    Counts {
        hosts: locked(&s.inner).hosts.len(),
        bridges: s.bridges.load(Ordering::Relaxed),
        registrations: s.registrations.load(Ordering::Relaxed),
        joins: s.joins.load(Ordering::Relaxed),
        unknown: s.unknown.load(Ordering::Relaxed),
        rate_limited: s.rate_limited.load(Ordering::Relaxed),
        relayed_bytes: s.relayed_bytes.load(Ordering::Relaxed),
    }
}

fn refuse(stream: &mut TcpStream, code: ErrorCode) -> io::Result<()> {
    wire::send(stream, &FromRelay::Error { code, game: None })
}

/// `PROXY TCP4 <src> <dst> <sport> <dport>`: the client's address.
fn proxy_line(line: &[u8]) -> Option<SocketAddr> {
    let text = std::str::from_utf8(line).ok()?.trim_end_matches('\r');
    let parts: Vec<&str> = text.split(' ').collect();
    match parts.as_slice() {
        ["PROXY", "TCP4" | "TCP6", src, _, sport, _] => {
            Some(SocketAddr::new(src.parse().ok()?, sport.parse().ok()?))
        }
        _ => None,
    }
}

/// IPv4 clients of a dual-stack listener show up as `::ffff:a.b.c.d`:
/// their address as IPv4, which is what the other end can dial.
fn unmap(peer: SocketAddr) -> SocketAddr {
    match peer {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(v4.into(), v6.port()),
            None => peer,
        },
        v4 => v4,
    }
}

/// A connection counted against its address's cap ([`limits::Limiter::open_connection`]), given
/// back however the connection ends.
struct OpenSlot<'a>(&'a State, std::net::IpAddr);
impl<'a> OpenSlot<'a> {
    fn take(state: &'a State, ip: std::net::IpAddr) -> Option<Self> {
        // Decided with the lock released before a slot exists: a refused one must never be built,
        // its Drop would give back a place it never took (and lock the limiter again).
        let open = locked(&state.limiter).open_connection(ip);
        open.then(|| Self(state, ip))
    }
}
impl Drop for OpenSlot<'_> {
    fn drop(&mut self) {
        locked(&self.0.limiter).close_connection(self.1);
    }
}

/// One of `max_connections`, given back when dropped.
struct ConnSlot(Arc<State>);
impl Drop for ConnSlot {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A host's attach token against the one given: every byte compared, whatever differs first.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn handle(state: &Arc<State>, mut stream: TcpStream, peer: SocketAddr) -> io::Result<()> {
    if state.stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    let mut peer = unmap(peer);
    // Counted per address before anything is read: past its cap a connection is closed at once,
    // not held for `first_line` with one of the relay's global slots. Behind the PROXY protocol the
    // client's address is the line the load balancer sends on connect, so it's counted after it.
    let mut _open = None;
    if !state.config.proxy_protocol {
        let Some(slot) = OpenSlot::take(state, peer.ip()) else {
            return Ok(());
        };
        _open = Some(slot);
    }
    // The first line (and an HTTP request) within `first_line` in all.
    let first_line = horizon_friends::deadline(&stream, state.config.first_line)?;
    stream.set_read_timeout(Some(state.config.first_line))?;
    let _ = stream.set_nodelay(true);
    let _ = horizon_friends::set_tcp_keepalive(&stream);
    let mut line = wire::read_line(&mut stream)?.ok_or(io::ErrorKind::UnexpectedEof)?;
    if state.config.proxy_protocol {
        if !state
            .config
            .proxy_from
            .iter()
            .any(|net| net.contains(peer.ip()))
        {
            // Not from our load balancer: its PROXY line would be the client's own say.
            return Ok(());
        }
        peer = proxy_line(&line)
            .map(unmap)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no PROXY line"))?;
        let Some(slot) = OpenSlot::take(state, peer.ip()) else {
            return Ok(());
        };
        _open = Some(slot);
        line = wire::read_line(&mut stream)?.ok_or(io::ErrorKind::UnexpectedEof)?;
    }
    if line.starts_with(b"GET ") {
        return http(state, &mut stream, &line, peer);
    }
    drop(first_line);
    let Ok(hello) = serde_json::from_slice::<ToRelay>(&line) else {
        return refuse(&mut stream, ErrorCode::BadRequest);
    };
    match hello {
        ToRelay::Host { v, protocol, game } if v == wire::VERSION => {
            host(state, stream, peer, protocol, game)
        }
        ToRelay::Join {
            v,
            nameplate,
            protocol,
            ..
        } if v == wire::VERSION => join(state, stream, peer, &nameplate, protocol),
        ToRelay::Attach { v, session, token } if v == wire::VERSION => {
            attach(state, stream, peer, session, &token)
        }
        _ => refuse(&mut stream, ErrorCode::BadRequest),
    }
}

fn http(state: &State, stream: &mut TcpStream, line: &[u8], peer: SocketAddr) -> io::Result<()> {
    // Drain the request headers (bounded) before answering.
    let mut headers = 0;
    while let Some(h) = wire::read_line(stream)? {
        headers += 1;
        if h.is_empty() || h == b"\r" || headers > 64 {
            break;
        }
    }
    let path = std::str::from_utf8(line)
        .ok()
        .and_then(|l| l.split(' ').nth(1))
        .unwrap_or("");
    let (status, body) = match path {
        "/health" => ("200 OK", counts(state).json()),
        // The address the relay sees for the caller: checks that the hosting
        // platform's proxy passes client addresses through.
        "/whoami" => ("200 OK", format!("{{\"address\":\"{peer}\"}}")),
        _ => ("404 Not Found", "{\"ok\":false}".to_owned()),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

/// One of an address's open hosts, given back when the host's connection
/// ends, however it ends.
struct HostSlot<'a>(&'a State, std::net::IpAddr);
impl Drop for HostSlot<'_> {
    fn drop(&mut self) {
        locked(&self.0.limiter).close_host(self.1);
    }
}

fn host(
    state: &Arc<State>,
    mut stream: TcpStream,
    peer: SocketAddr,
    protocol: i32,
    game: String,
) -> io::Result<()> {
    let now = Instant::now();
    if !locked(&state.limiter).allow_register(peer.ip(), now) {
        state.rate_limited.fetch_add(1, Ordering::Relaxed);
        return refuse(&mut stream, ErrorCode::RateLimited);
    }
    // One client holding its connections open must not take every host slot.
    if !locked(&state.limiter).open_host(peer.ip()) {
        return refuse(&mut stream, ErrorCode::Full);
    }
    let _slot = HostSlot(state, peer.ip());
    let (tx, rx) = channel();
    let token = random_token();
    let nameplate = {
        let mut inner = locked(&state.inner);
        let kept = state.config.ended_kept;
        inner
            .ended
            .retain(|_, t| now.saturating_duration_since(*t) < kept);
        let mut found = None;
        if inner.hosts.len() < state.config.max_hosts {
            for _ in 0..64 {
                let n = code::nameplate(random_u64() as u32);
                if !inner.hosts.contains_key(&n) && !inner.ended.contains_key(&n) {
                    found = Some(n);
                    break;
                }
            }
        }
        if let Some(n) = &found {
            inner.hosts.insert(
                n.clone(),
                HostEntry {
                    token: token.clone(),
                    protocol,
                    game: game.chars().take(32).collect(),
                    to_host: tx.clone(),
                    pending: 0,
                },
            );
        }
        found
    };
    let Some(nameplate) = nameplate else {
        return refuse(&mut stream, ErrorCode::Full);
    };
    state.registrations.fetch_add(1, Ordering::Relaxed);
    let result = serve_host(state, &mut stream, peer, &nameplate, &token, tx, rx);
    {
        let mut inner = locked(&state.inner);
        if inner
            .hosts
            .get(&nameplate)
            .is_some_and(|h| h.token == token)
        {
            inner.hosts.remove(&nameplate);
            inner.ended.insert(nameplate, Instant::now());
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    result
}

/// The host's control connection: joins and pings out, pongs in. A host
/// silent for `host_timeout` is gone.
fn serve_host(
    state: &Arc<State>,
    stream: &mut TcpStream,
    peer: SocketAddr,
    nameplate: &str,
    token: &str,
    tx: Sender<Out>,
    rx: Receiver<Out>,
) -> io::Result<()> {
    wire::send(
        stream,
        &FromRelay::Registered {
            nameplate: nameplate.to_owned(),
            token: token.to_owned(),
            observed: Some(peer.to_string()),
        },
    )?;
    let mut reader = stream.try_clone()?;
    reader.set_read_timeout(Some(state.config.host_timeout))?;
    std::thread::Builder::new()
        .name("relay-host-read".into())
        .spawn(move || {
            while let Ok(Some(_)) = wire::read_line(&mut reader) {}
            let _ = tx.send(Out::Closed);
        })?;
    while !state.stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(state.config.ping_every) {
            Ok(Out::Line(m)) => wire::send(stream, &m)?,
            Ok(Out::Closed) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => wire::send(stream, &FromRelay::Ping)?,
        }
    }
    Ok(())
}

/// One of `max_bridges`, reserved when a guest is matched with its host and given back when it drops:
/// on every way out of the join (no attach, a refusal, an error, a panic) and at the bridge's end. A
/// reservation counts from the match, not from the bridge, so joins waiting for their host's attach
/// can't add up past the cap.
struct BridgeSlot<'a>(&'a AtomicUsize);
impl<'a> BridgeSlot<'a> {
    fn take(count: &'a AtomicUsize, max: usize) -> Option<Self> {
        if count.fetch_add(1, Ordering::AcqRel) >= max {
            count.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self(count))
    }
}
impl Drop for BridgeSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

enum Lookup<'a> {
    Found(Sender<Out>, BridgeSlot<'a>),
    Unknown,
    Ended,
    Version(String),
    Full,
}

fn join(
    state: &Arc<State>,
    mut stream: TcpStream,
    peer: SocketAddr,
    nameplate: &str,
    protocol: i32,
) -> io::Result<()> {
    let now = Instant::now();
    state.joins.fetch_add(1, Ordering::Relaxed);
    // The budgets for unknown codes (the site's, then the global one) are checked before the
    // lookup, so a refusal says nothing about whether the nameplate exists. The site's comes
    // first: emptying the global budget takes many sites, not the /64s of one home's /56.
    let allowed = {
        let mut limiter = locked(&state.limiter);
        limiter.allow_join(peer.ip(), now) && limiter.site_unknown_has(peer.ip(), now)
    } && locked(&state.global_unknown).has(now);
    if !allowed {
        state.rate_limited.fetch_add(1, Ordering::Relaxed);
        return refuse(&mut stream, ErrorCode::RateLimited);
    }
    let Some(nameplate) = code::parse_nameplate(nameplate) else {
        return refuse(&mut stream, ErrorCode::BadRequest);
    };
    // The nameplate's own budget: what one address set can take of the global unknown budget for
    // a single short code. Checked before the lookup like the other budgets, so a refusal says
    // nothing about whether the code exists. It counts unknown short codes only, so a wrong secret
    // for an existing code stays the host's limit (five, then it retires the code), not the
    // relay's.
    if !locked(&state.limiter).nameplate_unknown_has(&nameplate, now) {
        state.rate_limited.fetch_add(1, Ordering::Relaxed);
        return refuse(&mut stream, ErrorCode::RateLimited);
    }
    let session = random_u64();
    let (atx, arx) = sync_channel(1);
    let lookup = {
        let mut inner = locked(&state.inner);
        let ended = inner
            .ended
            .get(&nameplate)
            .is_some_and(|t| now.saturating_duration_since(*t) < state.config.ended_kept);
        let lookup = match inner.hosts.get_mut(&nameplate) {
            None if ended => Lookup::Ended,
            None => Lookup::Unknown,
            Some(h) if h.protocol != protocol => Lookup::Version(h.game.clone()),
            Some(h) if h.pending >= state.config.max_pending_per_host => Lookup::Full,
            Some(h) => match BridgeSlot::take(&state.bridges, state.config.max_bridges) {
                Some(slot) => {
                    h.pending += 1;
                    Lookup::Found(h.to_host.clone(), slot)
                }
                None => Lookup::Full,
            },
        };
        if matches!(lookup, Lookup::Found(..)) {
            inner.pending.insert(
                session,
                Pending {
                    nameplate: nameplate.clone(),
                    attach: atx,
                },
            );
        }
        lookup
    };
    let (to_host, slot) = match lookup {
        Lookup::Found(tx, slot) => (tx, slot),
        Lookup::Unknown => {
            state.unknown.fetch_add(1, Ordering::Relaxed);
            {
                let mut limiter = locked(&state.limiter);
                limiter.unknown(peer.ip(), now);
                limiter.nameplate_unknown_take(&nameplate, now);
                limiter.site_unknown_take(peer.ip(), now);
            }
            locked(&state.global_unknown).try_take(now);
            return refuse(&mut stream, ErrorCode::UnknownCode);
        }
        Lookup::Ended => return refuse(&mut stream, ErrorCode::Ended),
        Lookup::Full => return refuse(&mut stream, ErrorCode::Full),
        Lookup::Version(game) => {
            return wire::send(
                &mut stream,
                &FromRelay::Error {
                    code: ErrorCode::VersionMismatch,
                    game: Some(game),
                },
            );
        }
    };
    let _ = to_host.send(Out::Line(FromRelay::Join { session }));
    let attached = arx.recv_timeout(state.config.attach_wait);
    {
        let mut inner = locked(&state.inner);
        inner.pending.remove(&session);
        if let Some(h) = inner.hosts.get_mut(&nameplate) {
            h.pending = h.pending.saturating_sub(1);
        }
    }
    match attached {
        Ok((host, host_peer)) => bridge(state, stream, peer, host, host_peer, session, slot),
        Err(_) => {
            drop(slot);
            refuse(&mut stream, ErrorCode::HostUnreachable)
        }
    }
}

fn attach(
    state: &Arc<State>,
    mut stream: TcpStream,
    peer: SocketAddr,
    session: u64,
    token: &str,
) -> io::Result<()> {
    let target = {
        let inner = locked(&state.inner);
        inner
            .pending
            .get(&session)
            .filter(|p| {
                inner
                    .hosts
                    .get(&p.nameplate)
                    .is_some_and(|h| same_token(&h.token, token))
            })
            .map(|p| p.attach.clone())
    };
    match target {
        Some(tx) => match tx.try_send((stream, peer)) {
            Ok(()) => Ok(()),
            Err(
                std::sync::mpsc::TrySendError::Full((mut stream, _))
                | std::sync::mpsc::TrySendError::Disconnected((mut stream, _)),
            ) => refuse(&mut stream, ErrorCode::BadRequest),
        },
        None => refuse(&mut stream, ErrorCode::BadRequest),
    }
}

/// Tell both ends they are bridged, then copy each way until either ends. `_slot` (the guest's
/// reservation of one of `max_bridges`) is given back when this returns, however it returns.
fn bridge(
    state: &Arc<State>,
    mut guest: TcpStream,
    guest_peer: SocketAddr,
    mut host: TcpStream,
    host_peer: SocketAddr,
    session: u64,
    _slot: BridgeSlot<'_>,
) -> io::Result<()> {
    (|| -> io::Result<()> {
        wire::send(
            &mut guest,
            &FromRelay::Bridged {
                session,
                observed: Some(guest_peer.to_string()),
            },
        )?;
        wire::send(
            &mut host,
            &FromRelay::Bridged {
                session,
                observed: Some(host_peer.to_string()),
            },
        )?;
        for s in [&guest, &host] {
            s.set_read_timeout(Some(state.config.idle))?;
            let _ = s.set_nodelay(true);
            let _ = horizon_friends::set_tcp_keepalive(s);
        }
        let (g, h, st) = (guest.try_clone()?, host.try_clone()?, state.clone());
        let up = std::thread::Builder::new()
            .name("relay-copy".into())
            .spawn(move || copy(&st, g, h))?;
        copy(state, host, guest);
        let _ = up.join();
        Ok(())
    })()
}

fn copy(state: &State, mut from: TcpStream, mut to: TcpStream) {
    let rate = state.config.session_rate;
    let mut bucket = Bucket::new(rate, (rate / 2.0).max(16384.0), Instant::now());
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let now = Instant::now();
        let wait = bucket
            .take(n as f64, now)
            .max(locked(&state.total).take(n as f64, now));
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        if to.write_all(&buf[..n]).is_err() {
            break;
        }
        state.relayed_bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_lock_still_opens() {
        let mutex = Arc::new(Mutex::new(7));
        let held = mutex.clone();
        let _ = std::thread::spawn(move || {
            let _guard = held.lock().unwrap();
            panic!("a connection's thread panics holding the lock");
        })
        .join();
        assert!(mutex.is_poisoned());
        assert_eq!(*locked(&mutex), 7);
    }

    #[test]
    fn mapped_addresses_become_ipv4() {
        assert_eq!(
            unmap("[::ffff:203.0.113.7]:5000".parse().unwrap()),
            "203.0.113.7:5000".parse().unwrap()
        );
        let v6: SocketAddr = "[2001:db8::1]:5000".parse().unwrap();
        assert_eq!(unmap(v6), v6);
    }

    #[test]
    fn proxy_protocol_needs_its_load_balancers() {
        let on = Config {
            proxy_protocol: true,
            ..Config::default()
        };
        assert!(on.check().unwrap_err().contains("RELAY_PROXY_FROM"));
        let listed = Config {
            proxy_from: Net::parse_list("10.0.0.0/8, 2001:db8::/32,192.0.2.7").unwrap(),
            ..on
        };
        assert!(listed.check().is_ok());
        assert!(Config::default().check().is_ok());
        for bad in ["10.0.0.0/33", "nope", "10.0.0.1/x", "::/129"] {
            assert!(Net::parse_list(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ranges_hold_their_addresses() {
        let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap();
        let ten = Net::parse("10.0.0.0/8").unwrap();
        assert!(ten.contains(ip("10.200.3.4")) && !ten.contains(ip("11.0.0.1")));
        assert!(
            ten.contains(ip("::ffff:10.1.1.1")),
            "IPv4 on a dual-stack listener"
        );
        let one = Net::parse("192.0.2.7").unwrap();
        assert!(one.contains(ip("192.0.2.7")) && !one.contains(ip("192.0.2.8")));
        let v6 = Net::parse("2001:db8::/32").unwrap();
        assert!(v6.contains(ip("2001:db8:1::5")) && !v6.contains(ip("2001:db9::1")));
        assert!(!v6.contains(ip("10.0.0.1")));
        assert!(Net::parse("0.0.0.0/0").unwrap().contains(ip("203.0.113.1")));
    }

    #[test]
    fn proxy_lines() {
        assert_eq!(
            proxy_line(b"PROXY TCP4 203.0.113.7 10.0.0.1 51000 7000\r"),
            Some("203.0.113.7:51000".parse().unwrap())
        );
        assert_eq!(
            proxy_line(b"PROXY TCP6 2001:db8::1 2001:db8::2 51000 7000"),
            Some("[2001:db8::1]:51000".parse().unwrap())
        );
        assert_eq!(proxy_line(b"PROXY UNKNOWN"), None);
        assert_eq!(proxy_line(b"{\"op\":\"pong\"}"), None);
    }
}
