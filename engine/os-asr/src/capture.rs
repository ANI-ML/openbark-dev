//! Microphone capture (cpal): WASAPI on Windows, CoreAudio on macOS/iOS,
//! ALSA/PulseAudio on Linux, AAudio on Android.
//!
//! The cpal callback only downmixes to mono and hands the samples to a
//! channel; resampling, WAV writing and recognition happen on the consumer
//! thread so the audio callback never blocks.
//!
//! A stream reports what goes wrong with it (the device unplugged, the
//! system default changed, the driver stopped calling back) through
//! [`MicCapture::fault`] and [`MicCapture::idle_for`]; [`crate::mic`] watches
//! those and reopens the microphone.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, Sample, SampleFormat};

/// Why a microphone couldn't be opened. The messages are shown to the clinic
/// as they are (and translated by the app), so each is a fixed sentence with
/// the next step; the technical detail goes to the log.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error(
        "No microphone was found. Plug one in or switch it on, then press Record again. On a Mac, also check System Settings, Privacy & Security, Microphone."
    )]
    NoDevice,
    #[error("The microphone chosen in Settings isn't connected. Plug it in, or choose another microphone in Settings.")]
    DeviceNotFound(String),
    #[error(
        "Another app is using the microphone and won't share it. Close the other app (a call in Teams or Zoom, for example), or turn off its exclusive control of the microphone in the Windows sound settings, then press Record again."
    )]
    Busy,
    #[error(
        "OpenBark isn't allowed to use the microphone. On a Mac: System Settings, Privacy & Security, Microphone, then turn on OpenBark. On Windows: Settings, Privacy & security, Microphone, then turn on microphone access for desktop apps."
    )]
    PermissionDenied,
    #[error(
        "The microphone couldn't be started. Unplug it and plug it back in, or choose another microphone in Settings, then press Record again."
    )]
    Device(String),
    #[error("This microphone sends audio in a format OpenBark can't read. Choose another microphone in Settings.")]
    Format(SampleFormat),
}

impl CaptureError {
    fn from_cpal(e: &cpal::Error) -> Self {
        match e.kind() {
            ErrorKind::DeviceBusy => Self::Busy,
            ErrorKind::PermissionDenied => Self::PermissionDenied,
            ErrorKind::DeviceNotAvailable => Self::NoDevice,
            _ => Self::Device(e.to_string()),
        }
    }

    /// Worth trying again shortly (the device may come back, or the other
    /// app may let go), rather than a setting the clinic has to change.
    pub fn transient(&self) -> bool {
        matches!(self, Self::NoDevice | Self::DeviceNotFound(_) | Self::Busy | Self::Device(_))
    }
}

/// A block of mono samples at the device's native rate.
pub struct AudioBlock {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// What a running stream reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The device went away (unplugged, Bluetooth dropped, disabled).
    Gone,
    /// The stream must be rebuilt: on Windows, the system default changed
    /// under a stream that follows it, or the device's format changed.
    Invalidated,
    /// Another app took the device.
    Busy,
    /// Any other error that stops the stream.
    Failed,
}

impl Fault {
    fn code(self) -> u8 {
        match self {
            Fault::Gone => 1,
            Fault::Invalidated => 2,
            Fault::Busy => 3,
            Fault::Failed => 4,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        match c {
            1 => Some(Fault::Gone),
            2 => Some(Fault::Invalidated),
            3 => Some(Fault::Busy),
            4 => Some(Fault::Failed),
            _ => None,
        }
    }
    /// How a cpal stream error maps; `None` for glitches that don't stop it.
    fn from_cpal(kind: ErrorKind) -> Option<Self> {
        match kind {
            ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => Some(Fault::Gone),
            ErrorKind::StreamInvalidated | ErrorKind::UnsupportedConfig => Some(Fault::Invalidated),
            ErrorKind::DeviceBusy => Some(Fault::Busy),
            // An xrun is a glitch; a refused real-time priority and a route
            // change the stream followed by itself leave it running.
            ErrorKind::Xrun | ErrorKind::RealtimeDenied | ErrorKind::DeviceChanged => None,
            _ => Some(Fault::Failed),
        }
    }
}

/// Shared between a stream's callbacks and its owner.
struct Flags {
    fault: AtomicU8,
    /// Milliseconds after `epoch` of the last data callback.
    last_data_ms: AtomicU64,
    epoch: Instant,
}

impl Flags {
    fn new() -> Self {
        Self { fault: AtomicU8::new(0), last_data_ms: AtomicU64::new(0), epoch: Instant::now() }
    }
    fn touch(&self) {
        self.last_data_ms.store(self.epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
}

/// Names of available input devices; the first is the system default.
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| device_name(&d));
    let mut names: Vec<String> =
        host.input_devices().map(|it| it.filter_map(|d| device_name(&d)).collect()).unwrap_or_default();
    names.dedup();
    if let Some(default) = default {
        names.retain(|n| n != &default);
        names.insert(0, default);
    }
    names
}

/// The system default microphone's name, if there is one.
pub fn default_input_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| device_name(&d))
}

