//! Rate and concurrency limits (plan section 5, "Limits"). Every time-dependent function takes `now`, so tests do not sleep.
use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_CONNECTIONS: usize = 256;
pub const MAX_CONNECTIONS_PER_IP: usize = 64;
pub const MAX_SOCKETS_PER_TOKEN: usize = 4;
pub const LOGIN_FAILURES: usize = 5;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(600);
/// Failed logins from everywhere together before every login is refused for a while.
pub const LOGIN_GLOBAL: usize = 100;
const MAP_CAP: usize = 10_000;

/// The key limits are counted under: an IPv6 peer is counted by its /64 (a client owns the whole prefix, rotating inside it must not reset a limit).
pub fn limit_key(ip: IpAddr) -> IpAddr {
    match nocturne_core::gateway::normalize(ip) {
        IpAddr::V6(v6) => {
            let mut s = v6.segments();
            s[4..].fill(0);
            IpAddr::V6(Ipv6Addr::from(s))
        }
        v4 => v4,
    }
}

/// Token buckets keyed by `K`: `rate` per second, up to `burst` at once.
pub struct Buckets<K> {
    rate: f64,
    burst: f64,
    map: Mutex<HashMap<K, (f64, Instant)>>,
}

impl<K: Hash + Eq + Clone> Buckets<K> {
    pub fn new(rate: f64, burst: f64) -> Self {
        Self {
            rate,
            burst,
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn allow(&self, key: &K, now: Instant) -> bool {
        let mut m = self.map.lock().unwrap();
        if m.len() >= MAP_CAP && !m.contains_key(key) {
            // ponytail: a flood of distinct keys clears the table (everyone gets a fresh bucket); per-key LRU if that ever matters.
            m.clear();
        }
        let e = m.entry(key.clone()).or_insert((self.burst, now));
        let gained = now.saturating_duration_since(e.1).as_secs_f64() * self.rate;
        e.0 = (e.0 + gained).min(self.burst);
        e.1 = now;
        if e.0 >= 1.0 {
            e.0 -= 1.0;
            true
        } else {
            false
        }
    }

    pub fn prune(&self, now: Instant) {
        let full = self.burst / self.rate.max(0.001);
        self.map
            .lock()
            .unwrap()
            .retain(|_, (_, t)| now.saturating_duration_since(*t).as_secs_f64() < full);
    }
}

/// Failed sign-ins: 5 per 10 minutes per (IP, username), then exponential back-off, plus a global ceiling.
#[derive(Default)]
pub struct LoginLimiter {
    inner: Mutex<LoginState>,
}

#[derive(Default)]
struct LoginState {
    by_key: HashMap<(IpAddr, String), Vec<Instant>>,
    all: Vec<Instant>,
}

impl LoginLimiter {
    fn key(ip: IpAddr, user: &str) -> (IpAddr, String) {
        (limit_key(ip), user.trim().to_lowercase())
    }

    /// `Err(wait)` when this attempt must be refused with 429.
    pub fn check(&self, ip: IpAddr, user: &str, now: Instant) -> Result<(), Duration> {
        let mut s = self.inner.lock().unwrap();
        s.all
            .retain(|t| now.saturating_duration_since(*t) < LOGIN_WINDOW);
        if s.all.len() >= LOGIN_GLOBAL {
            let oldest = s.all[0];
            return Err((oldest + LOGIN_WINDOW).saturating_duration_since(now));
        }
        let k = Self::key(ip, user);
        let Some(v) = s.by_key.get_mut(&k) else {
            return Ok(());
        };
        v.retain(|t| now.saturating_duration_since(*t) < LOGIN_WINDOW);
        if v.len() < LOGIN_FAILURES {
            return Ok(());
        }
        let extra = (v.len() - LOGIN_FAILURES).min(5) as u32;
        let wait = Duration::from_secs(30 * (1u64 << extra)).min(LOGIN_WINDOW);
        let until = *v.last().unwrap() + wait;
        if now < until {
            Err(until - now)
        } else {
            Ok(())
        }
    }

    pub fn failed(&self, ip: IpAddr, user: &str, now: Instant) {
        let mut s = self.inner.lock().unwrap();
        if s.by_key.len() >= MAP_CAP {
            s.by_key.clear();
        }
        s.by_key.entry(Self::key(ip, user)).or_default().push(now);
        s.all.push(now);
    }

    pub fn succeeded(&self, ip: IpAddr, user: &str) {
        self.inner
            .lock()
            .unwrap()
            .by_key
            .remove(&Self::key(ip, user));
    }

    pub fn prune(&self, now: Instant) {
        let mut s = self.inner.lock().unwrap();
        s.all
            .retain(|t| now.saturating_duration_since(*t) < LOGIN_WINDOW);
        s.by_key.retain(|_, v| {
            v.retain(|t| now.saturating_duration_since(*t) < LOGIN_WINDOW);
            !v.is_empty()
        });
    }
}

/// Open connections: 256 in all, 64 per peer. Dropping the guard frees the slot.
pub struct ConnLimits {
    all: Arc<tokio::sync::Semaphore>,
    per_ip: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

pub struct ConnGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
    ip: IpAddr,
    per_ip: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        let mut m = self.per_ip.lock().unwrap();
        if let Some(n) = m.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.ip);
            }
        }
    }
}

