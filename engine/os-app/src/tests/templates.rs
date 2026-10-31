//! Template editing in the app (ADR-0024 plan steps 9 and 10, with the
//! owner's decisions: the personal/hospital setting, and admins who approve
//! every change to the hospital's templates with their PIN).

use os_engine::template::{Layer, Scope};
use os_engine::users::TemplateMode;

use super::*;

fn approver(user: &User, pin: &str) -> Approver {
    Approver { id: user.id.clone(), pin: pin.into() }
}

/// SOAP with the Plan instruction changed.
fn soap_with(text: &str) -> Template {
    let mut t = LoadedTemplate::builtin("soap").unwrap().template;
    t.sections.iter_mut().find(|s| s.id == "plan").unwrap().instruction = Some(text.into());
    t
}

fn save(app: &App, by: Option<&str>, template: Template) -> Result<TemplateView> {
    app.save_template(SaveTemplateRequest {
        template,
        by: by.map(String::from),
        base_version: None,
        base_layer: None,
        hospital: false,
        approver: None,
    })
}

fn save_for_hospital(app: &App, by: &str, template: Template, ok: Option<Approver>) -> Result<TemplateView> {
    app.save_template(SaveTemplateRequest {
        template,
        by: Some(by.into()),
        base_version: None,
        base_layer: None,
        hospital: true,
        approver: ok,
    })
}

fn plan(view: &TemplateView) -> Option<String> {
    view.template.sections.iter().find(|s| s.id == "plan").unwrap().instruction.clone()
}

#[test]
fn each_person_drafts_with_their_own_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, _) = team(&app);
    let mine = save(&app, Some(&rvt.id), soap_with("Rechecks in days.")).unwrap();
    assert_eq!(mine.layer, Layer::Personal);
    assert!(mine.can.reset && mine.can.share);
    assert_eq!(app.profiles_root(), tmp.path().join("profiles"), "profiles follow the team file");
    // The technician's drafts follow the change; the veterinarian's and their prompt preview don't.
    let (theirs, _) = app.drafting_template("soap", Some(&Who::from(&rvt))).unwrap();
    assert_eq!(theirs.template.origin.as_ref().unwrap().layer, Layer::Personal);
    let (vets, _) = app.drafting_template("soap", Some(&Who::from(&dvm))).unwrap();
    assert_eq!(vets.template.origin.as_ref().unwrap().layer, Layer::Builtin);
    assert!(!app.template("soap", Some(&dvm.id)).unwrap().prompt.contains("Rechecks in days."));
    assert!(app.template("soap", Some(&rvt.id)).unwrap().prompt.contains("Rechecks in days."));
    let listed = app.list_templates(Some(&rvt.id)).unwrap();
    let soap = listed.iter().find(|t| t.id == "soap").unwrap();
    assert!(soap.layer == Layer::Personal && soap.edited);
    assert_eq!(
        app.list_templates(Some(&dvm.id)).unwrap().iter().find(|t| t.id == "soap").unwrap().layer,
        Layer::Builtin
    );
}

/// Each run records which version drafted it (plan step 9).
#[test]
fn a_run_records_its_template_origin_and_a_signed_note_never_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, _) = team(&app);
    save(&app, Some(&rvt.id), soap_with("Mine.")).unwrap();
    let s = seed_visit(&app.sessions_root, "Maple Okonkwo", &[], false);
    let (template, notice) = app.drafting_template("soap", Some(&Who::from(&rvt))).unwrap();
    assert!(notice.is_none());
    let mut handle = Session::open(&s.dir).unwrap();
    handle.save_run("gemma", &note_result(), &template).unwrap();
    let run = handle.latest_record().unwrap().clone();
    let origin = run.template_origin.clone().unwrap();
    assert_eq!(origin.layer, Layer::Personal);
    assert_eq!(origin.owner.as_ref().unwrap().name, "Sam");
    assert_eq!(origin.version, "1");
    assert_eq!(origin.fingerprint, os_engine::template::fingerprint(&template.template));
    assert_eq!(run.template_name.as_deref(), Some("SOAP note"));
    // The veterinarian signs it; the technician then resets, changes and deletes nothing in it.
    handle.sign_latest_by(&Who::from(&dvm)).unwrap();
    let dir = s.dir.join(&run.dir);
    let note = std::fs::read(dir.join("note.md")).unwrap();
    let snapshot = std::fs::read(dir.join("template.yaml")).unwrap();
    let reset = app.reset_template("soap", Some(&rvt.id)).unwrap();
    app.undo_reset_template("soap", Some(&rvt.id), &reset.undo).unwrap();
    save(&app, Some(&rvt.id), soap_with("Changed after signing.")).unwrap();
    app.reset_template("soap", Some(&rvt.id)).unwrap();
    assert_eq!(std::fs::read(dir.join("note.md")).unwrap(), note);
    assert_eq!(std::fs::read(dir.join("template.yaml")).unwrap(), snapshot);
    let view = app.open_session(&id_of(&s), None).unwrap();
    assert_eq!(
        view.latest_template.unwrap().origin.unwrap().layer,
        Layer::Personal,
        "the snapshot says whose"
    );
    app.add_addendum(&id_of(&s), "Owner called back.", &dvm.id, None).unwrap();
    // Show the template: the run's snapshot, read-only.
    let shown = app.run_template_view(&id_of(&s), None).unwrap();
    assert!(!shown.can.edit && shown.prompt.contains("Mine."));
}

