//! Local team members with roles and PINs (M1 item 11). Who recorded,
//! drafted, edited and signed is stored on every revision.
//!
//! This is a convenience on one shared computer, not an authentication
//! boundary: anyone with the OS account can read the files (encrypted at
//! rest, ADR-0008). Accounts, sign-in and audit arrive with the Clinic Hub
//! (M4). With no team members set up, OpenBark runs single-user and uses
//! the name from Settings.

use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Veterinarian: everything, including signing and addenda.
    Dvm,
    /// Veterinary technician / nurse: records, drafts, edits; doesn't sign.
    Rvt,
    /// Reception: records and enters patient details; doesn't touch notes.
    Reception,
}

impl Role {
    pub fn can_sign(self) -> bool {
        self == Role::Dvm
    }
    pub fn can_write_notes(self) -> bool {
        matches!(self, Role::Dvm | Role::Rvt)
    }
    /// Adding and removing team members.
    pub fn can_manage_team(self) -> bool {
        self == Role::Dvm
    }
}

/// Who did something, as stored on a revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Who {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
}

impl Who {
    /// Single-user mode: just a name.
    pub fn named(name: &str) -> Self {
        Self { id: None, name: name.trim().to_string(), role: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredUser {
    id: String,
    name: String,
    role: Role,
    salt: String,
    pin_hash: String,
    created_at: DateTime<Local>,
}

/// A team member as the app shows them (no PIN material).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub role: Role,
}

impl From<&StoredUser> for User {
    fn from(u: &StoredUser) -> Self {
        Self { id: u.id.clone(), name: u.name.clone(), role: u.role }
    }
}

impl From<&User> for Who {
    fn from(u: &User) -> Self {
        Self { id: Some(u.id.clone()), name: u.name.clone(), role: Some(u.role) }
    }
}

pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OPENBARK_USERS_FILE") {
        return PathBuf::from(p);
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark").join("users.json")
}

/// Slow enough to make guessing a PIN from a copied file tedious.
const ROUNDS: u32 = 100_000;

fn hash_pin(salt: &str, pin: &str) -> String {
    let mut h = Sha256::digest(format!("{salt}:{pin}").as_bytes());
    for _ in 1..ROUNDS {
        h = Sha256::digest(h);
    }
    hex::encode(h)
}

fn check_pin(pin: &str) -> Result<(), EngineError> {
    let ok = (4..=8).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit());
    if ok { Ok(()) } else { Err(EngineError::Session("A PIN is 4 to 8 digits.".into())) }
}

#[derive(Debug, Clone)]
pub struct UserStore {
    path: PathBuf,
}

impl Default for UserStore {
    fn default() -> Self {
        Self::new(default_path())
    }
}

impl UserStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Result<Vec<StoredUser>, EngineError> {
        match crate::vault::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| EngineError::Session(format!("users: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(EngineError::Session(format!("users: {e}"))),
        }
    }

