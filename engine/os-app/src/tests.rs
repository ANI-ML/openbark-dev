//! os-app tests. Nothing here may touch this computer's real files: every
//! test calls [`test_env`] first (a throwaway data key and temporary files for
//! the team, templates and folder choices) and runs on a temporary visits
//! folder with a temporary team file (docs/plans/deleting-visits.md, step 0).

use std::sync::OnceLock;

use chrono::TimeDelta;
use os_engine::audit::ChainReport;
use os_engine::faithfulness::FaithfulnessReport;
use os_engine::trash::DELETED_FILE;
use os_engine::{NoteResult, TemplateStore};

use super::*;

/// The process-wide environment every test shares, set once: a throwaway
/// data key (never this computer's own), and team, templates, folder-choice
/// and key files in a temporary folder. Environment variables are
/// process-wide and tests run in parallel, so nothing else may set them.
fn test_env() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("openbark-os-app-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: runs once, before any App exists; every test calls test_env() first.
        unsafe {
            std::env::set_var("OPENBARK_DATA_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
            std::env::set_var("OPENBARK_LOCATIONS_FILE", dir.join("locations.json"));
            std::env::set_var("OPENBARK_USERS_FILE", dir.join("users.json"));
            std::env::set_var("OPENBARK_TEMPLATES_DIR", dir.join("templates"));
            std::env::set_var("OPENBARK_KEY_FILE", dir.join("data.key"));
        }
        dir
    })
}

/// An App on a temporary visits folder, with its own team file.
fn app_in(tmp: &Path) -> App {
    test_env();
    let mut app = App::new(ModelStore::new(tmp.join("models")), tmp.join("sessions"));
    app.users = UserStore::new(tmp.join("users.json"));
    app.templates = TemplateStore::new(tmp.join("templates"));
    app
}

/// A real (silent) 16 kHz mono WAV, a tenth of a second long.
fn silent_wav(path: &Path) {
    let data = vec![0u8; 3200];
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(16_000u32.to_le_bytes());
    wav.extend(32_000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    std::fs::write(path, wav).unwrap();
}

fn note_result() -> NoteResult {
    NoteResult {
        status: "draft",
        template: "soap@0.1.0".into(),
        model: "test".into(),
        note: serde_json::json!({}),
        faithfulness: FaithfulnessReport { passed: true, ..Default::default() },
        stats: Default::default(),
        refinement: None,
        edit: None,
        by: None,
        markdown: "# SOAP note\n- Ear canals red\n".into(),
    }
}

/// A visit imported from a silent WAV, with a one-segment transcript and a
/// draft for each animal. `sign`: the last animal's note is signed (in a
/// multi-pet visit, only that one).
fn seed_visit(root: &Path, patient: &str, others: &[&str], sign: bool) -> Session {
    let audio = root.parent().unwrap().join(format!("{}.wav", patient.to_lowercase().replace(' ', "-")));
    silent_wav(&audio);
    let mut s = Session::create_from_file(root, &audio, 0.1).unwrap();
    let (name, last) = patient.split_once(' ').unwrap_or((patient, ""));
    s.set_patient(Patient {
        name: name.into(),
        last_name: last.into(),
        others: others.iter().map(|o| o.to_string()).collect(),
        reason: "Ear check".into(),
        ..Default::default()
    })
    .unwrap();
    let t = Transcript {
        segments: vec![Segment {
            id: 0,
            start: 0.0,
            end: 0.1,
            speaker: Some(0),
            text: "Both ears look red".into(),
            hints: Vec::new(),
            check: None,
        }],
    };
    s.save_transcript(&t, "parakeet", false, &Default::default()).unwrap();
    let soap = LoadedTemplate::builtin("soap").unwrap();
    let pets: Vec<Option<String>> = if others.is_empty() {
        vec![None]
    } else {
        std::iter::once(name).chain(others.iter().copied()).map(|p| Some(p.to_string())).collect()
    };
    let last_pet = pets.last().cloned().flatten();
    for pet in &pets {
        let mut handle = Session::open(&s.dir).unwrap().for_pet(pet.as_deref());
        handle.save_run("gemma", &note_result(), &soap).unwrap();
    }
    if sign {
        let mut handle = Session::open(&s.dir).unwrap().for_pet(last_pet.as_deref());
        handle.sign_latest_by(&Who::named("Dr Rivera")).unwrap();
    }
    Session::open(&s.dir).unwrap()
}

/// A veterinarian (PIN 2468), a technician (1357) and reception (1111).
fn team(app: &App) -> (User, User, User) {
    let dvm = app.users.add("Dr Rivera", Role::Dvm, "2468").unwrap();
    let rvt = app.users.add("Sam", Role::Rvt, "1357").unwrap();
    let desk = app.users.add("Lee", Role::Reception, "1111").unwrap();
    (dvm, rvt, desk)
}

fn because(kind: ReasonKind, by: Option<&str>) -> DeleteRequest {
    DeleteRequest {
        by: by.map(String::from),
        reason: DeleteReason { kind, note: String::new() },
        pin: None,
        confirm_signed: false,
    }
}

fn err<T>(r: Result<T>) -> String {
    match r {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.0,
    }
}

#[test]
fn the_harness_stays_in_temporary_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    assert!(app.users.path().starts_with(tmp.path()), "{}", app.users.path().display());
    assert!(os_engine::users::default_path().starts_with(test_env()));
    assert!(os_engine::template_store::default_root().starts_with(test_env()));
    assert!(os_models::locations::config_path().starts_with(test_env()));
    assert_eq!(os_engine::vault::status().source, Some(os_engine::vault::KeySource::Environment));
}

