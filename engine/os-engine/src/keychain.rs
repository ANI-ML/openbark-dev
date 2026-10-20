//! The operating system's keychain, service "OpenBark" (ADR-0008, ADR-0017
//! section 4, ADR-0018 section 5).
//!
//! - Windows, macOS and Linux: the `keyring` crate's platform store
//!   (Credential Manager, Keychain, Secret Service), as before.
//! - iOS: Apple's protected data store (`apple-native-keyring-store`'s
//!   `protected` store), each item "after first unlock, this device only":
//!   recording works while the phone is locked, and nothing reaches iCloud
//!   or another device. iOS has no key-file fallback; the caller decides what
//!   a failure means.
//! - Anything else: no keychain.

/// The keychain service every OpenBark secret lives under.
pub const SERVICE: &str = "OpenBark";

/// The iOS access policy: readable after the first unlock since boot, never
/// synced or restored to another device.
#[cfg(target_os = "ios")]
pub const IOS_ACCESS_POLICY: &str = "after-first-unlock-this-device-only";

/// The secret stored for `account`, or `None` if there is none.
pub fn get(account: &str) -> Result<Option<String>, String> {
    imp::get(account)
}

/// Store `value` for `account`, replacing any earlier one.
pub fn set(account: &str, value: &str) -> Result<(), String> {
    imp::set(account, value)
}

/// Remove the secret for `account`. `Ok(false)` when there was none.
pub fn delete(account: &str) -> Result<bool, String> {
    imp::delete(account)
}

/// Whether this platform has a keychain OpenBark can use at all.
pub const fn available() -> bool {
    cfg!(any(windows, target_os = "macos", target_os = "linux", target_os = "ios"))
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod imp {
    use super::SERVICE;

    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(account: &str, value: &str) -> Result<(), String> {
        entry(account)?.set_password(value).map_err(|e| e.to_string())
    }

    pub fn delete(account: &str) -> Result<bool, String> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use std::collections::HashMap;

    use apple_native_keyring_store::protected::Store;
    use keyring_core::api::CredentialStoreApi;
    use keyring_core::{Entry, Error};

    use super::{IOS_ACCESS_POLICY, SERVICE};

    fn entry(account: &str) -> Result<Entry, String> {
        let store = Store::new().map_err(|e| e.to_string())?;
        let modifiers = HashMap::from([("access-policy", IOS_ACCESS_POLICY)]);
        store.build(SERVICE, account, Some(&modifiers)).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(account: &str, value: &str) -> Result<(), String> {
        entry(account)?.set_password(value).map_err(|e| e.to_string())
    }

    pub fn delete(account: &str) -> Result<bool, String> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux", target_os = "ios")))]
mod imp {
    const NONE: &str = "no keychain on this platform";

    pub fn get(_: &str) -> Result<Option<String>, String> {
        Err(NONE.into())
    }

    pub fn set(_: &str, _: &str) -> Result<(), String> {
        Err(NONE.into())
    }

    pub fn delete(_: &str) -> Result<bool, String> {
        Ok(false)
    }
}
