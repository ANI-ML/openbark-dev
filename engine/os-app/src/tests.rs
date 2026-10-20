//! os-app tests. Nothing here may touch this computer's real files: every
//! test calls [`test_env`] first (a throwaway data key and temporary files for
//! the team, templates and folder choices) and runs on a temporary visits
//! folder with a temporary team file (docs/plans/deleting-visits.md, step 0).

use std::sync::OnceLock;

use chrono::TimeDelta;
use os_engine::audit::ChainReport;
use os_engine::faithfulness::FaithfulnessReport;
use os_engine::trash::DELETED_FILE;
use os_engine::{LoadedTemplate, NoteResult, TemplateStore};

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
            // The MLX engine pack's folder and its checks (ADR-0025).
            std::env::set_var("OPENBARK_ENGINES_DIR", dir.join("engines"));
            // Twilio settings and secrets (ADR-0023): a scratch folder, no keychain.
            std::env::set_var("OPENBARK_CONNECTIONS_DIR", dir.join("connections"));
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
    assert!(os_models::mlx_pack::engines_dir().starts_with(test_env()));
    assert!(app.store.root().starts_with(tmp.path()));
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

#[path = "tests/templates.rs"]
mod templates;

#[path = "tests/phone.rs"]
mod phone;

/// A tooth chart edit (ADR-0021 decision 4): with no species entered, the vet
/// chooses dog or cat in the chart editor; that choice goes into the new run's
/// template snapshot, a second, different choice is refused, and a tooth the
/// animal doesn't have is refused with a plain message.
#[test]
fn a_chart_edit_records_the_chosen_animal_and_refuses_teeth_it_doesnt_have() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Pepper Silva", &[], false);
    let id = id_of(&s);
    // Redraft the seeded visit with the dental template, species not entered.
    let dental = LoadedTemplate::builtin("dental").unwrap();
    assert_eq!(dental.chart().map(|c| c.1), Some(None), "no species, no dentition");
    let mut result = note_result();
    result.template = format!("dental@{}", dental.template.version);
    result.note = os_engine::template::empty_note(&dental.template);
    Session::open(&s.dir).unwrap().save_run("gemma", &result, &dental).unwrap();

    let normal = |tooth: &str| {
        serde_json::json!({"tooth": tooth, "text": "normal", "finding": "normal", "detail": null,
            "grade": null, "mm": null, "planned": false, "evidence": []})
    };
    // 105 isn't a cat's tooth: refused, and nothing is saved.
    let refused = err(app.edit_note(
        &id,
        "dental_chart",
        serde_json::json!([normal("105")]),
        "Dr Rivera",
        None,
        Some("feline"),
    ));
    assert!(refused.contains("105") && refused.contains("cat"), "{refused}");
    // A cat's tooth with "feline": saved, and the snapshot now draws a cat's mouth.
    app.edit_note(&id, "dental_chart", serde_json::json!([normal("204")]), "Dr Rivera", None, Some("feline"))
        .unwrap();
    let opened = Session::open(&s.dir).unwrap();
    let (run, note) = opened.latest_note().unwrap();
    let snapshot = opened.run_template(&run).unwrap();
    assert_eq!(snapshot.chart().and_then(|c| c.1).map(|d| d.key()), Some("feline"));
    assert_eq!(note["dental_chart"][0]["entered_by"], "Dr Rivera");
    // The animal can't change afterwards, and later edits keep to its teeth.
    let other = err(app.edit_note(
        &id,
        "dental_chart",
        serde_json::json!([normal("204")]),
        "Dr Rivera",
        None,
        Some("canine"),
    ));
    assert!(!other.is_empty());
    assert!(
        app.edit_note(&id, "dental_chart", serde_json::json!([normal("105")]), "Dr Rivera", None, None)
            .is_err()
    );
    app.edit_note(
        &id,
        "dental_chart",
        serde_json::json!([normal("204"), normal("104")]),
        "Dr Rivera",
        None,
        None,
    )
    .unwrap();
}

