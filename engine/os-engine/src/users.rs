//! Local team members with roles and PINs (M1 item 11). Who recorded,
//! drafted, edited and signed is stored on every revision.
//!
//! This is a convenience on one shared computer, not an authentication
//! boundary: anyone with the OS account can read the files (encrypted at
//! rest, ADR-0008). Accounts, sign-in and audit arrive with the Clinic Hub
//! (M4). With no team members set up, OpenBark runs single-user and uses
//! the name from Settings.

use std::path::PathBuf;

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
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
    /// The salt of an old SHA-256 hash; empty for Argon2id (its PHC string
    /// carries its own salt).
    #[serde(default)]
    salt: String,
    /// An Argon2id PHC string (`$argon2id$v=19$...`), or an old salted
    /// SHA-256 hash, rewritten as Argon2id the next time the PIN is entered.
    pin_hash: String,
    created_at: DateTime<Local>,
    /// Admins change the hospital's templates and the templates setting,
    /// each time with their PIN (ADR-0024 D2). Separate from the role.
    #[serde(default)]
    admin: bool,
}

/// A team member as the app shows them (no PIN material).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub role: Role,
    /// Can change the hospital's templates (with their PIN) and who is an admin.
    pub admin: bool,
}

impl From<&StoredUser> for User {
    fn from(u: &StoredUser) -> Self {
        Self { id: u.id.clone(), name: u.name.clone(), role: u.role, admin: u.admin }
    }
}

/// Whose templates the team drafts with (ADR-0024 D2), set by an admin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateMode {
    /// Each person's own: everyone may keep their own versions, and the
    /// hospital's templates are the defaults.
    #[default]
    Personal,
    /// Shared by the hospital: everyone drafts with the hospital's templates.
    /// Personal versions are kept but not used, and come back if this changes.
    Hospital,
}

/// Settings for the whole team, beside the team file (encrypted like it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamSettings {
    #[serde(default)]
    pub templates: TemplateMode,
    /// Tell the first admin, once, that templates changed while working
    /// alone are now the hospital's.
    #[serde(default)]
    pub setup_notice: bool,
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

/// There must always be at least one admin (ADR-0024 D2).
pub const LAST_ADMIN: &str = "Keep at least one admin on the team. Make someone else an admin first.";

/// Rounds of the old SHA-256 PIN hash (before M3). Only checked, never written.
const LEGACY_ROUNDS: u32 = 100_000;

/// Wrong PINs in a row, on any team member, before the picker locks.
pub const MAX_WRONG_PINS: u32 = 5;
/// How long the picker stays locked.
pub const LOCKOUT_MINUTES: i64 = 15;

/// Argon2id with OWASP's minimum: 19 MiB, 2 passes, 1 lane. Under a tenth of a
/// second on a phone, and memory-hard against a copied file.
fn argon2() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19 * 1024, 2, 1, None).expect("valid params"),
    )
}

/// A new PIN hash: an Argon2id PHC string with its own random salt.
fn hash_pin(pin: &str) -> Result<String, EngineError> {
    argon2()
        .hash_password(pin.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| EngineError::Session(format!("couldn't protect the PIN: {e}")))
}

/// The old salted, iterated SHA-256 (M1 item 11).
fn legacy_hash(salt: &str, pin: &str) -> String {
    let mut h = Sha256::digest(format!("{salt}:{pin}").as_bytes());
    for _ in 1..LEGACY_ROUNDS {
        h = Sha256::digest(h);
    }
    hex::encode(h)
}

/// Whether `pin` matches a stored hash, and whether that hash is the old kind.
fn pin_matches(user: &StoredUser, pin: &str) -> (bool, bool) {
    if user.pin_hash.starts_with("$argon2") {
        (argon2().verify_password(pin.as_bytes(), user.pin_hash.as_str()).is_ok(), false)
    } else {
        (legacy_hash(&user.salt, pin) == user.pin_hash, true)
    }
}

/// Wrong PINs in a row, kept beside the team file, encrypted like it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Lockout {
    wrong: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    locked_until: Option<DateTime<Local>>,
}

