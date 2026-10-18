//! Encryption at rest (ADR-0008). Every file in an encounter folder goes
//! through here: XChaCha20-Poly1305 under one random data key kept in the
//! OS keychain.
//!
//! - Small files: `OBK1` ‖ nonce (24) ‖ ciphertext; the file name is bound as
//!   associated data.
//! - Audio: `OBA1` ‖ chunk size (u32 LE) ‖ chunks of nonce (24) ‖ ciphertext
//!   (≤ 64 KiB + 16), the chunk index bound as associated data. Written as the
//!   recording happens; chunk 0 (with the WAV header) is re-encrypted with a
//!   fresh nonce when the sizes are known.
//!
//! Files without a magic header are read as plain (encounters from before
//! encryption); `encrypt_existing` rewrites them.
//!
//! The vault is off until [`init`] runs (the app and CLI call it), so
//! library tests and tools that never touch clinical data stay plain.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::Serialize;

const FILE_MAGIC: &[u8; 4] = b"OBK1";
const AUDIO_MAGIC: &[u8; 4] = b"OBA1";
const NONCE: usize = 24;
const TAG: usize = 16;
/// Audio plaintext per chunk: 2 s of 16 kHz 16-bit mono.
pub const CHUNK: usize = 64 * 1024;
const SERVICE: &str = "OpenBark";
const ACCOUNT: &str = "data-key";

/// Where the data key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// `OPENBARK_DATA_KEY` (servers, CI, restoring on a new machine).
    Environment,
    /// The OS keychain (Credential Manager, Keychain, Secret Service).
    Keychain,
    /// `<local data>/OpenBark/data.key`, protected by the OS account only.
    KeyFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct VaultStatus {
    pub enabled: bool,
    pub source: Option<KeySource>,
    /// Shown when the key fell back to a file, or couldn't be loaded.
    pub warning: Option<String>,
}

struct State {
    cipher: XChaCha20Poly1305,
    key: [u8; 32],
    source: KeySource,
    warning: Option<String>,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| err(format!("no secure random numbers: {e}")))?;
    Ok(buf)
}

fn key_file() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPENBARK_KEY_FILE") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark").join("data.key")
}

fn decode_key(text: &str) -> Option<[u8; 32]> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()?;
    bytes.try_into().ok()
}

