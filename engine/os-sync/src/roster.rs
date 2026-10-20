//! The roster: which devices belong to the account, with their names and
//! public keys, sealed (OBS1) under a key derived from the AMK and signed by
//! the device that last changed it. Clients accept object signatures only
//! from keys in it, so the server can't slip in a device of its own.
//!
//! It also carries the Recovery Kit's public key, so a device rotating the
//! keys wraps the new AMK to the real kit, not to a key the server offers.
//!
//! A removed device stays listed with when it was removed and the last
//! counter seen from it: what it sealed before stays valid; anything newer
//! is refused.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::SyncError;
use crate::keys::{self, DeviceKeys, SymKey};
use crate::seal::{self, Header};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub x25519_pub: String,
    pub ed25519_pub: String,
    pub added_at: DateTime<Utc>,
    #[serde(default)]
    pub removed_at: Option<DateTime<Utc>>,
    /// The highest counter seen from it when it was removed.
    #[serde(default)]
    pub last_ctr: Option<u64>,
}

impl Entry {
    pub fn ed_pub(&self) -> Option<[u8; 32]> {
        keys::unb64_32(&self.ed25519_pub).ok()
    }

    pub fn x_pub(&self) -> Option<[u8; 32]> {
        keys::unb64_32(&self.x25519_pub).ok()
    }

    /// The tag run folders carry (`os_engine::device`).
    pub fn tag(&self) -> String {
        os_engine::device::Device::from_id(&self.id).tag
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Roster {
    pub kit_pub: String,
    pub devices: Vec<Entry>,
}

fn roster_key(amk: &SymKey) -> SymKey {
    amk.derive("roster")
}

impl Roster {
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// The key that may sign an object from `dev` with counter `ctr`.
    pub fn signer(&self, dev: &str, ctr: u64) -> Option<[u8; 32]> {
        let e = self.get(dev)?;
        if e.removed_at.is_some() && e.last_ctr.is_some_and(|last| ctr > last) {
            return None;
        }
        e.ed_pub()
    }

    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.devices.iter().filter(|d| d.removed_at.is_none())
    }

    pub fn name_for_tag(&self, tag: &str) -> Option<String> {
        self.devices.iter().find(|d| d.tag() == tag).map(|d| d.name.clone())
    }

    /// Seal under the AMK, signed by `device`.
    pub fn seal(&self, amk: &SymKey, account: &str, version: i64, device: &DeviceKeys, device_id: &str) -> Result<Vec<u8>, SyncError> {
        let header = Header {
            v: 1,
            id: account.to_string(),
            space: account.to_string(),
            kind: "roster".into(),
            ver: version as u32,
            dev: device_id.to_string(),
            ctr: 0,
        };
        seal::seal_obs1(&header, &serde_json::to_vec(self).expect("serialises"), &roster_key(amk), device)
    }

    /// Open and check: the right account and version, and signed by a device
    /// the roster itself lists (not removed).
    pub fn open(bytes: &[u8], amk: &SymKey, account: &str, version: i64) -> Result<Self, SyncError> {
        let obj = seal::parse_obs1(bytes)?;
        if obj.header.kind != "roster" || obj.header.id != account || obj.header.ver as i64 != version {
            return Err(SyncError::Integrity("the device list isn't this account's current one".into()));
        }
        let plain = seal::open_obs1(&obj, &roster_key(amk))?;
        let roster: Roster =
            serde_json::from_slice(&plain).map_err(|_| SyncError::Format("damaged device list".into()))?;
        let signer = roster
            .active()
            .find(|d| d.id == obj.header.dev)
            .and_then(Entry::ed_pub)
            .ok_or_else(|| SyncError::Integrity("the device list was signed by a device it doesn't list".into()))?;
        if !seal::verify_obs1(&obj, &signer) {
            return Err(SyncError::Integrity("the device list's signature is wrong".into()));
        }
        Ok(roster)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roster_is_sealed_signed_and_bound_to_its_version() {
        let amk = SymKey::random().unwrap();
        let dev = DeviceKeys::generate().unwrap();
        let mut r = Roster { kit_pub: keys::b64(&[9; 32]), devices: vec![] };
        r.devices.push(Entry {
            id: "dev-a".into(),
            name: "Front desk".into(),
            platform: "windows".into(),
            x25519_pub: keys::b64(&dev.x_pub()),
            ed25519_pub: keys::b64(&dev.ed_pub()),
            added_at: Utc::now(),
            removed_at: None,
            last_ctr: None,
        });
        let sealed = r.seal(&amk, "acct", 3, &dev, "dev-a").unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("Front desk"));
        assert_eq!(Roster::open(&sealed, &amk, "acct", 3).unwrap(), r);
        assert!(Roster::open(&sealed, &amk, "acct", 2).is_err(), "an old version replayed");
        assert!(Roster::open(&sealed, &amk, "other", 3).is_err());
        assert!(Roster::open(&sealed, &SymKey::random().unwrap(), "acct", 3).is_err());
        // Signed by a device that isn't listed.
        let stranger = DeviceKeys::generate().unwrap();
        let forged = r.seal(&amk, "acct", 3, &stranger, "dev-x").unwrap();
        assert!(Roster::open(&forged, &amk, "acct", 3).is_err());
        // A removed device's later objects are refused.
        r.devices[0].removed_at = Some(Utc::now());
        r.devices[0].last_ctr = Some(10);
        assert!(r.signer("dev-a", 10).is_some());
        assert!(r.signer("dev-a", 11).is_none());
    }
}
