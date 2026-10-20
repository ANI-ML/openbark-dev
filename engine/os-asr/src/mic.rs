//! Keeps a recording's microphone open: reopens it when it fails, follows
//! the system default when that's what was chosen, and comes back to the
//! chosen microphone when it's plugged in again.
//!
//! - **Unplugged, or Bluetooth dropped:** the stream reports the device
//!   gone. The supervisor switches to the system default if there is one
//!   (and says so), otherwise reports the microphone lost and tries again
//!   every second. Nothing is written while it's lost, so the recording
//!   pauses rather than filling with silence.
//! - **Default changed** (AirPods connected, a headset plugged in): with
//!   "System default" chosen, recording moves to the new default.
//! - **Stalled:** a driver that stops calling back without an error (seen
//!   after sleep, or when a USB hub resets) is reopened after a few seconds.
//!
//! The supervisor is polled from the thread that owns the stream (cpal
//! streams can't move between threads). The device layer is a trait so the
//! logic is tested without hardware.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::capture::{AudioBlock, CaptureError, Fault, MicCapture};

/// What happened to the microphone, for the recording's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicEvent {
    /// Recording from `device`. `replaced` names the microphone it took over
    /// from, after a switch (or the chosen one, when that wasn't connected).
    Opened { device: String, sample_rate: u32, replaced: Option<String> },
    /// The microphone went away and no other could be opened: paused.
    Lost { device: String },
    /// The microphone stopped delivering audio; reopening it.
    Stalled { device: String },
}

/// A running stream, as the supervisor sees it.
pub trait MicStream {
    fn device_name(&self) -> &str;
    fn sample_rate(&self) -> u32;
    fn fault(&self) -> Option<Fault>;
    fn idle_for(&self) -> Duration;
}

/// The device layer: cpal in the app, a fake in tests.
pub trait MicBackend {
    type Stream: MicStream;
    /// The system default microphone's name.
    fn default_name(&self) -> Option<String>;
    /// Whether a microphone with this name is connected.
    fn has(&self, name: &str) -> bool;
    fn open(&self, name: Option<&str>, tx: Sender<AudioBlock>) -> Result<Self::Stream, CaptureError>;
}

/// The real thing.
pub struct CpalBackend;

impl MicStream for MicCapture {
    fn device_name(&self) -> &str {
        &self.device_name
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn fault(&self) -> Option<Fault> {
        MicCapture::fault(self)
    }
    fn idle_for(&self) -> Duration {
        MicCapture::idle_for(self)
    }
}

impl MicBackend for CpalBackend {
    type Stream = MicCapture;
    fn default_name(&self) -> Option<String> {
        crate::capture::default_input_name()
    }
    fn has(&self, name: &str) -> bool {
        crate::capture::input_devices().iter().any(|n| n == name)
    }
    fn open(&self, name: Option<&str>, tx: Sender<AudioBlock>) -> Result<MicCapture, CaptureError> {
        MicCapture::start(name, tx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// No audio for this long from a stream with no error: reopen it.
    pub stall: Duration,
    /// While no microphone can be opened, try again this often.
    pub retry: Duration,
    /// How often to look for a new default, or the chosen one coming back.
    pub watch: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self { stall: Duration::from_secs(3), retry: Duration::from_secs(1), watch: Duration::from_secs(2) }
    }
}

pub struct Supervisor<B: MicBackend> {
    backend: B,
    /// The microphone chosen in Settings; `None` follows the system default.
    wanted: Option<String>,
    tx: Sender<AudioBlock>,
    stream: Option<B::Stream>,
    /// The last microphone recorded from, for messages.
    last: String,
    timing: Timing,
    next_retry: Instant,
    next_watch: Instant,
    opened_at: Instant,
    /// `Lost` was reported for the current outage (once per outage).
    lost_reported: bool,
}

impl<B: MicBackend> Supervisor<B> {
    /// Open the chosen microphone, or the default when it isn't connected.
    /// Fails only when nothing can be opened at all.
    pub fn start(
        backend: B,
        wanted: Option<String>,
        tx: Sender<AudioBlock>,
        timing: Timing,
    ) -> Result<(Self, MicEvent), CaptureError> {
        let now = Instant::now();
        let mut s = Self {
            backend,
            wanted,
            tx,
            stream: None,
            last: String::new(),
            timing,
            next_retry: now,
            next_watch: now + timing.watch,
            opened_at: now,
            lost_reported: false,
        };
        let first = match s.wanted.clone() {
            Some(name) => match s.backend.open(Some(&name), s.tx.clone()) {
                Ok(stream) => Ok((stream, None)),
                // The chosen microphone isn't here: use the default, and say so.
                Err(CaptureError::DeviceNotFound(_)) => {
                    s.backend.open(None, s.tx.clone()).map(|stream| (stream, Some(name)))
                }
                Err(e) => Err(e),
            },
            None => s.backend.open(None, s.tx.clone()).map(|stream| (stream, None)),
        };
        let (stream, replaced) = first?;
        let event = s.adopt(stream, replaced, now);
        Ok((s, event))
    }

