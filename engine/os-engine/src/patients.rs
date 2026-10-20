//! The patient registry (ADR-0011): one record per animal, and every visit
//! links to its animals by id. Kept beside the encounters it indexes
//! (`<sessions>/patients.json`), encrypted like them (ADR-0008).
//!
//! Visits recorded before the registry existed carry only the names typed
//! at the time; they're matched to a record by animal and family name the
//! first time patients are listed, and the id is then written into the visit.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::EngineError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatientRecord {
    pub id: String,
    /// The animal's name.
    pub name: String,
    /// The family or client surname.
    #[serde(default)]
    pub last_name: String,
    /// Species as entered ("feline"); empty when not entered.
    #[serde(default)]
    pub species: String,
    pub created_at: DateTime<Local>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Local>>,
    /// The owner's phone number in E.164 ("+15555550142"), typed by the
    /// clinic, for **Call** (ADR-0023 section 8). The practice management
    /// system stays the source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_phone: Option<String>,
}

impl PatientRecord {
    /// "Maple Okonkwo".
    pub fn display_name(&self) -> String {
        format!("{} {}", self.name, self.last_name).trim().to_string()
    }
}

/// How two spellings of the same animal compare: case, spacing and accents
/// on the outside don't matter.
pub fn match_key(name: &str, last_name: &str) -> String {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    format!("{}|{}", norm(name), norm(last_name))
}

#[derive(Debug, Clone)]
pub struct PatientStore {
    path: PathBuf,
}

fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Session(format!("patients: {e}"))
}

impl PatientStore {
    /// The registry for an encounters folder.
    pub fn for_sessions(root: &Path) -> Self {
        Self { path: root.join("patients.json") }
    }

    pub fn list(&self) -> Result<Vec<PatientRecord>, EngineError> {
        match crate::vault::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(err),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(err(e)),
        }
    }

    fn save(&self, patients: &[PatientRecord]) -> Result<(), EngineError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let text = serde_json::to_string_pretty(patients).expect("patients serialise");
        crate::vault::write(&self.path, text.as_bytes()).map_err(err)
    }

    pub fn get(&self, id: &str) -> Option<PatientRecord> {
        self.list().ok()?.into_iter().find(|p| p.id == id)
    }

    /// A new patient. Names are trimmed; an animal needs a name.
    pub fn create(&self, name: &str, last_name: &str, species: &str) -> Result<PatientRecord, EngineError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EngineError::Session("Enter the animal's name.".into()));
        }
        let mut all = self.list()?;
        let record = PatientRecord {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            last_name: last_name.trim().to_string(),
            species: species.trim().to_string(),
            created_at: Local::now(),
            updated_at: None,
            owner_phone: None,
        };
        all.push(record.clone());
        self.save(&all)?;
        Ok(record)
    }

    /// Change a patient's details. Visits keep the names typed at the time.
    pub fn update(
        &self,
        id: &str,
        name: &str,
        last_name: &str,
        species: &str,
    ) -> Result<PatientRecord, EngineError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EngineError::Session("Enter the animal's name.".into()));
        }
        let mut all = self.list()?;
        let record = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| EngineError::Session("That patient isn't in this computer's records.".into()))?;
        record.name = name.to_string();
        record.last_name = last_name.trim().to_string();
        record.species = species.trim().to_string();
        record.updated_at = Some(Local::now());
        let out = record.clone();
        self.save(&all)?;
        Ok(out)
    }

    /// Set or clear the owner's phone number. Empty clears it; anything else
    /// must read as a full number ([`normalise_phone`]).
    pub fn set_owner_phone(&self, id: &str, phone: &str) -> Result<PatientRecord, EngineError> {
        let phone = if phone.trim().is_empty() { None } else { Some(normalise_phone(phone)?) };
        let mut all = self.list()?;
        let record = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| EngineError::Session("That patient isn't in this computer's records.".into()))?;
        if record.owner_phone != phone {
            record.owner_phone = phone;
            record.updated_at = Some(Local::now());
        }
        let out = record.clone();
        self.save(&all)?;
        Ok(out)
    }

    /// The record for this animal and family, created if there's none yet.
    /// A species fills in a record that had none.
    pub fn find_or_create(
        &self,
        name: &str,
        last_name: &str,
        species: &str,
    ) -> Result<PatientRecord, EngineError> {
        let key = match_key(name, last_name);
        let mut all = self.list()?;
        if let Some(found) = all.iter_mut().find(|p| match_key(&p.name, &p.last_name) == key) {
            if found.species.is_empty() && !species.trim().is_empty() {
                found.species = species.trim().to_string();
                let out = found.clone();
                self.save(&all)?;
                return Ok(out);
            }
            return Ok(found.clone());
        }
        drop(all);
        self.create(name, last_name, species)
    }
}

