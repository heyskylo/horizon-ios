//! Token buckets for request rates and relayed bandwidth, and the
//! per-address state behind them (kept in memory, forgotten after ten idle
//! minutes), and the hosts each address holds open.
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv6Addr},
    time::{Duration, Instant},
};

/// A token bucket: `rate` tokens a second up to `burst`.
#[derive(Clone, Debug)]
pub struct Bucket {
    tokens: f64,
    last: Instant,
    rate: f64,
    burst: f64,
}
impl Bucket {
    pub fn new(rate: f64, burst: f64, now: Instant) -> Self {
        Self {
            tokens: burst,
            last: now,
            rate,
            burst,
        }
    }
    fn refill(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + dt * self.rate).min(self.burst);
        self.last = now;
    }
    /// Take one token if there is one.
    pub fn try_take(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
    pub fn has(&mut self, now: Instant) -> bool {
        self.refill(now);
        self.tokens >= 1.0
    }
    /// Take `n` tokens, going into debt if needed; how long to wait before
    /// sending them so the average stays at the rate.
    pub fn take(&mut self, n: f64, now: Instant) -> Duration {
        self.refill(now);
        self.tokens -= n;
        if self.tokens >= 0.0 || self.rate <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-self.tokens / self.rate)
        }
    }
}

/// Addresses are counted by IPv4 address and by IPv6 /64 (one home).
pub fn address_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
        v4 => v4,
    }
}

#[derive(Clone, Debug)]
pub struct Rates {
    pub register_per_min: f64,
    pub register_burst: f64,
    pub join_per_min: f64,
    pub join_burst: f64,
    /// Unknown codes an address may try in `unknown_window` before it is
    /// blocked for `block`.
    pub unknown_limit: usize,
    pub unknown_window: Duration,
    pub block: Duration,
    /// Hosts one address may hold open at once (`RELAY_MAX_HOSTS_PER_ADDRESS`);
    /// an IPv6 /48 (one site, 65536 homes) may hold [`SITE_HOSTS`] times as many.
    pub hosts_per_address: usize,
    /// Connections of any kind one address may hold open at once
    /// (`RELAY_MAX_OPEN_PER_ADDRESS`), an IPv6 /48 [`SITE_HOSTS`] times as many. Without it one
    /// client could fill the relay's global cap with silent connections, each held for the
    /// first-line wait. A host keeps one connection per guest it plays with, hence the margin.
    pub open_per_address: usize,
    /// Unknown codes a minute (and the burst) from one site, an IPv4 /24 or an IPv6 /48, checked
    /// before the global budget, so emptying that takes many sites rather than one home's /64s.
    pub site_unknown_per_min: f64,
    pub site_unknown_burst: f64,
}
impl Default for Rates {
    fn default() -> Self {
        Self {
            register_per_min: 6.0,
            register_burst: 3.0,
            join_per_min: 12.0,
            join_burst: 6.0,
            unknown_limit: 5,
            unknown_window: Duration::from_secs(600),
            block: Duration::from_secs(600),
            hosts_per_address: 4,
            open_per_address: 64,
            site_unknown_per_min: 24.0,
            site_unknown_burst: 6.0,
        }
    }
}

/// How many times an address's open hosts an IPv6 /48 may hold.
pub const SITE_HOSTS: usize = 4;

/// What open hosts are counted under: the address key, and for IPv6 also its
/// /48 with the larger cap. The prefix length keeps a /64 key and a /48 key
/// that print alike apart.
fn host_keys(ip: IpAddr, per_address: usize) -> Vec<((IpAddr, u8), usize)> {
    let key = address_key(ip);
    let mut keys = vec![((key, 64), per_address)];
    if let IpAddr::V6(v6) = key {
        let s = v6.segments();
        let site = IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], 0, 0, 0, 0, 0));
        keys.push(((site, 48), per_address.saturating_mul(SITE_HOSTS)));
    }
    keys
}

/// The site an address belongs to for the unknown-code budget: its IPv4 /24, or its IPv6 /48.
pub fn site_key(ip: IpAddr) -> IpAddr {
    match address_key(ip) {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            IpAddr::V4(std::net::Ipv4Addr::new(o[0], o[1], o[2], 0))
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], 0, 0, 0, 0, 0))
        }
    }
}

struct Site {
    unknown: Bucket,
    seen: Instant,
}

struct Address {
    register: Bucket,
    join: Bucket,
    unknown: VecDeque<Instant>,
    blocked_until: Option<Instant>,
    seen: Instant,
}

