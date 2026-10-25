//! What the hub remembers between requests, in memory only: nonces seen in
//! the last few minutes, rate-limit buckets, and wrong join codes per client
//! address (ADR-0029 section 3).

use std::collections::HashMap;
use std::time::Instant;

/// Wrong join codes from one address before it is locked out.
pub const MAX_CODE_FAILURES: u32 = 10;
/// How long a lockout lasts, and the window the failures count in.
pub const LOCKOUT_SECS: i64 = 3600;
/// Nonces kept per device (more in five minutes is refused as a replay storm).
const MAX_NONCES: usize = 20_000;

struct Bucket {
    tokens: f64,
    at: Instant,
}

#[derive(Default)]
struct Failures {
    count: u32,
    first: i64,
    locked_until: Option<i64>,
}

#[derive(Default)]
pub struct Guard {
    nonces: HashMap<String, HashMap<String, i64>>,
    buckets: HashMap<String, Bucket>,
    failures: HashMap<String, Failures>,
    last_prune: i64,
}

impl Guard {
    /// Take one request from `key`'s bucket: `per_min` requests a minute,
    /// in bursts of up to `per_min`.
    pub fn allow(&mut self, key: &str, per_min: u32) -> bool {
        let cap = per_min.max(1) as f64;
        let now = Instant::now();
        let b = self.buckets.entry(key.to_string()).or_insert(Bucket { tokens: cap, at: now });
        let refill = now.duration_since(b.at).as_secs_f64() * cap / 60.0;
        b.tokens = (b.tokens + refill).min(cap);
        b.at = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Record a request's nonce; false when this device used it already in
    /// the window (a replay).
    pub fn nonce(&mut self, device: &str, nonce: &str, now: i64, window: i64) -> bool {
        self.prune(now, window);
        let seen = self.nonces.entry(device.to_string()).or_default();
        if seen.contains_key(nonce) || seen.len() >= MAX_NONCES {
            return false;
        }
        seen.insert(nonce.to_string(), now);
        true
    }

    fn prune(&mut self, now: i64, window: i64) {
        if now - self.last_prune < 30 {
            return;
        }
        self.last_prune = now;
        // A nonce is kept for twice the window: a request can be up to one
        // window early or late.
        let keep = 2 * window + 60;
        for seen in self.nonces.values_mut() {
            seen.retain(|_, t| now - *t < keep);
        }
        self.nonces.retain(|_, s| !s.is_empty());
        let stale = Instant::now();
        self.buckets.retain(|_, b| stale.duration_since(b.at).as_secs() < 3600);
        self.failures.retain(|_, f| f.locked_until.is_some_and(|t| t > now) || now - f.first < LOCKOUT_SECS);
    }

    pub fn locked(&self, key: &str, now: i64) -> bool {
        self.failures.get(key).and_then(|f| f.locked_until).is_some_and(|t| t > now)
    }

    /// A wrong join code from `key`; the tenth within an hour locks it out
    /// for an hour.
    pub fn fail(&mut self, key: &str, now: i64) {
        let f = self.failures.entry(key.to_string()).or_default();
        if now - f.first >= LOCKOUT_SECS {
            *f = Failures { count: 0, first: now, locked_until: None };
        }
        f.count += 1;
        if f.count >= MAX_CODE_FAILURES {
            f.locked_until = Some(now + LOCKOUT_SECS);
        }
    }

    pub fn clear(&mut self, key: &str) {
        if !self.locked(key, chrono::Utc::now().timestamp()) {
            self.failures.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nonce_is_used_once() {
        let mut g = Guard::default();
        assert!(g.nonce("d1", "n1", 1000, 300));
        assert!(!g.nonce("d1", "n1", 1001, 300));
        assert!(g.nonce("d2", "n1", 1001, 300), "per device");
    }

    #[test]
    fn buckets_run_out() {
        let mut g = Guard::default();
        for _ in 0..5 {
            assert!(g.allow("ip:1", 5));
        }
        assert!(!g.allow("ip:1", 5));
        assert!(g.allow("ip:2", 5));
    }

    #[test]
    fn ten_wrong_codes_lock_an_address_for_an_hour() {
        let mut g = Guard::default();
        for i in 0..9 {
            g.fail("a", 100 + i);
            assert!(!g.locked("a", 100 + i));
        }
        g.fail("a", 110);
        assert!(g.locked("a", 111));
        assert!(g.locked("a", 110 + LOCKOUT_SECS - 1));
        assert!(!g.locked("a", 110 + LOCKOUT_SECS + 1));
        assert!(!g.locked("b", 111));
    }
}
