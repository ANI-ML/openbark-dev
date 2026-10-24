//! The patient registry (ADR-0011): one record per animal, and every visit
//! links to its animals by id. Kept beside the encounters it indexes
//! (`<sessions>/patients.json`), encrypted like them (ADR-0008).
//!
//! Visits recorded before the registry existed carry only the names typed
//! at the time; they're matched to a record by animal and family name the
//! first time patients are listed, and the id is then written into the visit.
//!
//! The same animal created on two devices while offline becomes one record
//! when they meet ([`merge`], ADR-0017 section 9): the record with the lowest
//! UUIDv7 (the first made) keeps its id, and the other ids become its
//! aliases, so visits that link to them still find it.

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
    /// Ids of records merged into this one (ADR-0017 section 9). Links to
    /// them resolve to this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The owner's phone number in E.164 ("+15555550142"), typed by the
    /// clinic, for **Call** (ADR-0023 section 8). The practice management
    /// system stays the source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_phone: Option<String>,
}

impl PatientRecord {
    /// This record's id or one of its aliases.
    pub fn answers_to(&self, id: &str) -> bool {
        self.id == id || self.aliases.iter().any(|a| a == id)
    }

    /// When it last changed: `updated_at`, else `created_at`.
    fn changed_at(&self) -> DateTime<Local> {
        self.updated_at.unwrap_or(self.created_at)
    }

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

/// Merge patient records from several devices into one list (ADR-0017
/// section 9). Deterministic, whatever order the records come in:
/// - the same id twice: the copy changed last wins as a whole record, and
///   the aliases of both are kept;
/// - the same animal and family (`match_key`): one record with the lowest
///   id, the other ids as aliases. Its names and species are the ones changed
///   last; a species fills in from any copy that has one; `created_at` is the
///   earliest and `updated_at` the latest.
///
/// The result is sorted by id.
pub fn merge(records: impl IntoIterator<Item = PatientRecord>) -> Vec<PatientRecord> {
    use std::collections::BTreeMap;
    // 1. One copy per id.
    let mut by_id: BTreeMap<String, PatientRecord> = BTreeMap::new();
    for r in records {
        match by_id.get_mut(&r.id) {
            None => {
                by_id.insert(r.id.clone(), r);
            }
            Some(have) => {
                let aliases: Vec<String> = have.aliases.iter().chain(&r.aliases).cloned().collect();
                if (r.changed_at(), &r.name, &r.last_name, &r.species)
                    > (have.changed_at(), &have.name, &have.last_name, &have.species)
                {
                    *have = r;
                }
                have.aliases = aliases;
                have.aliases.sort();
                have.aliases.dedup();
            }
        }
    }
    // 2. One record per animal and family.
    let mut by_key: BTreeMap<String, Vec<PatientRecord>> = BTreeMap::new();
    for r in by_id.into_values() {
        by_key.entry(match_key(&r.name, &r.last_name)).or_default().push(r);
    }
    let mut out: Vec<PatientRecord> = by_key.into_values().map(merge_group).collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// One animal's records (same `match_key`) as one, sorted by id.
fn merge_group(mut group: Vec<PatientRecord>) -> PatientRecord {
    group.sort_by(|a, b| a.id.cmp(&b.id));
    let latest = group
        .iter()
        .max_by(|a, b| a.changed_at().cmp(&b.changed_at()).then_with(|| b.id.cmp(&a.id)))
        .cloned()
        .expect("a group has a record");
    let mut keep = group[0].clone();
    keep.name = latest.name.clone();
    keep.last_name = latest.last_name.clone();
    keep.species = if latest.species.is_empty() {
        group.iter().find(|r| !r.species.is_empty()).map(|r| r.species.clone()).unwrap_or_default()
    } else {
        latest.species.clone()
    };
    // The owner's number (ADR-0023 section 8): the latest copy's, else any copy's.
    keep.owner_phone =
        latest.owner_phone.clone().or_else(|| group.iter().find_map(|r| r.owner_phone.clone()));
    keep.created_at = group.iter().map(|r| r.created_at).min().expect("a group has a record");
    keep.updated_at = group.iter().filter_map(|r| r.updated_at).max();
    let mut aliases: Vec<String> = group
        .iter()
        .flat_map(|r| std::iter::once(r.id.clone()).chain(r.aliases.iter().cloned()))
        .filter(|id| *id != keep.id)
        .collect();
    aliases.sort();
    aliases.dedup();
    keep.aliases = aliases;
    keep
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

    /// The record with this id, or the one it was merged into.
    pub fn get(&self, id: &str) -> Option<PatientRecord> {
        let all = self.list().ok()?;
        all.iter().find(|p| p.id == id).or_else(|| all.iter().find(|p| p.answers_to(id))).cloned()
    }

    /// Merge records from another device into this registry ([`merge`]) and
    /// save it. Returns the registry as saved.
    pub fn merge_in(&self, incoming: Vec<PatientRecord>) -> Result<Vec<PatientRecord>, EngineError> {
        let merged = merge(self.list()?.into_iter().chain(incoming));
        self.save(&merged)?;
        Ok(merged)
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
            aliases: Vec::new(),
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

    fn rec(id: &str, name: &str, last: &str, species: &str, minute: u32) -> PatientRecord {
        use chrono::TimeZone;
        PatientRecord {
            id: id.into(),
            name: name.into(),
            last_name: last.into(),
            species: species.into(),
            created_at: Local.with_ymd_and_hms(2026, 10, 19, 9, minute, 0).unwrap(),
            updated_at: None,
            aliases: Vec::new(),
            owner_phone: None,
        }
    }

    /// Golden: two devices create the same animal offline, and a different
    /// one, then meet.
    #[test]
    fn offline_duplicates_merge_into_the_first_record() {
        // UUIDv7 order: the phone's record was made first.
        let phone = rec("0199f000-0000-7000-8000-000000000001", "Maple", "Okonkwo", "", 5);
        let mut pc = rec("0199f000-0000-7000-8000-000000000002", " maple", "OKONKWO", "canine", 7);
        pc.owner_phone = Some("+15555550142".into());
        let other = rec("0199f000-0000-7000-8000-000000000003", "Maple", "Haddad", "feline", 8);
        let merged = merge([pc.clone(), other.clone(), phone.clone()]);
        assert_eq!(merged.len(), 2);
        let maple = &merged[0];
        assert_eq!(maple.id, phone.id, "the lowest id keeps the record");
        assert_eq!(maple.aliases, [pc.id.as_str()]);
        assert_eq!(maple.species, "canine", "a species fills in from the other copy");
        assert_eq!(maple.owner_phone.as_deref(), Some("+15555550142"), "so does the owner's number");
        assert_eq!(
            (maple.name.as_str(), maple.last_name.as_str()),
            (" maple", "OKONKWO"),
            "names changed last win"
        );
        assert_eq!(maple.created_at, phone.created_at);
        assert!(maple.answers_to(&pc.id) && maple.answers_to(&phone.id));
        assert_eq!(merged[1], other);
        // The same result whatever the order, and merging again changes nothing.
        assert_eq!(merge([phone.clone(), other.clone(), pc.clone()]), merged);
        assert_eq!(merge(merged.clone()), merged);
        assert_eq!(merge(merged.iter().cloned().chain([pc.clone(), phone.clone()])), merged);
    }

    #[test]
    fn the_same_record_changed_on_two_devices_keeps_the_later_change_and_every_alias() {
        use chrono::TimeZone;
        let mut a = rec("0199f000-0000-7000-8000-00000000000a", "Biscuit", "Nakamura", "", 1);
        a.aliases = vec!["old-1".into()];
        let mut b = a.clone();
        b.aliases = vec!["old-2".into()];
        b.species = "lagomorph".into();
        b.updated_at = Some(Local.with_ymd_and_hms(2026, 10, 19, 10, 0, 0).unwrap());
        let merged = merge([a.clone(), b.clone()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].species, "lagomorph");
        assert_eq!(merged[0].aliases, ["old-1", "old-2"]);
        assert_eq!(merge([b, a]), merged);
    }

    #[test]
    fn a_merged_registry_still_finds_visits_by_their_old_ids() {
        crate::vault::tests::with_key(|| {
            let dir = tempfile::tempdir().unwrap();
            let store = PatientStore::for_sessions(dir.path());
            let mine = store.create("Pepper", "Silva", "").unwrap();
            // From another device, made offline a moment later.
            let theirs = PatientRecord { id: uuid::Uuid::now_v7().to_string(), ..mine.clone() };
            let saved = store.merge_in(vec![theirs.clone()]).unwrap();
            assert_eq!(saved.len(), 1);
            assert_eq!(store.get(&theirs.id).unwrap().id, mine.id, "the alias resolves");
            assert_eq!(store.get(&mine.id).unwrap().aliases, [theirs.id.as_str()]);
            // Registries from before aliases still load, and write back the same.
            let old = r#"[{"id":"x","name":"Old","created_at":"2026-01-01T00:00:00+00:00"}]"#;
            let parsed: Vec<PatientRecord> = serde_json::from_str(old).unwrap();
            assert!(parsed[0].aliases.is_empty());
            assert!(!serde_json::to_string(&parsed[0]).unwrap().contains("aliases"));
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
