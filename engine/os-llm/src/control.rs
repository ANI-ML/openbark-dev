//! Following and stopping one generation from another thread.
//!
//! The drafting board needs to know how far the model has read the prompt
//! (minutes on a weak processor, with no tokens yet) and how fast it writes,
//! and the Stop button needs to end a generation at the next prompt batch or
//! token. A [`Control`] is shared between the caller and the backend for
//! that: [`Control::cancel`] from any thread, [`Control::report`] from the
//! backend.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

/// Where a generation is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    /// Reading the prompt: tokens read so far, of `total`.
    Prompt { done: u32, total: u32 },
    /// Writing: tokens generated so far and the rate since the first one.
    /// Sent about once a second while tokens keep coming.
    Writing { tokens: u32, tokens_per_second: f32 },
    /// The model has finished; the caller is checking what it wrote.
    Generated,
}

type Observer = Box<dyn Fn(Phase) + Send + Sync>;

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    observer: Option<Observer>,
}

/// A cancel flag and a progress observer for one generation. Cloning shares
/// both.
#[derive(Clone, Default)]
pub struct Control(Arc<Inner>);

impl Control {
    /// A control with no observer: cancel only.
    pub fn new() -> Self {
        Self::default()
    }

    /// A control whose progress goes to `observer` (on the generating thread).
    pub fn observed(observer: impl Fn(Phase) + Send + Sync + 'static) -> Self {
        Self(Arc::new(Inner { cancelled: AtomicBool::new(false), observer: Some(Box::new(observer)) }))
    }

    /// Ask the generation to stop at its next prompt batch or token.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// `other` is a clone of this control.
    pub fn same(&self, other: &Control) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Tell the observer, if any, where the generation is.
    pub fn report(&self, phase: Phase) {
        if let Some(observer) = &self.0.observer {
            observer(phase);
        }
    }
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control")
            .field("cancelled", &self.is_cancelled())
            .field("observed", &self.0.observer.is_some())
            .finish()
    }
}

/// Reports [`Phase::Writing`] at most once a second, from the token loop.
pub(crate) struct Heartbeat {
    started: std::time::Instant,
    last: Option<std::time::Instant>,
}

impl Heartbeat {
    pub(crate) fn new() -> Self {
        Self { started: std::time::Instant::now(), last: None }
    }

    /// Called after each token: reports the first one, then once a second.
    pub(crate) fn tick(&mut self, control: Option<&Control>, tokens: u32) {
        let Some(control) = control else { return };
        let now = std::time::Instant::now();
        if self.last.is_some_and(|l| now.duration_since(l).as_millis() < 1000) {
            return;
        }
        self.last = Some(now);
        let secs = now.duration_since(self.started).as_secs_f32();
        let tokens_per_second = if secs > 0.0 { tokens as f32 / secs } else { 0.0 };
        control.report(Phase::Writing { tokens, tokens_per_second });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn a_cancel_from_another_clone_is_seen() {
        let control = Control::new();
        let other = control.clone();
        assert!(!control.is_cancelled());
        std::thread::spawn(move || other.cancel()).join().unwrap();
        assert!(control.is_cancelled());
    }

    #[test]
    fn the_heartbeat_reports_the_first_token_then_once_a_second() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let control = Control::observed(move |p| log.lock().unwrap().push(p));
        let mut beat = Heartbeat::new();
        for n in 1..=50 {
            beat.tick(Some(&control), n);
        }
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert!(matches!(seen[0], Phase::Writing { tokens: 1, .. }));
    }
}