/// Drafting a chart (ADR-0021 decision 4): each animal of a multi-pet visit
/// drafts with its own species from its patient record, and a redraft of a
/// chart whose animal still isn't entered keeps the dog or cat the vet chose.
#[test]
fn every_animal_drafts_with_its_own_species_and_a_redraft_keeps_the_chosen_mouth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Olive Haddad", &["Pickles"], false);
    let cat = app.patients.create("Pickles", "Haddad", "Cat").unwrap();
    let mut s = Session::open(&s.dir).unwrap();
    let mut info = s.meta.patient.clone().unwrap();
    info.species = "Dog".into();
    info.other_ids = vec![cat.id.clone()];
    s.set_patient(info).unwrap();
    let main = Session::open(&s.dir).unwrap().for_pet(Some("Olive"));
    let other = Session::open(&s.dir).unwrap().for_pet(Some("Pickles"));
    assert_eq!(app.species_of(&main).as_deref(), Some("Dog"));
    assert_eq!(app.species_of(&other).as_deref(), Some("Cat"), "from Pickles's patient record");
    let dental = LoadedTemplate::builtin("dental").unwrap();
    let pickles = dental.for_species(app.species_of(&other).as_deref());
    assert_eq!(pickles.chart().and_then(|c| c.1).map(|d| d.key()), Some("feline"));

    // No species anywhere: the last draft's chosen mouth carries over.
    let t = tempfile::tempdir().unwrap();
    let app = app_in(t.path());
    let s = seed_visit(&app.sessions_root, "Pepper Silva", &[], false);
    let mut handle = Session::open(&s.dir).unwrap();
    assert!(app.species_of(&handle).is_none());
    let unknown = dental.for_species(None);
    assert_eq!(App::with_last_dentition(&handle, unknown.clone()).chart().and_then(|c| c.1), None);
    let chosen = unknown.with_dentition("dental_chart", os_engine::dental::Dentition::Feline).unwrap();
    let mut result = note_result();
    result.template = chosen.label();
    result.note = os_engine::template::empty_note(&chosen.template);
    handle.save_run("gemma", &result, &chosen).unwrap();
    let handle = Session::open(&s.dir).unwrap();
    assert_eq!(
        handle.latest_record().and_then(|r| r.chart_version),
        Some(os_engine::dental::chart().version)
    );
    let redraft = App::with_last_dentition(&handle, unknown);
    assert_eq!(redraft.chart().and_then(|c| c.1).map(|d| d.key()), Some("feline"));
}

/// ADR-0021 decision 10 through the app: the view lists the drafted teeth
/// that aren't confirmed, signing is refused until a veterinarian confirms
/// each, a technician can't confirm, and the signed chart can't change.
#[test]
fn the_app_signs_a_drafted_chart_only_after_a_vet_confirms_each_tooth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Juno Abara", &[], false);
    let dental = LoadedTemplate::builtin("dental").unwrap().for_species(Some("dog"));
    let mut result = note_result();
    result.template = dental.label();
    result.note = os_engine::template::empty_note(&dental.template);
    result.note["dental_chart"] = serde_json::json!([
        {"tooth": "208", "text": "slab fracture", "finding": "fracture", "detail": null, "grade": null,
         "mm": null, "planned": false, "evidence": [0]},
        {"tooth": "104", "text": "normal", "finding": "normal", "detail": null, "grade": null,
         "mm": null, "planned": false, "evidence": [0], "entered_by": "Dr Rivera"}
    ]);
    Session::open(&s.dir).unwrap().save_run("gemma", &result, &dental).unwrap();
    let id = id_of(&s);

    let view = app.open_session(&id, None).unwrap();
    assert_eq!(view.unconfirmed_teeth, ["208"], "104 is the vet's own entry");
    assert!(view.chart_checks.is_empty());
    let refused = err(app.sign(&id, "Dr Rivera", None));
    assert!(refused.contains("Not confirmed yet: 208"), "{refused}");

    let view = app.confirm_chart(&id, &["208".to_string()], true, "Dr Rivera", None).unwrap();
    assert!(view.unconfirmed_teeth.is_empty());
    assert_eq!(view.chart_checks[0].by, "Dr Rivera");
    let signed = app.sign(&id, "Dr Rivera", None).unwrap();
    assert!(signed.signed_at.is_some());
    let after = err(app.confirm_chart(&id, &["208".to_string()], false, "Dr Rivera", None));
    assert!(after.contains("signed"), "{after}");

    // With a team, only a veterinarian confirms.
    let t = tempfile::tempdir().unwrap();
    let app = app_in(t.path());
    let (dvm, rvt, _) = team(&app);
    let s = seed_visit(&app.sessions_root, "Juno Abara", &[], false);
    Session::open(&s.dir).unwrap().save_run("gemma", &result, &dental).unwrap();
    let id = id_of(&s);
    let no = err(app.confirm_chart(&id, &["208".to_string()], true, &rvt.id, None));
    assert!(no.contains("Only a veterinarian can confirm"), "{no}");
    let view = app.confirm_chart(&id, &["208".to_string()], true, &dvm.id, None).unwrap();
    assert_eq!(view.chart_checks[0].who.as_ref().and_then(|w| w.role), Some(Role::Dvm));
}

// ---- models of your choice and removal (ADR-0025) ----------------------
//
// Every model folder here is a temporary one: removal is never tried
// against a real models folder.

/// A loaded note model that writes "{}" and counts its calls.
struct FakeLlm {
    backend: &'static str,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl FakeLlm {
    /// The backend (to hand to the slot, which then holds the only Arc) and
    /// its call counter.
    fn loaded(backend: &'static str) -> (Arc<dyn LlmBackend>, Arc<std::sync::atomic::AtomicUsize>) {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        (Arc::new(Self { backend, calls: calls.clone() }), calls)
    }
}

