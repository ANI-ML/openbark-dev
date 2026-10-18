//! Deleted visits (ADR-0022 section 3): an in-app trash inside the visits
//! folder, `<visits>/.openbark/deleted/<trash id>/`.
//!
//! Deleting a visit renames its folder in here (one atomic rename on the same
//! volume, so the patient's name leaves the folder name at once) and writes an
//! encrypted `deleted.json` beside `session.json`: the original folder name,
//! who deleted it, when and why, whether it was signed, when it will be
//! removed for good, and the id of its deletion record entry. The trash id is
//! the visit's `meta.id`, never the folder name.
//!
//! Removal for good renames the item to `<trash id>.purging` first, so a
//! crash part-way through never leaves a half-deleted visit that looks whole;
//! [`Trash::recover`] finishes the job at the next start.
//!
//! Renames and removals retry briefly when another program holds a file open
//! (antivirus, an Explorer window), then fail with a plain message.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Local, TimeDelta};
use serde::{Deserialize, Serialize};

use crate::EngineError;
use crate::audit::{DeleteReason, ReasonKind};
use crate::session::{SESSION_FILE, Session};
use crate::users::Who;

/// Written beside `session.json` in a deleted visit.
pub const DELETED_FILE: &str = "deleted.json";
/// How long Deleted visits keeps a visit before removing it for good.
pub const KEEP_DAYS: i64 = 30;
const PURGING: &str = ".purging";

pub const FILE_IN_USE: &str =
    "Another program is using a file in this visit. Close any window showing it, then try again.";
pub const STALE: &str =
    "That deleted visit is no longer there. It may have been restored or removed for good.";

/// What `deleted.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletedInfo {
    pub trash_id: String,
    /// The visit's folder name before it was deleted (it holds the patient's
    /// name, so it's only kept here, encrypted).
    pub original_name: String,
    pub deleted_at: DateTime<Local>,
    pub by: Who,
    pub reason: DeleteReason,
    /// Any note in the visit was signed, for any animal.
    pub signed: bool,
    pub purge_after: DateTime<Local>,
    /// The `visit_deleted` entry in the deletion record.
    pub event_id: String,
    /// Written at start after OpenBark closed mid-delete.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recovered: bool,
}

/// What [`Trash::recover`] put right.
#[derive(Debug)]
pub enum Recovered {
    /// A deleted visit had no `deleted.json` (OpenBark closed mid-delete): it
    /// got one, with a fresh 30 days. Its deletion record entry is missing.
    Described { info: DeletedInfo, session: Option<Session> },
    /// A visit left part-removed (`<trash id>.purging`) was removed. What it
    /// held is passed on so the removal can be recorded if it wasn't.
    Purged { trash_id: String, info: Option<DeletedInfo>, session: Option<Session> },
}

/// A trash id: lowercase hex and hyphens (a UUID, perhaps with a `-N` suffix).
pub fn valid_trash_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.starts_with(|c: char| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f' | '-'))
}

/// A visit folder name that can be used as is (session ids, `session_dir`).
fn valid_folder_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && s.len() <= 120
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Another program holds a file open (Windows sharing or lock violations).
fn in_use(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33))
}

/// Run a rename or removal, retrying three times 200 ms apart while another
/// program holds a file, then failing with [`FILE_IN_USE`].
fn retrying<T>(path: &Path, mut f: impl FnMut() -> io::Result<T>) -> Result<T, EngineError> {
    let mut attempt = 0;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if in_use(&e) && attempt < 3 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) if in_use(&e) => {
                // Not the path: a visit folder's name holds the patient's name.
                tracing::warn!(error = %e, "a file in a visit is in use");
                return Err(EngineError::Session(FILE_IN_USE.into()));
            }
            Err(e) => return Err(EngineError::Session(format!("{}: {e}", path.display()))),
        }
    }
}

fn rename_retrying(from: &Path, to: &Path) -> Result<(), EngineError> {
    retrying(from, || fs::rename(from, to))
}

/// Remove a file, retrying while another program holds it.
pub fn remove_file_retrying(path: &Path) -> Result<(), EngineError> {
    retrying(path, || fs::remove_file(path))
}