#[test]
fn visits_group_under_patients_and_stay_linked_after_a_rename() {
    let tmp = tempfile::tempdir().unwrap();
    test_env();
    let root = tmp.path().join("sessions");
    // Chosen folders, as Settings would save them (so they aren't "fixed").
    // Folder changes below save their choice in test_env()'s locations file,
    // never in the real one.
    let mut chosen = os_models::locations::Locations::default();
    chosen.set(FolderKind::Visits, Some(root.clone()));
    chosen.set(FolderKind::Models, Some(tmp.path().join("models")));
    os_models::locations::save(&chosen).unwrap();
    let app = app_in(tmp.path());
    let audio = tmp.path().join("visit.wav");
    silent_wav(&audio);
    let visit = |p: Patient| {
        let mut s = Session::create_from_file(&root, &audio, 1.0).unwrap();
        s.set_patient(p).unwrap();
    };
    let named =
        |name: &str, last: &str| Patient { name: name.into(), last_name: last.into(), ..Default::default() };
    visit(Patient { species: "canine".into(), ..named("Maple", "Okonkwo") });
    visit(named("maple ", "OKONKWO"));
    visit(Patient { others: vec!["Pickles".into()], ..named("Olive", "Haddad") });

    let list = app.list_patients().unwrap();
    let find = |name: &str| list.iter().find(|p| p.patient.name.eq_ignore_ascii_case(name)).unwrap().clone();
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(find("Maple").visits, 2, "the same animal, spelt two ways");
    assert_eq!(find("Maple").patient.species, "canine");
    assert_eq!(find("Pickles").visits, 1, "each animal in a multi-pet visit has its own patient");

    // Visits are now linked by id: renaming the patient keeps them.
    let maple = find("Maple").patient;
    app.update_patient_record(&maple.id, "Maple", "Okonkwo-Reid", "canine").unwrap();
    assert_eq!(app.patient(&maple.id).unwrap().visits.len(), 2);
    assert_eq!(app.list_patients().unwrap().len(), 3, "no duplicate made for the old spelling");
    let pickles = app.patient(&find("Pickles").patient.id).unwrap();
    assert_eq!(pickles.visits[0].pet.as_deref(), Some("Pickles"));

    // The work gate (ADR-0013 review): no folder change while something
    // is writing; after a change, this App takes no more work.
    struct Quiet;
    impl Events for Quiet {
        fn segment(&self, _: &Segment) {}
        fn token(&self, _: &str) {}
        fn level(&self, _: f32, _: f32) {}
        fn download(&self, _: &str, _: u64, _: u64) {}
    }
    let elsewhere = tmp.path().join("elsewhere");
    let elsewhere = elsewhere.to_str().unwrap();
    let drafting = app.begin_work().unwrap();
    let busy = app.change_folder(FolderKind::Visits, elsewhere, ChangeMode::Switch, false, &Quiet);
    let refusal = busy.err().map(|e| e.0).unwrap_or_default();
    assert!(refusal.contains("importing"), "refused while a draft is running: {refusal}");
    drop(drafting);
    app.change_folder(FolderKind::Visits, elsewhere, ChangeMode::Switch, false, &Quiet).unwrap();
    assert!(app.begin_work().is_err(), "a retired App refuses new work");
    assert!(app.create_patient("Late", "Writer", "").is_err(), "no writes to the old folder");
    assert!(app.change_folder(FolderKind::Visits, elsewhere, ChangeMode::Switch, false, &Quiet).is_err());
    let saved = std::fs::read_to_string(test_env().join("locations.json")).unwrap();
    assert!(saved.contains("elsewhere"), "the choice was saved");
}