impl LlmBackend for FakeLlm {
    fn capabilities(&self) -> os_llm::LlmCapabilities {
        os_llm::LlmCapabilities {
            backend: self.backend,
            model_name: "fake".into(),
            context_length: 4096,
            streaming: true,
            json_schema_constrained: true,
            audio_input: false,
        }
    }

    fn generate(
        &self,
        _request: &os_llm::GenerateRequest,
        on_token: &mut dyn FnMut(&str) -> bool,
    ) -> std::result::Result<os_llm::GenerateOutput, os_llm::LlmError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        on_token("{}");
        Ok(os_llm::GenerateOutput { text: "{}".into(), stats: Default::default() })
    }
}

struct Quiet;
impl Events for Quiet {
    fn segment(&self, _: &Segment) {}
    fn token(&self, _: &str) {}
    fn level(&self, _: f32, _: f32) {}
    fn download(&self, _: &str, _: u64, _: u64) {}
}

/// Put a model in the note-model slot, as a draft's first load does.
fn load_fake(app: &App, id: &str, backend: Arc<dyn LlmBackend>) {
    *app.llm.lock().unwrap() = Some((id.to_string(), backend));
}

fn loaded_id(app: &App) -> Option<String> {
    app.llm.lock().unwrap().as_ref().map(|(id, _)| id.clone())
}

/// A tiny but valid GGUF (magic, version 3).
fn tiny_gguf(path: &Path) -> Vec<u8> {
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[7u8; 120]);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, &bytes).unwrap();
    bytes
}

/// A downloaded Hub model in the temporary store: a GGUF, or with `mlx` an
/// MLX folder. Its files are written at their listed sizes.
fn hub_model(app: &App, name: &str, mlx: bool) -> CatalogEntry {
    let (files, layout, id) = if mlx {
        (
            serde_json::json!([
                { "path": "config.json", "size": 30, "git_sha1": "1".repeat(40) },
                { "path": "model.safetensors", "size": 64, "sha256": "2".repeat(64) },
                { "path": "tokenizer.json", "size": 20, "git_sha1": "3".repeat(40) },
            ]),
            serde_json::json!({ "type": "llm_mlx", "config": "config.json" }),
            format!("hf:someone/{name}/@mlx"),
        )
    } else {
        (
            serde_json::json!([{ "path": format!("{name}.gguf"), "size": 128, "sha256": "4".repeat(64) }]),
            serde_json::json!({ "type": "llm_gguf", "model": format!("{name}.gguf") }),
            format!("hf:someone/{name}-GGUF/{name}"),
        )
    };
    let entry: CatalogEntry = serde_json::from_value(serde_json::json!({
        "id": id,
        "display_name": format!("someone/{name}"),
        "slot": "llm",
        "license": "apache-2.0",
        "source": { "kind": "huggingface", "repo": format!("someone/{name}"), "revision": "a".repeat(40) },
        "files": files,
        "layout": layout,
    }))
    .unwrap();
    hub::save_user_entry(&app.store, entry.clone()).unwrap();
    for f in &entry.files {
        let path = app.store.file_path(&entry, &f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![b'x'; f.size as usize]).unwrap();
    }
    entry
}

fn row(app: &App, id: &str) -> Option<ModelRow> {
    app.models().into_iter().find(|r| r.id == id)
}

#[test]
fn rows_say_where_a_model_came_from_and_what_remove_does() {
    use os_engine::models::{
        DEFAULT_LLM, DEFAULT_STT, EMBEDDING, GPU_SECOND_STT, GPU_STT, SEGMENTATION, VAD,
    };
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    for id in [VAD, DEFAULT_STT, SEGMENTATION, EMBEDDING, os_engine::live::SECOND_STT] {
        assert_eq!(row(&app, id).unwrap().remove, RowRemove::Needed, "{id}");
    }
    let llm = row(&app, DEFAULT_LLM).unwrap();
    assert_eq!(llm.remove, RowRemove::Delete, "the default note model can be removed like any other");
    assert_eq!((llm.origin, llm.engine, llm.builtin), (Origin::Builtin, Some(LlmEngine::Llama), true));
    assert!(row(&app, GPU_STT).is_none() && row(&app, GPU_SECOND_STT).is_none(), "they come with the pack");
    assert_eq!(
        protected_model(GPU_STT),
        Some("This comes with the GPU speech pack. Remove the pack instead.")
    );

    let mlx = hub_model(&app, "Tiny-4bit", true);
    let r = row(&app, &mlx.id).unwrap();
    assert_eq!(
        (r.origin, r.engine, r.parts, r.publisher.as_str()),
        (Origin::Hub, Some(LlmEngine::Mlx), 3, "someone")
    );
    assert!(r.installed && r.disk_bytes == 114 && r.partial_bytes == 0);

    let file = tmp.path().join("elsewhere").join("Mine-Q4_K_M.gguf");
    tiny_gguf(&file);
    let added = app.import_model(file.to_str().unwrap(), ImportMode::InPlace, None, &Quiet).unwrap();
    assert_eq!(
        (added.origin, added.remove, added.publisher.as_str()),
        (Origin::LocalInPlace, RowRemove::ForgetOnly, "This computer")
    );
    assert_eq!((added.disk_bytes, added.missing), (0, false));
    let canonical = os_models::locations::plain(&std::fs::canonicalize(&file).unwrap());
    assert_eq!(added.path.as_deref().map(Path::new), Some(canonical.as_path()));
    let json = serde_json::to_value(&added).unwrap();
    assert_eq!(
        (json["origin"].as_str(), json["remove"].as_str(), json["engine"].as_str()),
        (Some("local_in_place"), Some("forget_only"), Some("llama"))
    );
}

