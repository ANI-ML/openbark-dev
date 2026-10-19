//! The Hugging Face token (ADR-0025 §8): optional, saved on request, sent only
//! for gated repos.
//!
//! Where it comes from, in order:
//! 1. `OPENBARK_HF_TOKEN`, then `HF_TOKEN`, set by whoever runs the server
//!    (shown as "Set by the server"; saving and removing don't touch it);
//! 2. the operating system's keychain (service `OpenBark`, account
//!    `huggingface-token`, the ADR-0008 pattern);
//! 3. without a keychain, `<local data>/OpenBark/hf-token.obk`, encrypted
//!    with the vault, only when encryption at rest is on;
//! 4. otherwise memory, until OpenBark closes.
//!
//! Never in plain text, never returned to the interface, never logged: no
//! type here that holds a token derives `Debug`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use crate::vault;

/// Keychain account for the token; the service is "OpenBark".
pub const HF_TOKEN_ACCOUNT: &str = "huggingface-token";
const SERVICE: &str = "OpenBark";
const FILE_NAME: &str = "hf-token.obk";
const ENV_VARS: [&str; 2] = ["OPENBARK_HF_TOKEN", "HF_TOKEN"];

/// Where the token in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenSource {
    /// `OPENBARK_HF_TOKEN` or `HF_TOKEN`.
    Server,
    Keychain,
    /// The vault-encrypted file in the data folder.
    VaultFile,
    /// Memory only, until OpenBark closes.
    Session,
}

/// A place secrets are kept. The real one is the OS keychain; tests use
/// [`MemoryBackend`] and never touch the keychain.
pub trait SecretBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// In memory, for tests.
#[derive(Default)]
pub struct MemoryBackend {
    values: Mutex<HashMap<String, String>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretBackend for MemoryBackend {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.values.lock().map_err(|_| "poisoned")?.get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.values.lock().map_err(|_| "poisoned")?.insert(account.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        self.values.lock().map_err(|_| "poisoned")?.remove(account);
        Ok(())
    }
}

/// The operating system's keychain (Credential Manager, Keychain, Secret Service).
struct Keychain;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl SecretBackend for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        entry.set_password(value).map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl SecretBackend for Keychain {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Err("no keychain on this platform".into())
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no keychain on this platform".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The token kept for this run only.
static SESSION: Mutex<Option<String>> = Mutex::new(None);

/// Everything the token logic touches, so tests can swap each part.
struct Stores<'a> {
    env: &'a dyn Fn(&str) -> Option<String>,
    backend: &'a dyn SecretBackend,
    file: PathBuf,
    session: &'a Mutex<Option<String>>,
}

fn token_file() -> PathBuf {
    os_models::locations::app_data_dir().join(FILE_NAME)
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn real_stores() -> Stores<'static> {
    Stores { env: &real_env, backend: &Keychain, file: token_file(), session: &SESSION }
}

/// The token in use and where it came from: env `OPENBARK_HF_TOKEN`, then
/// `HF_TOKEN` (Server); keychain; vault file; session.
pub fn hf_token() -> Option<(String, TokenSource)> {
    get(&real_stores())
}

/// Save a token: the keychain, else the vault file
/// `<local data>/OpenBark/hf-token.obk` when encryption at rest is on, else
/// session memory. Refuses anything that doesn't look like a token.
pub fn set_hf_token(token: &str) -> io::Result<TokenSource> {
    set(&real_stores(), token)
}

/// Remove a saved token from all three stores (a server's env var stays).
pub fn clear_hf_token() -> io::Result<()> {
    clear(&real_stores())
}

/// 8..=256 printable ASCII characters, no whitespace.
pub fn plausible_token(token: &str) -> bool {
    (8..=256).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

fn get(s: &Stores) -> Option<(String, TokenSource)> {
    for var in ENV_VARS {
        if let Some(t) = (s.env)(var).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            return Some((t, TokenSource::Server));
        }
    }
    match s.backend.get(HF_TOKEN_ACCOUNT) {
        Ok(Some(t)) if !t.is_empty() => return Some((t, TokenSource::Keychain)),
        Ok(_) => {}
        Err(e) => tracing::debug!(error = %e, "keychain not available for the Hugging Face token"),
    }
    if vault::enabled() && s.file.is_file() {
        match read_file(&s.file) {
            Ok(Some(t)) => return Some((t, TokenSource::VaultFile)),
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "the saved Hugging Face token can't be read"),
        }
    }
    s.session.lock().ok().and_then(|g| g.clone()).filter(|t| !t.is_empty()).map(|t| (t, TokenSource::Session))
}

