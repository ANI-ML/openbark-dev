//! Where the Twilio settings and secret are kept (ADR-0023 section 10).
//!
//! - `connections.json`: the non-secret settings. Written through
//!   `os_engine::vault`, so it's encrypted whenever the vault is on, like
//!   `patients.json` and `users.json`.
//! - The API key secret: the OS keychain, entry "OpenBark" /
//!   "twilio-api-key-secret". Without a keychain, `connections.bin`, sealed
//!   under the local data key (OBK1, the file name bound in, ADR-0008). That
//!   file is only ever written sealed: with the vault off, saving the secret
//!   there is refused rather than written in plain text.
//!
//! Both files sit in `<local data>/OpenBark` beside `data.key` and
//! `users.json`; `OPENBARK_CONNECTIONS_DIR` moves them (tests, demos). A
//! moved store never uses the keychain: its secret is sealed in its own
//! `connections.bin`, so a demo can't read or overwrite the clinic's real
//! keychain entry.
//!
//! `connections.json` also holds this computer's phone preferences
//! ([`PhonePrefs`]): whether the phone is on, whether calls are recorded, and
//! the announcement read at the start of a recorded call.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use os_engine::vault;
use serde::{Deserialize, Serialize};

use crate::ConnectError;
use crate::secrets::{
    Keychain, NoKeychain, OsKeychain, SEALED_FILE_NOTE, Secret, SecretId, SecretLocation, SecretStatus,
};
use crate::settings::{TwilioSettings, check_api_key_secret};
use crate::token::{self, AccessToken, TOKEN_TTL_SECS, TokenRequest, VoiceGrant};
use crate::twilio::TwilioClient;

/// The settings file.
pub const SETTINGS_FILE: &str = "connections.json";
/// The sealed secrets file, used only without a keychain.
pub const SEALED_FILE: &str = "connections.bin";

/// `<local data>/OpenBark`, or `OPENBARK_CONNECTIONS_DIR`.
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DIR_ENV) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark")
}

/// Moves the store, and turns the keychain off for it.
pub const DIR_ENV: &str = "OPENBARK_CONNECTIONS_DIR";

/// The announcement read at the start of every recorded call (ADR-0023
/// section 7). Spoken words that ask a question, so the client can answer.
pub const DEFAULT_ANNOUNCEMENT: &str = "This call is recorded for your pet's medical record. The recording stays at the clinic. Is that all right with you?";

/// Longest announcement accepted.
pub const MAX_ANNOUNCEMENT: usize = 600;

/// This computer's phone preferences (ADR-0023 section 10, Settings > Phone
/// and email). Not secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhonePrefs {
    /// "Turn on the phone on this computer". Off until a veterinarian turns
    /// it on; nothing reaches Twilio's calling service while it's off.
    #[serde(default)]
    pub enabled: bool,
    /// "Record calls". When on, every recorded call starts with the
    /// announcement; any one call can still go unrecorded.
    #[serde(default = "yes")]
    pub record_calls: bool,
    /// What the caller reads to the client when the call is answered.
    #[serde(default = "default_announcement")]
    pub announcement: String,
}

fn yes() -> bool {
    true
}

fn default_announcement() -> String {
    DEFAULT_ANNOUNCEMENT.to_string()
}

impl Default for PhonePrefs {
    fn default() -> Self {
        Self { enabled: false, record_calls: true, announcement: default_announcement() }
    }
}

impl PhonePrefs {
    /// Tidies and checks: the announcement can't be empty while calls are
    /// recorded, because recording without it isn't possible (owner decision 1).
    pub fn normalised(mut self) -> Result<Self, ConnectError> {
        self.announcement = self.announcement.split_whitespace().collect::<Vec<_>>().join(" ");
        if self.announcement.is_empty() {
            if self.record_calls {
                return Err(ConnectError::Invalid(
                    "Write the announcement. Recorded calls always start with it.".into(),
                ));
            }
            self.announcement = default_announcement();
        }
        if self.announcement.chars().count() > MAX_ANNOUNCEMENT {
            return Err(ConnectError::Invalid(format!(
                "Keep the announcement under {MAX_ANNOUNCEMENT} characters, so it's quick to read."
            )));
        }
        Ok(self)
    }
}

