//! Synthetic visits for checking playback: the desktop app's self-test and
//! the web-mode playback e2e (`apps/desktop/e2e/audio-playback.mjs`). Tones
//! only, with no voices and no patient details. Written the way OpenBark
//! writes its own recordings: 16 kHz 16-bit PCM, encrypted when the vault is
//! on.

use std::io::Write;
use std::path::Path;

use os_engine::session::Session;

use crate::{AppError, Result};

/// A visit made by [`audio_visits`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct DemoVisit {
    /// The visit's id (its folder name).
    pub id: String,
    pub label: &'static str,
    pub secs: f32,
    pub channels: u16,
}

/// Two visits in `root`: a three-minute mono recording, like the
/// microphone's, and a forty-second two-channel one, like a phone call's
/// (each side on its own channel).
pub fn audio_visits(root: &Path) -> Result<Vec<DemoVisit>> {
    std::fs::create_dir_all(root)?;
    Ok(vec![
        tone_visit(root, "Playback check mono", 180.0, 1)?,
        tone_visit(root, "Playback check call", 40.0, 2)?,
    ])
}

fn tone_visit(root: &Path, label: &'static str, secs: f32, channels: u16) -> Result<DemoVisit> {
    let rate = os_asr::SAMPLE_RATE;
    let mut session = Session::create_live(root, label)?;
    let path = session.audio_path();
    let sink: Box<dyn os_asr::AudioSink> = if os_engine::vault::enabled() {
        Box::new(os_engine::vault::AudioWriter::create(&path)?)
    } else {
        Box::new(std::io::BufWriter::new(std::fs::File::create(&path)?))
    };
    let frames = (secs * rate as f32) as usize;
    // A slow sweep per channel, so a decoder has real work and each side differs.
    let sample = |i: usize, ch: u16| {
        let t = i as f32 / rate as f32;
        let f = if ch == 0 { 300.0 + 2.0 * t } else { 600.0 - 2.0 * t };
        0.3 * (2.0 * std::f32::consts::PI * f * t).sin()
    };
    if channels == 1 {
        let mut w = os_asr::WavWriter::with_sink(sink, rate)?;
        for start in (0..frames).step_by(rate as usize) {
            let block: Vec<f32> =
                (start..(start + rate as usize).min(frames)).map(|i| sample(i, 0)).collect();
            w.write(&block)?;
        }
        w.finish()?;
    } else {
        let mut sink = sink;
        let data = (frames * 2 * channels as usize) as u32;
        let block_align = 2 * channels;
        let mut header = Vec::with_capacity(44);
        header.extend(b"RIFF");
        header.extend((36 + data).to_le_bytes());
        header.extend(b"WAVEfmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(1u16.to_le_bytes()); // PCM
        header.extend(channels.to_le_bytes());
        header.extend(rate.to_le_bytes());
        header.extend((rate * block_align as u32).to_le_bytes());
        header.extend(block_align.to_le_bytes());
        header.extend(16u16.to_le_bytes());
        header.extend(b"data");
        header.extend(data.to_le_bytes());
        sink.write_all(&header)?;
        let mut buf = Vec::with_capacity(rate as usize * block_align as usize);
        for i in 0..frames {
            for ch in 0..channels {
                buf.extend(((sample(i, ch) * i16::MAX as f32) as i16).to_le_bytes());
            }
            if buf.len() >= rate as usize * block_align as usize {
                sink.write_all(&buf)?;
                buf.clear();
            }
        }
        sink.write_all(&buf)?;
        sink.flush()?;
        sink.finish()?;
    }
    session.finish_live_audio("playback check", secs)?;
    let id = session
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| AppError("the visit folder has no name".into()))?;
    Ok(DemoVisit { id, label, secs, channels })
}