// ---- step 1: "signed" across every animal ------------------------------

#[test]
fn a_multi_pet_visit_with_one_signed_note_lists_as_signed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Olive Haddad", &["Pickles"], true);
    assert!(s.any_signed() && !s.is_signed());
    let listed = app.list_sessions();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].signed, "Recent visits shows it as signed, not as a draft");
    assert!(app.open_session(&id_of(&s), None).unwrap().any_signed);
}

// ---- step 4: the per-visit lock ----------------------------------------

#[test]
fn a_visit_lock_is_one_at_a_time_and_says_why() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let id = id_of(&s);
    let first = app.lock_visit(&id, Holder::Saving).unwrap();
    assert_eq!(
        err(app.lock_visit(&id, Holder::Saving)),
        "This visit is being saved in another window. Try again in a moment."
    );
    drop(first);
    let again = app.lock_visit(&id, Holder::Saving).unwrap();
    drop(again);

    // A delete waits for drafting and recording, and the folder is untouched.
    let drafting = app.lock_visit(&id, Holder::Drafting).unwrap();
    assert_eq!(
        err(app.delete_visit(&id, &because(ReasonKind::Mistake, Some("Sam")))),
        "This visit is being drafted. Delete it when that's finished."
    );
    assert_eq!(
        err(app.lock_visit(&id, Holder::Drafting)),
        "This visit is being saved in another window. Try again in a moment.",
        "two drafts of one visit are refused"
    );
    drop(drafting);
    let recording = app.lock_visit(&id, Holder::Recording).unwrap();
    assert_eq!(
        err(app.delete_visit(&id, &because(ReasonKind::Mistake, Some("Sam")))),
        "This visit is still recording. Stop the recording first."
    );
    assert_eq!(
        err(app.delete_recording(&id, Some("Sam"))),
        "This visit is still recording. Stop the recording first."
    );
    drop(recording);
    let importing = app.lock_visit(&id, Holder::Importing).unwrap();
    assert_eq!(
        err(app.delete_visit(&id, &because(ReasonKind::Mistake, Some("Sam")))),
        "This visit is still being transcribed. Delete it when that's finished."
    );
    drop(importing);
    let deleting = app.lock_visit(&id, Holder::Deleting).unwrap();
    assert_eq!(err(app.sign(&id, "Dr Rivera", None)), "This visit is being deleted.");
    drop(deleting);
    assert!(s.dir.join(session::SESSION_FILE).is_file());
    assert!(app.trash().list().is_empty());
}

#[test]
fn listing_patients_skips_a_visit_deleted_meanwhile() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let gone = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    seed_visit(&app.sessions_root, "Olive Haddad", &[], false);
    let listed = session::list(&app.sessions_root);
    assert_eq!(listed.len(), 2);
    // The folder leaves between listing and linking (another window deleted it).
    std::fs::remove_dir_all(&gone.dir).unwrap();
    let visits = app.visits_by_patient_from(listed).unwrap();
    assert_eq!(visits.values().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(app.list_patients().unwrap().iter().filter(|p| p.visits > 0).count(), 1);
}

// ---- step 6: delete, restore and remove ----------------------------------