/// A phone number as typed, in E.164: "+1 (555) 555-0142" and
/// "555-555-0142" and "15555550142" all become "+15555550142". Ten digits
/// without a country code are taken as North American (+1); any other number
/// needs its "+" and country code.
pub fn normalise_phone(typed: &str) -> Result<String, EngineError> {
    let bad = || {
        EngineError::Session(format!(
            "{} isn't a full phone number. Write it with + and the country code, for example +15555550142.",
            typed.trim()
        ))
    };
    let trimmed = typed.trim();
    if trimmed.chars().any(|c| !(c.is_ascii_digit() || " +-().".contains(c))) {
        return Err(bad());
    }
    let plus = trimmed.starts_with('+');
    if trimmed[usize::from(plus)..].contains('+') {
        return Err(bad());
    }
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    let e164 = if plus {
        format!("+{digits}")
    } else if digits.len() == 10 {
        format!("+1{digits}")
    } else if digits.len() == 11 && digits.starts_with('1') {
        format!("+{digits}")
    } else {
        return Err(bad());
    };
    let rest = &e164[1..];
    if !(8..=15).contains(&rest.len()) || rest.starts_with('0') {
        return Err(bad());
    }
    Ok(e164)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_found_by_name_and_family_whatever_the_spelling() {
        crate::vault::tests::with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let store = PatientStore::for_sessions(dir.path());
            let maple = store.find_or_create("Maple", "Okonkwo", "").unwrap();
            let again = store.find_or_create("  maple ", "OKONKWO", "canine").unwrap();
            assert_eq!(again.id, maple.id);
            assert_eq!(again.species, "canine", "a species fills in a record that had none");
            let other = store.find_or_create("Maple", "Haddad", "").unwrap();
            assert_ne!(other.id, maple.id, "same name, different family: a different animal");
            assert_eq!(store.list().unwrap().len(), 2);
            // Encrypted on disk, like the encounters.
            assert!(crate::vault::is_encrypted(&dir.path().join("patients.json")));
            let renamed = store.update(&maple.id, "Maple", "Okonkwo-Reid", "canine").unwrap();
            assert_eq!(store.get(&maple.id).unwrap().last_name, "Okonkwo-Reid");
            assert!(renamed.updated_at.is_some());
            assert!(store.create("  ", "x", "").is_err());
        });
    }

    #[test]
    fn owner_phone_numbers_are_kept_in_full_and_encrypted() {
        for typed in ["+1 (555) 555-0142", "555-555-0142", "15555550142", " 555.555.0142 "] {
            assert_eq!(normalise_phone(typed).unwrap(), "+15555550142", "{typed}");
        }
        assert_eq!(normalise_phone("+44 20 7946 0958").unwrap(), "+442079460958");
        for bad in ["555-0142", "+0 555 555 0142", "call me", "+1 555+555", "4420 7946 0958 1234 5"] {
            assert!(normalise_phone(bad).is_err(), "{bad}");
        }
        crate::vault::tests::with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let store = PatientStore::for_sessions(dir.path());
            let bella = store.create("Bella", "Patel", "canine").unwrap();
            assert_eq!(bella.owner_phone, None);
            let with = store.set_owner_phone(&bella.id, "(555) 555-0142").unwrap();
            assert_eq!(with.owner_phone.as_deref(), Some("+15555550142"));
            assert_eq!(store.get(&bella.id).unwrap().owner_phone.as_deref(), Some("+15555550142"));
            let raw = std::fs::read(dir.path().join("patients.json")).unwrap();
            assert!(!raw.windows(11).any(|w| w == b"15555550142"), "never on disk in plain text");
            assert!(store.set_owner_phone(&bella.id, "555").is_err());
            assert_eq!(store.set_owner_phone(&bella.id, " ").unwrap().owner_phone, None, "empty clears it");
            // Changing the name keeps the number.
            store.set_owner_phone(&bella.id, "5555550142").unwrap();
            store.update(&bella.id, "Bella", "Patel-Ruiz", "canine").unwrap();
            assert_eq!(store.get(&bella.id).unwrap().owner_phone.as_deref(), Some("+15555550142"));
        });
    }
}