#[test]
fn a_run_from_before_shows_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let s = seed_visit(&app.sessions_root, "Olive Haddad", &[], false);
    // An older session.json: no origin, name or icon on the run.
    let path = s.dir.join(session::SESSION_FILE);
    let mut meta: serde_json::Value =
        serde_json::from_str(&os_engine::vault::read_to_string(&path).unwrap()).unwrap();
    for r in meta["runs"].as_array_mut().unwrap() {
        let r = r.as_object_mut().unwrap();
        r.remove("template_origin");
        r.remove("template_name");
        r.remove("template_icon");
    }
    os_engine::vault::write(&path, meta.to_string().as_bytes()).unwrap();
    let listed = app.list_sessions();
    assert_eq!(listed[0].template, "SOAP note", "today's lookup");
    let v = app.open_session(&id_of(&s), None).unwrap();
    assert!(v.latest_markdown.is_some());
}

#[test]
fn working_alone_changes_go_to_the_templates_folder_without_a_pin() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let v = save(&app, Some("Dr Ahmed"), soap_with("Mine.")).unwrap();
    assert_eq!((v.layer, v.mode), (Layer::Team, TemplatesMode::Alone));
    assert!(tmp.path().join("templates").join("soap").join("template.yaml").is_file());
    let r = app.reset_template("soap", Some("Dr Ahmed")).unwrap();
    assert_eq!(r.view.layer, Layer::Builtin);
    assert_eq!(app.undo_reset_template("soap", Some("Dr Ahmed"), &r.undo).unwrap().layer, Layer::Team);
    assert!(app.undo_reset_template("soap", Some("Dr Ahmed"), &r.undo).is_err(), "once");
    let std =
        app.add_standard("soap", "", "Metric units", Some("Dr Ahmed"), Some(Scope::Personal), None).unwrap();
    assert_eq!(std.scope, Scope::Team, "working alone, every standard is everyone's");
    assert!(app.set_template_mode(TemplateMode::Hospital, Some("Dr Ahmed"), None).is_err());
}