fn device_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|desc| desc.name().to_string())
}

/// A running microphone stream. Capture stops when this is dropped.
///
/// `cpal::Stream` is not `Send` on every platform, so create and drop it on
/// the same thread.
pub struct MicCapture {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    flags: Arc<Flags>,
    opened: Instant,
}

impl MicCapture {
    /// Open `device` (or the system default) and send mono blocks to `tx`.
    pub fn start(device: Option<&str>, tx: Sender<AudioBlock>) -> Result<Self, CaptureError> {
        let host = cpal::default_host();
        let dev = match device {
            Some(name) => host
                .input_devices()
                .map_err(|e| CaptureError::from_cpal(&e))?
                .find(|d| device_name(d).as_deref() == Some(name))
                .ok_or_else(|| CaptureError::DeviceNotFound(name.to_string()))?,
            // The default device: on Windows cpal then reports a change of
            // default as an invalidated stream, which reopens on the new one.
            None => host.default_input_device().ok_or(CaptureError::NoDevice)?,
        };
        let device_name = device_name(&dev).unwrap_or_else(|| "microphone".into());
        let (config, format) = choose_config(&dev)?;
        let channels = config.channels.max(1) as usize;
        let sample_rate = config.sample_rate;
        let flags = Arc::new(Flags::new());

        macro_rules! stream {
            ($t:ty) => {{
                let tx = tx.clone();
                let data_flags = flags.clone();
                let err_flags = flags.clone();
                dev.build_input_stream(
                    config.clone(),
                    move |data: &[$t], _| {
                        data_flags.touch();
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32
                            })
                            .collect();
                        let _ = tx.send(AudioBlock { samples: mono, sample_rate });
                    },
                    move |e: cpal::Error| match Fault::from_cpal(e.kind()) {
                        Some(fault) => {
                            tracing::warn!(error = %e, ?fault, "microphone stream stopped");
                            let _ = err_flags.fault.compare_exchange(
                                0,
                                fault.code(),
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            );
                        }
                        None => tracing::debug!(error = %e, "microphone stream glitch"),
                    },
                    None,
                )
            }};
        }
        let stream = match format {
            SampleFormat::F32 => stream!(f32),
            SampleFormat::I16 => stream!(i16),
            SampleFormat::I32 => stream!(i32),
            SampleFormat::U16 => stream!(u16),
            SampleFormat::U8 => stream!(u8),
            SampleFormat::I8 => stream!(i8),
            SampleFormat::F64 => stream!(f64),
            SampleFormat::I24 => stream!(cpal::I24),
            f => return Err(CaptureError::Format(f)),
        }
        .map_err(|e| {
            tracing::warn!(error = %e, device = %device_name, "microphone stream couldn't be built");
            CaptureError::from_cpal(&e)
        })?;
        stream.play().map_err(|e| {
            tracing::warn!(error = %e, device = %device_name, "microphone stream couldn't start");
            CaptureError::from_cpal(&e)
        })?;
        tracing::info!(device = %device_name, sample_rate, channels, ?format, "microphone started");
        Ok(Self {
            _stream: stream,
            device_name,
            sample_rate,
            channels: channels as u16,
            flags,
            opened: Instant::now(),
        })
    }

    /// What stopped the stream, if anything has.
    pub fn fault(&self) -> Option<Fault> {
        Fault::from_code(self.flags.fault.load(Ordering::SeqCst))
    }

    /// Time since the device last delivered audio (or since opening, before
    /// the first block).
    pub fn idle_for(&self) -> Duration {
        let last = self.flags.last_data_ms.load(Ordering::Relaxed);
        if last == 0 {
            return self.opened.elapsed();
        }
        self.flags.epoch.elapsed().saturating_sub(Duration::from_millis(last))
    }
}

/// The device's own default configuration when it has one; otherwise the
/// best of what it supports: a format cpal can convert, a rate of 16 kHz or
/// more (closest to 48 kHz), and the fewest channels.
fn choose_config(dev: &cpal::Device) -> Result<(cpal::StreamConfig, SampleFormat), CaptureError> {
    match dev.default_input_config() {
        Ok(c) if readable(c.sample_format()) => return Ok((c.config(), c.sample_format())),
        Ok(c) => tracing::info!(format = ?c.sample_format(), "default microphone format unreadable; looking further"),
        Err(e) => tracing::info!(error = %e, "no default microphone format; looking further"),
    }
    let ranges: Vec<_> = dev.supported_input_configs().map_err(|e| CaptureError::from_cpal(&e))?.collect();
    let options: Vec<ConfigOption> = ranges
        .iter()
        .map(|r| ConfigOption {
            format: r.sample_format(),
            channels: r.channels(),
            min_rate: r.min_sample_rate(),
            max_rate: r.max_sample_rate(),
        })
        .collect();
    let (i, rate) = pick_config(&options).ok_or_else(|| match options.first() {
        Some(o) => CaptureError::Format(o.format),
        None => CaptureError::Device("the microphone lists no configurations".into()),
    })?;
    let range = ranges[i];
    let c = range.with_sample_rate(rate);
    Ok((c.config(), c.sample_format()))
}