/// What `connections.json` holds. Sections this version doesn't know (a
/// later SendGrid section) are kept as they are when it saves.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    twilio: Option<TwilioSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phone: Option<PhonePrefs>,
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

/// The settings and the secret together: what setup and calls need.
#[derive(Debug, Clone)]
pub struct TwilioCredentials {
    pub settings: TwilioSettings,
    pub secret: Secret,
    pub location: SecretLocation,
}

impl TwilioCredentials {
    /// A REST client for this account (`api.twilio.com`, or a loopback mock
    /// named by `OPENBARK_TWILIO_API_BASE`).
    pub fn client(&self) -> Result<TwilioClient, ConnectError> {
        TwilioClient::for_account(&self.settings.account_sid, &self.settings.api_key_sid, self.secret.clone())
    }

    /// A one-hour access token for this computer's phone window. Calls out
    /// only: the first release takes no incoming calls (ADR-0023 section 15,
    /// decision 2).
    pub fn voice_token(&self, now: u64) -> Result<AccessToken, ConnectError> {
        let app = self.settings.twiml_app_sid.as_deref().ok_or_else(|| {
            ConnectError::Invalid("Twilio isn't set up yet: connect it in Settings first.".into())
        })?;
        let identity = self.settings.identity();
        token::mint(
            &TokenRequest {
                account_sid: &self.settings.account_sid,
                api_key_sid: &self.settings.api_key_sid,
                api_key_secret: &self.secret,
                identity: &identity,
                grant: VoiceGrant { outgoing_application_sid: app.to_string(), incoming_allow: false },
                ttl_secs: TOKEN_TTL_SECS,
            },
            now,
        )
    }
}

/// What [`ConnectStore::remove_twilio`] removed.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Removed {
    pub keychain: bool,
    pub sealed_file: bool,
    pub settings: bool,
    /// Set when the keychain couldn't be reached to remove the entry there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// After **Disconnect**: the key still works until the clinic deletes it.
pub const REMOVE_NOTE: &str =
    "Also delete this key in your Twilio console, so it can't be used anywhere else.";

/// The settings file and the secrets, in one folder.
pub struct ConnectStore {
    dir: PathBuf,
    keychain: Box<dyn Keychain>,
}

impl std::fmt::Debug for ConnectStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectStore").field("dir", &self.dir).finish_non_exhaustive()
    }
}

impl Default for ConnectStore {
    /// The real store, with the OS keychain; or, with `OPENBARK_CONNECTIONS_DIR`
    /// set, a store in that folder that never touches the keychain.
    fn default() -> Self {
        Self::from_env(&|name| std::env::var_os(name))
    }
}

impl ConnectStore {
    /// A store in `dir` using the OS keychain.
    pub fn new(dir: PathBuf) -> Self {
        Self::with_keychain(dir, Box::new(OsKeychain))
    }