#[test]
fn a_technician_deletes_and_restores_a_multi_pet_visit() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (_dvm, rvt, _desk) = team(&app);
    let s = seed_visit(&app.sessions_root, "Olive Haddad", &["Pickles"], false);
    let id = id_of(&s);
    let patients = app.list_patients().unwrap();
    let pid = |n: &str| patients.iter().find(|p| p.patient.name == n).unwrap().patient.id.clone();
    let (olive, pickles) = (pid("Olive"), pid("Pickles"));
    let before = app.patient(&pickles).unwrap().visits.iter().map(|v| v.id.clone()).collect::<Vec<_>>();
    assert_eq!(before, std::slice::from_ref(&id));

    let mut req = because(ReasonKind::Duplicate, Some(&rvt.id));
    req.reason.note = "  same visit\nimported twice ".into();
    let deleted = app.delete_visit(&id, &req).unwrap();
    assert_eq!(deleted.trash_id, s.meta.id);
    assert_eq!(deleted.pets, ["Olive", "Pickles"]);
    assert_eq!(deleted.patient_id.as_deref(), Some(olive.as_str()));
    assert!(app.list_sessions().is_empty());
    for p in [&olive, &pickles] {
        let view = app.patient(p).unwrap();
        assert!(view.visits.is_empty(), "gone from both patients' pages");
        assert_eq!(view.deleted, 1, "the page says one deleted visit is kept");
    }
    let listed = app.list_deleted(Some(&rvt.id)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].patient, "Olive Haddad");
    assert_eq!(listed[0].pets, ["Olive", "Pickles"]);
    assert_eq!(listed[0].by, "Sam");
    assert_eq!(
        listed[0].reason,
        DeleteReason { kind: ReasonKind::Duplicate, note: "same visit imported twice".into() }
    );
    assert!(listed[0].bytes > 0 && !listed[0].signed);
    assert!(app.storage().deleted_bytes > 0);
    let audit = app.audit();
    assert_eq!(audit.verify().unwrap(), ChainReport::Complete { entries: 2 });
    let events = audit.read().unwrap();
    assert_eq!(events[1].kind, EventKind::VisitDeleted);
    assert_eq!(events[1].by.role, Some(Role::Rvt));
    assert!(events[1].team);
    // Opening the old id (another window, an old link) says where it went.
    assert_eq!(
        err(app.open_session(&id, None)),
        "This visit was deleted. Restore it from Deleted visits in Settings."
    );

    let restored = app.restore_visit(&deleted.trash_id, Some(&rvt.id)).unwrap();
    assert_eq!(restored.id, id, "the same folder name");
    assert_eq!(restored.patient_id.as_deref(), Some(olive.as_str()));
    let after = app.patient(&pickles).unwrap();
    assert_eq!(after.visits.iter().map(|v| v.id.clone()).collect::<Vec<_>>(), before);
    assert_eq!(after.deleted, 0);
    assert!(!s.dir.join(DELETED_FILE).exists());
    assert!(app.list_deleted(Some(&rvt.id)).unwrap().is_empty());
    assert_eq!(audit.verify().unwrap(), ChainReport::Complete { entries: 3 });
    assert_eq!(audit.read().unwrap()[2].kind, EventKind::VisitRestored);
    assert_eq!(
        err(app.restore_visit(&deleted.trash_id, Some(&rvt.id))),
        "That deleted visit is no longer there. It may have been restored or removed for good."
    );
}

#[test]
fn reception_cant_delete_restore_or_see_deleted_visits() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (_dvm, rvt, desk) = team(&app);
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let id = id_of(&s);
    let refused = err(app.delete_visit(&id, &because(ReasonKind::Mistake, Some(&desk.id))));
    assert_eq!(
        refused,
        "Lee can record and enter patient details; deleting visits needs a veterinarian or technician."
    );
    assert!(s.dir.join(session::SESSION_FILE).is_file(), "the folder is untouched");
    assert!(err(app.list_deleted(Some(&desk.id))).contains("deleting visits needs"));
    assert!(err(app.delete_recording(&id, Some(&desk.id))).contains("deleting visits needs"));
    assert!(s.has_audio());
    let trashed = app.delete_visit(&id, &because(ReasonKind::Mistake, Some(&rvt.id))).unwrap();
    assert!(err(app.restore_visit(&trashed.trash_id, Some(&desk.id))).contains("deleting visits needs"));
    assert!(
        err(app.purge_deleted(&trashed.trash_id, Some(&desk.id), None)).contains("deleting visits needs")
    );
    assert_eq!(
        err(app.delete_visit(&id, &because(ReasonKind::Mistake, None))),
        "This visit was deleted. Restore it from Deleted visits in Settings."
    );
}

#[test]
fn single_user_needs_a_name_to_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let id = id_of(&s);
    assert_eq!(
        err(app.delete_visit(&id, &because(ReasonKind::Test, Some("  ")))),
        "Add your name in Settings, then delete the visit."
    );
    assert!(s.dir.join(session::SESSION_FILE).is_file());
    let deleted = app.delete_visit(&id, &because(ReasonKind::Test, Some("Dr Rivera"))).unwrap();
    let event = &app.audit().read().unwrap()[1];
    assert_eq!(event.by, Who::named("Dr Rivera"));
    assert!(!event.team, "single-user, honour-based");
    assert_eq!(
        err(app.restore_visit(&deleted.trash_id, None)),
        "Add your name in Settings, then restore the visit."
    );
    assert_eq!(app.list_deleted(None).unwrap().len(), 1, "looking needs no name");
}

