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
//!
//! Every format can also be sealed and opened with an explicit [`Key`]
//! (the `*_with` functions and [`AudioWriter::with_key`]): sync (ADR-0017)
//! seals objects under keys of its own, and a visit adopted from another
//! device is written under this device's data key. The plain functions use
//! the data key, as before.

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
/// Append-only logs (the deletion record, ADR-0022): one encrypted line each.
const LINE_MAGIC: &[u8; 4] = b"OBL1";
const LINE_PREFIX: &str = "OBL1:";
const NONCE: usize = 24;
const TAG: usize = 16;
/// Audio plaintext per chunk: 2 s of 16 kHz 16-bit mono.
pub const CHUNK: usize = 64 * 1024;
/// Keychain account of the data key (service "OpenBark", [`crate::keychain`]).
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

/// A 256-bit key for the vault's formats. The data key is one; sync's
/// object keys are others (ADR-0017 section 4). Never printed.
#[derive(Clone)]
pub struct Key {
    bytes: [u8; 32],
    cipher: XChaCha20Poly1305,
}

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { cipher: XChaCha20Poly1305::new(&bytes.into()), bytes }
    }

    /// A new random key (`getrandom`).
    pub fn random() -> io::Result<Self> {
        Ok(Self::from_bytes(random::<32>()?))
    }

    /// The key in base64, as a recovery key is written.
    pub fn to_base64(&self) -> String {
        encode_key(&self.bytes)
    }

    pub fn from_base64(text: &str) -> Option<Self> {
        decode_key(text).map(Self::from_bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(..)")
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for Key {}

struct State {
    key: Key,
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

/// The data key from the OS keychain (on iOS, the protected store: "after
/// first unlock, this device only").
fn keychain_get() -> Result<Option<[u8; 32]>, String> {
    match crate::keychain::get(ACCOUNT)? {
        Some(text) => decode_key(&text).map(Some).ok_or_else(|| "the stored key is damaged".to_string()),
        None => Ok(None),
    }
}

fn keychain_set(key: &[u8; 32]) -> Result<(), String> {
    crate::keychain::set(ACCOUNT, &encode_key(key))?;
    // Read it back: some stores accept a write they can't keep.
    match keychain_get()? {
        Some(k) if &k == key => Ok(()),
        _ => Err("the keychain didn't keep the key".into()),
    }
}

fn keychain_delete() -> Result<bool, String> {
    crate::keychain::delete(ACCOUNT)
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
                        // iOS keeps keys only in the protected store: no
                        // key file next to the visits (ADR-0018 section 5).
                        Err(reason) if cfg!(target_os = "ios") => {
                            return Err(err(format!("The keychain couldn't keep the data key: {reason}.")));
                        }
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
    *STATE.write().expect("vault lock") = Some(State { key: Key::from_bytes(key), source, warning });
}

/// This device's data key, when encryption is on.
pub fn data_key() -> Option<Key> {
    STATE.read().expect("vault lock").as_ref().map(|s| s.key.clone())
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
    STATE.read().expect("vault lock").as_ref().map(|s| s.key.to_base64())
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
/// Delete this computer's data key from the keychain and the key file
/// (uninstalling with "remove my data"). Encrypted encounters can't be read
/// afterwards without the recovery key. Returns where a key was removed from.
pub fn forget() -> io::Result<Forgotten> {
    let keychain = keychain_delete();
    let mut out = forget_file()?;
    match keychain {
        Ok(true) => out.removed.insert(0, "keychain".to_string()),
        Ok(false) => {}
        // The key file is still removed; say the keychain couldn't be reached.
        Err(e) => out.warning = Some(format!("couldn't reach the keychain to remove the key there: {e}")),
    }
    Ok(out)
}

/// What [`forget`] removed.
#[derive(Debug, Default)]
pub struct Forgotten {
    /// "keychain" and/or the key file's path.
    pub removed: Vec<String>,
    pub warning: Option<String>,
}

/// The key-file half of [`forget`] (tests use it without touching a real keychain).
fn forget_file() -> io::Result<Forgotten> {
    let mut out = Forgotten::default();
    let file = key_file();
    if file.is_file() {
        fs::remove_file(&file)?;
        out.removed.push(file.display().to_string());
    }
    disable();
    Ok(out)
}

pub fn is_encrypted(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok()
        && (&magic == FILE_MAGIC || &magic == AUDIO_MAGIC)
}

/// Read a file, decrypting it with the data key if it's encrypted.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    read_inner(path, data_key().as_ref())
}

/// Read a file, decrypting it with `key` if it's encrypted.
pub fn read_with(key: &Key, path: &Path) -> io::Result<Vec<u8>> {
    read_inner(path, Some(key))
}

fn read_inner(path: &Path, key: Option<&Key>) -> io::Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    if bytes.starts_with(AUDIO_MAGIC) {
        return decrypt_audio(&bytes, path, key.ok_or_else(|| locked(path))?);
    }
    if !bytes.starts_with(FILE_MAGIC) {
        return Ok(bytes);
    }
    let key = key.ok_or_else(|| locked(path))?;
    if bytes.len() < 4 + NONCE + TAG {
        return Err(err(format!("{} is damaged.", path.display())));
    }
    open_with(key, &file_aad(path), &bytes)
        .map_err(|_| err(format!("{} can't be decrypted with this computer's key.", path.display())))
}

/// Seal `plain` in the small-file format (`OBK1` ‖ nonce ‖ ciphertext) with
/// `aad` bound in. Files use their name as `aad`; sync binds its header.
pub fn seal_with(key: &Key, aad: &[u8], plain: &[u8]) -> io::Result<Vec<u8>> {
    let nonce_bytes = random::<NONCE>()?;
    let ct = key
        .cipher
        .encrypt(&XNonce::from(nonce_bytes), Payload { msg: plain, aad })
        .map_err(|_| err("encryption failed"))?;
    Ok([FILE_MAGIC.as_slice(), &nonce_bytes, &ct].concat())
}

/// Open what [`seal_with`] sealed. A wrong key, wrong `aad` or changed bytes
/// is an `InvalidData` error.
pub fn open_with(key: &Key, aad: &[u8], sealed: &[u8]) -> io::Result<Vec<u8>> {
    if !sealed.starts_with(FILE_MAGIC) || sealed.len() < 4 + NONCE + TAG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a sealed object, or damaged"));
    }
    let nonce = XNonce::try_from(&sealed[4..4 + NONCE]).expect("24-byte nonce");
    key.cipher
        .decrypt(&nonce, Payload { msg: &sealed[4 + NONCE..], aad })
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "can't be opened with this key"))
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| err(format!("{}: {e}", path.display())))
}