#[test]
fn removing_checks_every_guard_in_order_and_never_cancels_a_draft() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let m = hub_model(&app, "Tiny", false);
    let file = app.store.file_path(&m, &m.files[0].path);

    // With a team, only a veterinarian; checked before anything else.
    let (dvm, rvt, desk) = team(&app);
    for who in [&rvt.id, &desk.id] {
        assert_eq!(
            err(app.remove_model(&m.id, None, false, Some(who))),
            "Only a veterinarian can change the models."
        );
    }
    assert_eq!(
        err(app.remove_model("nope", None, false, Some(&desk.id))),
        "Only a veterinarian can change the models."
    );
    let by = Some(dvm.id.as_str());
    // Then the folder-change gate.
    app.gate.changing.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(err(app.remove_model("nope", None, false, by)).contains("changing where it keeps its files"));
    app.gate.changing.store(false, std::sync::atomic::Ordering::SeqCst);
    // Then the catalog, the protected models, a download, the note model.
    assert_eq!(err(app.remove_model("nope", None, false, by)), "No model called nope in the catalog.");
    assert_eq!(
        err(app.remove_model(os_engine::models::DEFAULT_STT, None, false, by)),
        "OpenBark needs this to transcribe visits."
    );
    assert_eq!(
        err(app.remove_model(os_engine::models::GPU_SECOND_STT, None, false, by)),
        "This comes with the GPU speech pack. Remove the pack instead."
    );
    {
        let _downloading = app.start_download(&m.id).unwrap();
        assert_eq!(err(app.start_download(&m.id)), "This model is still downloading.");
        assert_eq!(err(app.remove_model(&m.id, None, false, by)), "This model is still downloading.");
    }
    assert_eq!(
        err(app.remove_model(&m.id, Some(&m.id), false, by)),
        "This model writes your notes. Choose another one under In use first."
    );
    // Loaded and in use by a draft (it holds a clone): refused, untouched.
    let (fake, calls) = FakeLlm::loaded("llama.cpp");
    load_fake(&app, &m.id, fake);
    let draft = app.load_llm(&m.id).unwrap();
    assert_eq!(
        err(app.remove_model(&m.id, None, false, by)),
        "A note is being written with this model. Remove it when that's finished."
    );
    assert!(file.is_file(), "nothing deleted");
    assert_eq!(loaded_id(&app).as_deref(), Some(m.id.as_str()), "still loaded");
    draft.generate(&os_llm::GenerateRequest::new(Vec::new()), &mut |_| true).unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "the draft carried on");
    drop(draft);
    // Idle now: unloaded first, then removed. The row stays, with Download.
    let removed = app.remove_model(&m.id, None, false, by).unwrap();
    assert_eq!((removed.freed_bytes, removed.files_removed, removed.forgotten), (128, 1, false));
    assert!(!file.exists());
    assert_eq!(loaded_id(&app), None, "unloaded");
    let r = row(&app, &m.id).unwrap();
    assert!(!r.installed && r.disk_bytes == 0);
    // "Remove from list" drops it.
    let again = app.remove_model(&m.id, None, true, by).unwrap();
    assert!(again.forgotten);
    assert!(row(&app, &m.id).is_none());
}

#[test]
fn a_removed_builtin_keeps_its_row_with_download() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let id = os_engine::models::DEFAULT_LLM;
    let entry = os_models::Catalog::builtin().get(id).unwrap().clone();
    // A download that stopped part-way, and a stranger's file beside it.
    let first = app.store.file_path(&entry, &entry.files[0].path);
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(first.with_file_name(format!("{}.part", entry.files[0].path)), vec![0u8; 500]).unwrap();
    let stranger = first.with_file_name("notes.txt");
    std::fs::write(&stranger, b"mine").unwrap();
    let r = row(&app, id).unwrap();
    assert!(!r.installed);
    assert_eq!(r.partial_bytes, 500);
    let removed = app.remove_model(id, None, true, None).unwrap();
    assert_eq!(removed.freed_bytes, 500);
    assert!(!removed.forgotten, "a built-in is never dropped from the list");
    assert!(stranger.is_file(), "only the listed files and their downloads go");
    let r = row(&app, id).unwrap();
    assert!(r.builtin && !r.installed && r.partial_bytes == 0 && r.remove == RowRemove::Delete);
}