#[test]
fn signed_visits_are_refused_in_phase_1() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, _rvt, _desk) = team(&app);
    let single = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], true);
    let multi = seed_visit(&app.sessions_root, "Olive Haddad", &["Pickles"], true);
    for s in [&single, &multi] {
        let mut req = because(ReasonKind::Other, Some(&dvm.id));
        req.pin = Some("2468".into());
        req.confirm_signed = true;
        assert_eq!(
            err(app.delete_visit(&id_of(s), &req)),
            "Signed visits can't be deleted yet. Add an addendum to correct the note."
        );
        assert!(s.dir.join(session::SESSION_FILE).is_file());
    }
    assert!(app.trash().list().is_empty());
    assert!(!app.audit().path().exists(), "nothing happened, so nothing was recorded");
}

#[test]
fn a_deletion_that_cant_be_recorded_doesnt_happen() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let id = id_of(&s);
    std::fs::create_dir_all(app.audit().path()).unwrap(); // audit.log is a folder: unwritable
    let refused = err(app.delete_visit(&id, &because(ReasonKind::Mistake, Some("Sam"))));
    assert!(
        refused.starts_with("OpenBark couldn't record the deletion, so the visit was kept."),
        "{refused}"
    );
    let listed = app.list_sessions();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id, "still listed under its original name");
    assert!(!s.dir.join(DELETED_FILE).exists());
    assert!(app.trash().list().is_empty());
    // Same for removing the recording.
    let refused = err(app.delete_recording(&id, Some("Sam")));
    assert!(
        refused.starts_with("OpenBark couldn't record the deletion, so the recording was kept."),
        "{refused}"
    );
    assert!(s.has_audio());
}

#[test]
fn deleted_visits_are_removed_for_good_after_30_days_or_by_hand() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let a = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let b = seed_visit(&app.sessions_root, "Olive Haddad", &[], false);
    assert_eq!(app.list_patients().unwrap().len(), 2);
    let ta = app.delete_visit(&id_of(&a), &because(ReasonKind::Test, Some("Sam"))).unwrap();
    let tb = app.delete_visit(&id_of(&b), &because(ReasonKind::Test, Some("Sam"))).unwrap();
    let now = Local::now();
    assert_eq!(app.purge_due_at(now + TimeDelta::days(29)).unwrap(), 0);
    assert_eq!(app.list_deleted(None).unwrap().len(), 2);

    // "Delete now".
    assert_eq!(
        err(app.purge_deleted(&tb.trash_id, Some(""), None)),
        "Add your name in Settings, then delete the visit."
    );
    app.purge_deleted(&tb.trash_id, Some("Sam"), None).unwrap();
    assert_eq!(app.list_deleted(None).unwrap().len(), 1);
    assert!(!app.trash().dir().join(&tb.trash_id).exists());

    assert_eq!(app.purge_due_at(now + TimeDelta::days(31)).unwrap(), 1);
    assert!(app.list_deleted(None).unwrap().is_empty());
    assert_eq!(std::fs::read_dir(app.trash().dir()).unwrap().count(), 0, "nothing left");
    let events = app.audit().read().unwrap();
    let purged: Vec<_> = events.iter().filter(|e| e.kind == EventKind::VisitPurged).collect();
    assert_eq!(purged.len(), 2);
    assert_eq!(purged[0].purge, Some(Purge::ByHand));
    assert_eq!(purged[0].by.name, "Sam");
    assert_eq!(purged[1].purge, Some(Purge::After30Days));
    assert_eq!(purged[1].by.name, "OpenBark");
    assert_eq!(purged[1].visit.as_ref().unwrap().trash_id.as_deref(), Some(ta.trash_id.as_str()));
    assert_eq!(app.audit().verify().unwrap(), ChainReport::Complete { entries: 5 });
    // Patient records are never deleted.
    assert_eq!(app.list_patients().unwrap().len(), 2);
}