/// Per-address limits; disabled (everything allowed) when the relay can't
/// see client addresses.
pub struct Limiter {
    rates: Rates,
    enabled: bool,
    addresses: HashMap<IpAddr, Address>,
    last_sweep: Instant,
    /// Open hosts by address key (and IPv6 site); never swept, each count
    /// is given back by [`Limiter::close_host`].
    hosts: HashMap<(IpAddr, u8), usize>,
    /// Open connections, counted the same way and given back by [`Limiter::close_connection`].
    open: HashMap<(IpAddr, u8), usize>,
    /// Unknown-code budgets by [`site_key`], swept with the addresses.
    sites: HashMap<IpAddr, Site>,
}
impl Limiter {
    pub fn new(rates: Rates, enabled: bool) -> Self {
        Self {
            rates,
            enabled,
            addresses: HashMap::new(),
            last_sweep: Instant::now(),
            hosts: HashMap::new(),
            open: HashMap::new(),
            sites: HashMap::new(),
        }
    }
    /// Count one more connection open from `ip`, unless its address (or IPv6 site) holds as many
    /// as it may already: then the caller closes it at once.
    pub fn open_connection(&mut self, ip: IpAddr) -> bool {
        if !self.enabled {
            return true;
        }
        let keys = host_keys(ip, self.rates.open_per_address);
        if keys
            .iter()
            .any(|(k, cap)| self.open.get(k).copied().unwrap_or(0) >= *cap)
        {
            return false;
        }
        for (k, _) in keys {
            *self.open.entry(k).or_insert(0) += 1;
        }
        true
    }
    /// A connection [`Limiter::open_connection`] let in has closed.
    pub fn close_connection(&mut self, ip: IpAddr) {
        if !self.enabled {
            return;
        }
        for (k, _) in host_keys(ip, self.rates.open_per_address) {
            if let Some(n) = self.open.get_mut(&k) {
                *n -= 1;
                if *n == 0 {
                    self.open.remove(&k);
                }
            }
        }
    }
    fn site(&mut self, ip: IpAddr, now: Instant) -> &mut Site {
        let r = &self.rates;
        let site = self.sites.entry(site_key(ip)).or_insert_with(|| Site {
            unknown: Bucket::new(r.site_unknown_per_min / 60.0, r.site_unknown_burst, now),
            seen: now,
        });
        site.seen = now;
        site
    }
    /// Whether `ip`'s site has unknown-code budget left (checked before every join, as the global
    /// budget is, so a refusal says nothing about whether a code exists).
    pub fn site_unknown_has(&mut self, ip: IpAddr, now: Instant) -> bool {
        !self.enabled || self.site(ip, now).unknown.has(now)
    }
    /// An unknown code from `ip`: spends one from its site's budget.
    pub fn site_unknown_take(&mut self, ip: IpAddr, now: Instant) {
        if self.enabled {
            self.site(ip, now).unknown.try_take(now);
        }
    }
    /// Count one more host open from `ip`, unless its address (or IPv6 site)
    /// holds as many as it may already.
    pub fn open_host(&mut self, ip: IpAddr) -> bool {
        if !self.enabled {
            return true;
        }
        let keys = host_keys(ip, self.rates.hosts_per_address);
        if keys
            .iter()
            .any(|(k, cap)| self.hosts.get(k).copied().unwrap_or(0) >= *cap)
        {
            return false;
        }
        for (k, _) in keys {
            *self.hosts.entry(k).or_insert(0) += 1;
        }
        true
    }
    /// A host [`Limiter::open_host`] let in has gone.
    pub fn close_host(&mut self, ip: IpAddr) {
        if !self.enabled {
            return;
        }
        for (k, _) in host_keys(ip, self.rates.hosts_per_address) {
            if let Some(n) = self.hosts.get_mut(&k) {
                *n -= 1;
                if *n == 0 {
                    self.hosts.remove(&k);
                }
            }
        }
    }
    fn entry(&mut self, ip: IpAddr, now: Instant) -> &mut Address {
        if now.saturating_duration_since(self.last_sweep) > Duration::from_secs(60) {
            self.last_sweep = now;
            let keep = Duration::from_secs(600).max(self.rates.block);
            self.addresses.retain(|_, a| {
                now.saturating_duration_since(a.seen) < keep
                    || a.blocked_until.is_some_and(|b| b > now)
            });
            // An idle site's bucket has refilled long before: dropping it changes nothing.
            self.sites
                .retain(|_, s| now.saturating_duration_since(s.seen) < keep);
        }
        let r = &self.rates;
        let a = self
            .addresses
            .entry(address_key(ip))
            .or_insert_with(|| Address {
                register: Bucket::new(r.register_per_min / 60.0, r.register_burst, now),
                join: Bucket::new(r.join_per_min / 60.0, r.join_burst, now),
                unknown: VecDeque::new(),
                blocked_until: None,
                seen: now,
            });
        a.seen = now;
        a
    }
    pub fn allow_register(&mut self, ip: IpAddr, now: Instant) -> bool {
        !self.enabled || self.entry(ip, now).register.try_take(now)
    }
    pub fn allow_join(&mut self, ip: IpAddr, now: Instant) -> bool {
        if !self.enabled {
            return true;
        }
        let a = self.entry(ip, now);
        if a.blocked_until.is_some_and(|b| b > now) {
            return false;
        }
        a.blocked_until = None;
        a.join.try_take(now)
    }
    /// An unknown code from this address; blocks it past the limit.
    pub fn unknown(&mut self, ip: IpAddr, now: Instant) {
        if !self.enabled {
            return;
        }
        let (limit, window, block) = (
            self.rates.unknown_limit,
            self.rates.unknown_window,
            self.rates.block,
        );
        let a = self.entry(ip, now);
        a.unknown.push_back(now);
        while a
            .unknown
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > window)
        {
            a.unknown.pop_front();
        }
        if a.unknown.len() >= limit {
            a.blocked_until = Some(now + block);
            a.unknown.clear();
        }
    }
    pub fn tracked(&self) -> usize {
        self.addresses.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_holds_only_so_many_connections_open() {
        let rates = Rates {
            open_per_address: 3,
            ..Rates::default()
        };
        let mut l = Limiter::new(rates, true);
        let home: IpAddr = "203.0.113.7".parse().unwrap();
        let other: IpAddr = "203.0.113.8".parse().unwrap();
        assert!((0..3).all(|_| l.open_connection(home)));
        assert!(!l.open_connection(home), "the fourth is refused");
        assert!(l.open_connection(other), "another address isn't");
        l.close_connection(home);
        assert!(l.open_connection(home), "a closed one gives its place back");
        // IPv6 counts by /64, and a /48 holds four times as many.
        let mut l = Limiter::new(
            Rates {
                open_per_address: 2,
                ..Rates::default()
            },
            true,
        );
        let in_64 = |i: u16| -> IpAddr { format!("2001:db8:1:{i}::{}", i + 1).parse().unwrap() };
        assert!(l.open_connection(in_64(0)) && l.open_connection(in_64(0)));
        assert!(
            !l.open_connection("2001:db8:1:0::99".parse().unwrap()),
            "same /64"
        );
        for i in 1..4 {
            assert!(l.open_connection(in_64(i)) && l.open_connection(in_64(i)));
        }
        assert!(!l.open_connection(in_64(4)), "the /48 is full at 8");
        // Off when the relay can't see client addresses.
        let mut off = Limiter::new(Rates::default(), false);
        assert!((0..1000).all(|_| off.open_connection(home)));
    }

    #[test]
    fn a_site_spends_its_own_unknown_code_budget() {
        let t = Instant::now();
        let mut l = Limiter::new(
            Rates {
                site_unknown_per_min: 6.0,
                site_unknown_burst: 2.0,
                ..Rates::default()
            },
            true,
        );
        // Two homes of one /24 share a budget: the burst, then nothing until it refills.
        let (a, b): (IpAddr, IpAddr) = (
            "198.51.100.1".parse().unwrap(),
            "198.51.100.200".parse().unwrap(),
        );
        l.site_unknown_take(a, t);
        assert!(l.site_unknown_has(b, t));
        l.site_unknown_take(b, t);
        assert!(!l.site_unknown_has(a, t) && !l.site_unknown_has(b, t));
        assert!(
            l.site_unknown_has("198.51.101.1".parse().unwrap(), t),
            "another /24 isn't"
        );
        assert!(
            l.site_unknown_has(a, t + Duration::from_secs(10)),
            "one a ten seconds back"
        );
        // IPv6: the /56 of one home is one /48.
        let mut v6 = Limiter::new(
            Rates {
                site_unknown_burst: 1.0,
                ..Rates::default()
            },
            true,
        );
        v6.site_unknown_take("2001:db8:7:100::1".parse().unwrap(), t);
        assert!(!v6.site_unknown_has("2001:db8:7:1ff::1".parse().unwrap(), t));
    }

    #[test]
    fn buckets_refill_at_their_rate() {
        let t = Instant::now();
        let mut b = Bucket::new(1.0, 2.0, t);
        assert!(b.try_take(t));
        assert!(b.try_take(t));
        assert!(!b.try_take(t));
        assert!(!b.try_take(t + Duration::from_millis(900)));
        assert!(b.try_take(t + Duration::from_millis(1000)));
        // Never above the burst.
        assert!(b.try_take(t + Duration::from_secs(100)));
        assert!(b.try_take(t + Duration::from_secs(100)));
        assert!(!b.try_take(t + Duration::from_secs(100)));
    }

    #[test]
    fn bandwidth_debt_turns_into_a_wait() {
        let t = Instant::now();
        let mut b = Bucket::new(1000.0, 500.0, t);
        assert_eq!(b.take(500.0, t), Duration::ZERO);
        assert_eq!(b.take(1000.0, t), Duration::from_secs(1));
        // After the wait the debt is paid.
        assert_eq!(b.take(0.0, t + Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn ipv6_counts_by_home() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(address_key(a), address_key(b));
        assert_ne!(address_key(a), address_key(c));
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        assert_eq!(
            address_key(mapped),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn unknown_codes_block_an_address_for_a_while() {
        let t = Instant::now();
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let other: IpAddr = "203.0.113.8".parse().unwrap();
        let mut l = Limiter::new(
            Rates {
                join_burst: 100.0,
                ..Rates::default()
            },
            true,
        );
        for _ in 0..4 {
            assert!(l.allow_join(ip, t));
            l.unknown(ip, t);
        }
        assert!(l.allow_join(ip, t));
        l.unknown(ip, t);
        assert!(!l.allow_join(ip, t));
        assert!(!l.allow_join(ip, t + Duration::from_secs(599)));
        assert!(l.allow_join(other, t));
        assert!(l.allow_join(ip, t + Duration::from_secs(601)));
        // Unknown codes spread beyond the window don't add up.
        let mut l = Limiter::new(Rates::default(), true);
        for i in 0..10 {
            l.unknown(ip, t + Duration::from_secs(i * 200));
        }
        assert!(l.allow_join(ip, t + Duration::from_secs(2000)));
    }

    #[test]
    fn open_hosts_per_address_and_ipv6_site() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let mut l = Limiter::new(
            Rates {
                hosts_per_address: 2,
                ..Rates::default()
            },
            true,
        );
        let v4 = ip("198.51.100.1");
        assert!(l.open_host(v4) && l.open_host(v4));
        assert!(!l.open_host(v4));
        assert!(l.open_host(ip("198.51.100.2")), "another address");
        l.close_host(v4);
        assert!(l.open_host(v4));
        // IPv6: 2 per /64, 8 per /48 however many /64s it spreads over.
        let mut opened = 0;
        for home in 0..10 {
            for _ in 0..2 {
                opened += usize::from(l.open_host(ip(&format!("2001:db8:7:{home}::1"))));
            }
        }
        assert_eq!(opened, 2 * SITE_HOSTS);
        assert!(l.open_host(ip("2001:db8:8::1")), "another site");
        l.close_host(ip("2001:db8:7:1::1"));
        assert!(
            !l.open_host(ip("2001:db8:7:0::1")),
            "that home is still full"
        );
        assert!(l.open_host(ip("2001:db8:7:9::1")));
        // Off when the relay can't see client addresses.
        let mut off = Limiter::new(Rates::default(), false);
        assert!((0..100).all(|_| off.open_host(v4)));
    }

    #[test]
    fn join_and_register_rates() {
        let t = Instant::now();
        let ip: IpAddr = "198.51.100.1".parse().unwrap();
        let mut l = Limiter::new(Rates::default(), true);
        assert_eq!((0..10).filter(|_| l.allow_register(ip, t)).count(), 3);
        assert_eq!((0..10).filter(|_| l.allow_join(ip, t)).count(), 6);
        assert!(l.allow_join(ip, t + Duration::from_secs(5)));
        let mut off = Limiter::new(Rates::default(), false);
        assert!((0..100).all(|_| off.allow_join(ip, t)));
        assert_eq!(off.tracked(), 0);
    }
}