#[test]
fn a_model_used_where_it_is_is_only_forgotten() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let file = tmp.path().join("LM Studio").join("Big-Q8_0.gguf");
    let bytes = tiny_gguf(&file);
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    let added = app.import_model(file.to_str().unwrap(), ImportMode::InPlace, None, &Quiet).unwrap();
    assert!(!app.store.root().join("local").exists(), "nothing was copied");
    let removed = app.remove_model(&added.id, None, false, None).unwrap();
    assert!(removed.kept_in_place && removed.forgotten);
    assert_eq!((removed.freed_bytes, removed.files_removed), (0, 0));
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "never changed");
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), before);
    assert!(row(&app, &added.id).is_none());

    // A model used in place that has gone missing: shown, and refused plainly.
    let other = tmp.path().join("usb").join("Gone.gguf");
    tiny_gguf(&other);
    let gone = app.import_model(other.to_str().unwrap(), ImportMode::InPlace, None, &Quiet).unwrap();
    std::fs::remove_file(&other).unwrap(); // the test's own file, never the store's
    let r = row(&app, &gone.id).unwrap();
    assert!(r.missing);
    let refusal = err(app.load_llm(&gone.id));
    assert!(
        refusal.contains("isn't where it was") && refusal.ends_with("Add it again from this computer."),
        "{refusal}"
    );
}

#[test]
fn a_copied_model_is_deleted_and_dropped_and_the_original_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let file = tmp.path().join("Downloads").join("Small-Q4_K_M.gguf");
    let bytes = tiny_gguf(&file);
    let preview = app.inspect_import(file.to_str().unwrap(), None).unwrap();
    struct Seen(Mutex<Vec<String>>);
    impl Events for Seen {
        fn segment(&self, _: &Segment) {}
        fn token(&self, _: &str) {}
        fn level(&self, _: f32, _: f32) {}
        fn download(&self, model: &str, _: u64, _: u64) {
            self.0.lock().unwrap().push(model.to_string());
        }
    }
    let seen = Seen(Mutex::new(Vec::new()));
    let added = app.import_model(file.to_str().unwrap(), ImportMode::Copy, None, &seen).unwrap();
    assert_eq!(added.id, preview.id);
    assert!(seen.0.lock().unwrap().iter().all(|m| *m == preview.id), "copy progress uses the new id");
    assert_eq!((added.origin, added.remove), (Origin::LocalCopy, RowRemove::DeleteAndForget));
    // On one drive the copy is a hard link: on disk, but its bytes are the
    // original's, so they show as shared and removing them frees nothing.
    assert_eq!(added.disk_bytes + added.shared_bytes, bytes.len() as u64);
    assert!(app.downloading.lock().unwrap().is_empty());
    let removed = app.remove_model(&added.id, None, false, None).unwrap();
    assert_eq!(
        (removed.freed_bytes, removed.shared_bytes),
        (added.disk_bytes, added.shared_bytes),
        "what the row promised is what was freed"
    );
    assert!(removed.forgotten);
    assert!(row(&app, &added.id).is_none());
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "the original is untouched");
}

#[test]
fn mlx_models_need_the_engine_and_the_engine_has_its_guards() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let m = hub_model(&app, "Tiny-4bit", true);
    // Without the engine pack (never installed in tests), loading says what
    // to do: download it on a Mac that runs MLX, else that MLX needs one.
    let expected = if mlx_runs_here() { MLX_FIRST } else { MLX_NEEDS };
    assert_eq!(err(app.load_llm(&m.id)), expected);
    let status = app.mlx_status();
    assert!(!status.pack_installed && status.check == CheckState::NotRun && !status.in_use);

    let (dvm, _, desk) = team(&app);
    assert_eq!(err(app.remove_mlx(None, Some(&desk.id))), "Only a veterinarian can change the models.");
    let by = Some(dvm.id.as_str());
    assert_eq!(
        err(app.remove_mlx(Some(&m.id), by)),
        "Your note model uses the MLX engine. Choose another one under In use first."
    );
    // An MLX model loaded and busy: refused; idle: unloaded, then removed.
    let (fake, _) = FakeLlm::loaded("mlx");
    load_fake(&app, &m.id, fake);
    assert!(app.mlx_status().in_use);
    let draft = app.load_llm(&m.id).unwrap();
    assert_eq!(
        err(app.remove_mlx(None, by)),
        "A note is being written with an MLX model. Remove the engine when that's finished."
    );
    assert_eq!(loaded_id(&app).as_deref(), Some(m.id.as_str()));
    drop(draft);
    let removed = app.remove_mlx(Some(os_engine::models::DEFAULT_LLM), by).unwrap();
    assert_eq!(removed.id, os_models::mlx_pack::PACK_ID);
    assert_eq!(loaded_id(&app), None);
    // MLX models stay on disk.
    assert!(row(&app, &m.id).unwrap().installed);
    // A llama.cpp model loaded doesn't hold the engine.
    load_fake(&app, "gguf", FakeLlm::loaded("llama.cpp").0);
    app.remove_mlx(None, by).unwrap();
    assert_eq!(loaded_id(&app).as_deref(), Some("gguf"));
}