fn locked(path: &Path) -> io::Error {
    err(format!("{} is encrypted and encryption isn't set up in this program.", path.display()))
}

/// Write a file atomically, encrypted under the data key when the vault is on.
pub fn write(path: &Path, plain: &[u8]) -> io::Result<()> {
    write_inner(path, plain, data_key().as_ref())
}

/// Write a file atomically, encrypted under `key`.
pub fn write_with(key: &Key, path: &Path, plain: &[u8]) -> io::Result<()> {
    write_inner(path, plain, Some(key))
}

fn write_inner(path: &Path, plain: &[u8], key: Option<&Key>) -> io::Result<()> {
    let data = match key {
        Some(k) => seal_with(k, &file_aad(path), plain)?,
        None => plain.to_vec(),
    };
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

fn line_aad(path: &Path) -> Vec<u8> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    [LINE_MAGIC.as_slice(), name.as_bytes()].concat()
}

/// Append one line to a log (ADR-0022 section 6). Encrypted, the line is
/// `OBL1:` then base64 of a 24-byte nonce and the ciphertext, with the magic
/// and the file name as associated data; with the vault off it's `plain` as
/// is (one line of JSON). The whole line and its newline go in one write to a
/// file opened for appending, then synced to disk. Returns the line as
/// stored, which the caller hashes into the next entry.
pub fn append_line(path: &Path, plain: &[u8]) -> io::Result<String> {
    if plain.contains(&b'\n') || plain.contains(&b'\r') {
        return Err(err("a log line can't contain a line break"));
    }
    let line = {
        match data_key() {
            Some(s) => {
                let nonce_bytes = random::<NONCE>()?;
                let ct = s
                    .cipher
                    .encrypt(&XNonce::from(nonce_bytes), Payload { msg: plain, aad: &line_aad(path) })
                    .map_err(|_| err("encryption failed"))?;
                let body =
                    base64::engine::general_purpose::STANDARD.encode([nonce_bytes.as_slice(), &ct].concat());
                format!("{LINE_PREFIX}{body}")
            }
            None => {
                String::from_utf8(plain.to_vec()).map_err(|e| err(format!("a log line must be text: {e}")))?
            }
        }
    };
    let mut file = fs::OpenOptions::new().create(true).append(true).read(true).open(path)?;
    // A line torn by a crash has no newline: start this one on its own line.
    let len = file.metadata()?.len();
    let mut out = String::with_capacity(line.len() + 2);
    if len > 0 {
        let mut last = [0u8; 1];
        file.seek(SeekFrom::Start(len - 1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            out.push('\n');
        }
    }
    out.push_str(&line);
    out.push('\n');
    file.write_all(out.as_bytes())?;
    file.sync_data()?;
    Ok(line)
}

/// Every line of a log, as stored and decrypted, in order. Blank lines are
/// skipped. A line that can't be decrypted or decoded is an error.
pub fn read_lines(path: &Path) -> io::Result<Vec<(String, Vec<u8>)>> {
    let text = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let stored = raw.trim_end_matches('\r');
        if stored.is_empty() {
            continue;
        }
        let plain = decrypt_line(path, stored)
            .map_err(|e| io::Error::new(e.kind(), format!("line {}: {e}", i + 1)))?;
        out.push((stored.to_string(), plain));
    }
    Ok(out)
}

