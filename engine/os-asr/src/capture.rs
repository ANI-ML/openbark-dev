//! Microphone capture (cpal): WASAPI on Windows, CoreAudio on macOS/iOS,
//! ALSA/PulseAudio on Linux, AAudio on Android.
//!
//! The cpal callback only downmixes to mono and hands the samples to a
//! channel; resampling, WAV writing and recognition happen on the consumer
//! thread so the audio callback never blocks.

//!
//! Without the `capture` feature (a build that doesn't record) there is no
//! microphone: [`input_devices`] is empty and [`MicCapture::start`] says so.

use std::sync::mpsc::Sender;

#[cfg(feature = "capture")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(feature = "capture")]
use cpal::{Sample, SampleFormat};

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no microphone found")]
    NoDevice,
    #[error("microphone '{0}' not found")]
    DeviceNotFound(String),
    #[error("microphone error: {0}")]
    Device(String),
    /// The device's sample format, as cpal names it.
    #[error("unsupported microphone sample format {0}")]
    Format(String),
}

/// A block of mono samples at the device's native rate.
pub struct AudioBlock {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// What a build without the `capture` feature answers.
#[cfg(not(feature = "capture"))]
const NO_CAPTURE: &str = "recording isn't in this build";

/// Names of available input devices: none without the `capture` feature.
#[cfg(not(feature = "capture"))]
pub fn input_devices() -> Vec<String> {
    Vec::new()
}

/// A microphone stream, in a build that can't record.
#[cfg(not(feature = "capture"))]
pub struct MicCapture {
    pub device_name: String,
    pub sample_rate: u32,
}

#[cfg(not(feature = "capture"))]
impl MicCapture {
    pub fn start(device: Option<&str>, tx: Sender<AudioBlock>) -> Result<Self, CaptureError> {
        let _ = (device, tx);
        Err(CaptureError::Device(NO_CAPTURE.into()))
    }
}

/// Names of available input devices; the first is the system default.
#[cfg(feature = "capture")]
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| device_name(&d));
    let mut names: Vec<String> =
        host.input_devices().map(|it| it.filter_map(|d| device_name(&d)).collect()).unwrap_or_default();
    if let Some(default) = default {
        names.retain(|n| n != &default);
        names.insert(0, default);
    }
    names
}

#[cfg(feature = "capture")]
fn device_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|desc| desc.name().to_string())
}

/// A running microphone stream. Capture stops when this is dropped.
///
/// `cpal::Stream` is not `Send` on every platform, so create and drop it on
/// the same thread.
#[cfg(feature = "capture")]
pub struct MicCapture {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: u32,
}

#[cfg(feature = "capture")]
impl MicCapture {
    /// Open `device` (or the system default) and send mono blocks to `tx`.
    pub fn start(device: Option<&str>, tx: Sender<AudioBlock>) -> Result<Self, CaptureError> {
        let host = cpal::default_host();
        let dev = match device {
            Some(name) => host
                .input_devices()
                .map_err(|e| CaptureError::Device(e.to_string()))?
                .find(|d| device_name(d).as_deref() == Some(name))
                .ok_or_else(|| CaptureError::DeviceNotFound(name.to_string()))?,
            None => host.default_input_device().ok_or(CaptureError::NoDevice)?,
        };
        let device_name = device_name(&dev).unwrap_or_else(|| "microphone".into());
        let supported = dev.default_input_config().map_err(|e| CaptureError::Device(e.to_string()))?;
        let config = supported.config();
        let channels = config.channels.max(1) as usize;
        let sample_rate = config.sample_rate;
        let err = |e| tracing::error!(error = %e, "microphone stream error");

        macro_rules! stream {
            ($t:ty) => {{
                let tx = tx.clone();
                dev.build_input_stream(
                    config.clone(),
                    move |data: &[$t], _| {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32
                            })
                            .collect();
                        let _ = tx.send(AudioBlock { samples: mono, sample_rate });
                    },
                    err,
                    None,
                )
            }};
        }
        let stream = match supported.sample_format() {
            SampleFormat::F32 => stream!(f32),
            SampleFormat::I16 => stream!(i16),
            SampleFormat::I32 => stream!(i32),
            SampleFormat::U16 => stream!(u16),
            SampleFormat::U8 => stream!(u8),
            SampleFormat::I8 => stream!(i8),
            f => return Err(CaptureError::Format(format!("{f:?}"))),
        }
        .map_err(|e| CaptureError::Device(e.to_string()))?;
        stream.play().map_err(|e| CaptureError::Device(e.to_string()))?;
        tracing::info!(device = %device_name, sample_rate, channels, "microphone started");
        Ok(Self { _stream: stream, device_name, sample_rate })
    }
}