    fn save(&self, users: &[StoredUser]) -> Result<(), EngineError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| EngineError::Session(e.to_string()))?;
        }
        let text = serde_json::to_string_pretty(users).expect("users serialise");
        crate::vault::write(&self.path, text.as_bytes()).map_err(|e| EngineError::Session(e.to_string()))
    }

    /// Team members by name. Empty: single-user mode.
    pub fn list(&self) -> Result<Vec<User>, EngineError> {
        let mut users: Vec<User> = self.load()?.iter().map(User::from).collect();
        users.sort_by_key(|u| u.name.to_lowercase());
        Ok(users)
    }

    pub fn is_team(&self) -> bool {
        self.load().map(|u| !u.is_empty()).unwrap_or(false)
    }

    pub fn get(&self, id: &str) -> Option<User> {
        self.load().ok()?.iter().find(|u| u.id == id).map(User::from)
    }

    /// Add a team member. The first one must be a veterinarian, so someone
    /// can always sign and manage the team.
    pub fn add(&self, name: &str, role: Role, pin: &str) -> Result<User, EngineError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EngineError::Session("Enter the team member's name.".into()));
        }
        check_pin(pin)?;
        let mut users = self.load()?;
        if users.is_empty() && role != Role::Dvm {
            return Err(EngineError::Session("The first team member must be a veterinarian.".into()));
        }
        if users.iter().any(|u| u.name.eq_ignore_ascii_case(name)) {
            return Err(EngineError::Session(format!("{name} is already on the team.")));
        }
        let salt = uuid::Uuid::now_v7().simple().to_string();
        let user = StoredUser {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            role,
            pin_hash: hash_pin(&salt, pin),
            salt,
            created_at: Local::now(),
        };
        let out = User::from(&user);
        users.push(user);
        self.save(&users)?;
        Ok(out)
    }

    /// Remove a team member; the last veterinarian can't be removed while
    /// others remain.
    pub fn remove(&self, id: &str) -> Result<(), EngineError> {
        let mut users = self.load()?;
        let before = users.len();
        users.retain(|u| u.id != id);
        if users.len() == before {
            return Err(EngineError::Session("That team member was already removed.".into()));
        }
        if !users.is_empty() && !users.iter().any(|u| u.role == Role::Dvm) {
            return Err(EngineError::Session("Keep at least one veterinarian on the team.".into()));
        }
        self.save(&users)
    }

    pub fn set_pin(&self, id: &str, pin: &str) -> Result<(), EngineError> {
        check_pin(pin)?;
        let mut users = self.load()?;
        let user = users
            .iter_mut()
            .find(|u| u.id == id)
            .ok_or_else(|| EngineError::Session("No such team member.".into()))?;
        user.salt = uuid::Uuid::now_v7().simple().to_string();
        user.pin_hash = hash_pin(&user.salt, pin);
        self.save(&users)
    }

    /// Check a PIN; returns the team member when it matches.
    pub fn verify(&self, id: &str, pin: &str) -> Result<User, EngineError> {
        let users = self.load()?;
        let user = users
            .iter()
            .find(|u| u.id == id)
            .ok_or_else(|| EngineError::Session("No such team member.".into()))?;
        if hash_pin(&user.salt, pin) == user.pin_hash {
            Ok(User::from(user))
        } else {
            Err(EngineError::Session("That PIN isn't right.".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_members_roles_and_pins() {
        // Writes through the vault: hold its lock so no test turns it on meanwhile.
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let store = UserStore::new(tmp.path().join("users.json"));
        assert!(!store.is_team());
        assert!(store.add("Sam", Role::Rvt, "1234").is_err(), "first member is a veterinarian");
        let dvm = store.add("Dr Patel", Role::Dvm, "2468").unwrap();
        let rvt = store.add("Sam", Role::Rvt, "1357").unwrap();
        assert!(store.add("sam", Role::Reception, "1111").is_err(), "names are unique");
        assert!(store.add("Lee", Role::Reception, "12").is_err(), "PIN length");
        assert!(store.is_team());
        assert_eq!(store.list().unwrap().len(), 2);

        assert_eq!(store.verify(&dvm.id, "2468").unwrap(), dvm);
        assert!(store.verify(&dvm.id, "0000").is_err());
        store.set_pin(&rvt.id, "9999").unwrap();
        assert!(store.verify(&rvt.id, "1357").is_err());
        assert!(store.verify(&rvt.id, "9999").is_ok());

        assert!(store.remove(&dvm.id).is_err(), "keep a veterinarian");
        store.remove(&rvt.id).unwrap();
        store.remove(&dvm.id).unwrap();
        assert!(!store.is_team(), "back to single-user");

        // No PIN or hash material is ever exposed.
        let raw = std::fs::read_to_string(tmp.path().join("users.json")).unwrap();
        assert!(!raw.contains("2468"));
        assert!(Role::Dvm.can_sign() && !Role::Rvt.can_sign() && Role::Rvt.can_write_notes());
        assert!(!Role::Reception.can_write_notes());
    }
}
