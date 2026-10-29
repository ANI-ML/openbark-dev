//! The app's log file (0.2.3): `<data>/OpenBark/logs/openbark.log`, so a
//! clinic can send what happened when a draft was slow or stopped (Settings,
//! About, Copy diagnostics). Release builds have no console, so before this
//! nothing was kept.
//!
//! Hard rule: no transcript text, patient or client names, prompts, note
//! text or tokens. Only counts, timings, stages, devices, model ids and
//! errors are logged anywhere in the engine (the drafting path logs error
//! kinds, not messages). As a second line of defence every line is scrubbed
//! of visit folder names, which carry the patient's name
//! ([`scrub`]), the MLX worker's own output is never written, and the file
//! stays small: it rolls over at [`MAX_BYTES`], keeping one older file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// The log rolls over at this size; one older file is kept.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const FILE: &str = "openbark.log";
const OLD_FILE: &str = "openbark.1.log";
/// Moves the log folder (tests, demos).
pub const DIR_ENV: &str = "OPENBARK_LOGS_DIR";

/// `OPENBARK_LOGS_DIR`, else `<local data>/OpenBark/logs`.
pub fn dir() -> PathBuf {
    match std::env::var_os(DIR_ENV) {
        Some(d) => PathBuf::from(d),
        None => os_models::locations::app_data_dir().join("logs"),
    }
}

struct Rolling {
    dir: PathBuf,
    file: Option<std::fs::File>,
    size: u64,
}

impl Rolling {
    fn open(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut r = Self { dir: dir.to_path_buf(), file: None, size: 0 };
        r.reopen()?;
        Ok(r)
    }

    fn reopen(&mut self) -> std::io::Result<()> {
        let path = self.dir.join(FILE);
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if size >= MAX_BYTES {
            let _ = std::fs::rename(&path, self.dir.join(OLD_FILE));
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        self.size = file.metadata().map_or(0, |m| m.len());
        self.file = Some(file);
        Ok(())
    }

    fn write_line(&mut self, bytes: &[u8]) {
        if self.size >= MAX_BYTES {
            self.file = None;
            let _ = self.reopen();
        }
        let text = scrub(&String::from_utf8_lossy(bytes));
        if let Some(f) = self.file.as_mut()
            && f.write_all(text.as_bytes()).is_ok()
        {
            self.size += text.len() as u64;
        }
    }
}

/// Where tracing's formatter writes: the rolling file, one event at a time.
#[derive(Clone)]
pub struct LogFile(Arc<Mutex<Rolling>>);

impl LogFile {
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        Ok(Self(Arc::new(Mutex::new(Rolling::open(dir)?))))
    }
}

pub struct LogWriter(Arc<Mutex<Rolling>>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut r) = self.0.lock() {
            r.write_line(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Ok(mut r) = self.0.lock()
            && let Some(f) = r.file.as_mut()
        {
            f.flush()?;
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(self.0.clone())
    }
}

/// What the file keeps: OpenBark's own information and up, never the MLX
/// worker's echoed output. `OPENBARK_LOG` can ask for more detail.
pub fn filter() -> tracing_subscriber::EnvFilter {
    let base = std::env::var("OPENBARK_LOG").unwrap_or_else(|_| "info".into());
    tracing_subscriber::EnvFilter::try_new(format!("{base},mlx_worker=off"))
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,mlx_worker=off"))
}

/// The dispatcher the desktop app logs through: the rolling file in `dir`.
pub fn dispatcher(dir: &Path) -> std::io::Result<tracing::Dispatch> {
    let file = LogFile::open(dir)?;
    let subscriber = tracing_subscriber::fmt()
        .with_writer(file)
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(filter())
        .finish();
    Ok(tracing::Dispatch::new(subscriber))
}

/// Log to the file in [`dir`] for the rest of the process, and log panics
/// there too. Nothing happens if a logger is already set or the folder
/// can't be written; the app runs the same without a log.
pub fn init() {
    let dir = dir();
    match dispatcher(&dir) {
        Ok(d) => {
            if tracing::dispatcher::set_global_default(d).is_ok() {
                install_panic_hook();
                tracing::info!(
                    version = env!("CARGO_PKG_VERSION"),
                    os = std::env::consts::OS,
                    arch = std::env::consts::ARCH,
                    "OpenBark started"
                );
            }
        }
        Err(e) => eprintln!("OpenBark couldn't open its log folder: {e}"),
    }
}

/// Panics go to the log (scrubbed and cut short) as well as to the default
/// hook.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let message: String = message.chars().take(300).collect();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        tracing::error!(at = %at, message = %scrub(&message), "panic");
        previous(info);
    }));
}

/// The last `lines` lines of the log, for Copy diagnostics.
pub fn tail(dir: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// `text` with visit folder names cut back to their date and time.
/// A visit's folder is `2026-10-28_101200_mochi-recheck` (date, time, then
/// the patient's name or the imported file's), so the part after the time
/// becomes `…`.
pub fn scrub(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut copied = 0;
    while i + 18 <= b.len() {
        if is_stamp(&b[i..i + 18]) {
            let mut end = i + 18;
            while end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'-' || b[end] == b'_') {
                end += 1;
            }
            if end > i + 18 {
                out.push_str(&text[copied..i + 18]);
                out.push('…');
                copied = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// `YYYY-MM-DD_HHMMSS_`.
fn is_stamp(s: &[u8]) -> bool {
    let digit = |i: usize| s[i].is_ascii_digit();
    (0..4).all(digit)
        && s[4] == b'-'
        && (5..7).all(digit)
        && s[7] == b'-'
        && (8..10).all(digit)
        && s[10] == b'_'
        && (11..17).all(digit)
        && s[17] == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visit_folder_names_lose_the_patient() {
        assert_eq!(
            scrub("could not open /v/2026-10-28_101200_mochi-tanaka/runs: denied"),
            "could not open /v/2026-10-28_101200_…/runs: denied"
        );
        assert_eq!(scrub("visit=2026-01-02_030405_x"), "visit=2026-01-02_030405_…");
        assert_eq!(scrub("no stamp here, 2026-10-28 at 10:12"), "no stamp here, 2026-10-28 at 10:12");
        assert_eq!(scrub("é 2026-10-28_101200_bé"), "é 2026-10-28_101200_…é");
    }

    #[test]
    fn the_file_rolls_over_and_keeps_one_older() {
        let tmp = tempfile::tempdir().unwrap();
        let file = LogFile::open(tmp.path()).unwrap();
        let mut line = vec![b'x'; 64 * 1024 - 1];
        line.push(b'\n');
        let mut w = file.make_writer();
        for _ in 0..40 {
            w.write_all(&line).unwrap();
        }
        let now = std::fs::metadata(tmp.path().join(FILE)).unwrap().len();
        assert!(now < MAX_BYTES + 64 * 1024, "{now}");
        assert!(tmp.path().join(OLD_FILE).exists());
        assert_eq!(tail(tmp.path(), 1).len(), 64 * 1024 - 1);
    }
}