    fn from_env(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Self {
        match env(DIR_ENV).filter(|d| !d.is_empty()) {
            Some(dir) => Self::with_keychain(PathBuf::from(dir), Box::new(NoKeychain)),
            None => Self::new(default_dir()),
        }
    }

    /// This computer's phone preferences (the defaults when none are saved).
    pub fn load_phone(&self) -> Result<PhonePrefs, ConnectError> {
        Ok(self.read_file()?.phone.unwrap_or_default())
    }

    /// Tidies, checks and saves the phone preferences.
    pub fn save_phone(&self, prefs: PhonePrefs) -> Result<PhonePrefs, ConnectError> {
        let prefs = prefs.normalised()?;
        let mut file = self.read_file()?;
        file.phone = Some(prefs.clone());
        self.write_file(&file)?;
        Ok(prefs)
    }

    /// A store with another keychain (tests use an in-memory one).
    pub fn with_keychain(dir: PathBuf, keychain: Box<dyn Keychain>) -> Self {
        Self { dir, keychain }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dir.join(SETTINGS_FILE)
    }

    pub fn sealed_path(&self) -> PathBuf {
        self.dir.join(SEALED_FILE)
    }

    fn read_file(&self) -> Result<ConnectionsFile, ConnectError> {
        let path = self.settings_path();
        if !path.is_file() {
            return Ok(ConnectionsFile::default());
        }
        let text = vault::read_to_string(&path).map_err(storage)?;
        serde_json::from_str(&text)
            .map_err(|e| ConnectError::Storage(format!("{} is damaged: {e}", path.display())))
    }

    fn write_file(&self, file: &ConnectionsFile) -> Result<(), ConnectError> {
        fs::create_dir_all(&self.dir).map_err(storage)?;
        let text = serde_json::to_string_pretty(file).map_err(|e| ConnectError::Storage(e.to_string()))?;
        vault::write(&self.settings_path(), text.as_bytes()).map_err(storage)
    }

    /// The saved Twilio settings, if Twilio is connected.
    pub fn load_twilio(&self) -> Result<Option<TwilioSettings>, ConnectError> {
        Ok(self.read_file()?.twilio)
    }

    /// Tidies, checks and saves the Twilio settings. Returns what was saved.
    pub fn save_twilio(&self, settings: TwilioSettings) -> Result<TwilioSettings, ConnectError> {
        let settings = settings.normalised();
        settings.validate()?;
        let mut file = self.read_file()?;
        file.twilio = Some(settings.clone());
        self.write_file(&file)?;
        Ok(settings)
    }

    /// Saves a secret in the keychain, or sealed in `connections.bin` when
    /// the keychain can't be used. Returns where it went.
    pub fn save_secret(&self, id: SecretId, secret: &Secret) -> Result<SecretLocation, ConnectError> {
        check_api_key_secret(secret)?;
        match self.keychain.set(id.account(), secret.expose()) {
            Ok(()) => {
                // A copy sealed earlier (from an SSH session, say) is now stale.
                // The keychain has the secret, so failing to tidy isn't fatal.
                if self.sealed_path().is_file()
                    && let Err(e) = self.sealed_remove(id)
                {
                    tracing::warn!(error = %e, "couldn't remove the stale sealed copy of the secret");
                }
                Ok(SecretLocation::Keychain)
            }
            Err(reason) => {
                tracing::warn!(%reason, secret = id.account(), "keychain unavailable; sealing the secret under the data key");
                let mut all = self.sealed_read()?;
                all.insert(id.account().to_string(), secret.expose().to_string());
                self.sealed_write(&all)?;
                Ok(SecretLocation::SealedFile)
            }
        }
    }

    /// A saved secret and where it was found: the keychain first, then the sealed file.
    pub fn load_secret(&self, id: SecretId) -> Result<Option<(Secret, SecretLocation)>, ConnectError> {
        match self.keychain.get(id.account()) {
            Ok(Some(value)) => return Ok(Some((Secret::new(value), SecretLocation::Keychain))),
            Ok(None) => {}
            Err(reason) => {
                tracing::debug!(%reason, secret = id.account(), "keychain unavailable; trying the sealed file");
            }
        }
        if !self.sealed_path().is_file() {
            return Ok(None);
        }
        Ok(self.sealed_read()?.remove(id.account()).map(|v| (Secret::new(v), SecretLocation::SealedFile)))
    }

    /// Where a secret is kept and its last four characters, for Settings.
    pub fn secret_status(&self, id: SecretId) -> Result<Option<SecretStatus>, ConnectError> {
        Ok(self.load_secret(id)?.map(|(secret, location)| SecretStatus {
            location,
            hint: secret.hint(),
            warning: (location == SecretLocation::SealedFile).then(|| SEALED_FILE_NOTE.to_string()),
        }))
    }

    /// The settings and secret together, when Twilio is connected.
    pub fn twilio_credentials(&self) -> Result<Option<TwilioCredentials>, ConnectError> {
        let Some(settings) = self.load_twilio()? else { return Ok(None) };
        let Some((secret, location)) = self.load_secret(SecretId::TwilioApiKeySecret)? else {
            return Ok(None);
        };
        Ok(Some(TwilioCredentials { settings, secret, location }))
    }

    /// **Disconnect**: removes the keychain entry, the sealed copy and the
    /// settings. The key itself lives on in Twilio until the clinic deletes
    /// it there ([`REMOVE_NOTE`]).
    pub fn remove_twilio(&self) -> Result<Removed, ConnectError> {
        let id = SecretId::TwilioApiKeySecret;
        let mut out = Removed::default();
        match self.keychain.delete(id.account()) {
            Ok(removed) => out.keychain = removed,
            Err(e) => out.warning = Some(format!("Couldn't reach the keychain to remove the key there: {e}")),
        }
        if self.sealed_path().is_file() {
            out.sealed_file = self.sealed_remove(id)?;
        }
        if self.settings_path().is_file() {
            let mut file = self.read_file()?;
            // The phone goes off with the account; the announcement is kept.
            if let Some(phone) = file.phone.as_mut() {
                phone.enabled = false;
            }
            if file.twilio.take().is_some() {
                out.settings = true;
                if file.other.is_empty() && file.phone.as_ref().is_none_or(|p| *p == PhonePrefs::default()) {
                    fs::remove_file(self.settings_path()).map_err(storage)?;
                } else {
                    self.write_file(&file)?;
                }
            }
        }
        Ok(out)
    }

    fn sealed_read(&self) -> Result<BTreeMap<String, String>, ConnectError> {
        let path = self.sealed_path();
        if !path.is_file() {
            return Ok(BTreeMap::new());
        }
        // Never trust a plain file here: this file is only ever written sealed.
        if !vault::is_encrypted(&path) {
            return Err(ConnectError::Storage(format!(
                "{} isn't sealed, so OpenBark won't use it. Delete it and connect again.",
                path.display()
            )));
        }
        let text = vault::read_to_string(&path).map_err(storage)?;
        serde_json::from_str(&text)
            .map_err(|e| ConnectError::Storage(format!("{} is damaged: {e}", path.display())))
    }

    fn sealed_write(&self, all: &BTreeMap<String, String>) -> Result<(), ConnectError> {
        let path = self.sealed_path();
        if all.is_empty() {
            if path.is_file() {
                fs::remove_file(&path).map_err(storage)?;
            }
            return Ok(());
        }
        if !vault::enabled() {
            return Err(ConnectError::Storage(
                "This computer's keychain isn't available and OpenBark's encryption isn't set up, so the key secret can't be kept safely. Nothing was saved.".into(),
            ));
        }
        fs::create_dir_all(&self.dir).map_err(storage)?;
        let text = serde_json::to_string(all).map_err(|e| ConnectError::Storage(e.to_string()))?;
        vault::write(&path, text.as_bytes()).map_err(storage)?;
        // vault::write seals whenever the vault is on; check, never assume.
        if !vault::is_encrypted(&path) {
            let _ = fs::remove_file(&path);
            return Err(ConnectError::Storage(
                "The key secret couldn't be sealed. Nothing was saved.".into(),
            ));
        }
        Ok(())
    }

    /// Removes one secret from the sealed file. True if it was there.
    fn sealed_remove(&self, id: SecretId) -> Result<bool, ConnectError> {
        let mut all = self.sealed_read()?;
        let had = all.remove(id.account()).is_some();
        if had {
            self.sealed_write(&all)?;
        }
        Ok(had)
    }

    /// The folder both files live in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn storage(e: std::io::Error) -> ConnectError {
    ConnectError::Storage(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::tests::{APP, sample};
    use crate::test_support::{MemoryKeychain, vault_off, vault_on};

    const SECRET: &str = "abcdefghijklmnopqrstuvwxyz01wxyz";

    fn store(dir: &Path, keychain: MemoryKeychain) -> ConnectStore {
        ConnectStore::with_keychain(dir.to_path_buf(), Box::new(keychain))
    }

    #[test]
    fn with_a_keychain_the_secret_goes_there_and_nowhere_else() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemoryKeychain::working();
        let s = store(dir.path(), keychain.clone());
        let loc = s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        assert_eq!(loc, SecretLocation::Keychain);
        assert_eq!(keychain.value("twilio-api-key-secret").as_deref(), Some(SECRET));
        assert!(!s.sealed_path().exists(), "no file when the keychain works");
        let (back, at) = s.load_secret(SecretId::TwilioApiKeySecret).unwrap().unwrap();
        assert_eq!((back.expose(), at), (SECRET, SecretLocation::Keychain));
        let status = s.secret_status(SecretId::TwilioApiKeySecret).unwrap().unwrap();
        assert_eq!(status.hint, "****wxyz");
        assert_eq!(status.warning, None);
    }

    #[test]
    fn without_a_keychain_the_secret_is_sealed_under_the_data_key() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::unavailable());
        let loc = s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        assert_eq!(loc, SecretLocation::SealedFile);
        let raw = fs::read(s.sealed_path()).unwrap();
        assert!(raw.starts_with(b"OBK1"), "sealed like every other file (ADR-0008)");
        assert!(!raw.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()), "no plain secret on disk");
        let (back, at) = s.load_secret(SecretId::TwilioApiKeySecret).unwrap().unwrap();
        assert_eq!((back.expose(), at), (SECRET, SecretLocation::SealedFile));
        let status = s.secret_status(SecretId::TwilioApiKeySecret).unwrap().unwrap();
        assert_eq!(status.location, SecretLocation::SealedFile);
        assert_eq!(status.warning.as_deref(), Some(SEALED_FILE_NOTE));
    }

    #[test]
    fn without_a_keychain_or_encryption_nothing_is_written() {
        let _v = vault_off();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::unavailable());
        let e = s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap_err();
        assert!(e.to_string().contains("Nothing was saved"), "{e}");
        assert!(!s.sealed_path().exists());
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none(), "the folder stays empty");
    }

    #[test]
    fn a_plain_sealed_file_is_never_trusted() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::unavailable());
        fs::write(s.sealed_path(), format!(r#"{{"twilio-api-key-secret":"{SECRET}"}}"#)).unwrap();
        let e = s.load_secret(SecretId::TwilioApiKeySecret).unwrap_err();
        assert!(e.to_string().contains("isn't sealed"), "{e}");
    }

    #[test]
    fn a_keychain_that_comes_back_replaces_the_sealed_copy() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemoryKeychain::unavailable();
        let s = store(dir.path(), keychain.clone());
        s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        assert!(s.sealed_path().exists());
        keychain.set_available(true);
        let loc = s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        assert_eq!(loc, SecretLocation::Keychain);
        assert!(!s.sealed_path().exists(), "the stale sealed copy is gone");
    }

    #[test]
    fn settings_round_trip_encrypted_and_keep_other_sections() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::working());
        assert_eq!(s.load_twilio().unwrap(), None);
        let mut typed = sample();
        typed.computer_name = "Front Desk".into();
        let saved = s.save_twilio(typed).unwrap();
        assert_eq!(saved, sample());
        assert_eq!(s.load_twilio().unwrap(), Some(sample()));
        let raw = fs::read(s.settings_path()).unwrap();
        assert!(raw.starts_with(b"OBK1"), "encrypted while the vault is on");
        assert!(!raw.windows(12).any(|w| w == b"+15555550100"));

        // A section from a later version survives a save.
        vault::write(&s.settings_path(), br#"{"sendgrid":{"from":"clinic@example.com"}}"#).unwrap();
        s.save_twilio(sample()).unwrap();
        let text = vault::read_to_string(&s.settings_path()).unwrap();
        assert!(text.contains("sendgrid") && text.contains("front-desk"), "{text}");
    }

    #[test]
    fn bad_settings_are_not_saved() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::working());
        let mut bad = sample();
        bad.account_sid = "AC123".into();
        assert!(s.save_twilio(bad).is_err());
        assert!(!s.settings_path().exists());
        assert!(s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new("has space")).is_err());
    }

    #[test]
    fn disconnect_removes_the_keychain_entry_the_sealed_copy_and_the_settings() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemoryKeychain::unavailable();
        let s = store(dir.path(), keychain.clone());
        s.save_twilio(sample()).unwrap();
        s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        keychain.set_available(true);
        keychain.insert("twilio-api-key-secret", SECRET);
        let removed = s.remove_twilio().unwrap();
        assert_eq!(removed, Removed { keychain: true, sealed_file: true, settings: true, warning: None });
        assert!(!s.sealed_path().exists());
        assert!(!s.settings_path().exists());
        assert_eq!(keychain.value("twilio-api-key-secret"), None);
        assert!(s.twilio_credentials().unwrap().is_none());
        // Nothing left: a second Disconnect removes nothing and doesn't fail.
        assert_eq!(s.remove_twilio().unwrap(), Removed::default());
    }

    #[test]
    fn disconnect_says_when_the_keychain_cant_be_reached() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::unavailable());
        s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        let removed = s.remove_twilio().unwrap();
        assert!(removed.sealed_file);
        assert!(removed.warning.unwrap().contains("keychain"));
    }

    #[test]
    fn a_moved_store_never_uses_the_keychain() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let env = move |name: &str| (name == DIR_ENV).then(|| path.clone().into_os_string());
        let s = ConnectStore::from_env(&env);
        assert_eq!(s.dir(), dir.path());
        let loc = s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        assert_eq!(loc, SecretLocation::SealedFile, "sealed in the scratch folder, not the keychain");
        assert!(s.sealed_path().is_file());
        let (back, _) = s.load_secret(SecretId::TwilioApiKeySecret).unwrap().unwrap();
        assert_eq!(back.expose(), SECRET);
        assert!(s.remove_twilio().unwrap().warning.is_some(), "says the keychain wasn't reached");
        assert!(!s.sealed_path().exists());
        // Without the variable, the real folder and the OS keychain.
        let real = ConnectStore::from_env(&|_| None);
        assert_eq!(real.dir(), default_dir());
    }

    #[test]
    fn phone_preferences_start_off_and_survive_disconnect() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::working());
        let prefs = s.load_phone().unwrap();
        assert_eq!(prefs, PhonePrefs::default());
        assert!(!prefs.enabled && prefs.record_calls, "off until turned on; calls recorded by default");
        assert_eq!(prefs.announcement, DEFAULT_ANNOUNCEMENT);
        let saved = s
            .save_phone(PhonePrefs {
                enabled: true,
                record_calls: true,
                announcement: "  We record   calls. OK? ".into(),
            })
            .unwrap();
        assert_eq!(saved.announcement, "We record calls. OK?");
        assert!(
            s.save_phone(PhonePrefs { announcement: " ".into(), ..saved.clone() }).is_err(),
            "never empty"
        );
        assert!(s.save_phone(PhonePrefs { announcement: "x".repeat(601), ..saved.clone() }).is_err());
        s.save_twilio(sample()).unwrap();
        s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        s.remove_twilio().unwrap();
        let after = s.load_phone().unwrap();
        assert!(!after.enabled, "Disconnect turns the phone off");
        assert_eq!(after.announcement, "We record calls. OK?", "the clinic's wording is kept");
        // Defaults left after Disconnect don't keep an otherwise empty file.
        let dir2 = tempfile::tempdir().unwrap();
        let s2 = store(dir2.path(), MemoryKeychain::working());
        s2.save_twilio(sample()).unwrap();
        s2.remove_twilio().unwrap();
        assert!(!s2.settings_path().exists());
    }

    #[test]
    fn credentials_mint_a_calls_out_token_once_set_up() {
        let _v = vault_on();
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MemoryKeychain::working());
        s.save_twilio(sample()).unwrap();
        s.save_secret(SecretId::TwilioApiKeySecret, &Secret::new(SECRET)).unwrap();
        let creds = s.twilio_credentials().unwrap().unwrap();
        assert!(creds.voice_token(1_792_324_800).unwrap_err().to_string().contains("Settings"));
        let mut set_up = sample();
        set_up.twiml_app_sid = Some(APP.into());
        s.save_twilio(set_up).unwrap();
        let creds = s.twilio_credentials().unwrap().unwrap();
        let token = creds.voice_token(1_792_324_800).unwrap();
        assert_eq!(token.identity, "openbark_front_desk");
        assert_eq!(token.expires_at - token.issued_at, 3600);
        let payload = token.jwt().split('.').nth(1).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(
            &base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload).unwrap(),
        )
        .unwrap();
        assert_eq!(claims["grants"]["voice"], serde_json::json!({"outgoing": {"application_sid": APP}}));
    }
}