fn encode_key(key: &[u8; 32]) -> String {
    base64::engine::general_purpose::STANDARD.encode(key)
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn keychain_get() -> Result<Option<[u8; 32]>, String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(text) => decode_key(&text).map(Some).ok_or_else(|| "the stored key is damaged".to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn keychain_set(key: &[u8; 32]) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    entry.set_password(&encode_key(key)).map_err(|e| e.to_string())?;
    // Read it back: some stores accept a write they can't keep.
    match keychain_get()? {
        Some(k) if &k == key => Ok(()),
        _ => Err("the keychain didn't keep the key".into()),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn keychain_get() -> Result<Option<[u8; 32]>, String> {
    Err("no keychain on this platform".into())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn keychain_set(_: &[u8; 32]) -> Result<(), String> {
    Err("no keychain on this platform".into())
}

fn write_key_file(key: &[u8; 32]) -> io::Result<()> {
    let path = key_file();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, encode_key(key))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

const FILE_WARNING: &str = "The encryption key is in a file in your user folder because this computer's keychain isn't available. Encounters copied off this computer stay protected; keep the recovery key safe.";

/// Load (or on first use, create) the data key and turn encryption on.
pub fn init() -> io::Result<VaultStatus> {
    let (key, source, warning) = if let Some(text) =
        std::env::var("OPENBARK_DATA_KEY").ok().filter(|t| !t.trim().is_empty())
    {
        let key = decode_key(&text)
            .ok_or_else(|| err("OPENBARK_DATA_KEY isn't a valid key (base64, 32 bytes)."))?;
        (key, KeySource::Environment, None)
    } else {
        let file = key_file();
        match keychain_get() {
            Ok(Some(key)) => (key, KeySource::Keychain, None),
            keychain => {
                if file.is_file() {
                    let text = fs::read_to_string(&file)?;
                    let key =
                        decode_key(&text).ok_or_else(|| err(format!("{} is damaged.", file.display())))?;
                    (key, KeySource::KeyFile, Some(FILE_WARNING.to_string()))
                } else {
                    let key = random::<32>()?;
                    let stored = match keychain {
                        Ok(None) => keychain_set(&key),
                        Ok(Some(_)) => unreachable!(),
                        Err(e) => Err(e),
                    };
                    match stored {
                        Ok(()) => (key, KeySource::Keychain, None),
                        Err(reason) => {
                            tracing::warn!(%reason, "keychain unavailable; keeping the data key in a file");
                            write_key_file(&key)?;
                            (key, KeySource::KeyFile, Some(FILE_WARNING.to_string()))
                        }
                    }
                }
            }
        }
    };
    use_key(key, source, warning.clone());
    Ok(status())
}

/// Turn encryption on with a given key (tests, `OPENBARK_DATA_KEY`).
pub fn use_key(key: [u8; 32], source: KeySource, warning: Option<String>) {
    let cipher = XChaCha20Poly1305::new(&key.into());
    *STATE.write().expect("vault lock") = Some(State { cipher, key, source, warning });
}

/// Turn encryption off (tests).
pub fn disable() {
    *STATE.write().expect("vault lock") = None;
}

pub fn status() -> VaultStatus {
    match STATE.read().expect("vault lock").as_ref() {
        Some(s) => VaultStatus { enabled: true, source: Some(s.source), warning: s.warning.clone() },
        None => VaultStatus { enabled: false, source: None, warning: None },
    }
}

pub fn enabled() -> bool {
    STATE.read().expect("vault lock").is_some()
}

/// The data key in base64, to keep somewhere safe (restores with
/// `OPENBARK_DATA_KEY` or `openbark key import`).
pub fn recovery_key() -> Option<String> {
    STATE.read().expect("vault lock").as_ref().map(|s| encode_key(&s.key))
}

/// Store a recovery key as this computer's data key. Refused when a
/// different key is already set, unless `replace`.
pub fn import_key(text: &str, replace: bool) -> io::Result<KeySource> {
    let key = decode_key(text).ok_or_else(|| err("That isn't a valid recovery key."))?;
    let existing = keychain_get()
        .ok()
        .flatten()
        .or_else(|| fs::read_to_string(key_file()).ok().and_then(|t| decode_key(&t)));
    if let Some(old) = existing
        && old != key
        && !replace
    {
        return Err(err(
            "This computer already has a different key. Encounters encrypted with it would become unreadable; pass --replace only if you're sure.",
        ));
    }
    let source = match keychain_set(&key) {
        Ok(()) => KeySource::Keychain,
        Err(_) => {
            write_key_file(&key)?;
            KeySource::KeyFile
        }
    };
    use_key(key, source, (source == KeySource::KeyFile).then(|| FILE_WARNING.to_string()));
    Ok(source)
}

fn file_aad(path: &Path) -> Vec<u8> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    [FILE_MAGIC.as_slice(), name.as_bytes()].concat()
}

/// Whether a file on disk is encrypted.
pub fn is_encrypted(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok()
        && (&magic == FILE_MAGIC || &magic == AUDIO_MAGIC)
}

/// Read a file, decrypting it if it's encrypted.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    if bytes.starts_with(AUDIO_MAGIC) {
        return decrypt_audio(&bytes, path);
    }
    if !bytes.starts_with(FILE_MAGIC) {
        return Ok(bytes);
    }
    let state = STATE.read().expect("vault lock");
    let s = state.as_ref().ok_or_else(|| locked(path))?;
    if bytes.len() < 4 + NONCE + TAG {
        return Err(err(format!("{} is damaged.", path.display())));
    }
    let nonce = XNonce::try_from(&bytes[4..4 + NONCE]).expect("24-byte nonce");
    s.cipher
        .decrypt(&nonce, Payload { msg: &bytes[4 + NONCE..], aad: &file_aad(path) })
        .map_err(|_| err(format!("{} can't be decrypted with this computer's key.", path.display())))
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| err(format!("{}: {e}", path.display())))
}

