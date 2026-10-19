//! Secrets (ADR-0023 section 10): the Twilio API key secret.
//!
//! A [`Secret`] never prints itself: `Debug` and `Display` show `****` and
//! its last four characters, and it has no `Serialize`, so it can't slip
//! into a log line, an error, an IPC reply or a settings file. Code that
//! needs the value (signing a token, basic auth to `api.twilio.com`) asks for
//! it by name with [`Secret::expose`].
//!
//! Secrets are kept in the OS keychain under the service "OpenBark", the way
//! the data key is (ADR-0008). [`crate::store::ConnectStore`] falls back to
//! `connections.bin`, sealed under the data key, when the keychain can't be
//! used (an SSH session, Linux without Secret Service).

use std::fmt;

use serde::Serialize;

/// The keychain service, shared with the data key (`os_engine::vault`).
pub const SERVICE: &str = "OpenBark";

/// A secret value. See the module notes.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a pasted value, trimming the spaces and line breaks a copy can bring.
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.len() == value.len() { Self(value) } else { Self(trimmed.to_string()) }
    }

    /// The value itself. Only for signing and for the `Authorization` header.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What Settings shows after saving: `****` and the last four characters
    /// (just `****` for a value too short to show any of it safely).
    pub fn hint(&self) -> String {
        let chars: Vec<char> = self.0.chars().collect();
        if chars.len() < 12 {
            return "****".to_string();
        }
        let last: String = chars[chars.len() - 4..].iter().collect();
        format!("****{last}")
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({})", self.hint())
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hint())
    }
}

/// Which secret. Each has its own keychain entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretId {
    /// "OpenBark" / "twilio-api-key-secret".
    TwilioApiKeySecret,
}

impl SecretId {
    /// The keychain account name (and the key in `connections.bin`).
    pub fn account(self) -> &'static str {
        match self {
            SecretId::TwilioApiKeySecret => "twilio-api-key-secret",
        }
    }
}

/// Where a secret is kept. Settings says which (ADR-0023 section 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretLocation {
    /// The OS keychain (Credential Manager, Keychain, Secret Service).
    Keychain,
    /// `connections.bin`, sealed under the local data key.
    SealedFile,
}

/// What Settings may show about a saved secret: where it is and its last
/// four characters, never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretStatus {
    pub location: SecretLocation,
    pub hint: String,
    /// Set when the keychain couldn't be used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Shown when a secret had to go in the sealed file.
pub const SEALED_FILE_NOTE: &str = "This computer's keychain isn't available, so the key secret is kept in a file sealed with OpenBark's encryption key.";

/// A keychain. [`OsKeychain`] is the real one; tests use an in-memory fake,
/// so they never touch this computer's Credential Manager.
pub trait Keychain: Send + Sync {
    /// `Ok(None)` when there's no entry; `Err` when the keychain can't be used.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Stores and reads back the value; `Err` if the keychain didn't keep it.
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    /// `Ok(true)` when an entry was removed, `Ok(false)` when there was none.
    fn delete(&self, account: &str) -> Result<bool, String>;
}

/// The OS keychain, through `keyring` as in `os_engine::vault`.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeychain;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl Keychain for OsKeychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(text) => Ok(Some(text)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        entry.set_password(value).map_err(|e| e.to_string())?;
        // Read it back: some stores accept a write they can't keep (ADR-0008).
        match self.get(account)? {
            Some(stored) if stored == value => Ok(()),
            _ => Err("the keychain didn't keep the secret".into()),
        }
    }

    fn delete(&self, account: &str) -> Result<bool, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// No keychain at all: every call fails, so secrets go to the sealed file.
/// Used when `OPENBARK_CONNECTIONS_DIR` moves the store (demos, tests and
/// scratch runs), so they never touch this computer's real keychain entry.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoKeychain;

/// Why [`NoKeychain`] refuses.
pub const NO_KEYCHAIN: &str = "the keychain isn't used with OPENBARK_CONNECTIONS_DIR";

impl Keychain for NoKeychain {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Err(NO_KEYCHAIN.into())
    }

    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err(NO_KEYCHAIN.into())
    }

    fn delete(&self, _: &str) -> Result<bool, String> {
        Err(NO_KEYCHAIN.into())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl Keychain for OsKeychain {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Err("no keychain on this platform".into())
    }

    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no keychain on this platform".into())
    }

    fn delete(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_prints_itself() {
        let s = Secret::new("  abcdefghijklmnopqrstuvwxyz0123456789\n");
        assert_eq!(s.expose(), "abcdefghijklmnopqrstuvwxyz0123456789", "pasted spaces trimmed");
        assert_eq!(s.hint(), "****6789");
        assert_eq!(format!("{s}"), "****6789");
        assert_eq!(format!("{s:?}"), "Secret(****6789)");
        assert_eq!(format!("{:#?}", Some(&s)), "Some(\n    Secret(****6789),\n)");
        // Too short to show any of it.
        assert_eq!(Secret::new("abc123").hint(), "****");
    }

    #[test]
    fn keychain_entries_are_named_as_the_adr_says() {
        assert_eq!(SERVICE, "OpenBark");
        assert_eq!(SecretId::TwilioApiKeySecret.account(), "twilio-api-key-secret");
    }
}