#[test]
fn a_draft_still_running_after_the_slot_changed_keeps_its_model_busy() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let m = hub_model(&app, "Tiny", false);
    let mlx = hub_model(&app, "Tiny-4bit", true);
    // Drafts on both started, then another model took the slot.
    let (gguf_draft, _) = FakeLlm::loaded("llama.cpp");
    let (mlx_draft, _) = FakeLlm::loaded("mlx");
    app.loaned.lock().unwrap().push((m.id.clone(), false, Arc::downgrade(&gguf_draft)));
    app.loaned.lock().unwrap().push((mlx.id.clone(), true, Arc::downgrade(&mlx_draft)));
    load_fake(&app, "another", FakeLlm::loaded("llama.cpp").0);
    assert_eq!(
        err(app.remove_model(&m.id, None, false, None)),
        "A note is being written with this model. Remove it when that's finished."
    );
    assert_eq!(
        err(app.remove_mlx(None, None)),
        "A note is being written with an MLX model. Remove the engine when that's finished."
    );
    assert!(app.store.file_path(&m, &m.files[0].path).is_file());
    drop(gguf_draft);
    drop(mlx_draft);
    app.remove_model(&m.id, None, false, None).unwrap();
    app.remove_mlx(None, None).unwrap();
    assert_eq!(loaded_id(&app).as_deref(), Some("another"), "an unrelated model stays loaded");
}

/// The major finding: after an OpenBark or macOS update, the pack check's
/// key no longer matches, and drafting said "Download the MLX engine" until
/// someone opened the Models screen. Now the check runs again by itself, and
/// a failed check has its own message, pointing to Try again.
#[test]
fn a_stale_mlx_check_runs_again_by_itself_instead_of_asking_for_a_download() {
    let rec = |ok| os_models::mlx_pack::CheckRecord { ok, at: "2026-10-19T00:00:00Z".into(), message: None };
    assert_eq!(mlx_gate(false, true, Some(&rec(true))), MlxGate::NotHere);
    assert_eq!(mlx_gate(true, false, None), MlxGate::NotInstalled);
    assert_eq!(mlx_gate(true, true, None), MlxGate::NeedsCheck, "installed, no result for this version");
    assert_eq!(mlx_gate(true, true, Some(&rec(true))), MlxGate::Ready);
    assert_eq!(mlx_gate(true, true, Some(&rec(false))), MlxGate::Failed);

    let passed = || PackCheck {
        ok: true,
        device: "gpu".into(),
        metal: true,
        versions: BTreeMap::new(),
        checks: vec![("imports".into(), true)],
        message: None,
    };
    let runs = std::cell::Cell::new(0);
    ensure_mlx(MlxGate::NeedsCheck, || {
        runs.set(runs.get() + 1);
        Ok(passed())
    })
    .unwrap();
    assert_eq!(runs.get(), 1, "checked once, then drafting goes on");
    let failed =
        PackCheck { ok: false, message: Some("Metal isn't available on this Mac".into()), ..passed() };
    assert_eq!(err(ensure_mlx(MlxGate::NeedsCheck, || Ok(failed))), MLX_CHECK_FAILED);
    assert_eq!(err(ensure_mlx(MlxGate::NeedsCheck, || Err(AppError("no python".into())))), MLX_CHECK_FAILED);
    assert_eq!(
        err(ensure_mlx(MlxGate::Failed, || panic!("a failed check isn't re-run here"))),
        MLX_CHECK_FAILED
    );
    ensure_mlx(MlxGate::Ready, || panic!("a passed check isn't re-run")).unwrap();
    assert_eq!(err(ensure_mlx(MlxGate::NotInstalled, || panic!("nothing to check"))), MLX_FIRST);
    assert_eq!(err(ensure_mlx(MlxGate::NotHere, || panic!("nothing to check"))), MLX_NEEDS);
    assert!(!MLX_CHECK_FAILED.contains("Download"), "never sends anyone to download what they have");
}

