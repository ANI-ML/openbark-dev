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
    samples: u64,
}

impl WavWriter {
    pub fn create(path: &Path, sample_rate: u32) -> std::io::Result<Self> {
        Self::with_sink(Box::new(BufWriter::new(File::create(path)?)), sample_rate)
    }

    /// Write into any sink, e.g. an encrypting one.
    pub fn with_sink(mut out: Box<dyn AudioSink>, sample_rate: u32) -> std::io::Result<Self> {
        write_header(&mut out, sample_rate, 0)?;
        Ok(Self { out, sample_rate, samples: 0 })
    }

    /// Append mono samples in [-1, 1].
    pub fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
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
        write_header(&mut self.out, self.sample_rate, self.samples)?;
        self.out.flush()?;
        self.out.finish()?;
        Ok(secs)
    }
}

fn write_header(w: &mut impl Write, sample_rate: u32, samples: u64) -> std::io::Result<()> {
    let data_len = (samples * 2).min(u32::MAX as u64 - 36) as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&(sample_rate * 2).to_le_bytes())?; // byte rate
    w.write_all(&2u16.to_le_bytes())?; // block align
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
}
