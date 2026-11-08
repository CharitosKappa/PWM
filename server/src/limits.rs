//! Rate limits (D29), per client IP for the unauthenticated auth endpoints
//! and per account for everything else. Each server process keeps its own
//! state. Buckets use GCRA (one "theoretical arrival time" per key) in whole
//! nanoseconds, so limits and `Retry-After` values are exact.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::api::{ApiError, AppState};

/// `burst` requests at once, refilled evenly over `per`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    pub burst: u32,
    pub per: Duration,
}

impl Rate {
    pub const fn new(burst: u32, per: Duration) -> Self {
        Self { burst, per }
    }

    /// Time one request uses up.
    fn interval(&self) -> Duration {
        self.per / self.burst.max(1)
    }
}

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// `register/start` and `register/finish`, per IP: 5 sign-ups an hour.
    pub register: Rate,
    /// `prelogin` and `login/*`, per IP.
    pub login: Rate,
    /// `refresh`, per IP.
    pub refresh: Rate,
    /// Every authenticated request, per account.
    pub account: Rate,
    /// Anonymous autofill reports (D38), per IP: 10 an hour.
    pub report: Rate,
    /// New device approval requests (D46), per IP: 5 in 10 minutes.
    pub approval_ask: Rate,
    /// A new device polling its approval request, per IP.
    pub approval_poll: Rate,
    /// Proving the password to delete an account (D74 §1), per IP: 10 an
    /// hour, a budget of its own so it can't be used to guess.
    pub delete: Rate,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            register: Rate::new(10, HOUR),
            login: Rate::new(30, MINUTE),
            refresh: Rate::new(10, MINUTE),
            account: Rate::new(600, MINUTE),
            report: Rate::new(10, HOUR),
            approval_ask: Rate::new(5, Duration::from_secs(600)),
            approval_poll: Rate::new(120, MINUTE),
            delete: Rate::new(10, HOUR),
        }
    }
}

impl Limits {
    /// Limits no test reaches.
    pub fn unlimited() -> Self {
        let rate = Rate::new(u32::MAX, Duration::from_secs(1));
        Self {
            register: rate,
            login: rate,
            refresh: rate,
            account: rate,
            report: rate,
            approval_ask: rate,
            approval_poll: rate,
            delete: rate,
        }
    }
}

/// Time for the buckets; tests use [`ManualClock`].
pub trait Clock: Send + Sync {
    /// Time since some fixed start.
    fn now(&self) -> Duration;
}

struct SystemClock(Instant);

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

pub fn system_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock(Instant::now()))
}

/// A clock that moves only when told to.
#[derive(Default)]
pub struct ManualClock(Mutex<Duration>);