#[test]
fn removing_a_signed_item_needs_a_veterinarian_and_their_pin() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, _desk) = team(&app);
    // A signed item can't be deleted in phase 1; put one in Deleted visits directly.
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], true);
    let trash = app.trash();
    let now = Local::now();
    let info = DeletedInfo {
        trash_id: trash.new_id(&s.meta.id),
        original_name: id_of(&s),
        deleted_at: now,
        by: Who::named("Dr Rivera"),
        reason: DeleteReason { kind: ReasonKind::Duplicate, note: "imported twice".into() },
        signed: true,
        purge_after: trash::purge_after(now),
        event_id: audit::new_id(),
        recovered: false,
    };
    trash.put(&s.dir, &info).unwrap();
    assert_eq!(
        err(app.purge_deleted(&info.trash_id, Some(&rvt.id), None)),
        "Only a veterinarian can delete a signed visit. Sam is using OpenBark now."
    );
    assert_eq!(err(app.purge_deleted(&info.trash_id, Some(&dvm.id), Some("0000"))), "That PIN isn't right.");
    assert_eq!(err(app.purge_deleted(&info.trash_id, Some(&dvm.id), None)), "That PIN isn't right.");
    assert_eq!(app.list_deleted(Some(&dvm.id)).unwrap().len(), 1, "still there");
    app.purge_deleted(&info.trash_id, Some(&dvm.id), Some("2468")).unwrap();
    assert!(app.list_deleted(Some(&dvm.id)).unwrap().is_empty());
    let purged = app.audit().read().unwrap().pop().unwrap();
    let signed = &purged.visit.as_ref().unwrap().signed;
    assert_eq!(signed.len(), 1, "the record keeps the signed run");
    assert_eq!(signed[0].signer.name, "Dr Rivera");
    assert_eq!(signed[0].note_sha256.len(), 64);
}

#[test]
fn recovery_records_a_delete_interrupted_before_its_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let trash = app.trash();
    let now = Local::now();
    // A crash between the rename and the record: trash.put alone.
    let info = DeletedInfo {
        trash_id: trash.new_id(&s.meta.id),
        original_name: id_of(&s),
        deleted_at: now,
        by: Who::named("Sam"),
        reason: DeleteReason { kind: ReasonKind::Mistake, note: String::new() },
        signed: false,
        purge_after: trash::purge_after(now),
        event_id: audit::new_id(),
        recovered: false,
    };
    trash.put(&s.dir, &info).unwrap();
    // And one with no deleted.json at all (a crash right after the rename).
    let t = seed_visit(&app.sessions_root, "Olive Haddad", &[], false);
    std::fs::rename(&t.dir, trash.dir().join(&t.meta.id)).unwrap();
    assert!(!app.audit().contains(&info.event_id));

    app.housekeeping_now(now);
    let events = app.audit().read().unwrap();
    let recovered: Vec<_> = events.iter().filter(|e| e.recovered).collect();
    assert_eq!(recovered.len(), 2, "{events:?}");
    assert!(recovered.iter().all(|e| e.kind == EventKind::VisitDeleted));
    let mine = recovered.iter().find(|e| e.id == info.event_id).unwrap();
    assert_eq!(mine.by.name, "Sam", "who deleted it is known from deleted.json");
    assert_eq!(app.list_deleted(None).unwrap().len(), 2);
    assert!(app.list_deleted(None).unwrap().iter().any(|d| d.recovered));
    assert_eq!(app.audit().verify().unwrap(), ChainReport::Complete { entries: 3 });
    app.housekeeping_now(now);
    assert_eq!(app.audit().read().unwrap().len(), 3, "nothing recorded twice");

    // A removal interrupted after the rename to .purging is finished and recorded.
    std::fs::rename(trash.dir().join(&info.trash_id), trash.dir().join(format!("{}.purging", info.trash_id)))
        .unwrap();
    app.housekeeping_now(now);
    assert!(!trash.dir().join(format!("{}.purging", info.trash_id)).exists());
    let last = app.audit().read().unwrap().pop().unwrap();
    assert_eq!(last.kind, EventKind::VisitPurged);
    assert!(last.recovered);
}

#[test]
fn deleting_takes_a_work_ticket() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    app.gate.changing.store(true, std::sync::atomic::Ordering::SeqCst);
    let refused = err(app.delete_visit(&id_of(&s), &because(ReasonKind::Mistake, Some("Sam"))));
    assert!(refused.contains("changing where it keeps its files"), "{refused}");
    assert!(err(app.delete_recording(&id_of(&s), Some("Sam"))).contains("changing where it keeps its files"));
    assert_eq!(app.purge_due_at(Local::now() + TimeDelta::days(60)).unwrap(), 0);
    app.gate.changing.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(s.dir.join(session::SESSION_FILE).is_file());
    // And while a delete holds its ticket, the folders can't change.
    let _ticket = app.begin_work().unwrap();
    assert!(app.is_busy());
}

