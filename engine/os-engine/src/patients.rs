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
}
