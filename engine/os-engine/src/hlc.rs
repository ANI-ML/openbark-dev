//! A hybrid logical clock (ADR-0017 section 9): every change carries one, so
//! changes from several devices order correctly even when a phone's clock is
//! wrong.
//!
//! A timestamp is the wall-clock milliseconds the device last saw (its own or
//! a later one from another device), a counter for changes within the same
//! millisecond, and the device id as the final tie-break. It never goes
//! backwards on a device, and it's always after every timestamp the device
//! has received.
//!
//! As text it is `<ms, 13 digits>-<counter, 4 hex>-<device>`, so sorting the
//! text sorts the timestamps.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One hybrid logical clock timestamp.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch, as the clock last saw them.
    pub wall_ms: u64,
    /// Changes within the same `wall_ms`.
    pub counter: u16,
    /// The device that made the change (breaks exact ties).
    pub device: String,
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:013}-{:04x}-{}", self.wall_ms, self.counter, self.device)
    }
}

/// A timestamp that isn't `<13 digits>-<4 hex>-<device>`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a clock timestamp: {0}")]
pub struct BadHlc(pub String);

impl FromStr for Hlc {
    type Err = BadHlc;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadHlc(s.to_string());
        let mut parts = s.splitn(3, '-');
        let (ms, counter, device) = (parts.next(), parts.next(), parts.next());
        let ms = ms.filter(|m| m.len() == 13 && m.bytes().all(|b| b.is_ascii_digit())).ok_or_else(bad)?;
        let counter =
            counter.filter(|c| c.len() == 4 && c.bytes().all(|b| b.is_ascii_hexdigit())).ok_or_else(bad)?;
        let device = device.filter(|d| !d.is_empty()).ok_or_else(bad)?;
        Ok(Hlc {
            wall_ms: ms.parse().map_err(|_| bad())?,
            counter: u16::from_str_radix(counter, 16).map_err(|_| bad())?,
            device: device.to_string(),
        })
    }
}

impl Serialize for Hlc {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hlc {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

/// One device's clock.
#[derive(Debug, Clone)]
pub struct Clock {
    device: String,
    last: Option<(u64, u16)>,
}

impl Clock {
    pub fn new(device: impl Into<String>) -> Self {
        Self { device: device.into(), last: None }
    }

    /// A timestamp for a change made now.
    pub fn now(&mut self) -> Hlc {
        self.tick(wall_ms())
    }

    /// A timestamp for a change made when the wall clock read `wall_ms`
    /// (the testable core of [`Clock::now`]).
    pub fn tick(&mut self, wall_ms: u64) -> Hlc {
        let (ms, counter) = match self.last {
            Some((last_ms, c)) if last_ms >= wall_ms => advance(last_ms, c),
            _ => (wall_ms, 0),
        };
        self.last = Some((ms, counter));
        Hlc { wall_ms: ms, counter, device: self.device.clone() }
    }

    /// Take in a timestamp received from another device, so this device's
    /// next timestamps come after it. Returns the timestamp for the receipt.
    pub fn observe(&mut self, remote: &Hlc) -> Hlc {
        self.observe_at(remote, wall_ms())
    }

    /// [`Clock::observe`] with the wall clock reading `wall_ms`.
    pub fn observe_at(&mut self, remote: &Hlc, wall_ms: u64) -> Hlc {
        let (last_ms, last_c) = self.last.unwrap_or((0, 0));
        let top = wall_ms.max(last_ms).max(remote.wall_ms);
        let (ms, counter) = if top == last_ms && top == remote.wall_ms {
            advance(top, last_c.max(remote.counter))
        } else if top == last_ms && self.last.is_some() {
            advance(top, last_c)
        } else if top == remote.wall_ms {
            advance(top, remote.counter)
        } else {
            (top, 0)
        };
        self.last = Some((ms, counter));
        Hlc { wall_ms: ms, counter, device: self.device.clone() }
    }
}

/// The next counter in the same millisecond; past the counter's range, the
/// next millisecond (a clock can't stall forever).
fn advance(ms: u64, counter: u16) -> (u64, u16) {
    match counter.checked_add(1) {
        Some(c) => (ms, c),
        None => (ms + 1, 0),
    }
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_goes_backwards_even_when_the_wall_clock_does() {
        let mut c = Clock::new("a3f9");
        let a = c.tick(1_000);
        let b = c.tick(1_000);
        let d = c.tick(900); // the phone's clock was set back
        let e = c.tick(2_000);
        assert!(a < b && b < d && d < e);
        assert_eq!((b.wall_ms, b.counter), (1_000, 1));
        assert_eq!((d.wall_ms, d.counter), (1_000, 2));
        assert_eq!((e.wall_ms, e.counter), (2_000, 0));
    }

    #[test]
    fn a_received_change_is_always_before_the_next_local_one() {
        // The phone's clock is an hour behind the computer's.
        let mut phone = Clock::new("phone");
        let mut pc = Clock::new("pc");
        let from_pc = pc.tick(5_000_000);
        let receipt = phone.observe_at(&from_pc, 1_400_000);
        assert!(receipt > from_pc);
        let next = phone.tick(1_400_001);
        assert!(next > from_pc, "the slow phone's next change still orders after the computer's");
        // Equal milliseconds: the higher counter wins, plus one.
        let mut x = Clock::new("x");
        x.tick(10);
        x.tick(10);
        let r = x.observe_at(&Hlc { wall_ms: 10, counter: 7, device: "y".into() }, 5);
        assert_eq!((r.wall_ms, r.counter), (10, 8));
    }

    #[test]
    fn devices_break_ties_and_text_sorts_like_timestamps() {
        let a = Hlc { wall_ms: 42, counter: 0, device: "aaaa".into() };
        let b = Hlc { wall_ms: 42, counter: 0, device: "bbbb".into() };
        let c = Hlc { wall_ms: 42, counter: 1, device: "aaaa".into() };
        let d = Hlc { wall_ms: 1_760_000_000_000, counter: 0, device: "aaaa".into() };
        let mut stamps = vec![d.clone(), c.clone(), b.clone(), a.clone()];
        stamps.sort();
        assert_eq!(stamps, [a.clone(), b.clone(), c.clone(), d.clone()]);
        let mut texts: Vec<String> = [&d, &c, &b, &a].iter().map(|h| h.to_string()).collect();
        texts.sort();
        assert_eq!(texts, [a.to_string(), b.to_string(), c.to_string(), d.to_string()]);
        assert_eq!(a.to_string(), "0000000000042-0000-aaaa");
        assert_eq!(a.to_string().parse::<Hlc>().unwrap(), a);
        assert_eq!(serde_json::to_string(&c).unwrap(), "\"0000000000042-0001-aaaa\"");
        assert_eq!(serde_json::from_str::<Hlc>("\"0000000000042-0001-aaaa\"").unwrap(), c);
        for bad in ["", "42-0000-a", "0000000000042-00g0-a", "0000000000042-0000-", "x"] {
            assert!(bad.parse::<Hlc>().is_err(), "{bad}");
        }
    }

    #[test]
    fn the_counter_rolls_into_the_next_millisecond() {
        let mut c = Clock::new("d");
        c.last = Some((7, u16::MAX));
        let h = c.tick(7);
        assert_eq!((h.wall_ms, h.counter), (8, 0));
    }
}