/// The vault file's token, only if it really is encrypted (never plain text).
fn read_file(path: &Path) -> io::Result<Option<String>> {
    if !vault::is_encrypted(path) {
        return Ok(None);
    }
    let text = vault::read_to_string(path)?;
    Ok(Some(text.trim().to_string()).filter(|t| !t.is_empty()))
}

fn set(s: &Stores, token: &str) -> io::Result<TokenSource> {
    let token = token.trim();
    if !plausible_token(token) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "That doesn't look like a Hugging Face token.",
        ));
    }
    // Keychain, read back: some stores accept a write they can't keep.
    let kept =
        s.backend.set(HF_TOKEN_ACCOUNT, token).and_then(|()| match s.backend.get(HF_TOKEN_ACCOUNT)? {
            Some(t) if t == token => Ok(()),
            _ => Err("the keychain didn't keep the token".to_string()),
        });
    let source = match kept {
        Ok(()) => TokenSource::Keychain,
        Err(e) => {
            tracing::info!(error = %e, "keychain unavailable; Hugging Face token kept elsewhere");
            if vault::enabled() {
                if let Some(dir) = s.file.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                vault::write(&s.file, token.as_bytes())?;
                if !vault::is_encrypted(&s.file) {
                    // Encryption went off in between: never leave it in plain text.
                    let _ = std::fs::remove_file(&s.file);
                    return Err(io::Error::other("encryption isn't on, so the token wasn't saved"));
                }
                TokenSource::VaultFile
            } else {
                TokenSource::Session
            }
        }
    };
    // One token at a time: drop copies in the other stores.
    if source != TokenSource::VaultFile {
        remove_file(&s.file)?;
    }
    let mut session = s.session.lock().map_err(|_| io::Error::other("token lock poisoned"))?;
    *session = (source == TokenSource::Session).then(|| token.to_string());
    Ok(source)
}

fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn clear(s: &Stores) -> io::Result<()> {
    if let Ok(mut session) = s.session.lock() {
        *session = None;
    }
    remove_file(&s.file)?;
    // Only an entry that is there must go; a missing keychain isn't an error.
    if let Ok(Some(_)) = s.backend.get(HF_TOKEN_ACCOUNT) {
        s.backend
            .delete(HF_TOKEN_ACCOUNT)
            .map_err(|e| io::Error::other(format!("couldn't remove the token from the keychain: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keychain that's never there (headless Linux, for example).
    struct NoKeychain;

    impl SecretBackend for NoKeychain {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Err("no keychain".into())
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Err("no keychain".into())
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            Err("no keychain".into())
        }
    }

    /// A keychain that accepts a write and keeps nothing.
    struct Forgetful;

    impl SecretBackend for Forgetful {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    const TOKEN: &str = "hf_abcdefghijklmnopqrstuvwxyz0123";

    #[test]
    fn plausible_tokens() {
        assert!(plausible_token(TOKEN));
        assert!(plausible_token("12345678"));
        assert!(!plausible_token("short"));
        assert!(!plausible_token("hf_has space inside"));
        assert!(!plausible_token("hf_tab\there"));
        assert!(!plausible_token("hf_naïve_token"));
        assert!(!plausible_token(&"a".repeat(257)));
        assert!(plausible_token(&"a".repeat(256)));
    }

    #[test]
    fn the_keychain_is_used_first() {
        let dir = tempfile::tempdir().unwrap();
        let backend = MemoryBackend::new();
        let session = Mutex::new(None);
        let s =
            Stores { env: &no_env, backend: &backend, file: dir.path().join(FILE_NAME), session: &session };
        assert!(get(&s).is_none());
        assert_eq!(set(&s, &format!("  {TOKEN}\n")).unwrap(), TokenSource::Keychain);
        assert_eq!(backend.get(HF_TOKEN_ACCOUNT).unwrap().as_deref(), Some(TOKEN), "trimmed");
        assert_eq!(get(&s), Some((TOKEN.to_string(), TokenSource::Keychain)));
        assert!(session.lock().unwrap().is_none());
        assert!(!s.file.exists());
        clear(&s).unwrap();
        assert!(get(&s).is_none());
        assert_eq!(backend.get(HF_TOKEN_ACCOUNT).unwrap(), None);
    }

    #[test]
    fn implausible_tokens_are_refused() {
        let backend = MemoryBackend::new();
        let session = Mutex::new(None);
        let s = Stores { env: &no_env, backend: &backend, file: PathBuf::from("unused"), session: &session };
        let err = set(&s, "not a token").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!err.to_string().contains("not a token"), "the input isn't echoed");
        assert_eq!(backend.get(HF_TOKEN_ACCOUNT).unwrap(), None);
    }

    #[test]
    fn the_server_env_wins() {
        let backend = MemoryBackend::new();
        backend.set(HF_TOKEN_ACCOUNT, TOKEN).unwrap();
        let session = Mutex::new(None);
        let env = |name: &str| match name {
            "HF_TOKEN" => Some("hf_from_hf_token_var".to_string()),
            _ => None,
        };
        let s = Stores { env: &env, backend: &backend, file: PathBuf::from("unused"), session: &session };
        assert_eq!(get(&s), Some(("hf_from_hf_token_var".to_string(), TokenSource::Server)));
        let both = |name: &str| match name {
            "OPENBARK_HF_TOKEN" => Some("hf_openbark_var_wins".to_string()),
            "HF_TOKEN" => Some("hf_from_hf_token_var".to_string()),
            _ => None,
        };
        let s = Stores { env: &both, ..s };
        assert_eq!(get(&s).unwrap(), ("hf_openbark_var_wins".to_string(), TokenSource::Server));
        let blank = |name: &str| (name == "OPENBARK_HF_TOKEN").then(|| "  ".to_string());
        let s = Stores { env: &blank, ..s };
        assert_eq!(get(&s).unwrap().1, TokenSource::Keychain, "a blank variable doesn't count");
    }

    #[test]
    fn without_a_keychain_or_encryption_it_lasts_the_session() {
        let _g = vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        vault::disable();
        let dir = tempfile::tempdir().unwrap();
        let session = Mutex::new(None);
        for backend in [&NoKeychain as &dyn SecretBackend, &Forgetful] {
            let s = Stores { env: &no_env, backend, file: dir.path().join(FILE_NAME), session: &session };
            assert_eq!(set(&s, TOKEN).unwrap(), TokenSource::Session);
            assert_eq!(get(&s), Some((TOKEN.to_string(), TokenSource::Session)));
            assert!(!s.file.exists(), "never written in plain text");
            clear(&s).unwrap();
            assert!(get(&s).is_none());
        }
    }

    #[test]
    fn without_a_keychain_it_goes_in_the_vault_file_when_encryption_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("OpenBark").join(FILE_NAME);
        vault::tests::with_key(|| {
            let session = Mutex::new(None);
            let s = Stores { env: &no_env, backend: &NoKeychain, file: file.clone(), session: &session };
            assert_eq!(set(&s, TOKEN).unwrap(), TokenSource::VaultFile);
            let raw = std::fs::read(&file).unwrap();
            assert!(vault::is_encrypted(&file));
            assert!(!raw.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes()), "no plain text on disk");
            assert_eq!(get(&s), Some((TOKEN.to_string(), TokenSource::VaultFile)));
            assert!(session.lock().unwrap().is_none());

            // A keychain that comes back takes over, and the file goes.
            let backend = MemoryBackend::new();
            let s2 = Stores { env: &no_env, backend: &backend, file: file.clone(), session: &session };
            assert_eq!(set(&s2, TOKEN).unwrap(), TokenSource::Keychain);
            assert!(!file.exists());
            clear(&s2).unwrap();
            assert!(get(&s2).is_none());
        });
    }

    #[test]
    fn a_plain_text_file_is_never_read_as_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        std::fs::write(&file, TOKEN).unwrap();
        vault::tests::with_key(|| {
            let session = Mutex::new(None);
            let s = Stores { env: &no_env, backend: &NoKeychain, file: file.clone(), session: &session };
            assert!(get(&s).is_none());
            clear(&s).unwrap();
            assert!(!file.exists(), "clear removes it");
        });
    }

    #[test]
    fn token_sources_serialise_for_the_interface() {
        assert_eq!(serde_json::to_value(TokenSource::VaultFile).unwrap(), "vault_file");
        assert_eq!(serde_json::to_value(TokenSource::Server).unwrap(), "server");
    }
}