/// The roles table (ADR-0024 section 7, with D2 and D3): who may do what,
/// and every refusal says why.
#[test]
fn who_may_change_what() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, desk) = team(&app);
    assert!(dvm.admin && !rvt.admin && !desk.admin, "the person who set up the team is the admin");
    let other_tech = app.users.add("Jo", Role::Rvt, "8642").unwrap();

    // Reception reads, but changes nothing.
    assert_eq!(
        err(save(&app, Some(&desk.id), soap_with("x"))),
        "Lee can read templates; changing them needs a veterinarian or technician."
    );
    assert!(!app.template("soap", Some(&desk.id)).unwrap().can.edit);
    assert_eq!(err(save(&app, None, soap_with("x"))), "Choose who you are first.");

    // The hospital's templates need an admin's PIN, every time, even the admin's own.
    assert_eq!(
        err(save_for_hospital(&app, &dvm.id, soap_with("x"), None)),
        "Changing the hospital's templates needs an admin. Ask one to approve with their PIN."
    );
    assert_eq!(
        err(save_for_hospital(&app, &dvm.id, soap_with("x"), Some(approver(&dvm, "0000")))),
        "That PIN isn't right."
    );
    assert_eq!(
        err(save_for_hospital(&app, &rvt.id, soap_with("x"), Some(approver(&other_tech, "8642")))),
        "Only an admin can approve this."
    );
    assert!(!tmp.path().join("templates").join("soap").exists(), "nothing written");
    let hospital =
        save_for_hospital(&app, &dvm.id, soap_with("Hospital plan."), Some(approver(&dvm, "2468"))).unwrap();
    assert_eq!(hospital.layer, Layer::Team);
    assert_eq!(hospital.origin.as_ref().unwrap().approved_by.as_ref().unwrap().name, "Dr Rivera");

    // A technician shares with an admin's approval; the hospital's version records both.
    save(&app, Some(&rvt.id), soap_with("Tech plan.")).unwrap();
    assert!(app.share_template("soap", Some(&rvt.id), ShareMode::Replace, None, None).is_err());
    assert!(
        app.share_template("soap", Some(&rvt.id), ShareMode::Replace, None, Some(&approver(&dvm, "1111")))
            .is_err()
    );
    let shared = app
        .share_template("soap", Some(&rvt.id), ShareMode::Replace, None, Some(&approver(&dvm, "2468")))
        .unwrap();
    assert_eq!(shared.layer, Layer::Team, "the personal version retired");
    let origin = shared.origin.unwrap();
    assert_eq!(origin.saved_by.unwrap().name, "Sam");
    assert_eq!(origin.approved_by.unwrap().name, "Dr Rivera");
    assert_eq!(plan(&app.template("soap", Some(&desk.id)).unwrap()).as_deref(), Some("Tech plan."));

    // Resetting the hospital's version, and deleting a hospital template, need an admin's PIN.
    assert!(app.reset_team_template("soap", Some(&rvt.id), None).is_err());
    let reset = app.reset_team_template("soap", Some(&rvt.id), Some(&approver(&dvm, "2468"))).unwrap();
    assert_eq!(reset.view.layer, Layer::Builtin);
    assert!(app.undo_reset_template("soap", Some(&dvm.id), &reset.undo).is_err(), "only whoever reset it");
    assert_eq!(app.undo_reset_template("soap", Some(&rvt.id), &reset.undo).unwrap().layer, Layer::Team);

    // Standards: for oneself freely, for everyone with an admin's PIN.
    let mine =
        app.add_standard("soap", "", "Metric units", Some(&rvt.id), Some(Scope::Personal), None).unwrap();
    assert_eq!(mine.scope, Scope::Personal);
    assert!(
        app.add_standard("soap", "", "Everyone: metric", Some(&rvt.id), Some(Scope::Team), None).is_err()
    );
    let team_std = app
        .add_standard(
            "soap",
            "",
            "Everyone: metric",
            Some(&rvt.id),
            Some(Scope::Team),
            Some(&approver(&dvm, "2468")),
        )
        .unwrap();
    assert!(app.remove_standard("soap", &team_std.id, Some(&rvt.id), None).is_err());
    app.remove_standard("soap", &mine.id, Some(&rvt.id), None).unwrap();
    assert!(app.add_standard("soap", "", "x", Some(&desk.id), Some(Scope::Personal), None).is_err());

    // The setting and the admin flag: an admin with their PIN; always one admin.
    assert!(
        app.set_template_mode(TemplateMode::Hospital, Some(&rvt.id), Some(&approver(&rvt, "1357"))).is_err()
    );
    assert!(app.set_user_admin(&rvt.id, true, Some(&rvt.id), None).is_err());
    app.set_user_admin(&rvt.id, true, Some(&dvm.id), Some(&approver(&dvm, "2468"))).unwrap();
    app.set_user_admin(&dvm.id, false, Some(&rvt.id), Some(&approver(&rvt, "1357"))).unwrap();
    assert_eq!(
        err(app.set_user_admin(&rvt.id, false, Some(&rvt.id), Some(&approver(&rvt, "1357")))),
        "Keep at least one admin on the team. Make someone else an admin first."
    );
    // D3: an admin who works at reception can change the hospital's templates with their PIN.
    app.set_user_admin(&desk.id, true, Some(&rvt.id), Some(&approver(&rvt, "1357"))).unwrap();
    let desk_view = app.template("soap", Some(&desk.id)).unwrap();
    assert!(desk_view.can.edit && desk_view.can.save_needs_pin);
    assert_eq!(desk_view.can.save_to, Some(Layer::Team));
    save_for_hospital(&app, &desk.id, soap_with("From the desk."), Some(approver(&desk, "1111"))).unwrap();
}