// ---- step 7: delete the recording ----------------------------------------

#[test]
fn a_veterinarian_deletes_the_recording_of_a_signed_visit_and_the_note_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, desk) = team(&app);
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], true);
    let id = id_of(&s);
    let signed_before = s.meta.runs[0].signed_at;
    assert_eq!(
        err(app.delete_recording(&id, Some(&rvt.id))),
        "Only a veterinarian can delete the recording of a signed visit."
    );
    assert_eq!(
        err(app.delete_recording(&id, Some(&desk.id))),
        "Only a veterinarian can delete the recording of a signed visit."
    );
    assert!(app.session_audio(&id).is_ok());

    let view = app.delete_recording(&id, Some(&dvm.id)).unwrap();
    assert!(!view.has_audio);
    assert_eq!(view.audio_deleted.as_ref().unwrap().by.name, "Dr Rivera");
    assert!(view.transcript.is_some() && view.latest_run.is_some());
    assert!(!s.audio_path().exists());
    let today = Local::now().format("%Y-%m-%d").to_string();
    assert_eq!(err(app.session_audio(&id)), format!("This visit's recording was deleted on {today}."));
    assert_eq!(err(app.delete_recording(&id, Some(&dvm.id))), "This visit has no recording.");
    // Addenda and PDFs work from the transcript and the note.
    app.add_addendum(&id, "Owner called: much better.", &dvm.id, None).unwrap();
    let (pdf, _) = app.export_pdf(&id, &PdfRequest::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let reopened = Session::open(&s.dir).unwrap();
    assert_eq!(reopened.meta.runs[0].signed_at, signed_before, "the signature is unchanged");
    assert_eq!(reopened.meta.audio_sha256, s.meta.audio_sha256);
    // The record holds the recording's hash and the signed run.
    let event = app.audit().read().unwrap().pop().unwrap();
    assert_eq!(event.kind, EventKind::RecordingDeleted);
    let tomb = event.visit.unwrap();
    assert_eq!(tomb.audio_sha256, s.meta.audio_sha256);
    assert_eq!(tomb.signed.len(), 1);
    assert!(tomb.trash_id.is_none());
    let maple = app.list_patients().unwrap().remove(0).patient.id;
    assert!(!app.patient(&maple).unwrap().visits[0].has_audio);
}

#[test]
fn a_technician_deletes_the_recording_of_an_unsigned_visit() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (_dvm, rvt, _desk) = team(&app);
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let view = app.delete_recording(&id_of(&s), Some(&rvt.id)).unwrap();
    assert!(!view.has_audio && view.audio_deleted.is_some());
    assert_eq!(app.audit().verify().unwrap(), ChainReport::Complete { entries: 2 });
    // The visit itself can still be deleted afterwards.
    app.delete_visit(&id_of(&s), &because(ReasonKind::Mistake, Some(&rvt.id))).unwrap();
}

#[test]
fn delete_requests_never_print_the_pin() {
    let mut req = because(ReasonKind::Other, Some("u"));
    req.pin = Some("2468".into());
    let shown = format!("{req:?}");
    assert!(!shown.contains("2468") && shown.contains("***"), "{shown}");
}

// ---- review: trying to break deleting (ADR-0022) ---------------------------

/// On Windows a trailing dot is dropped from a path, so "<id>." and "<id>.."
/// open the same folder as "<id>" (and case doesn't matter). None of them may
/// get past the lock a draft holds on "<id>": the delete would move the folder
/// while the draft's `save_run` recreates it without its audio or transcript.
#[test]
fn an_alias_of_the_id_cant_slip_past_the_visit_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let id = id_of(&s);
    let drafting = app.lock_visit(&id, Holder::Drafting).unwrap();
    for alias in [format!("{id}."), format!("{id}.."), id.to_uppercase()] {
        let deleted = app.delete_visit(&alias, &because(ReasonKind::Mistake, Some("Sam")));
        assert!(deleted.is_err(), "{alias} deleted a visit while it was being drafted");
        assert!(
            app.delete_recording(&alias, Some("Sam")).is_err(),
            "{alias} deleted the recording mid-draft"
        );
    }
    drop(drafting);
    assert!(s.dir.join(session::SESSION_FILE).is_file() && s.has_audio(), "the visit is untouched");
    assert!(app.trash().list().is_empty());
}

