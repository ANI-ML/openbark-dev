//! Keep the machine awake while recording or processing.
//!
//! A laptop that sleeps mid-consult loses the recording or stalls the note,
//! so every long job holds an [`AwakeGuard`] until it finishes. Desktop OSes
//! use the `keepawake` crate (Windows `SetThreadExecutionState`, macOS IOKit
//! power assertions, Linux systemd/D-Bus inhibitors). iOS and Android keep
//! awake through the native audio session / foreground service (spike S1);
//! there the guard is a no-op.
//!
//! Failing to acquire the guard never fails the job: it is logged instead.

use crate::PRODUCT_NAME;

/// What the machine is being kept awake for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwakeReason {
    /// Live recording: keep the display on too, so the clinician sees it.
    Recording,
    /// Transcription or note generation: the display may turn off.
    Processing,
}

/// Prevents idle sleep until dropped.
pub struct AwakeGuard {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    _inner: Option<keepawake::KeepAwake>,
}

impl AwakeGuard {
    pub fn acquire(reason: AwakeReason) -> Self {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            let why = match reason {
                AwakeReason::Recording => "Recording a consultation",
                AwakeReason::Processing => "Transcribing and drafting a note",
            };
            let inner = keepawake::Builder::default()
                .idle(true)
                .display(reason == AwakeReason::Recording)
                .reason(why)
                .app_name(PRODUCT_NAME)
                .app_reverse_domain("health.animl.openbark")
                .create()
                .map_err(
                    |e| tracing::warn!(error = %e, "could not prevent sleep; keep the device awake manually"),
                )
                .ok();
            Self { _inner: inner }
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = (reason, PRODUCT_NAME);
            Self {}
        }
    }
}
