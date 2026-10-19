//! Test helpers: an in-memory keychain (tests never touch this computer's
//! Credential Manager) and the process-wide vault, one test at a time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use os_engine::vault::{self, KeySource};

use crate::secrets::Keychain;

/// The vault is process-wide; tests that depend on it run one at a time.
static VAULT: Mutex<()> = Mutex::new(());

/// Holds the vault on (with a fixed test key) or off until dropped.
pub struct VaultGuard {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for VaultGuard {
    fn drop(&mut self) {
        vault::disable();
    }
}

pub fn vault_on() -> VaultGuard {
    let lock = VAULT.lock().unwrap_or_else(|e| e.into_inner());
    vault::use_key([7u8; 32], KeySource::Environment, None);
    VaultGuard { _lock: lock }
}

pub fn vault_off() -> VaultGuard {
    let lock = VAULT.lock().unwrap_or_else(|e| e.into_inner());
    vault::disable();
    VaultGuard { _lock: lock }
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, String>,
    unavailable: bool,
}

/// A keychain in memory. `unavailable()` behaves like Windows over SSH
/// (no logon session): every call fails.
#[derive(Clone, Default)]
pub struct MemoryKeychain(Arc<Mutex<Inner>>);

impl MemoryKeychain {
    pub fn working() -> Self {
        Self::default()
    }

    pub fn unavailable() -> Self {
        let k = Self::default();
        k.set_available(false);
        k
    }

    pub fn set_available(&self, available: bool) {
        self.0.lock().unwrap().unavailable = !available;
    }

    pub fn value(&self, account: &str) -> Option<String> {
        self.0.lock().unwrap().entries.get(account).cloned()
    }

    pub fn insert(&self, account: &str, value: &str) {
        self.0.lock().unwrap().entries.insert(account.into(), value.into());
    }
}

const NO_SESSION: &str = "Platform secure storage failure: A specified logon session does not exist.";

impl Keychain for MemoryKeychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let inner = self.0.lock().unwrap();
        if inner.unavailable {
            return Err(NO_SESSION.into());
        }
        Ok(inner.entries.get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        if inner.unavailable {
            return Err(NO_SESSION.into());
        }
        inner.entries.insert(account.into(), value.into());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, String> {
        let mut inner = self.0.lock().unwrap();
        if inner.unavailable {
            return Err(NO_SESSION.into());
        }
        Ok(inner.entries.remove(account).is_some())
    }
}
