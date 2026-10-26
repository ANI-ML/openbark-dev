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
//!
//! Without the `capture` feature (a build that doesn't record) there is no
//! microphone: [`input_devices`] is empty and [`MicCapture::start`] says so.

#[cfg(feature = "capture")]
use std::sync::Arc;
#[cfg(feature = "capture")]
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;
#[cfg(feature = "capture")]
use std::time::Instant;

#[cfg(feature = "capture")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(feature = "capture")]
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
    #[error(
        "The microphone chosen in Settings isn't connected. Plug it in, or choose another microphone in Settings."
    )]
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
        "OpenBark isn't allowed to use the microphone. Turn it on in the iPhone's Settings, OpenBark, Microphone, then press Record again."
    )]
    PermissionDeniedPhone,
    #[error(
        "The microphone couldn't be started. Unplug it and plug it back in, or choose another microphone in Settings, then press Record again."
    )]
    Device(String),
    #[error(
        "This microphone sends audio in a format OpenBark can't read. Choose another microphone in Settings."
    )]
    #[cfg(feature = "capture")]
    Format(SampleFormat),
}

impl CaptureError {
    #[cfg(feature = "capture")]
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
    #[cfg(feature = "capture")]
    fn code(self) -> u8 {
        match self {
            Fault::Gone => 1,
            Fault::Invalidated => 2,
            Fault::Busy => 3,
            Fault::Failed => 4,
        }
    }
    #[cfg(feature = "capture")]
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
    #[cfg(feature = "capture")]
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
#[cfg(feature = "capture")]
struct Flags {
    fault: AtomicU8,
    /// Milliseconds after `epoch` of the last data callback.
    last_data_ms: AtomicU64,
    epoch: Instant,
}

#[cfg(feature = "capture")]
impl Flags {
    fn new() -> Self {
        Self { fault: AtomicU8::new(0), last_data_ms: AtomicU64::new(0), epoch: Instant::now() }
    }
    fn touch(&self) {
        self.last_data_ms.store(self.epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
}

/// What a build without the `capture` feature answers.
#[cfg(not(feature = "capture"))]
const NO_CAPTURE: &str = "recording isn't in this build";

/// Names of available input devices: none without the `capture` feature.
#[cfg(not(feature = "capture"))]
pub fn input_devices() -> Vec<String> {
    Vec::new()
}

/// No system default microphone without the `capture` feature.
#[cfg(not(feature = "capture"))]
pub fn default_input_name() -> Option<String> {
    None
}

/// A microphone stream, in a build that can't record: never opened.
#[cfg(not(feature = "capture"))]
pub struct MicCapture {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[cfg(not(feature = "capture"))]
impl MicCapture {
    pub fn start(device: Option<&str>, tx: Sender<AudioBlock>) -> Result<Self, CaptureError> {
        let _ = (device, tx);
        Err(CaptureError::Device(NO_CAPTURE.into()))
    }

    pub fn fault(&self) -> Option<Fault> {
        None
    }

    pub fn idle_for(&self) -> Duration {
        Duration::ZERO
    }
}

/// Names of available input devices; the first is the system default.
#[cfg(feature = "capture")]
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
#[cfg(feature = "capture")]
pub fn default_input_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| device_name(&d))
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
    pub channels: u16,
    flags: Arc<Flags>,
    opened: Instant,
}

#[cfg(feature = "capture")]
impl MicCapture {
    /// Open `device` (or the system default) and send mono blocks to `tx`.
    pub fn start(device: Option<&str>, tx: Sender<AudioBlock>) -> Result<Self, CaptureError> {
        #[cfg(target_os = "ios")]
        crate::ios_audio::prepare()?;
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
                let mut mixer = Downmixer::new(channels, sample_rate);
                dev.build_input_stream(
                    config.clone(),
                    move |data: &[$t], _| {
                        data_flags.touch();
                        let mono = if channels == 1 {
                            data.iter().map(|s| s.to_sample::<f32>()).collect()
                        } else {
                            let samples: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();
                            mixer.process(&samples)
                        };
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
#[cfg(feature = "capture")]
fn choose_config(dev: &cpal::Device) -> Result<(cpal::StreamConfig, SampleFormat), CaptureError> {
    match dev.default_input_config() {
        Ok(c) if readable(c.sample_format()) => return Ok((c.config(), c.sample_format())),
        Ok(c) => {
            tracing::info!(format = ?c.sample_format(), "default microphone format unreadable; looking further")
        }
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

#[cfg(feature = "capture")]
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

/// A channel whose block never goes above this (about -80 dBFS) carries
/// nothing: an unused input of an audio interface, or a dead side.
const SILENT_CHANNEL: f32 = 1e-4;

/// Interleaved frames to mono: the average of the channels that carry
/// signal. A plain average of every channel would bury a microphone on input
/// 1 of a 16-channel interface (or a virtual device) 24 dB down, and one on
/// the left of a two-input interface 6 dB down; a stereo microphone (both
/// sides alike) comes out as it is either way.
///
/// A channel counts as soon as it carries signal, and stops counting only
/// after a second without any, so a noisy channel hovering near the
/// threshold doesn't make the level jump 6 dB from block to block.
pub struct Downmixer {
    channels: usize,
    active: Vec<bool>,
    /// Frames each channel has been silent for.
    quiet: Vec<usize>,
    /// Frames of silence before a channel stops counting (one second).
    hold: usize,
}

impl Downmixer {
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        let channels = channels.max(1);
        Self {
            channels,
            active: vec![false; channels],
            quiet: vec![0; channels],
            hold: sample_rate.max(1) as usize,
        }
    }

    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let channels = self.channels;
        if channels == 1 {
            return interleaved.to_vec();
        }
        let frames = interleaved.len() / channels;
        let mut peaks = vec![0.0f32; channels];
        for frame in interleaved.chunks_exact(channels) {
            for (p, s) in peaks.iter_mut().zip(frame) {
                *p = p.max(s.abs());
            }
        }
        for ((peak, active), quiet) in peaks.iter().zip(self.active.iter_mut()).zip(self.quiet.iter_mut()) {
            if *peak >= SILENT_CHANNEL {
                *active = true;
                *quiet = 0;
            } else {
                *quiet = quiet.saturating_add(frames);
                if *quiet >= self.hold {
                    *active = false;
                }
            }
        }
        let active: Vec<usize> = (0..channels).filter(|&c| self.active[c]).collect();
        let used: Vec<usize> = if active.is_empty() { (0..channels).collect() } else { active };
        let n = used.len() as f32;
        interleaved
            .chunks_exact(channels)
            .map(|frame| used.iter().map(|&c| frame[c]).sum::<f32>() / n)
            .collect()
    }
}

/// [`Downmixer`] on one block with no history (tests and one-off use).
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    Downmixer::new(channels, 48_000).process(interleaved)
}

/// One supported range, as [`pick_config`] sees it.
#[derive(Debug, Clone, Copy)]
#[cfg(feature = "capture")]
pub struct ConfigOption {
    pub format: SampleFormat,
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
}

/// Index and rate of the best range: readable, 48 kHz or the nearest rate
/// to it (16 kHz or more preferred), then fewer channels, then float.
#[cfg(feature = "capture")]
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

#[cfg(all(test, feature = "capture"))]
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
    fn downmix_keeps_a_microphone_on_one_input_at_its_level() {
        // Stereo microphone, both sides alike: unchanged.
        assert_eq!(downmix(&[0.5, 0.5, -0.25, -0.25], 2), vec![0.5, -0.25]);
        // Microphone on input 1 of a two-input interface, input 2 unused.
        assert_eq!(downmix(&[0.5, 0.0, -0.25, 0.0], 2), vec![0.5, -0.25]);
        // A 16-channel virtual device with speech on the first two.
        let mut frame = vec![0.0f32; 16];
        frame[0] = 0.4;
        frame[1] = 0.2;
        let out = downmix(&frame, 16);
        assert!(out.len() == 1 && (out[0] - 0.3).abs() < 1e-6, "{out:?}");
        // Two different voices on two inputs: both kept, averaged.
        assert_eq!(downmix(&[0.4, -0.2], 2), vec![0.1]);
        // Silence everywhere stays silence.
        assert_eq!(downmix(&[0.0; 8], 4), vec![0.0, 0.0]);
        // A trailing partial frame is dropped.
        assert_eq!(downmix(&[0.5, 0.5, 0.1], 2), vec![0.5]);
    }

    #[test]
    fn a_noisy_channel_near_the_threshold_does_not_make_the_level_jump() {
        // Speech on input 1; input 2 carries noise that dips below the
        // threshold every other block. Without a hold, the output would
        // alternate between ch1 and (ch1 + ch2) / 2: a 6 dB jump each block.
        let mut m = Downmixer::new(2, 48_000);
        let mut levels = Vec::new();
        for block in 0..40 {
            let noise = if block % 2 == 0 { 2e-4 } else { 5e-5 };
            let frames: Vec<f32> = (0..480).flat_map(|_| [0.5f32, noise]).collect();
            let out = m.process(&frames);
            levels.push(out[0]);
        }
        let (lo, hi) = levels[2..].iter().fold((f32::MAX, f32::MIN), |(lo, hi), &l| (lo.min(l), hi.max(l)));
        assert!(hi - lo < 1e-3, "level jumped between {lo} and {hi}");
    }

    #[test]
    fn a_channel_that_goes_quiet_stops_counting_after_a_second() {
        let mut m = Downmixer::new(2, 1_000);
        // Both inputs live, then input 2 goes silent.
        assert_eq!(m.process(&[0.4, 0.2]), vec![0.3]);
        let quiet: Vec<f32> = (0..500).flat_map(|_| [0.4f32, 0.0]).collect();
        assert!((m.process(&quiet)[0] - 0.2).abs() < 1e-6, "still counted half a second in");
        let out = m.process(&quiet);
        assert!((out[out.len() - 1] - 0.4).abs() < 1e-6, "dropped after a second: {}", out[out.len() - 1]);
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