#[test]
fn shared_by_the_hospital_turns_personal_versions_off_and_back_on() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, _) = team(&app);
    save(&app, Some(&rvt.id), soap_with("Mine.")).unwrap();
    app.add_standard("soap", "", "My metric", Some(&rvt.id), Some(Scope::Personal), None).unwrap();
    app.set_template_mode(TemplateMode::Hospital, Some(&dvm.id), Some(&approver(&dvm, "2468"))).unwrap();
    assert_eq!(app.status().templates, TemplatesMode::Hospital);
    let v = app.template("soap", Some(&rvt.id)).unwrap();
    assert_eq!(v.layer, Layer::Builtin, "the hospital's (here the built-in), not theirs");
    assert!(!v.can.edit && !v.prompt.contains("My metric"));
    assert_eq!(
        err(save(&app, Some(&rvt.id), soap_with("x"))),
        "Templates are shared by the hospital, so only an admin can change them."
    );
    // The admin changes the hospital's template, with their PIN.
    let admin_view = app.template("soap", Some(&dvm.id)).unwrap();
    assert!(admin_view.can.edit && admin_view.can.save_needs_pin);
    assert!(save(&app, Some(&dvm.id), soap_with("Hospital.")).is_err(), "needs the PIN");
    save_for_hospital(&app, &dvm.id, soap_with("Hospital."), Some(approver(&dvm, "2468"))).unwrap();
    assert_eq!(plan(&app.template("soap", Some(&rvt.id)).unwrap()).as_deref(), Some("Hospital."));
    // Back to each person's own: the technician's version is there again.
    app.set_template_mode(TemplateMode::Personal, Some(&dvm.id), Some(&approver(&dvm, "2468"))).unwrap();
    let back = app.template("soap", Some(&rvt.id)).unwrap();
    assert_eq!(back.layer, Layer::Personal);
    assert_eq!(plan(&back).as_deref(), Some("Mine."));
    assert!(back.prompt.contains("My metric"));
}

#[test]
fn a_new_template_is_only_its_makers_until_shared() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (dvm, rvt, _) = team(&app);
    let mut t = soap_with("x");
    t.id = String::new();
    t.name = "Sam's recheck".into();
    let v = save(&app, Some(&rvt.id), t).unwrap();
    assert!(v.can.delete && v.can.share && !v.can.reset);
    let id = v.template.id.clone();
    assert!(!app.list_templates(Some(&dvm.id)).unwrap().iter().any(|t| t.id == id));
    // Drafting with it as someone else falls back to the default, and says so.
    let (fallback, notice) = app.drafting_template(&id, Some(&Who::from(&dvm))).unwrap();
    assert_eq!(fallback.template.id, "soap");
    assert_eq!(notice.as_deref(), Some("Sam's recheck is only available to Sam. Drafting with SOAP note."));
    app.share_template(&id, Some(&rvt.id), ShareMode::Replace, None, Some(&approver(&dvm, "2468"))).unwrap();
    assert!(app.list_templates(Some(&dvm.id)).unwrap().iter().any(|t| t.id == id));
}

/// A new template starts with no name and a heading with no title: the
/// preview says what's missing, and what this person may do comes with it, the
/// same whether or not the draft is complete (the editor's inputs follow it).
#[test]
fn a_blank_new_template_previews_with_its_problem_and_the_same_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let blank = || {
        let mut t = soap_with("x");
        t.id = String::new();
        t.name = String::new();
        t.sections.truncate(1);
        t.sections[0].id = String::new();
        t.sections[0].title = String::new();
        t
    };
    let untitled_heading = || {
        let mut t = soap_with("x");
        t.id = String::new();
        t.name = "Dental procedure".into();
        t.sections[0].title = " ".into();
        t
    };
    let complete = || {
        let mut t = soap_with("x");
        t.id = String::new();
        t.name = "Soap".into();
        t
    };
    let can = |by: Option<&str>, t: Template| {
        let v = app.preview_template(t, by).unwrap();
        (serde_json::to_value(&v.can).unwrap(), v.problem)
    };
    let alone = app.preview_template(blank(), Some("Dr Ahmed")).unwrap();
    assert_eq!(alone.problem.as_deref(), Some("Give the template a name."));
    assert!(alone.can.edit && !alone.can.save_needs_pin);
    assert_eq!((alone.can.save_to, alone.mode), (Some(Layer::Team), TemplatesMode::Alone));

    let (dvm, rvt, desk) = team(&app);
    // The hospital's own SOAP: a new template named "Soap" still can't reset it.
    save_for_hospital(&app, &dvm.id, soap_with("Hospital."), Some(approver(&dvm, "2468"))).unwrap();
    for by in [&dvm.id, &rvt.id, &desk.id] {
        let (blank_can, problem) = can(Some(by.as_str()), blank());
        assert_eq!(problem.as_deref(), Some("Give the template a name."));
        let (heading_can, problem) = can(Some(by.as_str()), untitled_heading());
        assert_eq!(problem.as_deref(), Some("A heading has no title."));
        let (complete_can, problem) = can(Some(by.as_str()), complete());
        assert_eq!(problem, None);
        assert_eq!(blank_can, complete_can, "{by}");
        assert_eq!(heading_can, complete_can, "{by}");
        assert_eq!(complete_can["reset_team"], false, "{by}");
    }
    let tech = app.preview_template(blank(), Some(&rvt.id)).unwrap();
    assert!(tech.can.edit && !tech.can.save_needs_pin);
    assert_eq!((tech.can.save_to, tech.layer), (Some(Layer::Personal), Layer::Personal));
    assert!(!app.preview_template(blank(), Some(&desk.id)).unwrap().can.edit, "reception reads");
    // An admin at reception saves new templates for the hospital, with a PIN.
    app.set_user_admin(&desk.id, true, Some(&dvm.id), Some(&approver(&dvm, "2468"))).unwrap();
    let desk_admin = app.preview_template(blank(), Some(&desk.id)).unwrap();
    assert!(desk_admin.can.edit && desk_admin.can.save_needs_pin);
    assert_eq!(desk_admin.can.save_to, Some(Layer::Team));
    // Shared by the hospital: only an admin edits.
    app.set_template_mode(TemplateMode::Hospital, Some(&dvm.id), Some(&approver(&dvm, "2468"))).unwrap();
    assert!(!app.preview_template(blank(), Some(&rvt.id)).unwrap().can.edit);
    let admin = app.preview_template(blank(), Some(&dvm.id)).unwrap();
    assert!(admin.can.edit && admin.can.save_needs_pin);
    assert_eq!((admin.can.save_to, admin.mode), (Some(Layer::Team), TemplatesMode::Hospital));
}