/// One stored log line, decrypted (or as is, when it was written plain).
pub fn decrypt_line(path: &Path, stored: &str) -> io::Result<Vec<u8>> {
    let Some(body) = stored.strip_prefix(LINE_PREFIX) else {
        return Ok(stored.as_bytes().to_vec());
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| err(format!("{} has a damaged line.", path.display())))?;
    if bytes.len() < NONCE + TAG {
        return Err(err(format!("{} has a damaged line.", path.display())));
    }
    let s = data_key().ok_or_else(|| locked(path))?;
    let nonce = XNonce::try_from(&bytes[..NONCE]).expect("24-byte nonce");
    s.cipher.decrypt(&nonce, Payload { msg: &bytes[NONCE..], aad: &line_aad(path) }).map_err(|_| {
        err(format!("{} has a line that can't be decrypted with this computer's key.", path.display()))
    })
}

fn audio_aad(index: u64) -> [u8; 12] {
    let mut aad = [0u8; 12];
    aad[..4].copy_from_slice(AUDIO_MAGIC);
    aad[4..].copy_from_slice(&index.to_le_bytes());
    aad
}

fn decrypt_audio(bytes: &[u8], path: &Path, s: &Key) -> io::Result<Vec<u8>> {
    if bytes.len() < 8 {
        return Err(err(format!("{} is damaged.", path.display())));
    }
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
    /// The key it seals under: given ([`AudioWriter::with_key`]) or the
    /// data key at the moment it was created.
    key: Option<Key>,
    first: Vec<u8>,
    first_written: bool,
    current: Vec<u8>,
    index: u64,
    len: u64,
    pos: u64,
}

impl AudioWriter {
    /// A recording sealed under the data key.
    pub fn create(path: &Path) -> io::Result<Self> {
        Self::open(path, data_key())
    }