impl ManualClock {
    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Class {
    Register,
    Login,
    Refresh,
    Account,
    Report,
    ApprovalAsk,
    ApprovalPoll,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Ip(IpAddr),
    Account(Uuid),
}

/// When the bucket is empty again if no more requests come.
struct Bucket {
    tat: Duration,
}

/// Buckets beyond this many trigger a sweep of the idle ones.
const SWEEP_AT: usize = 50_000;

pub struct RateLimiter {
    limits: Limits,
    clock: Arc<dyn Clock>,
    buckets: Mutex<HashMap<(Class, Key), Bucket>>,
}

impl RateLimiter {
    pub fn new(limits: Limits, clock: Arc<dyn Clock>) -> Self {
        Self {
            limits,
            clock,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn rate(&self, class: Class) -> Rate {
        match class {
            Class::Register => self.limits.register,
            Class::Login => self.limits.login,
            Class::Refresh => self.limits.refresh,
            Class::Account => self.limits.account,
            Class::Report => self.limits.report,
            Class::ApprovalAsk => self.limits.approval_ask,
            Class::ApprovalPoll => self.limits.approval_poll,
            Class::Delete => self.limits.delete,
        }
    }

    /// Counts one request, or says how many whole seconds until one is
    /// allowed again.
    pub(crate) fn check(&self, class: Class, key: Key) -> Result<(), ApiError> {
        let rate = self.rate(class);
        let interval = rate.interval();
        let tolerance = rate.per.saturating_sub(interval);
        let now = self.clock.now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() >= SWEEP_AT {
            // A bucket that has drained is the same as no bucket.
            buckets.retain(|_, b| b.tat > now);
        }
        let bucket = buckets.entry((class, key)).or_insert(Bucket { tat: now });
        let tat = bucket.tat.max(now);
        let ahead = tat - now;
        if ahead > tolerance {
            let wait = (ahead - tolerance).as_nanos().div_ceil(1_000_000_000);
            return Err(ApiError::RateLimited(
                u64::try_from(wait).unwrap_or(u64::MAX),
            ));
        }
        bucket.tat = tat + interval;
        Ok(())
    }
}

/// Proxies whose `X-Forwarded-For` is believed (`PWM_TRUSTED_PROXIES`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<(IpAddr, u8)>);

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidProxyList;

impl TrustedProxies {
    /// A comma-separated list of addresses and CIDR ranges, such as
    /// `10.0.0.0/8, 192.0.2.1`. Empty means none.
    pub fn parse(text: &str) -> Result<Self, InvalidProxyList> {
        let mut nets = Vec::new();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (addr, prefix) = match part.split_once('/') {
                Some((addr, prefix)) => (addr, Some(prefix)),
                None => (part, None),
            };
            let addr: IpAddr = addr.parse().map_err(|_| InvalidProxyList)?;
            let max = if addr.is_ipv4() { 32 } else { 128 };
            let prefix = match prefix {
                Some(p) => p
                    .parse::<u8>()
                    .ok()
                    .filter(|p| *p <= max)
                    .ok_or(InvalidProxyList)?,
                None => max,
            };
            nets.push((addr, prefix));
        }
        Ok(Self(nets))
    }

    fn contains(&self, ip: IpAddr) -> bool {
        self.0
            .iter()
            .any(|(net, prefix)| same_prefix(*net, ip, *prefix))
    }
}

fn same_prefix(a: IpAddr, b: IpAddr, prefix: u8) -> bool {
    let (a, b, bits) = match (a, b) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (u128::from(u32::from(a)), u128::from(u32::from(b)), 32),
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a), u128::from(b), 128),
        _ => return false,
    };
    let shift = bits - u32::from(prefix);
    shift >= bits || (a >> shift) == (b >> shift)
}

/// The client's address: the connection's, or the last `X-Forwarded-For`
/// entry when the connection comes from a trusted proxy. IPv6 addresses
/// are reduced to their /64, which one subscriber usually holds whole.
pub(crate) fn client_ip(
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
    trusted: &TrustedProxies,
) -> IpAddr {
    let peer = peer.map_or(IpAddr::from([0, 0, 0, 0]), |p| p.ip());
    let forwarded = || {
        let last = headers.get_all("x-forwarded-for").iter().next_back()?;
        last.to_str()
            .ok()?
            .rsplit(',')
            .next()?
            .trim()
            .parse::<IpAddr>()
            .ok()
    };
    let ip = if trusted.contains(peer) {
        forwarded().unwrap_or(peer)
    } else {
        peer
    };
    match ip {
        IpAddr::V6(v6) => {
            let net = u128::from(v6) & !((1u128 << 64) - 1);
            IpAddr::V6(Ipv6Addr::from(net))
        }
        v4 => v4,
    }
}