fn remove_dir_retrying(path: &Path) -> Result<(), EngineError> {
    retrying(path, || match fs::remove_dir_all(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    })
}

fn read_info(dir: &Path) -> Result<DeletedInfo, EngineError> {
    let path = dir.join(DELETED_FILE);
    let text = crate::vault::read_to_string(&path)
        .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))
}

fn write_info(dir: &Path, info: &DeletedInfo) -> Result<(), EngineError> {
    let path = dir.join(DELETED_FILE);
    let text = serde_json::to_string_pretty(info).expect("deleted info serialises");
    crate::vault::write(&path, text.as_bytes())
        .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))
}

/// When a visit deleted at `at` is removed for good.
pub fn purge_after(at: DateTime<Local>) -> DateTime<Local> {
    at + TimeDelta::days(KEEP_DAYS)
}

/// The Deleted visits of one visits folder.
#[derive(Debug, Clone)]
pub struct Trash {
    root: PathBuf,
}

impl Trash {
    /// `root` is the visits folder.
    pub fn for_sessions(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    /// `<visits>/.openbark/deleted`.
    pub fn dir(&self) -> PathBuf {
        crate::deleted_dir(&self.root)
    }

    /// Bytes Deleted visits take (Settings, Storage).
    pub fn bytes(&self) -> u64 {
        os_models::locations::dir_size(&self.dir())
    }

    fn taken(&self, id: &str) -> bool {
        let dir = self.dir();
        dir.join(id).exists() || dir.join(format!("{id}{PURGING}")).exists()
    }

    /// A free trash id for a visit: its `meta.id` when that's a UUID
    /// (otherwise a new one), with `-1`, `-2`... if the name is taken.
    pub fn new_id(&self, meta_id: &str) -> String {
        let base = uuid::Uuid::parse_str(meta_id)
            .map(|u| u.hyphenated().to_string())
            .unwrap_or_else(|_| uuid::Uuid::now_v7().to_string());
        (0..)
            .map(|n| if n == 0 { base.clone() } else { format!("{base}-{n}") })
            .find(|id| !self.taken(id))
            .expect("a free name")
    }

    fn item(&self, trash_id: &str) -> Result<PathBuf, EngineError> {
        if !valid_trash_id(trash_id) {
            return Err(EngineError::Session(format!("'{trash_id}' is not a deleted visit.")));
        }
        let dir = self.dir().join(trash_id);
        if dir.join(SESSION_FILE).is_file() { Ok(dir) } else { Err(EngineError::Session(STALE.into())) }
    }

    /// Move a visit into Deleted visits: rename its folder, then write
    /// `deleted.json`. If that can't be written the rename is undone.
    pub fn put(&self, visit_dir: &Path, info: &DeletedInfo) -> Result<PathBuf, EngineError> {
        if !valid_trash_id(&info.trash_id) {
            return Err(EngineError::Session(format!("'{}' is not a trash id.", info.trash_id)));
        }
        let deleted = self.dir();
        fs::create_dir_all(&deleted)
            .map_err(|e| EngineError::Session(format!("{}: {e}", deleted.display())))?;
        let target = deleted.join(&info.trash_id);
        if self.taken(&info.trash_id) {
            return Err(EngineError::Session(format!("{} is already in Deleted visits.", info.trash_id)));
        }
        rename_retrying(visit_dir, &target)?;
        if let Err(e) = write_info(&target, info) {
            let _ = fs::remove_file(target.join(format!("{DELETED_FILE}.tmp")));
            if let Err(back) = rename_retrying(&target, visit_dir) {
                tracing::error!(error = %back, "a visit couldn't be moved back after a failed delete");
            }
            return Err(e);
        }
        Ok(target)
    }

    /// Undo [`Trash::put`] (the deletion couldn't be recorded).
    pub fn undo_put(&self, trashed: &Path, original: &Path) -> Result<(), EngineError> {
        match fs::remove_file(trashed.join(DELETED_FILE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                return Err(EngineError::Session(format!("{}: {e}", trashed.display())));
            }
            _ => {}
        }
        rename_retrying(trashed, original)
    }

    /// Deleted visits, newest first. Items still being removed, or without
    /// `deleted.json` (until [`Trash::recover`] runs), aren't listed.
    pub fn list(&self) -> Vec<(DeletedInfo, Session)> {
        let mut out: Vec<(DeletedInfo, Session)> = fs::read_dir(self.dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| valid_trash_id(&e.file_name().to_string_lossy()))
            .filter_map(|e| {
                let dir = e.path();
                let info = read_info(&dir).ok()?;
                let session = Session::open(&dir).ok()?;
                Some((info, session))
            })
            .collect();
        out.sort_by_key(|(info, _)| std::cmp::Reverse(info.deleted_at));
        out
    }

    /// One deleted visit, with its folder.
    pub fn get(&self, trash_id: &str) -> Result<(DeletedInfo, Session), EngineError> {
        let dir = self.item(trash_id)?;
        let info = read_info(&dir)?;
        Ok((info, Session::open(&dir)?))
    }

    /// The deleted visit that had this folder name, if it's still here.
    pub fn find_by_original(&self, name: &str) -> Option<DeletedInfo> {
        self.list().into_iter().map(|(info, _)| info).find(|info| info.original_name == name)
    }

    /// Put a deleted visit back under its original folder name (with a
    /// suffix if that's taken) and remove `deleted.json`. Returns the folder.
    pub fn restore(&self, trash_id: &str) -> Result<PathBuf, EngineError> {
        let dir = self.item(trash_id)?;
        let info = read_info(&dir)?;
        let base = if valid_folder_name(&info.original_name) {
            info.original_name.clone()
        } else {
            folder_name_for(&Session::open(&dir)?)
        };
        let target = (0..)
            .map(|n| if n == 0 { base.clone() } else { format!("{base}-{n}") })
            .map(|name| self.root.join(name))
            .find(|p| !p.exists())
            .expect("a free name");
        rename_retrying(&dir, &target)?;
        if let Err(e) = fs::remove_file(target.join(DELETED_FILE)) {
            tracing::warn!(error = %e, "deleted.json stayed in a restored visit");
        }
        Ok(target)
    }

    /// Start removing a deleted visit for good: rename it to `<id>.purging`.
    pub fn begin_purge(&self, trash_id: &str) -> Result<PathBuf, EngineError> {
        let dir = self.item(trash_id)?;
        let purging = self.dir().join(format!("{trash_id}{PURGING}"));
        if purging.exists() {
            return Err(EngineError::Session("This visit is being deleted.".into()));
        }
        rename_retrying(&dir, &purging)?;
        Ok(purging)
    }

    /// Undo [`Trash::begin_purge`] (the removal couldn't be recorded).
    pub fn undo_purge(&self, purging: &Path) -> Result<(), EngineError> {
        let name = purging.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let id = name.strip_suffix(PURGING).unwrap_or(&name);
        rename_retrying(purging, &self.dir().join(id))
    }

    /// Remove a `.purging` folder and everything in it.
    pub fn finish_purge(&self, purging: &Path) -> Result<(), EngineError> {
        remove_dir_retrying(purging)
    }

    /// Items whose 30 days are up.
    pub fn due(&self, now: DateTime<Local>) -> Vec<String> {
        self.list()
            .into_iter()
            .filter(|(info, _)| info.purge_after <= now)
            .map(|(info, _)| info.trash_id)
            .collect()
    }

    /// Put right what a crash left: an item without `deleted.json` gets one
    /// with a fresh 30 days, and a leftover `.purging` folder is removed.
    pub fn recover(&self, now: DateTime<Local>) -> Vec<Recovered> {
        let mut out = Vec::new();
        for entry in fs::read_dir(self.dir()).into_iter().flatten().flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(PURGING) {
                if !valid_trash_id(id) {
                    continue;
                }
                let info = read_info(&dir).ok();
                let session = Session::open(&dir).ok();
                match remove_dir_retrying(&dir) {
                    Ok(()) => out.push(Recovered::Purged { trash_id: id.to_string(), info, session }),
                    Err(e) => tracing::warn!(error = %e, "a part-removed visit couldn't be removed yet"),
                }
                continue;
            }
            if !valid_trash_id(&name) || dir.join(DELETED_FILE).exists() {
                continue;
            }
            let session = Session::open(&dir).ok();
            let info = DeletedInfo {
                trash_id: name.clone(),
                original_name: session
                    .as_ref()
                    .map(folder_name_for)
                    .unwrap_or_else(|| "recovered-visit".into()),
                deleted_at: now,
                by: Who::named(crate::PRODUCT_NAME),
                reason: DeleteReason { kind: ReasonKind::Other, note: String::new() },
                signed: session.as_ref().is_some_and(Session::any_signed),
                purge_after: purge_after(now),
                event_id: crate::audit::new_id(),
                recovered: true,
            };
            match write_info(&dir, &info) {
                Ok(()) => out.push(Recovered::Described { info, session }),
                Err(e) => tracing::warn!(error = %e, "a deleted visit couldn't be described"),
            }
        }
        out
    }
}