    /// A recording sealed under `key`, whatever the data key is (ADR-0017
    /// section 13).
    pub fn with_key(path: &Path, key: Key) -> io::Result<Self> {
        Self::open(path, Some(key))
    }

    fn open(path: &Path, key: Option<Key>) -> io::Result<Self> {
        let mut file = fs::File::create(path)?;
        file.write_all(AUDIO_MAGIC)?;
        file.write_all(&(CHUNK as u32).to_le_bytes())?;
        Ok(Self {
            file,
            key,
            first: Vec::new(),
            first_written: false,
            current: Vec::new(),
            index: 1,
            len: 0,
            pos: 0,
        })
    }

    fn seal(&self, index: u64, plain: &[u8]) -> io::Result<Vec<u8>> {
        let s = self.key.as_ref().ok_or_else(|| err("encryption isn't set up"))?;
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

/// Reads byte ranges of a recording without decrypting all of it, for the
/// player (which asks for ranges as it seeks). Gives the same bytes as
/// [`read`]: a torn last chunk is left out, and an unpatched WAV header gets
/// sizes that match the data. Plain files (from before encryption) are read
/// as they are.
pub struct AudioReader {
    file: fs::File,
    path: PathBuf,
    len: u64,
    /// Set for an encrypted (`OBA1`) recording.
    sealed: Option<Sealed>,
}

struct Sealed {
    chunk: u64,
    /// Chunk 0, decrypted, with the WAV sizes fixed: read on first use.
    first: Option<Vec<u8>>,
}

impl AudioReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = fs::File::open(path)?;
        let file_len = file.metadata()?.len();
        let mut magic = [0u8; 8];
        let sealed = file_len >= 8 && {
            file.read_exact(&mut magic)?;
            &magic[..4] == AUDIO_MAGIC
        };
        let mut reader = Self { file, path: path.to_path_buf(), len: file_len, sealed: None };
        if !sealed {
            return Ok(reader);
        }
        if STATE.read().expect("vault lock").is_none() {
            return Err(locked(path));
        }
        let chunk = u32::from_le_bytes(magic[4..8].try_into().expect("4 bytes")) as u64;
        if chunk == 0 {
            return Err(err(format!("{} is damaged.", path.display())));
        }
        reader.sealed = Some(Sealed { chunk, first: None });
        let slot = NONCE as u64 + chunk + TAG as u64;
        let body = file_len - 8;
        let (full, rest) = (body / slot, body % slot);
        // The last chunk, as `decrypt_audio` would take it.
        let (last, last_slot) = match rest {
            0 if full == 0 => (None, 0),
            0 => (Some(full - 1), slot),
            r => (Some(full), r),
        };
        reader.len = match last {
            None => 0,
            Some(i) if last_slot < (NONCE + TAG) as u64 => i * chunk, // a torn final write
            Some(i) => match reader.decrypt_chunk(i) {
                Ok(plain) => i * chunk + plain.len() as u64,
                // A recording cut off mid-chunk (power loss): keep what came before.
                Err(_) if i > 0 => i * chunk,
                Err(e) => return Err(e),
            },
        };
        Ok(reader)
    }

    /// The recording's length in bytes, as [`read`] would return it.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes `start..=end` (clamped to the length).
    pub fn read_range(&mut self, start: u64, end: u64) -> io::Result<Vec<u8>> {
        if self.len == 0 || start >= self.len || end < start {
            return Ok(Vec::new());
        }
        let end = end.min(self.len - 1);
        let Some(chunk) = self.sealed.as_ref().map(|s| s.chunk) else {
            let mut out = vec![0u8; (end - start + 1) as usize];
            self.file.seek(SeekFrom::Start(start))?;
            self.file.read_exact(&mut out)?;
            return Ok(out);
        };
        let mut out = Vec::with_capacity((end - start + 1) as usize);
        for index in start / chunk..=end / chunk {
            let plain = if index == 0 { self.first()?.clone() } else { self.decrypt_chunk(index)? };
            let base = index * chunk;
            let from = start.saturating_sub(base) as usize;
            let to = ((end - base + 1) as usize).min(plain.len());
            if from >= to {
                return Err(err(format!("{} is damaged.", self.path.display())));
            }
            out.extend_from_slice(&plain[from..to]);
        }
        Ok(out)
    }

