//! Streaming 16-bit PCM WAV writer: the recording is on disk as it happens,
//! so a crash or power loss mid-consult keeps everything up to that moment
//! (the header is patched on `finish`; readers tolerate an unpatched one).

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// Where a recording goes: a plain file, or an encrypting writer (the
/// engine's vault). `finish` is called once, after the header is patched.
pub trait AudioSink: Write + Seek + Send {
    fn finish(self: Box<Self>) -> std::io::Result<()>;
}

impl AudioSink for BufWriter<File> {
    fn finish(self: Box<Self>) -> std::io::Result<()> {
        let file = (*self).into_inner().map_err(|e| e.into_error())?;
        file.sync_all()
    }
}

pub struct WavWriter {
    out: Box<dyn AudioSink>,
    sample_rate: u32,
    /// 1 (mono), or 2 for a phone call: the clinic left, the client right.
    channels: u16,
    /// Frames written: one sample per channel each.
    samples: u64,
}

impl WavWriter {
    pub fn create(path: &Path, sample_rate: u32) -> std::io::Result<Self> {
        Self::with_sink(Box::new(BufWriter::new(File::create(path)?)), sample_rate)
    }

    /// Write into any sink, e.g. an encrypting one.
    pub fn with_sink(out: Box<dyn AudioSink>, sample_rate: u32) -> std::io::Result<Self> {
        Self::with_sink_channels(out, sample_rate, 1)
    }

    /// A writer with `channels` interleaved channels (1 or 2). Phone calls
    /// are written as two channels, each side on its own (ADR-0023 section 7).
    pub fn with_sink_channels(
        mut out: Box<dyn AudioSink>,
        sample_rate: u32,
        channels: u16,
    ) -> std::io::Result<Self> {
        if !(1..=2).contains(&channels) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "1 or 2 channels"));
        }
        write_header(&mut out, sample_rate, channels, 0)?;
        Ok(Self { out, sample_rate, channels, samples: 0 })
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Append two channels as interleaved frames: `left[i]`, `right[i]`.
    /// Both slices must be the same length. Only for a two-channel writer.
    pub fn write_stereo(&mut self, left: &[f32], right: &[f32]) -> std::io::Result<()> {
        if self.channels != 2 || left.len() != right.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "write_stereo needs a two-channel writer and equal lengths",
            ));
        }
        let mut buf = Vec::with_capacity(left.len() * 4);
        for (l, r) in left.iter().zip(right) {
            for s in [l, r] {
                let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        self.out.write_all(&buf)?;
        self.samples += left.len() as u64;
        Ok(())
    }

    /// Append mono samples in [-1, 1]. Only for a one-channel writer.
    pub fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        if self.channels != 1 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "write needs a mono writer"));
        }
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            buf.extend_from_slice(&v.to_le_bytes());
        }
        self.out.write_all(&buf)?;
        self.samples += samples.len() as u64;
        Ok(())
    }

    pub fn duration_secs(&self) -> f32 {
        self.samples as f32 / self.sample_rate as f32
    }

    /// Flush and write the final sizes into the header.
    pub fn finish(mut self) -> std::io::Result<f32> {
        let secs = self.duration_secs();
        self.out.flush()?;
        self.out.seek(SeekFrom::Start(0))?;
        write_header(&mut self.out, self.sample_rate, self.channels, self.samples)?;
        self.out.flush()?;
        self.out.finish()?;
        Ok(secs)
    }
}

fn write_header(w: &mut impl Write, sample_rate: u32, channels: u16, frames: u64) -> std::io::Result<()> {
    let block = 2 * channels;
    let data_len = (frames * block as u64).min(u32::MAX as u64 - 36) as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&(sample_rate * block as u32).to_le_bytes())?; // byte rate
    w.write_all(&block.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits per sample
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_readable_wav() {
        let dir = std::env::temp_dir().join(format!("ob-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let mut w = WavWriter::create(&path, 16_000).unwrap();
        w.write(&vec![0.5; 8_000]).unwrap();
        w.write(&vec![-0.5; 8_000]).unwrap();
        assert!((w.finish().unwrap() - 1.0).abs() < 1e-6);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 32_000);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 32_000);
        let audio = crate::Audio::load_wav(&path).unwrap();
        assert_eq!(audio.samples.len(), 16_000);
        assert!((audio.samples[0] - 0.5).abs() < 1e-3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_two_channels_that_read_back_mixed() {
        let dir = std::env::temp_dir().join(format!("ob-wav2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("call.wav");
        let file = Box::new(BufWriter::new(File::create(&path).unwrap()));
        let mut w = WavWriter::with_sink_channels(file, 16_000, 2).unwrap();
        assert!(w.write(&[0.0; 4]).is_err(), "mono writes refused on a two-channel writer");
        assert!(w.write_stereo(&[0.0; 3], &[0.0; 2]).is_err(), "sides must line up");
        w.write_stereo(&vec![0.5; 16_000], &vec![0.0; 16_000]).unwrap();
        assert!((w.finish().unwrap() - 1.0).abs() < 1e-6, "seconds count frames, not samples");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 64_000);
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "two channels in the header");
        assert_eq!(u32::from_le_bytes(bytes[28..32].try_into().unwrap()), 64_000, "byte rate");
        // Interleaved: clinic (left) then client (right).
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), 16_384);
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), 0);
        let audio = crate::Audio::load(&path).unwrap();
        assert_eq!(audio.samples.len(), 16_000, "read back as one channel");
        assert!((audio.samples[8_000] - 0.25).abs() < 0.01, "both sides mixed, not the first alone");
        let three = Box::new(BufWriter::new(File::create(dir.join("x.wav")).unwrap()));
        assert!(WavWriter::with_sink_channels(three, 16_000, 3).is_err(), "only one or two channels");
        std::fs::remove_dir_all(&dir).ok();
    }
}
