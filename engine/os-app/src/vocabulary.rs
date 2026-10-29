//! The clinic's own words and drug names (Settings, Words and drug names;
//! ADR-0031). They live in one file on this computer
//! (`os_engine::vocab::user_file`), read at the start of each recording, so a
//! change applies from the next recording. The file is the hospital's: in a
//! team, changing it needs an admin's PIN, as the hospital's templates do
//! (ADR-0024).

use std::path::{Path, PathBuf};

use os_engine::vocab::{self, ClinicTerm, Lexicon};
use serde::Serialize;

use crate::templates::Approver;
use crate::{App, AppError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct VocabularyView {
    /// The clinic's terms, in file order.
    pub terms: Vec<ClinicTerm>,
    /// Terms that come with OpenBark: written for it, and from public drug registers.
    pub builtin: usize,
    pub regulatory: usize,
    /// Where the clinic's file is.
    pub file: String,
    /// Changing the list needs an admin's PIN (a team).
    pub needs_admin: bool,
}

impl App {
    pub fn vocabulary(&self) -> Result<VocabularyView> {
        self.vocabulary_at(&clinic_file()?)
    }

    pub fn save_vocabulary_term(
        &self,
        term: &ClinicTerm,
        by: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<VocabularyView> {
        let path = clinic_file()?;
        self.need_vocabulary_admin(by, approver)?;
        vocab::save_clinic_term(&path, term).map_err(AppError)?;
        self.vocabulary_at(&path)
    }

    pub fn remove_vocabulary_term(
        &self,
        term: &str,
        by: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<VocabularyView> {
        let path = clinic_file()?;
        self.need_vocabulary_admin(by, approver)?;
        vocab::remove_clinic_term(&path, term).map_err(AppError)?;
        self.vocabulary_at(&path)
    }

    /// Working alone, anyone; in a team, a member chosen and an admin's PIN.
    pub(crate) fn need_vocabulary_admin(&self, by: Option<&str>, approver: Option<&Approver>) -> Result<()> {
        let ctx = self.writer(by)?;
        self.need_admin(&ctx, approver)?;
        Ok(())
    }

    pub(crate) fn vocabulary_at(&self, path: &Path) -> Result<VocabularyView> {
        let terms = vocab::read_clinic_terms(path);
        let (hand, regulatory) = Lexicon::load(None).counts();
        Ok(VocabularyView {
            builtin: hand,
            regulatory,
            terms,
            file: path.display().to_string(),
            needs_admin: self.users.is_team(),
        })
    }
}

fn clinic_file() -> Result<PathBuf> {
    vocab::user_file().ok_or_else(|| AppError("This computer has no folder for OpenBark's data.".into()))
}