impl ConnLimits {
    pub fn new(all: usize) -> Self {
        Self {
            all: Arc::new(tokio::sync::Semaphore::new(all)),
            per_ip: Arc::default(),
        }
    }

    pub fn try_acquire(&self, ip: IpAddr) -> Option<ConnGuard> {
        let ip = limit_key(ip);
        let permit = self.all.clone().try_acquire_owned().ok()?;
        let mut m = self.per_ip.lock().unwrap();
        let n = m.entry(ip).or_insert(0);
        if *n >= MAX_CONNECTIONS_PER_IP {
            return None;
        }
        *n += 1;
        drop(m);
        Some(ConnGuard {
            _permit: permit,
            ip,
            per_ip: self.per_ip.clone(),
        })
    }
}

/// WebSocket slots per token (4). The WebSocket module takes one for the life of a socket.
#[derive(Default)]
pub struct SocketSlots {
    open: Arc<Mutex<HashMap<String, usize>>>,
}

pub struct SocketGuard {
    token: String,
    open: Arc<Mutex<HashMap<String, usize>>>,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let mut m = self.open.lock().unwrap();
        if let Some(n) = m.get_mut(&self.token) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.token);
            }
        }
    }
}

impl SocketSlots {
    pub fn acquire(&self, token_id: &str) -> Option<SocketGuard> {
        let mut m = self.open.lock().unwrap();
        let n = m.entry(token_id.to_string()).or_insert(0);
        if *n >= MAX_SOCKETS_PER_TOKEN {
            return None;
        }
        *n += 1;
        Some(SocketGuard {
            token: token_id.to_string(),
            open: self.open.clone(),
        })
    }
}

/// Everything the middleware and handlers draw on. `command` is for the session routes (20 per second per token).
pub struct Limits {
    pub ip: Buckets<IpAddr>,
    pub token: Buckets<String>,
    pub command: Buckets<String>,
    pub quick_connect: Buckets<IpAddr>,
    pub login: LoginLimiter,
    pub conns: ConnLimits,
    pub sockets: SocketSlots,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            ip: Buckets::new(100.0, 200.0),
            token: Buckets::new(100.0, 200.0),
            command: Buckets::new(20.0, 20.0),
            quick_connect: Buckets::new(0.5, 5.0),
            login: LoginLimiter::default(),
            conns: ConnLimits::new(MAX_CONNECTIONS),
            sockets: SocketSlots::default(),
        }
    }
}

