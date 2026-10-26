//! The iPhone's audio session (ADR-0018). cpal opens RemoteIO but leaves the
//! session in its default category, which only plays: the microphone has no
//! input and iOS never asks for permission. Before every open we ask for the
//! microphone (the system prompt, once) and switch the session to play and
//! record.
//!
//! Called from the capture thread, never the main thread: the prompt is shown
//! on the main thread while this waits for the answer.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryOptions, AVAudioSessionCategoryPlayAndRecord,
    AVAudioSessionModeDefault, AVAudioSessionRecordPermission,
};

use crate::capture::CaptureError;

/// How long the permission prompt may stay unanswered.
const ASK_TIMEOUT: Duration = Duration::from_secs(120);

/// Ask for the microphone if iOS hasn't yet, then make the session record.
#[allow(deprecated)] // AVAudioApplication's replacements need iOS 17; we support older.
pub(crate) fn prepare() -> Result<(), CaptureError> {
    // SAFETY: the shared session is a process-wide singleton, safe to use from any thread.
    let session = unsafe { AVAudioSession::sharedInstance() };
    let permission = unsafe { session.recordPermission() };
    if permission == AVAudioSessionRecordPermission::Denied {
        return Err(CaptureError::PermissionDeniedPhone);
    }
    if permission != AVAudioSessionRecordPermission::Granted {
        let (tx, rx) = mpsc::channel::<bool>();
        let answer = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        unsafe { session.requestRecordPermission(&answer) };
        match rx.recv_timeout(ASK_TIMEOUT) {
            Ok(true) => {}
            Ok(false) => return Err(CaptureError::PermissionDeniedPhone),
            Err(_) => {
                tracing::warn!("the microphone permission prompt went unanswered");
                return Err(CaptureError::PermissionDeniedPhone);
            }
        }
    }
    // Speaker rather than the earpiece for anything played back, and
    // Bluetooth headsets as microphones.
    let options = AVAudioSessionCategoryOptions::DefaultToSpeaker | AVAudioSessionCategoryOptions::AllowBluetooth;
    // SAFETY: the category and mode constants are provided by AVFAudio for the process lifetime.
    let (category, mode) = unsafe { (AVAudioSessionCategoryPlayAndRecord, AVAudioSessionModeDefault) };
    let (Some(category), Some(mode)) = (category, mode) else {
        return Err(CaptureError::Device("AVAudioSession constants missing".into()));
    };
    unsafe { session.setCategory_mode_options_error(category, mode, options) }.map_err(|e| {
        tracing::warn!(error = %e.localizedDescription(), "couldn't set the audio session to record");
        CaptureError::Device(e.localizedDescription().to_string())
    })?;
    unsafe { session.setActive_error(true) }.map_err(|e| {
        tracing::warn!(error = %e.localizedDescription(), "couldn't start the audio session");
        CaptureError::Busy
    })?;
    if !unsafe { session.isInputAvailable() } {
        return Err(CaptureError::NoDevice);
    }
    Ok(())
}