/// MLX-only downloads where MLX can't run (Windows, Linux outside test mode,
/// Intel Macs, macOS 13) would fetch gigabytes that never write a note.
#[test]
fn mlx_downloads_are_refused_where_mlx_cant_run() {
    if mlx_runs_here() {
        println!("skipped: MLX runs here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let m = hub_model(&app, "Tiny-4bit", true);
    assert_eq!(err(app.download_model(&m.id, None, &Quiet)), MLX_NEEDS);
    // Refused before any Hub call is made.
    assert_eq!(err(app.hub_add("mlx-community/Qwen3-0.6B-4bit", "@mlx", false)), MLX_NEEDS);
    assert!(app.downloading.lock().unwrap().is_empty());
}

/// Inspecting a path reveals what is on the computer running OpenBark, so
/// with a team it needs a veterinarian, like adding the model.
#[test]
fn inspecting_an_import_needs_a_veterinarian_with_a_team() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let file = tmp.path().join("Downloads").join("Small-Q4_K_M.gguf");
    tiny_gguf(&file);
    let path = file.to_str().unwrap();
    assert!(app.inspect_import(path, None).is_ok(), "single-user: anyone at the computer");
    let (dvm, rvt, desk) = team(&app);
    assert_eq!(err(app.inspect_import(path, None)), "Choose who you are first.");
    for by in [&rvt.id, &desk.id] {
        assert_eq!(err(app.inspect_import(path, Some(by))), "Only a veterinarian can change the models.");
    }
    assert_eq!(app.inspect_import(path, Some(&dvm.id)).unwrap().total_bytes, 128);
}

/// GET /api/mlx used to wait on the note-model slot, which a model load
/// holds for minutes: the MLX card stayed blank. It reads a hint instead.
#[test]
fn the_mlx_status_never_waits_for_a_model_to_load() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let loading = app.llm.lock().unwrap(); // as load_llm holds it while loading
    app.mlx_in_slot.store(true, std::sync::atomic::Ordering::SeqCst);
    let started = std::time::Instant::now();
    assert!(app.mlx_status().in_use, "an MLX model is loading");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    drop(loading);
    load_fake(&app, "gguf", FakeLlm::loaded("llama.cpp").0);
    assert!(!app.mlx_status().in_use, "read from the slot when it's free");
}

/// A removal waiting for a model load used to hold the download list, so no
/// download could start or finish until the load ended (and a download
/// starting on the loading thread would have deadlocked).
#[test]
fn a_removal_waiting_for_a_model_load_doesnt_hold_up_downloads() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let m = hub_model(&app, "Tiny", false);
    std::thread::scope(|scope| {
        let loading = app.llm.lock().unwrap();
        let remover = scope.spawn(|| app.remove_model(&m.id, None, false, None));
        std::thread::sleep(std::time::Duration::from_millis(300)); // it now waits for the slot
        let started = std::time::Instant::now();
        let other = app.start_download("hf:someone/other-GGUF/other").unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(other);
        drop(loading);
        let removed = remover.join().unwrap().unwrap();
        assert_eq!(removed.files_removed, 1);
    });
}

// ---- shutdown order (os_llm::shutdown; the Metal abort on quit) ------------
// `App::shutdown` also sets os-llm's process-wide stop flag, which would stop
// other tests' models, so these drive its release step directly. The flag
// itself is tested in os-llm/tests/shutdown.rs, in its own process.

#[test]
fn release_frees_the_loaded_note_model() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (backend, _) = FakeLlm::loaded("llama.cpp");
    let weak = Arc::downgrade(&backend);
    app.loaned.lock().unwrap().push(("gguf".into(), false, weak.clone()));
    load_fake(&app, "gguf", backend);
    assert!(app.release_llms(std::time::Instant::now()));
    assert_eq!(loaded_id(&app), None);
    assert_eq!(weak.strong_count(), 0, "the model itself is freed, not just unlisted");
}

#[test]
fn release_waits_for_a_running_draft_then_frees_its_model() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (backend, _) = FakeLlm::loaded("llama.cpp");
    let weak = Arc::downgrade(&backend);
    app.loaned.lock().unwrap().push(("gguf".into(), false, weak.clone()));
    load_fake(&app, "gguf", backend.clone());
    // A draft holds the model for a moment, then finishes.
    let draft = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        drop(backend);
    });
    let started = std::time::Instant::now();
    assert!(app.release_llms(started + std::time::Duration::from_secs(10)));
    assert!(started.elapsed() >= std::time::Duration::from_millis(100), "waited for the draft");
    assert_eq!(weak.strong_count(), 0);
    draft.join().unwrap();
}

#[test]
fn release_gives_up_at_the_deadline_while_a_draft_still_holds_the_model() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (held, _) = FakeLlm::loaded("llama.cpp");
    app.loaned.lock().unwrap().push(("gguf".into(), false, Arc::downgrade(&held)));
    let started = std::time::Instant::now();
    assert!(!app.release_llms(started + std::time::Duration::from_millis(50)));
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "bounded");
    drop(held);
    assert!(app.release_llms(std::time::Instant::now()));
}