    fn first(&mut self) -> io::Result<&Vec<u8>> {
        if self.sealed.as_ref().is_some_and(|s| s.first.is_none()) {
            let mut plain = self.decrypt_chunk(0)?;
            // Sizes from the whole recording's length, as fix_wav_sizes does.
            if plain.len() >= 44 && &plain[0..4] == b"RIFF" && &plain[36..40] == b"data" {
                let data = (self.len.saturating_sub(44)).min(u32::MAX as u64 - 36) as u32;
                if u32::from_le_bytes(plain[40..44].try_into().expect("4 bytes")) != data {
                    plain[4..8].copy_from_slice(&(36 + data).to_le_bytes());
                    plain[40..44].copy_from_slice(&data.to_le_bytes());
                }
            }
            if let Some(s) = self.sealed.as_mut() {
                s.first = Some(plain);
            }
        }
        Ok(self.sealed.as_ref().and_then(|s| s.first.as_ref()).expect("chunk 0 decrypted"))
    }

    fn decrypt_chunk(&mut self, index: u64) -> io::Result<Vec<u8>> {
        let chunk = self.sealed.as_ref().map(|s| s.chunk).ok_or_else(|| err("not encrypted"))?;
        let slot = NONCE as u64 + chunk + TAG as u64;
        let at = 8 + index * slot;
        let file_len = self.file.metadata()?.len();
        let size = slot.min(file_len.saturating_sub(at));
        if size < (NONCE + TAG) as u64 {
            return Err(err(format!("{} is damaged.", self.path.display())));
        }
        let mut sealed = vec![0u8; size as usize];
        self.file.seek(SeekFrom::Start(at))?;
        self.file.read_exact(&mut sealed)?;
        let state = STATE.read().expect("vault lock");
        let s = state.as_ref().ok_or_else(|| locked(&self.path))?;
        let nonce = XNonce::try_from(&sealed[..NONCE]).expect("24-byte nonce");
        s.cipher
            .decrypt(&nonce, Payload { msg: &sealed[NONCE..], aad: &audio_aad(index) })
            .map_err(|_| err(format!("{} can't be decrypted with this computer's key.", self.path.display())))
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

    /// Every range the reader returns matches the whole decrypted file.
    fn assert_ranges_match(path: &Path) {
        let whole = read(path).unwrap();
        let mut r = AudioReader::open(path).unwrap();
        assert_eq!(r.len(), whole.len() as u64);
        let n = whole.len() as u64;
        let c = CHUNK as u64;
        let mut ranges = vec![(0, 0), (0, 1), (0, 43), (0, n - 1), (n - 1, n - 1), (40, 47), (n / 2, n - 1)];
        for k in 1..=(n / c) {
            ranges.extend([(k * c - 1, k * c), (k * c - 3, k * c + 5), (k * c, (k * c + c - 1).min(n - 1))]);
        }
        ranges.push((c - 10, 2 * c + 10));
        for (a, b) in ranges {
            if a > b || b >= n {
                continue;
            }
            assert_eq!(r.read_range(a, b).unwrap(), &whole[a as usize..=b as usize], "range {a}-{b}");
        }
        // Past the end: clamped, or empty.
        assert_eq!(r.read_range(n - 2, n + 100).unwrap(), &whole[n as usize - 2..]);
        assert!(r.read_range(n, n + 5).unwrap().is_empty());
    }

    #[test]
    fn audio_ranges_decrypt_only_what_is_asked_and_match_the_whole_file() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            for len in [100usize, CHUNK - 44, CHUNK, CHUNK * 3 + 1234, CHUNK * 2] {
                let path = dir.path().join(format!("audio-{len}.wav"));
                let mut w = AudioWriter::create(&path).unwrap();
                let mut header = vec![0u8; 44];
                header[0..4].copy_from_slice(b"RIFF");
                header[36..40].copy_from_slice(b"data");
                header[40..44].copy_from_slice(&(len as u32).to_le_bytes());
                w.write_all(&header).unwrap();
                w.write_all(&(0..len as u32).map(|i| (i % 253) as u8).collect::<Vec<_>>()).unwrap();
                w.finish().unwrap();
                assert_ranges_match(&path);
            }
        });
    }

    #[test]
    fn audio_ranges_of_a_recording_cut_off_mid_way() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("audio.wav");
            let mut w = AudioWriter::create(&path).unwrap();
            let mut header = vec![0u8; 44];
            header[0..4].copy_from_slice(b"RIFF");
            header[36..40].copy_from_slice(b"data"); // sizes still 0
            w.write_all(&header).unwrap();
            w.write_all(&vec![3u8; CHUNK * 3 + 500]).unwrap();
            drop(w); // power loss: the last part never reached the disk
            assert_ranges_match(&path);
            let mut r = AudioReader::open(&path).unwrap();
            let head = r.read_range(0, 43).unwrap();
            assert_eq!(u32::from_le_bytes(head[40..44].try_into().unwrap()) as u64, r.len() - 44);
            // A write torn inside the last chunk.
            let raw = fs::read(&path).unwrap();
            fs::write(&path, &raw[..raw.len() - 1000]).unwrap();
            assert_ranges_match(&path);
            // ... and inside the last chunk's nonce.
            let slot = NONCE + CHUNK + TAG;
            fs::write(&path, &raw[..8 + 2 * slot + 10]).unwrap();
            assert_ranges_match(&path);
        });
    }

    #[test]
    fn audio_ranges_of_plain_and_locked_files() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("old.wav");
        fs::write(&plain, b"RIFF0000WAVEdata-plain-bytes").unwrap();
        let sealed = dir.path().join("audio.wav");
        with_key(|| {
            let mut r = AudioReader::open(&plain).unwrap();
            assert_eq!(r.len(), 28);
            assert_eq!(r.read_range(4, 7).unwrap(), b"0000");
            write_audio(&sealed, &[9u8; 5000]).unwrap();
        });
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        disable();
        assert!(AudioReader::open(&sealed).err().unwrap().to_string().contains("isn't set up"));
        assert_eq!(AudioReader::open(&plain).unwrap().read_range(0, 3).unwrap(), b"RIFF");
        let tiny = dir.path().join("tiny.wav");
        fs::write(&tiny, b"RIF").unwrap();
        assert_eq!(AudioReader::open(&tiny).unwrap().len(), 3);
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

    #[test]
    fn forgetting_removes_the_key_file() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("openbark-forget-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("data.key");
        write_key_to(&file, &[7u8; 32]);
        // SAFETY: tests touching the environment hold SERIAL.
        unsafe { std::env::set_var("OPENBARK_KEY_FILE", &file) };
        let out = forget_file().unwrap();
        unsafe { std::env::remove_var("OPENBARK_KEY_FILE") };
        assert!(!file.exists());
        assert_eq!(out.removed, [file.display().to_string()]);
        assert!(!enabled());
        let _ = fs::remove_dir_all(&dir);
    }

    fn write_key_to(path: &Path, key: &[u8; 32]) {
        fs::write(path, encode_key(key)).unwrap();
    }

    #[test]
    fn log_lines_append_encrypted_and_are_bound_to_their_file() {
        with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("audit.log");
            let a = append_line(&log, br#"{"seq":1,"reason":"secret words"}"#).unwrap();
            let b = append_line(&log, br#"{"seq":2}"#).unwrap();
            assert!(a.starts_with("OBL1:") && b.starts_with("OBL1:"));
            let raw = fs::read_to_string(&log).unwrap();
            assert_eq!(raw, format!("{a}\n{b}\n"));
            assert!(!raw.contains("secret"));
            let lines = read_lines(&log).unwrap();
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0].0, a);
            assert_eq!(lines[1].1, br#"{"seq":2}"#);
            // A line copied into a file with another name doesn't decrypt.
            let other = dir.path().join("other.log");
            fs::write(&other, format!("{a}\n")).unwrap();
            assert!(read_lines(&other).is_err());
            // A torn last line (no newline) doesn't swallow the next one.
            fs::write(&log, format!("{a}\n{}", &b[..10])).unwrap();
            let c = append_line(&log, b"{}").unwrap();
            assert!(fs::read_to_string(&log).unwrap().ends_with(&format!("\n{c}\n")));
            assert!(append_line(&log, b"two\nlines").is_err());
        });
    }

    #[test]
    fn log_lines_are_plain_json_with_the_vault_off() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        disable();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("audit.log");
        let a = append_line(&log, br#"{"seq":1}"#).unwrap();
        assert_eq!(a, r#"{"seq":1}"#);
        assert_eq!(read_lines(&log).unwrap()[0].1, br#"{"seq":1}"#);
    }

    // ---- explicit keys (ADR-0017 section 13) ------------------------------------

    #[test]
    fn explicit_keys_seal_and_open_without_the_data_key() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        disable();
        let dir = tempfile::tempdir().unwrap();
        let a = Key::from_bytes([1u8; 32]);
        let b = Key::from_bytes([2u8; 32]);
        let file = dir.path().join("transcript.json");
        write_with(&a, &file, b"{\"segments\":[]}").unwrap();
        assert!(is_encrypted(&file));
        assert_eq!(read_with(&a, &file).unwrap(), b"{\"segments\":[]}");
        assert!(read_with(&b, &file).is_err(), "another key can't open it");
        assert!(read(&file).is_err(), "with the vault off it stays locked");
        // Bound to its name, as before.
        let moved = dir.path().join("session.json");
        fs::copy(&file, &moved).unwrap();
        assert!(read_with(&a, &moved).is_err());

        // seal/open with associated data.
        let sealed = seal_with(&a, b"header", b"body").unwrap();
        assert_eq!(open_with(&a, b"header", &sealed).unwrap(), b"body");
        assert_eq!(open_with(&a, b"other", &sealed).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(open_with(&b, b"header", &sealed).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(open_with(&a, b"header", b"OBK1short").is_err());

        // Keys round-trip as recovery text and never print.
        assert_eq!(Key::from_base64(&a.to_base64()), Some(a.clone()));
        assert_eq!(format!("{a:?}"), "Key(..)");
        assert_ne!(Key::random().unwrap(), Key::random().unwrap());
    }

    #[test]
    fn audio_writer_with_key_ignores_the_data_key() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        use_key([7u8; 32], KeySource::Environment, None);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.wav");
        let other = Key::from_bytes([9u8; 32]);
        let audio: Vec<u8> = (0..(CHUNK * 2 + 100)).map(|i| (i % 251) as u8).collect();
        let mut w = AudioWriter::with_key(&path, other.clone()).unwrap();
        w.write_all(&audio).unwrap();
        w.finish().unwrap();
        assert!(read(&path).is_err(), "the data key can't open it");
        assert_eq!(read_with(&other, &path).unwrap(), audio);
        // create() still uses the data key, and data_key() is that key.
        let plain_path = dir.path().join("audio2.wav");
        let mut w = AudioWriter::create(&plain_path).unwrap();
        w.write_all(&audio).unwrap();
        w.finish().unwrap();
        assert_eq!(read(&plain_path).unwrap(), audio);
        assert_eq!(read_with(&data_key().unwrap(), &plain_path).unwrap(), audio);
        disable();
        assert!(data_key().is_none());
    }
}