/// A crash or power cut mid-append can leave a torn line (or a run of zero
/// bytes) at the end of the record. `append` already copes with that; crash
/// recovery must too, or a visit renamed into Deleted visits just before the
/// crash stays invisible, in neither list, for good.
#[test]
fn recovery_still_runs_after_a_torn_line_in_the_record() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let a = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    app.delete_visit(&id_of(&a), &because(ReasonKind::Test, Some("Sam"))).unwrap();
    // The next delete was cut off: its folder was renamed into Deleted
    // visits, then the program died while writing.
    let b = seed_visit(&app.sessions_root, "Olive Haddad", &[], false);
    let trash = app.trash();
    std::fs::rename(&b.dir, trash.dir().join(&b.meta.id)).unwrap();
    let mut log = std::fs::OpenOptions::new().append(true).open(app.audit().path()).unwrap();
    log.write_all(b"OBL1:AAAAAAAA").unwrap();
    drop(log);
    assert_eq!(app.list_deleted(None).unwrap().len(), 1, "before recovery only the first is listed");

    app.housekeeping_now(Local::now());
    let listed = app.list_deleted(None).unwrap();
    assert_eq!(listed.len(), 2, "the half-deleted visit is described and listed again");
    let (info, _) = trash.get(&b.meta.id).unwrap();
    assert!(info.recovered);
    assert!(app.audit().contains(&info.event_id), "its deletion is recorded, marked as recovered");
    // The torn line itself still shows when the record is checked.
    assert!(matches!(app.audit().verify().unwrap(), ChainReport::Broken { .. }));
}

/// Housekeeping runs hourly on its own thread. A delete that finishes while
/// it's going through Deleted visits must not end up recorded twice: two
/// entries with one id read as a changed record ("Check the record": Broken).
#[test]
fn housekeeping_racing_deletes_never_records_one_twice() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let visits: Vec<Session> =
        (0..60).map(|i| seed_visit(&app.sessions_root, &format!("Pet{i} Family{i}"), &[], false)).collect();
    // Deleted visits already holds some, so each housekeeping pass takes a while.
    for s in &visits[..48] {
        app.delete_visit(&id_of(s), &because(ReasonKind::Test, Some("Sam"))).unwrap();
    }
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                app.recover_deleted(Local::now());
            }
        });
        for _ in 0..5 {
            for s in &visits[48..] {
                let d = app.delete_visit(&id_of(s), &because(ReasonKind::Mistake, Some("Sam"))).unwrap();
                app.restore_visit(&d.trash_id, Some("Sam")).unwrap();
            }
        }
        stop.store(true, Ordering::SeqCst);
    });
    let events = app.audit().read().unwrap();
    let mut ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "an entry was recorded twice");
    let report = app.audit().verify().unwrap();
    assert!(matches!(report, ChainReport::Complete { .. }), "{report:?}");
    assert_eq!(events.iter().filter(|e| e.recovered).count(), 0, "nothing here was interrupted");
}

/// "Delete now" can't be undone, so it checks what the folder holds, not
/// only what `deleted.json` says (another copy of OpenBark on the same
/// folder could have signed the visit as it was deleted: the per-visit lock
/// is per process).
#[test]
fn removing_a_signed_visit_for_good_checks_the_folder_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, _desk) = team(&app);
    let s = seed_visit(&app.sessions_root, "Olive Haddad", &["Pickles"], true);
    let trash = app.trash();
    let now = Local::now();
    let info = DeletedInfo {
        trash_id: trash.new_id(&s.meta.id),
        original_name: id_of(&s),
        deleted_at: now,
        by: Who::named("Sam"),
        reason: DeleteReason { kind: ReasonKind::Mistake, note: String::new() },
        signed: false, // wrong: Pickles's note is signed
        purge_after: trash::purge_after(now),
        event_id: audit::new_id(),
        recovered: false,
    };
    trash.put(&s.dir, &info).unwrap();
    assert_eq!(
        err(app.purge_deleted(&info.trash_id, Some(&rvt.id), None)),
        "Only a veterinarian can delete a signed visit. Sam is using OpenBark now."
    );
    assert_eq!(err(app.purge_deleted(&info.trash_id, Some(&dvm.id), None)), "That PIN isn't right.");
    assert_eq!(trash.list().len(), 1, "still in Deleted visits");
    app.purge_deleted(&info.trash_id, Some(&dvm.id), Some("2468")).unwrap();
    assert!(trash.list().is_empty());
}