#[test]
fn the_first_member_gets_the_setup_notice_and_removal_takes_the_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    save(&app, Some("Dr Rivera"), soap_with("Changed while alone.")).unwrap();
    let dvm = app.add_user("Dr Rivera", Role::Dvm, "2468", None).unwrap();
    let rvt = app.add_user("Sam", Role::Rvt, "1357", Some(&dvm.id)).unwrap();
    assert!(app.team_settings().setup_notice);
    app.dismiss_setup_notice(Some(&dvm.id)).unwrap();
    assert!(!app.team_settings().setup_notice, "once");
    // The technician's profile: one version and one standard.
    save(&app, Some(&rvt.id), soap_with("Tech.")).unwrap();
    app.add_standard("dental", "", "Triadan numbers", Some(&rvt.id), Some(Scope::Personal), None).unwrap();
    let summary = app.profile_summary(&rvt.id, Some(&dvm.id)).unwrap();
    assert_eq!((summary.versions, summary.own_templates, summary.standards), (1, 0, 1));
    assert!(app.profile_summary(&rvt.id, Some(&rvt.id)).is_err(), "managing the team is a veterinarian's");
    assert!(app.profiles_root().join(&rvt.id).is_dir());
    app.remove_user(&rvt.id, Some(&dvm.id)).unwrap();
    assert!(!app.profiles_root().join(&rvt.id).exists());
}

#[test]
fn a_conflicting_save_is_refused_and_the_change_is_not_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let (_, rvt, _) = team(&app);
    let opened = app.template("soap", Some(&rvt.id)).unwrap();
    save(&app, Some(&rvt.id), soap_with("From the desktop app.")).unwrap();
    let stale = app.save_template(SaveTemplateRequest {
        template: soap_with("From the browser."),
        by: Some(rvt.id.clone()),
        base_version: Some(opened.template.version.clone()),
        base_layer: Some(opened.layer),
        hospital: false,
        approver: None,
    });
    assert_eq!(
        err(stale),
        "This template changed while you were editing. Your changes are still here. Compare, then save again."
    );
}

#[test]
fn requests_never_print_a_pin() {
    let a = Approver { id: "0199".into(), pin: "2468".into() };
    let save = SaveTemplateRequest {
        template: soap_with("x"),
        by: None,
        base_version: None,
        base_layer: None,
        hospital: true,
        approver: Some(a.clone()),
    };
    let refine: RefineRequest = serde_json::from_value(serde_json::json!({
        "llm": "x", "heading": "plan", "request": "y", "approver": { "id": "0199", "pin": "2468" }
    }))
    .unwrap();
    for text in [format!("{a:?}"), format!("{save:?}"), format!("{refine:?}")] {
        assert!(!text.contains("2468"), "{text}");
    }
}