    pub fn device_name(&self) -> Option<&str> {
        self.stream.as_ref().map(|s| s.device_name())
    }

    fn adopt(&mut self, stream: B::Stream, replaced: Option<String>, now: Instant) -> MicEvent {
        let event = MicEvent::Opened {
            device: stream.device_name().to_string(),
            sample_rate: stream.sample_rate(),
            replaced: replaced.filter(|r| r != stream.device_name()),
        };
        self.last = stream.device_name().to_string();
        self.stream = Some(stream);
        self.opened_at = now;
        self.lost_reported = false;
        event
    }

    /// The microphone to open now: the chosen one if it's connected, else
    /// the default.
    fn open_preferred(&mut self) -> Result<B::Stream, CaptureError> {
        if let Some(name) = self.wanted.clone()
            && self.backend.has(&name)
        {
            match self.backend.open(Some(&name), self.tx.clone()) {
                Ok(s) => return Ok(s),
                Err(e) => tracing::info!(error = %e, "chosen microphone didn't open; trying the default"),
            }
        }
        self.backend.open(None, self.tx.clone())
    }

    /// Check the stream; reopen or switch as needed. Call every 50 ms or so.
    pub fn poll(&mut self, now: Instant) -> Vec<MicEvent> {
        let mut events = Vec::new();
        if let Some(stream) = &self.stream {
            let fault = stream.fault();
            let ended = match fault {
                Some(fault) => {
                    tracing::info!(device = %self.last, ?fault, "microphone stream ended; reopening");
                    true
                }
                None if stream.idle_for() >= self.timing.stall => {
                    tracing::info!(device = %self.last, "microphone stopped delivering audio; reopening");
                    events.push(MicEvent::Stalled { device: self.last.clone() });
                    true
                }
                None => false,
            };
            if ended {
                // Release the old stream before opening another on the same device.
                self.stream = None;
                // A stream that fails as soon as it opens isn't reopened in a tight loop.
                self.next_retry = now.max(self.opened_at + self.timing.retry);
                if fault == Some(Fault::Gone) && now < self.next_retry {
                    // Gone, and not retried yet: say so now rather than in a second.
                    self.lost_reported = true;
                    events.push(MicEvent::Lost { device: self.last.clone() });
                }
            }
        }
        if self.stream.is_none() {
            if now < self.next_retry {
                return events;
            }
            match self.open_preferred() {
                Ok(stream) => {
                    let replaced = Some(self.last.clone()).filter(|p| !p.is_empty());
                    events.push(self.adopt(stream, replaced, now));
                }
                Err(e) => {
                    tracing::debug!(error = %e, "no microphone to reopen yet");
                    if !self.lost_reported {
                        self.lost_reported = true;
                        events.push(MicEvent::Lost { device: self.last.clone() });
                    }
                    self.next_retry = now + self.timing.retry;
                }
            }
            return events;
        }
        if now >= self.next_watch {
            self.next_watch = now + self.timing.watch;
            let current = self.last.clone();
            let switch_to = match &self.wanted {
                // Following the default: has it changed?
                None => self.backend.default_name().filter(|d| *d != current),
                // On the default because the chosen one was missing: is it back?
                Some(w) if *w != current && self.backend.has(w) => Some(w.clone()),
                Some(_) => None,
            };
            if let Some(target) = switch_to {
                tracing::info!(from = %current, to = %target, "switching microphone");
                // Open the new one before letting go of the old, so a failed
                // switch keeps recording.
                let opened = match &self.wanted {
                    None => self.backend.open(None, self.tx.clone()),
                    Some(w) => self.backend.open(Some(w), self.tx.clone()),
                };
                match opened {
                    Ok(stream) => {
                        self.stream = None;
                        events.push(self.adopt(stream, Some(current), now));
                    }
                    Err(e) => tracing::info!(error = %e, "couldn't switch microphone; staying on the current one"),
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::mpsc;

    use super::*;

    /// Microphones that can be plugged, unplugged, made default, or made busy.
    #[derive(Default)]
    struct World {
        /// (name, rate)
        devices: Vec<(String, u32)>,
        default: Option<String>,
        busy: bool,
        opened: Vec<String>,
        /// How long the open stream has gone without audio.
        idle: Duration,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<RefCell<World>>);

    struct FakeStream {
        name: String,
        rate: u32,
        world: Rc<RefCell<World>>,
    }

    impl MicStream for FakeStream {
        fn device_name(&self) -> &str {
            &self.name
        }
        fn sample_rate(&self) -> u32 {
            self.rate
        }
        fn fault(&self) -> Option<Fault> {
            let w = self.world.borrow();
            if w.devices.iter().any(|(n, _)| *n == self.name) { None } else { Some(Fault::Gone) }
        }
        fn idle_for(&self) -> Duration {
            self.world.borrow().idle
        }
    }

    impl MicBackend for Fake {
        type Stream = FakeStream;
        fn default_name(&self) -> Option<String> {
            self.0.borrow().default.clone()
        }
        fn has(&self, name: &str) -> bool {
            self.0.borrow().devices.iter().any(|(n, _)| n == name)
        }
        fn open(&self, name: Option<&str>, _tx: Sender<AudioBlock>) -> Result<FakeStream, CaptureError> {
            let mut w = self.0.borrow_mut();
            if w.busy {
                return Err(CaptureError::Busy);
            }
            let name = match name {
                Some(n) => n.to_string(),
                None => w.default.clone().ok_or(CaptureError::NoDevice)?,
            };
            let rate = w
                .devices
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, r)| *r)
                .ok_or_else(|| CaptureError::DeviceNotFound(name.clone()))?;
            w.opened.push(name.clone());
            Ok(FakeStream { name, rate, world: self.0.clone() })
        }
    }

    impl Fake {
        fn with(devices: &[(&str, u32)], default: Option<&str>) -> Self {
            let f = Fake::default();
            {
                let mut w = f.0.borrow_mut();
                w.devices = devices.iter().map(|(n, r)| (n.to_string(), *r)).collect();
                w.default = default.map(str::to_string);
            }
            f
        }
        fn unplug(&self, name: &str) {
            let mut w = self.0.borrow_mut();
            w.devices.retain(|(n, _)| n != name);
            if w.default.as_deref() == Some(name) {
                w.default = w.devices.first().map(|(n, _)| n.clone());
            }
        }
        fn plug(&self, name: &str, rate: u32, make_default: bool) {
            let mut w = self.0.borrow_mut();
            w.devices.push((name.to_string(), rate));
            if make_default || w.default.is_none() {
                w.default = Some(name.to_string());
            }
        }
        fn set_idle(&self, d: Duration) {
            self.0.borrow_mut().idle = d;
        }
        fn opens(&self) -> usize {
            self.0.borrow().opened.len()
        }
    }

    fn timing() -> Timing {
        Timing {
            stall: Duration::from_millis(3_000),
            retry: Duration::from_millis(1_000),
            watch: Duration::from_millis(2_000),
        }
    }

    fn opened(device: &str, rate: u32, replaced: Option<&str>) -> MicEvent {
        MicEvent::Opened { device: device.into(), sample_rate: rate, replaced: replaced.map(str::to_string) }
    }

    fn start(fake: &Fake, wanted: Option<&str>) -> (Supervisor<Fake>, MicEvent, Instant) {
        let (tx, _rx) = mpsc::channel();
        let t0 = Instant::now();
        let (s, e) = Supervisor::start(fake.clone(), wanted.map(str::to_string), tx, timing()).unwrap();
        (s, e, t0)
    }

    #[test]
    fn no_microphone_at_all_fails_to_start_with_a_plain_error() {
        let (tx, _rx) = mpsc::channel();
        let err = Supervisor::start(Fake::with(&[], None), None, tx, timing()).err().unwrap();
        assert!(matches!(err, CaptureError::NoDevice));
    }

    #[test]
    fn a_microphone_another_app_holds_fails_to_start_with_a_plain_error() {
        let fake = Fake::with(&[("Built-in", 48_000)], Some("Built-in"));
        fake.0.borrow_mut().busy = true;
        let (tx, _rx) = mpsc::channel();
        assert!(matches!(Supervisor::start(fake, None, tx, timing()).err().unwrap(), CaptureError::Busy));
    }

    #[test]
    fn the_chosen_microphone_missing_at_start_falls_back_to_the_default_and_says_so() {
        let fake = Fake::with(&[("Built-in", 48_000)], Some("Built-in"));
        let (_s, e, _) = start(&fake, Some("USB mic"));
        assert_eq!(e, opened("Built-in", 48_000, Some("USB mic")));
    }

    #[test]
    fn unplugging_moves_to_the_new_default_without_a_gap() {
        let fake = Fake::with(&[("AirPods", 24_000), ("Built-in", 48_000)], Some("AirPods"));
        let (mut s, e, t0) = start(&fake, None);
        assert_eq!(e, opened("AirPods", 24_000, None));
        fake.unplug("AirPods");
        let events = s.poll(t0 + Duration::from_millis(1_500));
        assert_eq!(events, vec![opened("Built-in", 48_000, Some("AirPods"))]);
        assert_eq!(s.device_name(), Some("Built-in"));
    }

    #[test]
    fn unplugging_the_only_microphone_pauses_once_then_resumes_when_one_returns() {
        let fake = Fake::with(&[("USB mic", 44_100)], Some("USB mic"));
        let (mut s, _, t0) = start(&fake, None);
        let ms = |n| t0 + Duration::from_millis(n);
        fake.unplug("USB mic");
        assert_eq!(s.poll(ms(1_100)), vec![MicEvent::Lost { device: "USB mic".into() }]);
        // Reported once per outage, retried every second.
        assert_eq!(s.poll(ms(1_500)), vec![]);
        assert_eq!(s.poll(ms(2_200)), vec![]);
        assert_eq!(s.device_name(), None);
        fake.plug("USB mic", 44_100, true);
        assert_eq!(s.poll(ms(2_500)), vec![], "waits for the next retry");
        assert_eq!(s.poll(ms(3_300)), vec![opened("USB mic", 44_100, None)]);
        // A second outage is reported again.
        fake.unplug("USB mic");
        assert_eq!(s.poll(ms(4_300)), vec![MicEvent::Lost { device: "USB mic".into() }]);
    }

    #[test]
    fn a_new_default_is_followed_when_system_default_was_chosen() {
        let fake = Fake::with(&[("Built-in", 48_000)], Some("Built-in"));
        let (mut s, _, t0) = start(&fake, None);
        fake.plug("AirPods Hands-Free", 16_000, true);
        // Checked every 2 s, not on every poll.
        assert_eq!(s.poll(t0 + Duration::from_millis(500)), vec![]);
        assert_eq!(
            s.poll(t0 + Duration::from_millis(2_100)),
            vec![opened("AirPods Hands-Free", 16_000, Some("Built-in"))]
        );
    }

    #[test]
    fn a_failed_switch_keeps_recording_on_the_current_microphone() {
        let fake = Fake::with(&[("Built-in", 48_000)], Some("Built-in"));
        let (mut s, _, t0) = start(&fake, None);
        fake.plug("Teams Headset", 16_000, true);
        fake.0.borrow_mut().busy = true;
        assert_eq!(s.poll(t0 + Duration::from_millis(2_100)), vec![]);
        assert_eq!(s.device_name(), Some("Built-in"));
    }

    #[test]
    fn a_chosen_microphone_is_kept_when_the_default_changes() {
        let fake = Fake::with(&[("USB mic", 48_000), ("Built-in", 48_000)], Some("Built-in"));
        let (mut s, e, t0) = start(&fake, Some("USB mic"));
        assert_eq!(e, opened("USB mic", 48_000, None));
        fake.plug("AirPods", 24_000, true);
        assert_eq!(s.poll(t0 + Duration::from_millis(2_100)), vec![]);
        assert_eq!(s.device_name(), Some("USB mic"));
    }

    #[test]
    fn the_chosen_microphone_is_taken_back_when_it_returns() {
        let fake = Fake::with(&[("USB mic", 48_000), ("Built-in", 44_100)], Some("Built-in"));
        let (mut s, _, t0) = start(&fake, Some("USB mic"));
        fake.unplug("USB mic");
        assert_eq!(s.poll(t0 + Duration::from_millis(1_100)), vec![opened("Built-in", 44_100, Some("USB mic"))]);
        fake.plug("USB mic", 48_000, false);
        assert_eq!(s.poll(t0 + Duration::from_millis(2_200)), vec![opened("USB mic", 48_000, Some("Built-in"))]);
    }

    #[test]
    fn a_stalled_stream_is_reopened() {
        let fake = Fake::with(&[("Built-in", 48_000)], Some("Built-in"));
        let (mut s, _, t0) = start(&fake, None);
        fake.set_idle(Duration::from_millis(3_500));
        let events = s.poll(t0 + Duration::from_millis(1_200));
        assert_eq!(events[0], MicEvent::Stalled { device: "Built-in".into() });
        assert_eq!(events[1], opened("Built-in", 48_000, None));
        assert_eq!(fake.opens(), 2);
    }

    #[test]
    fn a_stream_that_fails_as_soon_as_it_opens_is_not_reopened_in_a_tight_loop() {
        let fake = Fake::with(&[("Built-in", 48_000)], Some("Built-in"));
        let (mut s, _, t0) = start(&fake, None);
        fake.set_idle(Duration::from_secs(10));
        let first = s.poll(t0 + Duration::from_millis(100));
        assert_eq!(first, vec![MicEvent::Stalled { device: "Built-in".into() }]);
        assert_eq!(fake.opens(), 1, "waits for the retry interval");
        assert_eq!(s.poll(t0 + Duration::from_millis(500)), vec![]);
        assert!(matches!(s.poll(t0 + Duration::from_millis(1_100)).as_slice(), [MicEvent::Opened { .. }]));
    }
}