fn class_of(method: &Method, path: &str) -> Option<Class> {
    // The new device's side of an approval (D46). The signed-in side is
    // limited per account, like any authenticated request.
    if let Some(rest) = path.strip_prefix("/v1/approvals") {
        let parts: Vec<&str> = rest.split('/').skip(1).collect();
        return match (method, parts.as_slice()) {
            (&Method::POST, []) => Some(Class::ApprovalAsk),
            (&Method::GET | &Method::DELETE, [_]) | (&Method::POST, [_, "reveal"]) => {
                Some(Class::ApprovalPoll)
            }
            _ => None,
        };
    }
    match path {
        "/v1/auth/register/start" | "/v1/auth/register/finish" => Some(Class::Register),
        "/v1/auth/prelogin" | "/v1/auth/login/start" | "/v1/auth/login/finish" => {
            Some(Class::Login)
        }
        "/v1/account/delete/start" | "/v1/account/delete/finish" => Some(Class::Delete),
        "/v1/auth/refresh" => Some(Class::Refresh),
        "/v1/reports/autofill" => Some(Class::Report),
        _ => None,
    }
}

/// Per-IP limits for the unauthenticated endpoints (auth and reports). Authenticated
/// requests are limited per account in the `Authenticated` extractor.
pub(crate) async fn by_ip(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if let Some(class) = class_of(request.method(), request.uri().path()) {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        let ip = client_ip(peer, request.headers(), &state.trusted_proxies);
        if let Err(err) = state.limiter.check(class, Key::Ip(ip)) {
            return err.into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bucket_refills_evenly_and_caps_at_its_burst() {
        let clock = Arc::new(ManualClock::default());
        let limits = Limits {
            account: Rate::new(2, Duration::from_secs(10)),
            ..Limits::unlimited()
        };
        let limiter = RateLimiter::new(limits, clock.clone());
        let key = Key::Account(Uuid::nil());
        assert!(limiter.check(Class::Account, key).is_ok());
        assert!(limiter.check(Class::Account, key).is_ok());
        assert_eq!(
            limiter.check(Class::Account, key),
            Err(ApiError::RateLimited(5))
        );
        clock.advance(Duration::from_secs(4));
        assert_eq!(
            limiter.check(Class::Account, key),
            Err(ApiError::RateLimited(1))
        );
        clock.advance(Duration::from_secs(1));
        assert!(limiter.check(Class::Account, key).is_ok());
        // A long pause refills to the burst, not beyond.
        clock.advance(Duration::from_secs(3600));
        assert!(limiter.check(Class::Account, key).is_ok());
        assert!(limiter.check(Class::Account, key).is_ok());
        assert!(limiter.check(Class::Account, key).is_err());
    }

    #[test]
    fn prefixes_match_only_their_family() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(same_prefix(ip("10.0.0.0"), ip("10.255.1.2"), 8));
        assert!(!same_prefix(ip("10.0.0.0"), ip("11.0.0.1"), 8));
        assert!(same_prefix(ip("0.0.0.0"), ip("1.2.3.4"), 0));
        assert!(same_prefix(ip("192.0.2.1"), ip("192.0.2.1"), 32));
        assert!(!same_prefix(ip("::ffff:10.0.0.1"), ip("10.0.0.1"), 8));
        assert!(same_prefix(ip("2001:db8::"), ip("2001:db8::99"), 32));
    }

    #[test]
    fn only_the_new_devices_side_of_an_approval_is_limited_per_ip() {
        let id = "/v1/approvals/00000000-0000-4000-8000-000000000001";
        assert_eq!(
            class_of(&Method::POST, "/v1/approvals"),
            Some(Class::ApprovalAsk)
        );
        assert_eq!(class_of(&Method::GET, id), Some(Class::ApprovalPoll));
        assert_eq!(class_of(&Method::DELETE, id), Some(Class::ApprovalPoll));
        assert_eq!(
            class_of(&Method::POST, &format!("{id}/reveal")),
            Some(Class::ApprovalPoll)
        );
        // The signed-in device's requests: per account instead.
        assert_eq!(class_of(&Method::GET, "/v1/approvals"), None);
        assert_eq!(class_of(&Method::POST, &format!("{id}/accept")), None);
        assert_eq!(class_of(&Method::POST, &format!("{id}/complete")), None);
        assert_eq!(class_of(&Method::POST, &format!("{id}/deny")), None);
    }
}