fn readable(f: SampleFormat) -> bool {
    matches!(
        f,
        SampleFormat::F32
            | SampleFormat::I16
            | SampleFormat::I32
            | SampleFormat::U16
            | SampleFormat::U8
            | SampleFormat::I8
            | SampleFormat::F64
            | SampleFormat::I24
    )
}

/// One supported range, as [`pick_config`] sees it.
#[derive(Debug, Clone, Copy)]
pub struct ConfigOption {
    pub format: SampleFormat,
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
}

/// Index and rate of the best range: readable, 48 kHz or the nearest rate
/// to it (16 kHz or more preferred), then fewer channels, then float.
pub fn pick_config(options: &[ConfigOption]) -> Option<(usize, u32)> {
    const IDEAL: u32 = 48_000;
    options
        .iter()
        .enumerate()
        .filter(|(_, o)| readable(o.format) && o.channels > 0 && o.min_rate <= o.max_rate)
        .map(|(i, o)| {
            let rate = IDEAL.clamp(o.min_rate, o.max_rate);
            let low = rate < 16_000;
            let float = o.format == SampleFormat::F32;
            (i, rate, (low, rate.abs_diff(IDEAL), o.channels, !float))
        })
        .min_by_key(|(_, _, key)| *key)
        .map(|(i, rate, _)| (i, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(format: SampleFormat, channels: u16, min_rate: u32, max_rate: u32) -> ConfigOption {
        ConfigOption { format, channels, min_rate, max_rate }
    }

    #[test]
    fn picks_48k_mono_float_when_offered() {
        let options = [
            opt(SampleFormat::I16, 2, 44_100, 44_100),
            opt(SampleFormat::F32, 1, 8_000, 96_000),
            opt(SampleFormat::F32, 2, 8_000, 96_000),
        ];
        assert_eq!(pick_config(&options), Some((1, 48_000)));
    }

    #[test]
    fn a_headset_that_only_offers_16k_or_8k_gets_16k() {
        let options = [opt(SampleFormat::I16, 1, 8_000, 8_000), opt(SampleFormat::I16, 1, 16_000, 16_000)];
        assert_eq!(pick_config(&options), Some((1, 16_000)));
    }

    #[test]
    fn a_fixed_44k_stereo_device_is_used_as_it_is() {
        assert_eq!(pick_config(&[opt(SampleFormat::I32, 2, 44_100, 44_100)]), Some((0, 44_100)));
    }

    #[test]
    fn telephone_rate_only_is_still_usable() {
        assert_eq!(pick_config(&[opt(SampleFormat::U8, 1, 8_000, 8_000)]), Some((0, 8_000)));
    }

    #[test]
    fn nothing_usable_is_none() {
        assert_eq!(pick_config(&[]), None);
        assert_eq!(pick_config(&[opt(SampleFormat::F32, 0, 48_000, 48_000)]), None);
    }

    #[test]
    fn stream_errors_map_to_what_the_supervisor_does() {
        assert_eq!(Fault::from_cpal(ErrorKind::DeviceNotAvailable), Some(Fault::Gone));
        assert_eq!(Fault::from_cpal(ErrorKind::StreamInvalidated), Some(Fault::Invalidated));
        assert_eq!(Fault::from_cpal(ErrorKind::DeviceBusy), Some(Fault::Busy));
        assert_eq!(Fault::from_cpal(ErrorKind::Xrun), None);
        assert_eq!(Fault::from_cpal(ErrorKind::DeviceChanged), None);
        assert_eq!(Fault::from_cpal(ErrorKind::BackendError), Some(Fault::Failed));
        for f in [Fault::Gone, Fault::Invalidated, Fault::Busy, Fault::Failed] {
            assert_eq!(Fault::from_code(f.code()), Some(f));
        }
    }

    #[test]
    fn open_errors_are_plain_sentences_with_a_next_step() {
        let all = [
            CaptureError::NoDevice,
            CaptureError::DeviceNotFound("USB mic".into()),
            CaptureError::Busy,
            CaptureError::PermissionDenied,
            CaptureError::Device("0x88890004".into()),
            CaptureError::Format(SampleFormat::U64),
        ];
        for e in all {
            let text = e.to_string();
            // Fixed text (the app translates it by its English wording): no device names or codes.
            assert!(!text.contains("USB mic") && !text.contains("0x"), "{text}");
            assert!(text.ends_with('.'), "{text}");
            assert!(text.split(". ").count() >= 2, "no next step: {text}");
        }
        assert!(CaptureError::Busy.transient());
        assert!(!CaptureError::PermissionDenied.transient());
    }
}