fn locked(path: &Path) -> io::Error {
    err(format!("{} is encrypted and encryption isn't set up in this program.", path.display()))
}

/// Write a file atomically, encrypted when the vault is on.
pub fn write(path: &Path, plain: &[u8]) -> io::Result<()> {
    let data = {
        let state = STATE.read().expect("vault lock");
        match state.as_ref() {
            Some(s) => {
                let nonce_bytes = random::<NONCE>()?;
                let nonce = XNonce::from(nonce_bytes);
                let ct = s
                    .cipher
                    .encrypt(&nonce, Payload { msg: plain, aad: &file_aad(path) })
                    .map_err(|_| err("encryption failed"))?;
                [FILE_MAGIC.as_slice(), &nonce_bytes, &ct].concat()
            }
            None => plain.to_vec(),
        }
    };
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

fn audio_aad(index: u64) -> [u8; 12] {
    let mut aad = [0u8; 12];
    aad[..4].copy_from_slice(AUDIO_MAGIC);
    aad[4..].copy_from_slice(&index.to_le_bytes());
    aad
}

fn decrypt_audio(bytes: &[u8], path: &Path) -> io::Result<Vec<u8>> {
    let state = STATE.read().expect("vault lock");
    let s = state.as_ref().ok_or_else(|| locked(path))?;
    let chunk = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes")) as usize;
    let slot = NONCE + chunk + TAG;
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 8;
    let mut index = 0u64;
    while at < bytes.len() {
        let end = (at + slot).min(bytes.len());
        if end - at < NONCE + TAG {
            break; // a torn final write: keep what came before
        }
        let nonce = XNonce::try_from(&bytes[at..at + NONCE]).expect("24-byte nonce");
        match s.cipher.decrypt(&nonce, Payload { msg: &bytes[at + NONCE..end], aad: &audio_aad(index) }) {
            Ok(plain) => out.extend(plain),
            // A recording cut off mid-chunk (power loss): keep what came before.
            Err(_) if end == bytes.len() && index > 0 => break,
            Err(_) => {
                return Err(err(format!("{} can't be decrypted with this computer's key.", path.display())));
            }
        }
        at = end;
        index += 1;
    }
    fix_wav_sizes(&mut out);
    Ok(out)
}

/// A WAV whose header was never patched (the recording didn't finish) gets
/// sizes that match its data, so players and decoders read all of it.
fn fix_wav_sizes(wav: &mut [u8]) {
    if wav.len() >= 44 && &wav[0..4] == b"RIFF" && &wav[36..40] == b"data" {
        let data = (wav.len() - 44) as u32;
        if u32::from_le_bytes(wav[40..44].try_into().expect("4 bytes")) != data {
            wav[4..8].copy_from_slice(&(36 + data).to_le_bytes());
            wav[40..44].copy_from_slice(&data.to_le_bytes());
        }
    }
}

/// A seekable sink for recordings: streams encrypted chunks to disk. Bytes
/// can be overwritten only within the first chunk (the WAV header).
pub struct AudioWriter {
    file: fs::File,
    first: Vec<u8>,
    first_written: bool,
    current: Vec<u8>,
    index: u64,
    len: u64,
    pos: u64,
}

impl AudioWriter {
    pub fn create(path: &Path) -> io::Result<Self> {
        let mut file = fs::File::create(path)?;
        file.write_all(AUDIO_MAGIC)?;
        file.write_all(&(CHUNK as u32).to_le_bytes())?;
        Ok(Self {
            file,
            first: Vec::new(),
            first_written: false,
            current: Vec::new(),
            index: 1,
            len: 0,
            pos: 0,
        })
    }

    fn seal(&self, index: u64, plain: &[u8]) -> io::Result<Vec<u8>> {
        let state = STATE.read().expect("vault lock");
        let s = state.as_ref().ok_or_else(|| err("encryption isn't set up"))?;
        let nonce_bytes = random::<NONCE>()?;
        let ct = s
            .cipher
            .encrypt(&XNonce::from(nonce_bytes), Payload { msg: plain, aad: &audio_aad(index) })
            .map_err(|_| err("encryption failed"))?;
        Ok([nonce_bytes.as_slice(), &ct].concat())
    }

    fn slot_offset(index: u64) -> u64 {
        8 + index * (NONCE + CHUNK + TAG) as u64
    }

    fn write_first(&mut self) -> io::Result<()> {
        let sealed = self.seal(0, &self.first)?;
        self.file.seek(SeekFrom::Start(8))?;
        self.file.write_all(&sealed)?;
        self.first_written = true;
        Ok(())
    }

    fn flush_current(&mut self) -> io::Result<()> {
        if self.current.is_empty() {
            return Ok(());
        }
        let sealed = self.seal(self.index, &self.current)?;
        self.file.seek(SeekFrom::Start(Self::slot_offset(self.index)))?;
        self.file.write_all(&sealed)?;
        self.current.clear();
        self.index += 1;
        Ok(())
    }

    /// Write the last chunk and the final first chunk, and sync to disk.
    pub fn finish(mut self) -> io::Result<()> {
        self.flush_current()?;
        self.write_first()?;
        self.file.sync_all()
    }
}

impl Write for AudioWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buf.len() {
            let rest = &buf[written..];
            if (self.pos as usize) < CHUNK {
                // Inside the first chunk: kept in memory (header rewrites land here).
                let at = self.pos as usize;
                let n = rest.len().min(CHUNK - at);
                if self.first.len() < at + n {
                    self.first.resize(at + n, 0);
                }
                self.first[at..at + n].copy_from_slice(&rest[..n]);
                self.pos += n as u64;
                written += n;
                if self.first.len() == CHUNK && !self.first_written {
                    self.write_first()?; // on disk early, so a crash keeps it
                }
            } else {
                if self.pos != self.len {
                    return Err(err("audio can only be appended after the first chunk"));
                }
                let n = rest.len().min(CHUNK - self.current.len());
                self.current.extend_from_slice(&rest[..n]);
                self.pos += n as u64;
                written += n;
                if self.current.len() == CHUNK {
                    self.flush_current()?;
                }
            }
            self.len = self.len.max(self.pos);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for AudioWriter {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => self.len as i64 + d,
        };
        if target < 0 || target as u64 > self.len || (target as u64) < self.len && target as usize >= CHUNK {
            return Err(err("audio can only be rewritten within the first chunk"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

impl os_asr::AudioSink for AudioWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        AudioWriter::finish(*self)
    }
}

/// Encrypt whole audio bytes into a file (migrating an old plain WAV).
pub fn write_audio(path: &Path, plain: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("wav.tmp");
    let mut w = AudioWriter::create(&tmp)?;
    w.write_all(plain)?;
    w.finish()?;
    fs::rename(&tmp, path)
}

/// Encrypt every plain file in `dir` and its subfolders (an encounter folder).
/// Returns how many files were rewritten.
pub fn encrypt_folder(dir: &Path) -> io::Result<usize> {
    if !enabled() {
        return Err(err("encryption isn't set up"));
    }
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            count += encrypt_folder(&path)?;
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.ends_with(".tmp") || is_encrypted(&path) {
            continue;
        }
        let plain = fs::read(&path)?;
        if name.ends_with(".wav") {
            write_audio(&path, &plain)?;
        } else {
            write(&path, &plain)?;
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The vault is process-wide; tests that turn it on run one at a time.
    pub static SERIAL: Mutex<()> = Mutex::new(());

    pub fn with_key<T>(f: impl FnOnce() -> T) -> T {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        use_key([7u8; 32], KeySource::Environment, None);
        let out = f();
        disable();
        out
    }

    #[test]
    fn small_files_round_trip_and_are_bound_to_their_name() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let a = dir.path().join("note.json");
            write(&a, b"{\"temp\": 39.1}").unwrap();
            let raw = fs::read(&a).unwrap();
            assert!(raw.starts_with(FILE_MAGIC));
            assert!(!raw.windows(4).any(|w| w == b"39.1"), "no plain text on disk");
            assert_eq!(read(&a).unwrap(), b"{\"temp\": 39.1}");
            // Swapping files between names is detected.
            let b = dir.path().join("note.md");
            fs::copy(&a, &b).unwrap();
            assert!(read(&b).is_err());
            // Plain files from before encryption still read.
            let old = dir.path().join("session.json");
            fs::write(&old, b"{}").unwrap();
            assert_eq!(read(&old).unwrap(), b"{}");
        });
    }

    #[test]
    fn another_key_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("transcript.jsonl");
        with_key(|| write(&a, b"secret").unwrap());
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        use_key([9u8; 32], KeySource::Environment, None);
        assert!(read(&a).is_err());
        disable();
        assert!(read(&a).unwrap_err().to_string().contains("isn't set up"));
    }

    #[test]
    fn audio_streams_encrypted_with_a_rewritten_header() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("audio.wav");
            let mut w = AudioWriter::create(&path).unwrap();
            w.write_all(&[0xAA; 44]).unwrap(); // provisional header
            let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
            w.write_all(&body).unwrap();
            w.seek(SeekFrom::Start(0)).unwrap();
            w.write_all(&[0xBB; 44]).unwrap(); // final header
            assert!(
                w.seek(SeekFrom::Start(CHUNK as u64 + 10)).is_err(),
                "only the first chunk is rewritable"
            );
            w.finish().unwrap();
            let raw = fs::read(&path).unwrap();
            assert!(raw.starts_with(AUDIO_MAGIC));
            let plain = read(&path).unwrap();
            assert_eq!(plain.len(), 44 + body.len());
            assert_eq!(&plain[..44], &[0xBB; 44]);
            assert_eq!(&plain[44..], &body[..]);
        });
    }

    #[test]
    fn a_recording_cut_off_mid_way_keeps_what_was_written() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("audio.wav");
            let mut w = AudioWriter::create(&path).unwrap();
            let mut header = vec![0u8; 44];
            header[0..4].copy_from_slice(b"RIFF");
            header[36..40].copy_from_slice(b"data"); // sizes left at 0, as while recording
            w.write_all(&header).unwrap();
            w.write_all(&vec![1u8; CHUNK * 3]).unwrap();
            drop(w); // power loss: no finish
            let plain = read(&path).unwrap();
            assert!(plain.len() >= CHUNK * 3, "{}", plain.len());
            let data = u32::from_le_bytes(plain[40..44].try_into().unwrap()) as usize;
            assert_eq!(data, plain.len() - 44, "header sizes fixed on read");
        });
    }

    #[test]
    fn folders_encrypt_in_place() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir_all(dir.path().join("runs/001")).unwrap();
            fs::write(dir.path().join("session.json"), b"{\"a\":1}").unwrap();
            fs::write(dir.path().join("audio.wav"), b"RIFF0000WAVEdata").unwrap();
            fs::write(dir.path().join("runs/001/note.md"), b"# Note").unwrap();
            assert_eq!(encrypt_folder(dir.path()).unwrap(), 3);
            assert_eq!(encrypt_folder(dir.path()).unwrap(), 0, "already encrypted");
            assert!(is_encrypted(&dir.path().join("audio.wav")));
            assert_eq!(read(&dir.path().join("runs/001/note.md")).unwrap(), b"# Note");
            assert_eq!(read(&dir.path().join("audio.wav")).unwrap(), b"RIFF0000WAVEdata");
        });
    }
}