#[test]
fn release_gives_up_at_the_deadline_while_a_model_is_loading() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    // A load holds the slot's lock for as long as it runs.
    let loading = app.llm.lock().unwrap();
    std::thread::scope(|s| {
        let app = &app;
        let gave_up = s.spawn(move || {
            app.release_llms(std::time::Instant::now() + std::time::Duration::from_millis(50))
        });
        assert!(!gave_up.join().unwrap());
    });
    drop(loading);
    assert!(app.release_llms(std::time::Instant::now()));
}

// ---- recordings for the player (bug: replay failed in the macOS app) -------

#[test]
fn range_headers_are_planned_like_http_says() {
    use RangePlan::*;
    assert_eq!(plan_range(None, 100), Whole);
    assert_eq!(plan_range(Some("bytes=0-1"), 100), Part(0, 1));
    assert_eq!(plan_range(Some("bytes=0-"), 100), Part(0, 99));
    assert_eq!(plan_range(Some("bytes=10-1000"), 100), Part(10, 99));
    assert_eq!(plan_range(Some("bytes=-10"), 100), Part(90, 99));
    assert_eq!(plan_range(Some("bytes=-1000"), 100), Part(0, 99));
    assert_eq!(plan_range(Some("bytes=5-9, 20-30"), 100), Part(5, 9), "the first of several");
    assert_eq!(plan_range(Some("bytes=100-"), 100), Unsatisfiable);
    assert_eq!(plan_range(Some("bytes=-0"), 100), Unsatisfiable);
    assert_eq!(plan_range(Some("bytes=0-"), 0), Unsatisfiable);
    // Nonsense is ignored: the whole recording.
    for h in ["items=0-1", "bytes=9-3", "bytes=a-b", "bytes=-", "bytes=0"] {
        assert_eq!(plan_range(Some(h), 100), Whole, "{h}");
    }
}

#[test]
fn recordings_are_served_in_ranges_that_match_the_whole_file() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let visits = demo::audio_visits(&app.sessions_root).unwrap();
    assert_eq!(visits.len(), 2);
    for v in &visits {
        let whole = app.session_audio(&v.id).unwrap();
        assert!(os_engine::vault::is_encrypted(&app.sessions_root.join(&v.id).join("audio.wav")));
        let total = whole.len() as u64;
        // 16 kHz, 16-bit, `channels` wide, plus the 44-byte header.
        assert_eq!(total, 44 + (v.secs * 16_000.0) as u64 * 2 * v.channels as u64);
        assert_eq!(&whole[22..24], &v.channels.to_le_bytes());
        // WebKit's first probe, then the open-ended request (capped).
        let probe = app.session_audio_part(&v.id, Some("bytes=0-1"), false).unwrap();
        assert_eq!((probe.status, probe.start, probe.end, probe.total), (206, 0, 1, total));
        assert_eq!(probe.body, &whole[..2]);
        let open = app.session_audio_part(&v.id, Some("bytes=0-"), false).unwrap();
        assert_eq!(open.status, 206);
        assert_eq!(open.end, AUDIO_PART_MAX - 1, "an open-ended answer is capped");
        assert_eq!(open.body, &whole[..AUDIO_PART_MAX as usize]);
        let tail = app.session_audio_part(&v.id, Some(&format!("bytes={}-", total - 1000)), false).unwrap();
        assert_eq!((tail.start, tail.end), (total - 1000, total - 1));
        assert_eq!(tail.body, &whole[total as usize - 1000..]);
        let suffix = app.session_audio_part(&v.id, Some("bytes=-44"), false).unwrap();
        assert_eq!(suffix.body, &whole[total as usize - 44..]);
        let past = app.session_audio_part(&v.id, Some(&format!("bytes={total}-")), false).unwrap();
        assert_eq!(past.status, 416);
        assert!(past.headers().contains(&("Content-Range", format!("bytes */{total}"))));
        let head = app.session_audio_part(&v.id, None, true).unwrap();
        assert_eq!((head.status, head.body.len()), (200, 0));
        assert!(head.headers().contains(&("Content-Length", total.to_string())));
        let full = app.session_audio_part(&v.id, None, false).unwrap();
        assert_eq!(full.body, whole);
        let h = open.headers();
        for want in [
            ("Content-Type", "audio/wav".to_string()),
            ("Accept-Ranges", "bytes".to_string()),
            ("Cache-Control", "no-store".to_string()),
            ("Content-Range", format!("bytes 0-{}/{total}", AUDIO_PART_MAX - 1)),
            ("Content-Length", AUDIO_PART_MAX.to_string()),
        ] {
            assert!(h.contains(&want), "{want:?} in {h:?}");
        }
    }
    assert!(app.session_audio_part("no-such-visit", None, false).is_err());
    assert!(app.session_audio_part("../escape", None, false).is_err());
}