/// The picker is locked after too many wrong PINs.
pub fn locked_message(minutes: i64) -> String {
    if minutes <= 1 {
        "Too many wrong PINs. Try again in a minute.".into()
    } else {
        format!("Too many wrong PINs. Try again in {minutes} minutes.")
    }
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

    /// The file the team is kept in.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn load(&self) -> Result<Vec<StoredUser>, EngineError> {
        let mut users: Vec<StoredUser> = match crate::vault::read_to_string(&self.path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| EngineError::Session(format!("users: {e}")))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(EngineError::Session(format!("users: {e}"))),
        };
        // A team from before admins existed: whoever set it up is the first admin.
        if !users.is_empty() && !users.iter().any(|u| u.admin) {
            let first = (0..users.len()).min_by_key(|&i| users[i].created_at).expect("not empty");
            users[first].admin = true;
        }
        Ok(users)
    }

    /// `pin-lockout.json`, beside the team file.
    fn lockout_path(&self) -> PathBuf {
        self.path.with_file_name("pin-lockout.json")
    }

    fn lockout(&self) -> Lockout {
        crate::vault::read_to_string(&self.lockout_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_lockout(&self, l: &Lockout) -> Result<(), EngineError> {
        let path = self.lockout_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| EngineError::Session(e.to_string()))?;
        }
        let text = serde_json::to_string(l).expect("lockout serialises");
        crate::vault::write(&path, text.as_bytes()).map_err(|e| EngineError::Session(e.to_string()))
    }

    /// When the picker unlocks again, if it's locked now.
    pub fn locked_until(&self, now: DateTime<Local>) -> Option<DateTime<Local>> {
        self.lockout().locked_until.filter(|t| *t > now)
    }

    /// `team-settings.json`, beside the team file.
    fn settings_path(&self) -> PathBuf {
        self.path.with_file_name("team-settings.json")
    }

    /// The team's settings (the defaults when none are saved).
    pub fn settings(&self) -> TeamSettings {
        crate::vault::read_to_string(&self.settings_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn set_settings(&self, settings: &TeamSettings) -> Result<(), EngineError> {
        let path = self.settings_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| EngineError::Session(e.to_string()))?;
        }
        let text = serde_json::to_string_pretty(settings).expect("settings serialise");
        crate::vault::write(&path, text.as_bytes()).map_err(|e| EngineError::Session(e.to_string()))
    }

    /// Grant or remove the admin flag. There is always at least one admin.
    pub fn set_admin(&self, id: &str, admin: bool) -> Result<User, EngineError> {
        let mut users = self.load()?;
        let user = users
            .iter_mut()
            .find(|u| u.id == id)
            .ok_or_else(|| EngineError::Session("No such team member.".into()))?;
        user.admin = admin;
        let out = User::from(&*user);
        if !users.iter().any(|u| u.admin) {
            return Err(EngineError::Session(LAST_ADMIN.into()));
        }
        self.save(&users)?;
        Ok(out)
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
        let user = StoredUser {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            role,
            pin_hash: hash_pin(pin)?,
            salt: String::new(),
            created_at: Local::now(),
            // The person who sets up the team is its first admin (ADR-0024 D2).
            admin: users.is_empty(),
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
        if !users.is_empty() && !users.iter().any(|u| u.admin) {
            return Err(EngineError::Session(LAST_ADMIN.into()));
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
        user.salt = String::new();
        user.pin_hash = hash_pin(pin)?;
        self.save(&users)
    }

    /// Check a PIN; returns the team member when it matches.
    ///
    /// Five wrong PINs in a row, on any team member, lock the picker for 15
    /// minutes; a right PIN resets the count. A PIN stored with the old
    /// SHA-256 hash is rewritten as Argon2id when it's entered correctly.
    pub fn verify(&self, id: &str, pin: &str) -> Result<User, EngineError> {
        self.verify_at(id, pin, Local::now())
    }

    /// [`UserStore::verify`] at a given time (tests).
    pub fn verify_at(&self, id: &str, pin: &str, now: DateTime<Local>) -> Result<User, EngineError> {
        let mut lock = self.lockout();
        if let Some(until) = lock.locked_until {
            if until > now {
                let minutes = ((until - now).num_seconds() + 59) / 60;
                return Err(EngineError::Session(locked_message(minutes)));
            }
            lock = Lockout::default();
        }
        let mut users = self.load()?;
        let index = users
            .iter()
            .position(|u| u.id == id)
            .ok_or_else(|| EngineError::Session("No such team member.".into()))?;
        let (ok, legacy) = pin_matches(&users[index], pin);
        if !ok {
            lock.wrong += 1;
            if lock.wrong >= MAX_WRONG_PINS {
                lock.wrong = 0;
                lock.locked_until = Some(now + chrono::TimeDelta::minutes(LOCKOUT_MINUTES));
                self.save_lockout(&lock)?;
                return Err(EngineError::Session(locked_message(LOCKOUT_MINUTES)));
            }
            self.save_lockout(&lock)?;
            return Err(EngineError::Session("That PIN isn't right.".into()));
        }
        if lock.wrong > 0 || lock.locked_until.is_some() {
            self.save_lockout(&Lockout::default())?;
        }
        if legacy {
            users[index].pin_hash = hash_pin(pin)?;
            users[index].salt = String::new();
            self.save(&users)?;
        }
        Ok(User::from(&users[index]))
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

        // The first member is the admin, and there is always at least one.
        let dvm = store.add("Dr Patel", Role::Dvm, "2468").unwrap();
        assert!(dvm.admin);
        let dvm2 = store.add("Dr Okafor", Role::Dvm, "1111").unwrap();
        assert!(!dvm2.admin);
        assert!(store.set_admin(&dvm.id, false).is_err(), "the only admin");
        store.set_admin(&dvm2.id, true).unwrap();
        store.set_admin(&dvm.id, false).unwrap();
        assert!(store.remove(&dvm2.id).is_err(), "the last admin stays while others remain");
        store.set_admin(&dvm.id, true).unwrap();
        store.remove(&dvm2.id).unwrap();
        // Team settings default to each person's own templates.
        assert_eq!(store.settings().templates, TemplateMode::Personal);
        store.set_settings(&TeamSettings { templates: TemplateMode::Hospital, setup_notice: false }).unwrap();
        assert_eq!(store.settings().templates, TemplateMode::Hospital);
        store.remove(&dvm.id).unwrap();

        // No PIN or hash material is ever exposed.
        let raw = std::fs::read_to_string(tmp.path().join("users.json")).unwrap();
        assert!(!raw.contains("2468"));
        assert!(Role::Dvm.can_sign() && !Role::Rvt.can_sign() && Role::Rvt.can_write_notes());
        assert!(!Role::Reception.can_write_notes());
    }

    #[test]
    fn pins_are_argon2id_and_old_hashes_are_upgraded_once() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("users.json");
        // A team file from 0.1.x: salted, iterated SHA-256.
        let salt = "0199a0000000700080000000000000aa";
        let old = serde_json::json!([{
            "id": "0199a000-0000-7000-8000-000000000001",
            "name": "Dr Patel",
            "role": "dvm",
            "salt": salt,
            "pin_hash": legacy_hash(salt, "2468"),
            "created_at": "2026-10-01T09:00:00-04:00",
            "admin": true
        }]);
        std::fs::write(&path, old.to_string()).unwrap();
        let store = UserStore::new(path.clone());
        let id = "0199a000-0000-7000-8000-000000000001";
        assert!(store.verify(id, "1111").is_err());
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("$argon2id$"), "a wrong PIN doesn't rewrite anything");
        // The old hash works once, and is then stored as Argon2id.
        assert_eq!(store.verify(id, "2468").unwrap().name, "Dr Patel");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("$argon2id$v=19$m=19456,t=2,p=1$"), "{raw}");
        assert!(!raw.contains(&legacy_hash(salt, "2468")));
        assert!(store.verify(id, "2468").is_ok(), "and keeps working");
        assert!(store.verify(id, "1357").is_err());
        // New and changed PINs are Argon2id with their own salt.
        let rvt = store.add("Sam", Role::Rvt, "1357").unwrap();
        store.set_pin(&rvt.id, "8642").unwrap();
        assert!(store.verify(&rvt.id, "8642").is_ok());
        let users: Vec<StoredUser> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(users.iter().all(|u| u.pin_hash.starts_with("$argon2id$") && u.salt.is_empty()));
        let hashes: std::collections::HashSet<_> = users.iter().map(|u| u.pin_hash.clone()).collect();
        assert_eq!(hashes.len(), 2);
    }

    #[test]
    fn five_wrong_pins_lock_the_picker_for_15_minutes() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let store = UserStore::new(tmp.path().join("users.json"));
        let dvm = store.add("Dr Patel", Role::Dvm, "2468").unwrap();
        let rvt = store.add("Sam", Role::Rvt, "1357").unwrap();
        let t0 = Local::now();
        // A right PIN resets the count.
        for _ in 0..4 {
            assert_eq!(
                store.verify_at(&dvm.id, "0000", t0).unwrap_err().to_string(),
                "That PIN isn't right."
            );
        }
        assert!(store.verify_at(&dvm.id, "2468", t0).is_ok());
        // Five wrong in a row, across team members, lock the picker.
        for i in 0..4 {
            let who = if i % 2 == 0 { &dvm.id } else { &rvt.id };
            assert!(store.verify_at(who, "0000", t0).is_err());
        }
        let err = store.verify_at(&rvt.id, "0000", t0).unwrap_err().to_string();
        assert_eq!(err, "Too many wrong PINs. Try again in 15 minutes.");
        assert!(store.locked_until(t0).is_some());
        // Locked: even the right PIN is refused, for everyone.
        let later = t0 + chrono::TimeDelta::minutes(14);
        assert_eq!(
            store.verify_at(&dvm.id, "2468", later).unwrap_err().to_string(),
            "Too many wrong PINs. Try again in a minute."
        );
        assert!(store.verify_at(&rvt.id, "1357", t0 + chrono::TimeDelta::minutes(10)).is_err());
        // After 15 minutes it opens again, with a fresh count.
        let open = t0 + chrono::TimeDelta::minutes(15) + chrono::TimeDelta::seconds(1);
        assert!(store.locked_until(open).is_none());
        assert!(store.verify_at(&dvm.id, "2468", open).is_ok());
        assert!(store.verify_at(&dvm.id, "0000", open).is_err());
        assert!(store.verify_at(&dvm.id, "2468", open).is_ok());
        // The lockout file holds no PIN material.
        let raw = std::fs::read_to_string(tmp.path().join("pin-lockout.json")).unwrap();
        assert!(!raw.contains("2468") && !raw.contains("1357"));
    }
}