impl Limits {
    pub fn prune(&self, now: Instant) {
        self.ip.prune(now);
        self.token.prune(now);
        self.command.prune(now);
        self.quick_connect.prune(now);
        self.login.prune(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn bucket_refills_with_time() {
        let b = Buckets::<u8>::new(2.0, 3.0);
        let t = Instant::now();
        assert!([(); 3].iter().all(|_| b.allow(&1, t)));
        assert!(!b.allow(&1, t));
        assert!(b.allow(&2, t), "other keys are independent");
        assert!(
            b.allow(&1, t + Duration::from_millis(600)),
            "1.2 tokens back"
        );
        assert!(!b.allow(&1, t + Duration::from_millis(600)));
    }

    #[test]
    fn login_blocks_after_five_and_backs_off() {
        let l = LoginLimiter::default();
        let t = Instant::now();
        let a = ip("192.168.1.9");
        for i in 0..5 {
            assert!(l.check(a, "Nocturne", t).is_ok(), "attempt {i}");
            l.failed(a, "Nocturne", t);
        }
        let wait = l.check(a, "nocturne ", t).unwrap_err();
        assert_eq!(wait, Duration::from_secs(30), "case and space insensitive");
        assert!(l.check(a, "nocturne", t + Duration::from_secs(31)).is_ok());
        l.failed(a, "nocturne", t + Duration::from_secs(31));
        let wait2 = l
            .check(a, "nocturne", t + Duration::from_secs(32))
            .unwrap_err();
        assert!(wait2 > Duration::from_secs(50), "exponential: {wait2:?}");
        assert!(
            l.check(ip("192.168.1.10"), "nocturne", t).is_ok(),
            "other IP"
        );
        assert!(l.check(a, "someone", t).is_ok(), "other user");
        assert!(
            l.check(a, "nocturne", t + LOGIN_WINDOW + Duration::from_secs(90))
                .is_ok()
        );
    }

    #[test]
    fn success_clears_the_count() {
        let l = LoginLimiter::default();
        let t = Instant::now();
        let a = ip("10.0.0.1");
        for _ in 0..4 {
            l.failed(a, "u", t);
        }
        l.succeeded(a, "u");
        for _ in 0..4 {
            l.failed(a, "u", t);
        }
        assert!(l.check(a, "u", t).is_ok());
    }

    #[test]
    fn global_ceiling_blocks_everyone() {
        let l = LoginLimiter::default();
        let t = Instant::now();
        for i in 0..LOGIN_GLOBAL {
            l.failed(
                ip(&format!("10.0.{}.{}", i / 200, i % 200)),
                &format!("u{i}"),
                t,
            );
        }
        assert!(l.check(ip("172.16.0.1"), "fresh", t).is_err());
        assert!(l.check(ip("172.16.0.1"), "fresh", t + LOGIN_WINDOW).is_ok());
    }

    #[test]
    fn ipv6_prefix_rotation_shares_a_key() {
        assert_eq!(
            limit_key(ip("2001:db8:1:2:aaaa::1")),
            limit_key(ip("2001:db8:1:2:bbbb::9"))
        );
        assert_ne!(
            limit_key(ip("2001:db8:1:2::1")),
            limit_key(ip("2001:db8:1:3::1"))
        );
        assert_eq!(limit_key(ip("::ffff:1.2.3.4")), ip("1.2.3.4"));
        let l = LoginLimiter::default();
        let t = Instant::now();
        for i in 0..5 {
            l.failed(ip(&format!("2001:db8:1:2:{i}::1")), "u", t);
        }
        assert!(l.check(ip("2001:db8:1:2:ffff::1"), "u", t).is_err());
    }

    #[tokio::test]
    async fn connection_slots_free_on_drop() {
        let c = ConnLimits::new(3);
        let a = ip("10.0.0.1");
        let g: Vec<_> = (0..3).map(|_| c.try_acquire(a).unwrap()).collect();
        assert!(c.try_acquire(ip("10.0.0.2")).is_none(), "global cap");
        drop(g);
        assert!(c.try_acquire(a).is_some());
        let c = ConnLimits::new(1000);
        let held: Vec<_> = (0..MAX_CONNECTIONS_PER_IP)
            .map(|_| c.try_acquire(a).unwrap())
            .collect();
        assert!(c.try_acquire(a).is_none(), "per-IP cap");
        assert!(c.try_acquire(ip("10.0.0.2")).is_some());
        drop(held);
        assert!(c.try_acquire(a).is_some());
    }

    #[test]
    fn socket_slots_per_token() {
        let s = SocketSlots::default();
        let g: Vec<_> = (0..MAX_SOCKETS_PER_TOKEN)
            .map(|_| s.acquire("t").unwrap())
            .collect();
        assert!(s.acquire("t").is_none());
        assert!(s.acquire("u").is_some());
        drop(g);
        assert!(s.acquire("t").is_some());
    }
}