/// A folder name like the one OpenBark gives a new visit, for a deleted visit
/// whose original name was lost: "2026-10-17_143205_maple-okonkwo".
fn folder_name_for(s: &Session) -> String {
    let who = s.meta.patient.as_ref().map(|p| crate::session::slug(&p.display_name())).unwrap_or_default();
    format!(
        "{}_{}",
        s.meta.created_at.format("%Y-%m-%d_%H%M%S"),
        if who.is_empty() { "recording".to_string() } else { who }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::PatientInfo;
    use crate::transcript::{Segment, Transcript};
    use crate::vault;

    fn visit(root: &Path) -> Session {
        let mut s = Session::create_live(root, "Maple Okonkwo").unwrap();
        s.set_patient(PatientInfo {
            name: "Maple".into(),
            last_name: "Okonkwo".into(),
            reason: "Limping".into(),
            ..Default::default()
        })
        .unwrap();
        let t = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "Right hind lameness".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        s.save_transcript(&t, "parakeet", false, &Default::default()).unwrap();
        s
    }

    fn info(trash: &Trash, s: &Session, now: DateTime<Local>) -> DeletedInfo {
        DeletedInfo {
            trash_id: trash.new_id(&s.meta.id),
            original_name: s.dir.file_name().unwrap().to_string_lossy().into_owned(),
            deleted_at: now,
            by: Who::named("Sam"),
            reason: DeleteReason { kind: ReasonKind::Mistake, note: String::new() },
            signed: false,
            purge_after: purge_after(now),
            event_id: crate::audit::new_id(),
            recovered: false,
        }
    }

    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.path().is_dir() { files(&e.path(), out) } else { out.push(e.path()) }
        }
    }

    #[test]
    fn put_list_restore_round_trip() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let s = visit(&root);
            let name = s.dir.file_name().unwrap().to_string_lossy().into_owned();
            let trash = Trash::for_sessions(&root);
            let now = Local::now();
            let i = info(&trash, &s, now);
            assert_eq!(i.trash_id, s.meta.id, "the trash id is the visit's meta.id");
            let trashed = trash.put(&s.dir, &i).unwrap();
            assert!(!s.dir.exists());
            assert!(crate::session::list(&root).is_empty(), "deleted visits never list as visits");
            // The patient's name left the folder name, and nothing in the trash holds it in the clear.
            assert!(!trashed.to_string_lossy().to_lowercase().contains("maple"));
            let mut all = Vec::new();
            files(&trash.dir(), &mut all);
            assert!(all.len() >= 3, "{all:?}");
            for f in &all {
                let raw = fs::read(f).unwrap();
                assert!(!raw.windows(5).any(|w| w == b"Maple"), "{} holds the name", f.display());
                assert!(!raw.windows(7).any(|w| w == b"Okonkwo"), "{} holds the name", f.display());
            }
            let listed = trash.list();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].0, i);
            assert_eq!(trash.find_by_original(&name).unwrap().trash_id, i.trash_id);
            assert_eq!(trash.get(&i.trash_id).unwrap().0, i);

            let back = trash.restore(&i.trash_id).unwrap();
            assert_eq!(back.file_name().unwrap().to_string_lossy(), name);
            assert!(!back.join(DELETED_FILE).exists());
            let reopened = Session::open(&back).unwrap();
            assert_eq!(reopened.meta.id, s.meta.id);
            assert_eq!(reopened.load_transcript().unwrap().segments[0].text, "Right hind lameness");
            assert!(trash.list().is_empty());
            assert_eq!(trash.restore(&i.trash_id).unwrap_err().to_string(), STALE);
        });
    }

    #[test]
    fn restore_picks_a_free_name() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let s = visit(&root);
            let trash = Trash::for_sessions(&root);
            let i = info(&trash, &s, Local::now());
            trash.put(&s.dir, &i).unwrap();
            fs::create_dir_all(&s.dir).unwrap(); // something new took the name meanwhile
            let back = trash.restore(&i.trash_id).unwrap();
            assert_eq!(back.file_name().unwrap().to_string_lossy(), format!("{}-1", i.original_name));
        });
    }

    #[test]
    fn a_failed_description_moves_the_visit_back() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let s = visit(&root);
            let trash = Trash::for_sessions(&root);
            let mut i = info(&trash, &s, Local::now());
            i.trash_id = "../x".into();
            assert!(trash.put(&s.dir, &i).is_err());
            assert!(s.dir.join(SESSION_FILE).is_file(), "the visit wasn't touched");
            let i = info(&trash, &s, Local::now());
            let trashed = trash.put(&s.dir, &i).unwrap();
            trash.undo_put(&trashed, &s.dir).unwrap();
            assert!(s.dir.join(SESSION_FILE).is_file() && !s.dir.join(DELETED_FILE).exists());
        });
    }

    #[test]
    fn purging_removes_everything() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let s = visit(&root);
            let trash = Trash::for_sessions(&root);
            let i = info(&trash, &s, Local::now());
            trash.put(&s.dir, &i).unwrap();
            let purging = trash.begin_purge(&i.trash_id).unwrap();
            assert!(trash.list().is_empty(), "an item being removed isn't listed");
            trash.undo_purge(&purging).unwrap();
            assert_eq!(trash.list().len(), 1);
            let purging = trash.begin_purge(&i.trash_id).unwrap();
            trash.finish_purge(&purging).unwrap();
            assert_eq!(fs::read_dir(trash.dir()).unwrap().count(), 0, "nothing left in Deleted visits");
            assert_eq!(trash.begin_purge(&i.trash_id).unwrap_err().to_string(), STALE);
        });
    }

    #[test]
    fn recovery_describes_half_deleted_visits_and_finishes_removals() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let trash = Trash::for_sessions(&root);
            // A crash between the rename and deleted.json.
            let a = visit(&root);
            fs::create_dir_all(trash.dir()).unwrap();
            fs::rename(&a.dir, trash.dir().join(&a.meta.id)).unwrap();
            // A crash during removal.
            let b = visit(&root);
            let bi = info(&trash, &b, Local::now());
            trash.put(&b.dir, &bi).unwrap();
            fs::rename(trash.dir().join(&bi.trash_id), trash.dir().join(format!("{}.purging", bi.trash_id)))
                .unwrap();
            let now = Local::now();
            let recovered = trash.recover(now);
            assert_eq!(recovered.len(), 2, "{recovered:?}");
            let described = recovered
                .iter()
                .find_map(|r| match r {
                    Recovered::Described { info, .. } => Some(info.clone()),
                    _ => None,
                })
                .unwrap();
            assert_eq!(described.trash_id, a.meta.id);
            assert_eq!(described.purge_after, purge_after(now), "a fresh 30 days");
            assert!(described.recovered);
            assert!(described.original_name.ends_with("_maple-okonkwo"), "{}", described.original_name);
            assert!(recovered.iter().any(
                |r| matches!(r, Recovered::Purged { trash_id, info: Some(_), .. } if *trash_id == bi.trash_id)
            ));
            assert!(!trash.dir().join(format!("{}.purging", bi.trash_id)).exists());
            assert_eq!(trash.list().len(), 1);
            assert!(trash.recover(now).is_empty(), "nothing more to do");
        });
    }

    #[test]
    fn due_after_thirty_days() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let s = visit(&root);
            let trash = Trash::for_sessions(&root);
            let now = Local::now();
            let i = info(&trash, &s, now);
            trash.put(&s.dir, &i).unwrap();
            assert_eq!(trash.due(now + TimeDelta::days(31)), std::slice::from_ref(&i.trash_id));
            assert!(trash.due(now + TimeDelta::days(29)).is_empty());
        });
    }

    /// A directory link: a junction on Windows (no admin rights needed), a
    /// symlink elsewhere.
    fn link_dir(target: &Path, link: &Path) {
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    /// ADR-0022 section 1: nothing outside OpenBark's own folders is ever
    /// deleted. A link inside a visit (or a visit folder that is itself a
    /// link) is removed as a link; what it points to stays.
    #[test]
    fn removing_for_good_never_follows_a_link_out_of_the_visit() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let models = tmp.path().join("models");
            fs::create_dir_all(&models).unwrap();
            fs::write(models.join("model.gguf"), b"weights").unwrap();
            let trash = Trash::for_sessions(&root);

            // A link inside the visit, to the models folder.
            let s = visit(&root);
            link_dir(&models, &s.dir.join("runs").join("linked"));
            let i = info(&trash, &s, Local::now());
            trash.put(&s.dir, &i).unwrap();
            assert!(trash.bytes() < 1_000_000, "the link isn't counted as Deleted visits' space");
            let purging = trash.begin_purge(&i.trash_id).unwrap();
            trash.finish_purge(&purging).unwrap();
            assert!(!purging.exists());
            assert_eq!(
                fs::read(models.join("model.gguf")).unwrap(),
                b"weights",
                "the models folder survives"
            );

            // A visit folder that is a link to a folder kept elsewhere.
            let elsewhere = tmp.path().join("elsewhere");
            let real = visit(&elsewhere);
            let linked = root.join(real.dir.file_name().unwrap());
            link_dir(&real.dir, &linked);
            let shown = Session::open(&linked).unwrap();
            let i = info(&trash, &shown, Local::now());
            trash.put(&linked, &i).unwrap();
            let purging = trash.begin_purge(&i.trash_id).unwrap();
            trash.finish_purge(&purging).unwrap();
            assert!(!purging.exists() && !linked.exists());
            assert!(real.dir.join(SESSION_FILE).is_file(), "the folder it pointed to is untouched");

            // The same for a removal left part-done by a crash.
            let again = root.join("again");
            link_dir(&real.dir, &again);
            let id = "0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b";
            fs::create_dir_all(trash.dir()).unwrap();
            fs::rename(&again, trash.dir().join(format!("{id}{PURGING}"))).unwrap();
            assert_eq!(trash.recover(Local::now()).len(), 1);
            assert!(real.dir.join(SESSION_FILE).is_file() && real.dir.join("transcript.jsonl").is_file());
        });
    }

    #[test]
    fn trash_ids_are_checked() {
        for bad in ["", "..", "a/b", ".openbark", "A0", "x", "-1", "0199\\x", "0199 x"] {
            assert!(!valid_trash_id(bad), "{bad:?}");
        }
        for good in ["0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b", "0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b-2"] {
            assert!(valid_trash_id(good), "{good}");
        }
        let tmp = tempfile::tempdir().unwrap();
        let trash = Trash::for_sessions(tmp.path());
        for bad in ["..", "a/b", ".openbark", ""] {
            assert!(
                trash.restore(bad).is_err() && trash.begin_purge(bad).is_err() && trash.get(bad).is_err()
            );
        }
        // Not a UUID: a fresh one. Taken: a suffix.
        let fresh = trash.new_id("not-a-uuid");
        assert!(valid_trash_id(&fresh) && uuid::Uuid::parse_str(&fresh).is_ok());
        let id = "0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b";
        fs::create_dir_all(trash.dir().join(id)).unwrap();
        assert_eq!(trash.new_id(id), format!("{id}-1"));
    }
}
