//! Template store tests (ADR-0024 plan steps 3 to 8). Every test has its own
//! templates folder and profiles folder; personal files go through the vault,
//! so tests hold its lock (plain files) or turn it on with a throwaway key.

use std::path::PathBuf;

use super::*;
use crate::template::{Document, LoadedTemplate};
use crate::users::Role;

struct Env {
    _tmp: tempfile::TempDir,
    store: TemplateStore,
    profiles: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let store = TemplateStore::new(tmp.path().join("templates"));
    let profiles = tmp.path().join("profiles");
    Env { _tmp: tmp, store, profiles }
}

/// The same folders with another set of built-ins (a simulated release).
fn release(e: &Env, change: impl Fn(&mut Vec<(String, String)>)) -> TemplateStore {
    let mut set: Vec<(String, String)> = LoadedTemplate::builtin_ids()
        .map(|id| (id.to_string(), LoadedTemplate::builtin(id).unwrap().to_yaml()))
        .collect();
    change(&mut set);
    TemplateStore::with_builtins(e.store.root().to_path_buf(), set)
}

/// SOAP with a "Pain score" heading, as a new release would ship it.
fn soap_with_pain_score(version: &str) -> String {
    let mut t = soap();
    t.version = version.into();
    let pain: crate::template::Section =
        serde_yaml_ng::from_str("title: Pain score\nkind: statements\n").unwrap();
    t.sections.insert(3, pain);
    LoadedTemplate::new(t).unwrap().to_yaml()
}

fn plain() -> std::sync::MutexGuard<'static, ()> {
    crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn soap() -> Template {
    LoadedTemplate::builtin("soap").unwrap().template
}

fn member(id: &str, name: &str, role: Role) -> Who {
    Who { id: Some(id.into()), name: name.into(), role: Some(role) }
}

fn tech(e: &Env) -> Viewer {
    Viewer::member(member("0199-tech", "Sam", Role::Rvt), e.profiles.clone(), true)
}

fn vet(e: &Env) -> Viewer {
    Viewer::member(member("0199-vet", "Dr Rivera", Role::Dvm), e.profiles.clone(), true)
}

fn save(
    store: &TemplateStore,
    viewer: &Viewer,
    template: Template,
    target: Layer,
) -> Result<Resolved, EngineError> {
    let req = SaveRequest {
        template,
        base_version: None,
        base_layer: None,
        target,
        saved_by: viewer.member.clone(),
        approved_by: None,
    };
    store.save_version(viewer, req)
}

/// SOAP with the Plan instruction changed.
fn changed_soap(text: &str) -> Template {
    let mut t = soap();
    t.sections.iter_mut().find(|s| s.id == "plan").unwrap().instruction = Some(text.into());
    t
}

fn plan_instruction(r: &Resolved) -> Option<String> {
    r.loaded.template.sections.iter().find(|s| s.id == "plan").unwrap().instruction.clone()
}

fn builtin_fingerprint(id: &str) -> String {
    fingerprint(&LoadedTemplate::builtin(id).unwrap().template)
}

// ---- step 3: resolution --------------------------------------------------------

#[test]
fn a_personal_version_is_only_its_owners() {
    let _plain = plain();
    let e = env();
    let saved = save(&e.store, &tech(&e), changed_soap("Rechecks in days."), Layer::Personal).unwrap();
    assert_eq!(saved.layer, Layer::Personal);
    assert_eq!(saved.origin().owner.as_ref().unwrap().name, "Sam");
    assert_eq!(
        plan_instruction(&e.store.resolve_for("soap", &tech(&e)).unwrap()).as_deref(),
        Some("Rechecks in days.")
    );
    let theirs = e.store.resolve_for("soap", &vet(&e)).unwrap();
    assert_eq!(theirs.layer, Layer::Builtin);
    assert_eq!(theirs.origin().fingerprint, builtin_fingerprint("soap"));
    assert_eq!(e.store.resolve_for("soap", &Viewer::alone()).unwrap().layer, Layer::Builtin);
    // Hospital mode: the same person's version is kept but not used.
    let hospital = Viewer { personal: false, ..tech(&e) };
    assert_eq!(e.store.resolve_for("soap", &hospital).unwrap().layer, Layer::Builtin);
    assert_eq!(e.store.resolve_for("soap", &tech(&e)).unwrap().layer, Layer::Personal, "back again");
}

#[test]
fn personal_beats_team_beats_builtin() {
    let _plain = plain();
    let e = env();
    save(&e.store, &Viewer::alone(), changed_soap("Team plan."), Layer::Team).unwrap();
    assert_eq!(
        plan_instruction(&e.store.resolve_for("soap", &vet(&e)).unwrap()).as_deref(),
        Some("Team plan.")
    );
    save(&e.store, &vet(&e), changed_soap("My plan."), Layer::Personal).unwrap();
    let mine = e.store.resolve_for("soap", &vet(&e)).unwrap();
    assert_eq!(mine.layer, Layer::Personal);
    assert_eq!(plan_instruction(&mine).as_deref(), Some("My plan."));
    assert_eq!(mine.original.as_ref().unwrap().layer, Layer::Team, "the team's version is the original");
    assert_eq!(e.store.resolve_for("soap", &tech(&e)).unwrap().layer, Layer::Team);
}

#[test]
fn the_teams_version_of_a_builtin_is_listed_once_and_edited() {
    let _plain = plain();
    let e = env();
    save(&e.store, &Viewer::alone(), changed_soap("Team plan."), Layer::Team).unwrap();
    let list = e.store.list_for(&Viewer::alone());
    let soaps: Vec<&TemplateSummary> = list.iter().filter(|t| t.id == "soap").collect();
    assert_eq!(soaps.len(), 1);
    assert!(soaps[0].edited && soaps[0].builtin && soaps[0].shared);
    assert_eq!(soaps[0].layer, Layer::Team);
    assert_eq!(list.len(), LoadedTemplate::builtin_ids().count());
}

#[test]
fn the_command_line_and_eval_keep_the_shipped_builtins() {
    let _plain = plain();
    let e = env();
    save(&e.store, &Viewer::alone(), changed_soap("Team plan."), Layer::Team).unwrap();
    // LoadedTemplate::resolve is built-in first, whatever any templates folder holds.
    assert_eq!(fingerprint(&LoadedTemplate::resolve("soap").unwrap().template), builtin_fingerprint("soap"));
}

#[test]
fn new_ids_are_unique_across_builtins_team_and_every_profile() {
    let _plain = plain();
    let e = env();
    let mut t = soap();
    t.id = String::new();
    t.name = "SOAP".into();
    let first = save(&e.store, &tech(&e), t.clone(), Layer::Personal).unwrap();
    assert_eq!(first.loaded.template.id, "soap-2", "not the built-in's id");
    assert_eq!(first.layer, Layer::Personal);
    // Another person's new "SOAP" can't take the technician's id either.
    let second = save(&e.store, &vet(&e), t.clone(), Layer::Personal).unwrap();
    assert_eq!(second.loaded.template.id, "soap-3");
    let alone = Viewer { profiles: e.profiles.clone(), ..Viewer::alone() };
    let team = save(&e.store, &alone, t, Layer::Team).unwrap();
    assert_eq!(team.loaded.template.id, "soap-4");
    // Only its maker sees a new template until it's shared (D6).
    assert!(e.store.list_for(&tech(&e)).iter().any(|s| s.id == "soap-2" && !s.shared));
    assert!(!e.store.list_for(&vet(&e)).iter().any(|s| s.id == "soap-2"));
    assert!(e.store.resolve_for("soap-2", &vet(&e)).is_err());
    assert_eq!(e.store.private_owner(&e.profiles, "soap-2").unwrap(), ("0199-tech".into(), "SOAP".into()));
}

#[test]
fn personal_files_are_encrypted_on_disk() {
    crate::vault::tests::with_key(|| {
        let e = env();
        save(&e.store, &tech(&e), changed_soap("Pocket depths in millimetres."), Layer::Personal).unwrap();
        let dir = e.profiles.join("0199-tech").join("templates").join("soap");
        for name in ["template.yaml", "base.yaml"] {
            let raw = std::fs::read(dir.join(name)).unwrap();
            assert!(!String::from_utf8_lossy(&raw).contains("Subjective"), "{name} is plain");
        }
        let mine = e.store.resolve_for("soap", &tech(&e)).unwrap();
        assert_eq!(plan_instruction(&mine).as_deref(), Some("Pocket depths in millimetres."));
        // History decrypts too.
        save(&e.store, &tech(&e), changed_soap("Second."), Layer::Personal).unwrap();
        let history = crate::vault::read_to_string(&dir.join("history").join("1.yaml")).unwrap();
        assert!(history.contains("Pocket depths"));
        // The team layer stays plain YAML.
        save(&e.store, &Viewer::alone(), changed_soap("Team plan."), Layer::Team).unwrap();
        let team = std::fs::read_to_string(e.store.root().join("soap").join("template.yaml")).unwrap();
        assert!(team.contains("Team plan."));
    });
}

#[test]
fn ids_that_could_escape_are_refused_for_personal_paths_too() {
    let _plain = plain();
    let e = env();
    for bad in ["../x", "SOAP", "a/b", "..", &"x".repeat(49)] {
        assert!(e.store.resolve_for(bad, &tech(&e)).is_err(), "{bad}");
        assert!(e.store.reset_version(&tech(&e), bad, Layer::Personal).is_err(), "{bad}");
    }
    let odd = Viewer::member(member("../evil", "X", Role::Rvt), e.profiles.clone(), true);
    assert!(odd.personal_dir("soap").is_none(), "a member id that isn't an id gets no folder");
}

// ---- step 4: saving versions ---------------------------------------------------

#[test]
fn saving_versions_numbers_them_and_keeps_history() {
    let _plain = plain();
    let e = env();
    let first = save(&e.store, &tech(&e), changed_soap("One."), Layer::Personal).unwrap();
    assert_eq!(first.loaded.template.version, "1");
    let based = first.origin().based_on.as_ref().unwrap();
    assert_eq!((based.layer, based.version.as_str()), (Layer::Builtin, "0.1.1"));
    assert_eq!(based.fingerprint, builtin_fingerprint("soap"));
    let dir = e.profiles.join("0199-tech").join("templates").join("soap");
    assert!(dir.join("template.yaml").is_file() && dir.join("base.yaml").is_file());
    // The same again keeps version 1; a change makes version 2 and keeps 1 in history.
    let again = save(&e.store, &tech(&e), changed_soap("One."), Layer::Personal).unwrap();
    assert_eq!(again.loaded.template.version, "1");
    let second = save(&e.store, &tech(&e), changed_soap("Two."), Layer::Personal).unwrap();
    assert_eq!(second.loaded.template.version, "2");
    assert!(dir.join("history").join("1.yaml").is_file());
    assert_eq!(second.origin().based_on, first.origin().based_on, "still based on the same original");
    // Saving the original unchanged makes no version.
    let none = save(&e.store, &vet(&e), soap(), Layer::Personal).unwrap();
    assert_eq!(none.layer, Layer::Builtin);
    assert!(!e.profiles.join("0199-vet").join("templates").join("soap").join("template.yaml").exists());
}

#[test]
fn a_stale_save_is_refused_and_nothing_is_written() {
    let _plain = plain();
    let e = env();
    save(&e.store, &tech(&e), changed_soap("One."), Layer::Personal).unwrap();
    // Another window started from the built-in.
    let req = SaveRequest {
        template: changed_soap("From the other window."),
        base_version: Some("0.1.1".into()),
        base_layer: Some(Layer::Builtin),
        target: Layer::Personal,
        saved_by: None,
        approved_by: None,
    };
    let err = e.store.save_version(&tech(&e), req.clone()).unwrap_err().to_string();
    assert!(err.contains("changed while you were editing"), "{err}");
    assert_eq!(plan_instruction(&e.store.resolve_for("soap", &tech(&e)).unwrap()).as_deref(), Some("One."));
    // The same version number on another layer is still a conflict.
    let wrong_layer =
        SaveRequest { base_version: Some("1".into()), base_layer: Some(Layer::Team), ..req.clone() };
    assert!(e.store.save_version(&tech(&e), wrong_layer).is_err());
    let right = SaveRequest { base_version: Some("1".into()), base_layer: Some(Layer::Personal), ..req };
    assert_eq!(e.store.save_version(&tech(&e), right).unwrap().loaded.template.version, "2");
}

#[test]
fn who_its_for_and_fixed_rules_are_locked_in_a_version() {
    let _plain = plain();
    let e = env();
    let mut client = changed_soap("x");
    client.document = Document::Client;
    let err = save(&e.store, &tech(&e), client, Layer::Personal).unwrap_err().to_string();
    assert!(err.contains("Who it's for is fixed"), "{err}");
    let mut loosened = changed_soap("x");
    loosened.fixed_rules.clear();
    let err = save(&e.store, &tech(&e), loosened, Layer::Personal).unwrap_err().to_string();
    assert!(err.contains("Fixed rules can't be changed"), "{err}");
    let mut team = changed_soap("x");
    team.fixed_rules.push("Something else.".into());
    assert!(save(&e.store, &Viewer::alone(), team, Layer::Team).is_err(), "the team's version too");
}

#[test]
fn twenty_one_saves_keep_twenty_in_history() {
    let _plain = plain();
    let e = env();
    for n in 0..22 {
        save(&e.store, &tech(&e), changed_soap(&format!("Version {n}.")), Layer::Personal).unwrap();
    }
    let history = e.profiles.join("0199-tech").join("templates").join("soap").join("history");
    assert_eq!(std::fs::read_dir(history).unwrap().count(), MAX_HISTORY);
}

#[test]
fn working_alone_a_change_goes_to_the_templates_folder() {
    let _plain = plain();
    let e = env();
    let saved = save(&e.store, &Viewer::alone(), changed_soap("Mine."), Layer::Team).unwrap();
    assert_eq!(saved.layer, Layer::Team);
    assert!(e.store.root().join("soap").join("template.yaml").is_file());
    assert!(e.store.root().join("soap").join("base.yaml").is_file());
    assert!(!e.profiles.exists(), "no profile working alone");
    assert!(save(&e.store, &Viewer::alone(), changed_soap("x"), Layer::Personal).is_err());
}

// ---- step 5: reset, undo, keep, delete -------------------------------------------

#[test]
fn reset_and_undo() {
    let _plain = plain();
    let e = env();
    let mine = save(&e.store, &tech(&e), changed_soap("Mine."), Layer::Personal).unwrap();
    let token = e.store.reset_version(&tech(&e), "soap", Layer::Personal).unwrap();
    let after = e.store.resolve_for("soap", &tech(&e)).unwrap();
    assert_eq!(after.layer, Layer::Builtin);
    assert_eq!(after.origin().fingerprint, builtin_fingerprint("soap"), "exactly the built-in");
    let back = e.store.undo_reset(&tech(&e), &token).unwrap();
    assert_eq!(back.origin().fingerprint, mine.origin().fingerprint);
    assert_eq!(back.loaded.template.version, mine.loaded.template.version);
    assert!(back.has_base);
    let again = e.store.undo_reset(&tech(&e), &token);
    assert!(again.is_err(), "a token works once");
    // Reset again, then a new version numbers after the history.
    e.store.reset_version(&tech(&e), "soap", Layer::Personal).unwrap();
    let err = e.store.reset_version(&tech(&e), "soap", Layer::Personal).unwrap_err().to_string();
    assert!(err.contains("already using the original"), "{err}");
    let next = save(&e.store, &tech(&e), changed_soap("Later."), Layer::Personal).unwrap();
    assert_eq!(next.loaded.template.version, "2");
}

#[test]
fn deleting_is_for_templates_without_an_original() {
    let _plain = plain();
    let e = env();
    assert!(e.store.delete_template(&Viewer::alone(), "soap", Layer::Team).is_err(), "a built-in");
    save(&e.store, &tech(&e), changed_soap("Mine."), Layer::Personal).unwrap();
    let err = e.store.delete_template(&tech(&e), "soap", Layer::Personal).unwrap_err().to_string();
    assert!(err.contains("Reset it"), "{err}");
    let mut own = soap();
    own.id = String::new();
    own.name = "My dental".into();
    save(&e.store, &tech(&e), own, Layer::Personal).unwrap();
    assert!(e.store.delete_template(&vet(&e), "my-dental", Layer::Personal).is_err(), "someone else's");
    e.store.delete_template(&tech(&e), "my-dental", Layer::Personal).unwrap();
    assert!(e.store.resolve_for("my-dental", &tech(&e)).is_err());
}

// ---- step 6: when the original changes ---------------------------------------------

#[test]
fn a_newer_original_is_noticed_compared_kept_and_noticed_again() {
    let _plain = plain();
    let e = env();
    save(&e.store, &vet(&e), changed_soap("Mine."), Layer::Personal).unwrap();
    let new = release(&e, |set| {
        set.iter_mut().find(|(i, _)| i == "soap").unwrap().1 = soap_with_pain_score("0.1.9")
    });
    let r = new.resolve_for("soap", &vet(&e)).unwrap();
    let changed = r.original_changed.clone().unwrap();
    assert_eq!(changed.reason, OriginalChange::NewerBuiltin);
    assert_eq!(changed.version, "0.1.9");
    assert!(new.list_for(&vet(&e)).iter().find(|t| t.id == "soap").unwrap().original_changed.is_some());
    // What changed in the original: exactly one added heading.
    let diff = new.compare(&vet(&e), "soap", true).unwrap();
    assert_eq!(diff.headings.len(), 1);
    assert_eq!(diff.headings[0].title, "Pain score");
    assert_eq!(diff.headings[0].changes, vec![crate::template_diff::HeadingChange::Added]);
    assert!(diff.note.is_empty());
    // Yours and the original: the heading, and the Plan instruction.
    let yours = new.compare(&vet(&e), "soap", false).unwrap();
    assert_eq!(yours.headings.len(), 2);
    // Keep mine: no notice, and the version number stays.
    let kept = new.keep_version(&vet(&e), "soap", Layer::Personal, &changed.fingerprint).unwrap();
    assert!(kept.original_changed.is_none());
    assert_eq!(kept.loaded.template.version, "1");
    // Another release: the notice is back.
    let newer = release(&e, |set| {
        set.iter_mut().find(|(i, _)| i == "soap").unwrap().1 =
            soap_with_pain_score("0.2.0").replace("Pain score", "Pain")
    });
    assert!(newer.resolve_for("soap", &vet(&e)).unwrap().original_changed.is_some());
}

#[test]
fn each_change_to_the_teams_version_gives_its_notice() {
    let _plain = plain();
    let e = env();
    save(&e.store, &vet(&e), changed_soap("Mine."), Layer::Personal).unwrap();
    let reason = || e.store.resolve_for("soap", &vet(&e)).unwrap().original_changed.map(|c| c.reason);
    assert_eq!(reason(), None);
    // The team makes its version.
    save(&e.store, &Viewer::alone(), changed_soap("Team one."), Layer::Team).unwrap();
    assert_eq!(reason(), Some(OriginalChange::TeamChanged));
    // Keep mine, then the team changes it again.
    let fp = e.store.resolve_for("soap", &vet(&e)).unwrap().original_changed.unwrap().fingerprint;
    e.store.keep_version(&vet(&e), "soap", Layer::Personal, &fp).unwrap();
    assert_eq!(reason(), None);
    save(&e.store, &Viewer::alone(), changed_soap("Team two."), Layer::Team).unwrap();
    assert_eq!(reason(), Some(OriginalChange::TeamChanged));
    // A version based on the team's: the team resets to the built-in.
    save(&e.store, &tech(&e), changed_soap("Tech."), Layer::Personal).unwrap();
    e.store.reset_team("soap").unwrap();
    let tech_reason = e.store.resolve_for("soap", &tech(&e)).unwrap().original_changed.unwrap().reason;
    assert_eq!(tech_reason, OriginalChange::TeamReset);
    // A version of a team template that's then deleted still drafts.
    let mut dental = soap();
    dental.id = String::new();
    dental.name = "Dental procedure (team)".into();
    let id = save(&e.store, &Viewer::alone(), dental, Layer::Team).unwrap().loaded.template.id;
    let mut mine = e.store.resolve_for(&id, &vet(&e)).unwrap().loaded.template;
    mine.instructions = "Mine.".into();
    save(&e.store, &vet(&e), mine, Layer::Personal).unwrap();
    e.store.delete(&id).unwrap();
    let orphan = e.store.resolve_for(&id, &vet(&e)).unwrap();
    assert_eq!(orphan.original_changed.unwrap().reason, OriginalChange::TeamDeleted);
    assert_eq!(orphan.layer, Layer::Personal);
}

#[test]
fn a_release_that_removes_a_builtin_leaves_versions_working() {
    let _plain = plain();
    let e = env();
    save(&e.store, &vet(&e), changed_soap("Mine."), Layer::Personal).unwrap();
    let gone = release(&e, |set| set.retain(|(i, _)| i != "soap"));
    let r = gone.resolve_for("soap", &vet(&e)).unwrap();
    assert_eq!(r.original_changed.unwrap().reason, OriginalChange::NoLongerBuiltin);
    assert!(!r.loaded.template.fixed_rules.is_empty(), "its own copy of the fixed rules");
    assert!(crate::note::system_prompt(&r.loaded).contains("Differentials only if"));
}

// ---- step 7: standards for you and for the team --------------------------------------

#[test]
fn with_nothing_personal_the_prompt_is_the_builtins() {
    let _plain = plain();
    let e = env();
    for id in LoadedTemplate::builtin_ids() {
        let r = e.store.resolve_for(id, &vet(&e)).unwrap();
        let shipped = LoadedTemplate::builtin(id).unwrap();
        assert_eq!(crate::note::system_prompt(&r.loaded), crate::note::system_prompt(&shipped), "{id}");
    }
}

#[test]
fn personal_standards_and_muting_reach_only_their_owners_prompt() {
    let _plain = plain();
    let e = env();
    let team = e
        .store
        .add_standard_for(
            &vet(&e),
            "soap",
            "plan/medications",
            "List fluids with rate",
            Some("Dr Rivera"),
            Scope::Team,
        )
        .unwrap();
    e.store
        .add_standard_for(&tech(&e), "soap", "", "Use metric units", Some("Sam"), Scope::Personal)
        .unwrap();
    let prompt = |v: &Viewer| crate::note::system_prompt(&e.store.resolve_for("soap", v).unwrap().loaded);
    assert!(prompt(&tech(&e)).contains("- Standard: Use metric units\n"));
    assert!(!prompt(&vet(&e)).contains("Use metric units"));
    assert!(prompt(&tech(&e)).contains("Team standard: List fluids with rate."));
    e.store.mute_standard(&tech(&e), "soap", &team.id, true).unwrap();
    assert!(!prompt(&tech(&e)).contains("List fluids with rate"));
    assert!(prompt(&vet(&e)).contains("Team standard: List fluids with rate."));
    assert_eq!(e.store.resolve_for("soap", &tech(&e)).unwrap().muted, vec![team.id.clone()]);
    e.store.mute_standard(&tech(&e), "soap", &team.id, false).unwrap();
    assert!(prompt(&tech(&e)).contains("List fluids with rate"));
    // Personal standards apply without a personal version, and stay out in hospital mode.
    assert_eq!(e.store.resolve_for("soap", &tech(&e)).unwrap().layer, Layer::Builtin);
    let hospital = Viewer { personal: false, ..tech(&e) };
    assert!(
        !crate::note::system_prompt(&e.store.resolve_for("soap", &hospital).unwrap().loaded)
            .contains("metric")
    );
    // Removing: one's own, then the team's.
    let mine = e.store.resolve_for("soap", &tech(&e)).unwrap().loaded.template.standards;
    let own = mine.iter().find(|s| s.scope == Scope::Personal).unwrap();
    assert_eq!(e.store.standard_scope(&tech(&e), "soap", &own.id).unwrap(), Some(Scope::Personal));
    e.store.remove_standard_for(&tech(&e), "soap", &own.id).unwrap();
    assert_eq!(e.store.standard_scope(&tech(&e), "soap", &team.id).unwrap(), Some(Scope::Team));
}

#[test]
fn a_standard_for_a_heading_the_version_removed_isnt_applied() {
    let _plain = plain();
    let e = env();
    e.store
        .add_standard_for(&tech(&e), "soap", "plan/medications", "Fluids with rate", None, Scope::Personal)
        .unwrap();
    let mut t = soap();
    let plan = t.sections.iter_mut().find(|s| s.id == "plan").unwrap();
    plan.parts.retain(|p| p.key != "medications");
    save(&e.store, &tech(&e), t, Layer::Personal).unwrap();
    let r = e.store.resolve_for("soap", &tech(&e)).unwrap();
    assert_eq!(r.not_used.len(), 1);
    assert!(!crate::note::system_prompt(&r.loaded).contains("Fluids with rate"));
    // A standard for everyone must name a heading the team's template has.
    assert!(e.store.add_standard_for(&tech(&e), "soap", "plan/nope", "x", None, Scope::Team).is_err());
}

// ---- step 8: the rules that always apply -------------------------------------------

#[test]
fn a_hand_edited_copy_of_fixed_rules_still_drafts_with_the_originals() {
    let _plain = plain();
    let e = env();
    save(&e.store, &tech(&e), changed_soap("Mine."), Layer::Personal).unwrap();
    let file = e.profiles.join("0199-tech").join("templates").join("soap").join("template.yaml");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("Differentials only if"));
    std::fs::write(
        &file,
        text.replace("Differentials only if the veterinarian voiced them.", "Anything goes."),
    )
    .unwrap();
    let r = e.store.resolve_for("soap", &tech(&e)).unwrap();
    assert_eq!(r.loaded.template.fixed_rules, soap().fixed_rules);
    let prompt = crate::note::system_prompt(&r.loaded);
    assert!(prompt.contains("Differentials only if") && !prompt.contains("Anything goes"));
}

#[test]
fn every_prompt_starts_its_rules_with_the_core_then_the_fixed_rules() {
    for id in LoadedTemplate::builtin_ids() {
        let t = LoadedTemplate::builtin(id).unwrap();
        let prompt = crate::note::system_prompt(&t);
        let block = prompt.split("Rules that always apply:\n").nth(1).unwrap();
        let lines: Vec<&str> =
            block.lines().take(crate::template::CORE_RULES.len() + t.template.fixed_rules.len()).collect();
        let want: Vec<String> = crate::template::CORE_RULES
            .iter()
            .map(|r| format!("- {r}"))
            .chain(t.template.fixed_rules.iter().map(|r| format!("- {r}")))
            .collect();
        assert_eq!(lines, want, "{id}");
        assert!(prompt.contains(crate::note::PRECEDENCE), "{id}");
    }
    assert_eq!(
        crate::template::CORE_RULES.len(),
        crate::template::CORE_RULE_SUMMARIES.len(),
        "every core rule needs a plain-language summary"
    );
}

// ---- sharing and profiles -------------------------------------------------------

#[test]
fn sharing_replaces_the_teams_version_or_adds_a_template() {
    let _plain = plain();
    let e = env();
    save(&e.store, &tech(&e), changed_soap("Tech plan."), Layer::Personal).unwrap();
    let approver = member("0199-vet", "Dr Rivera", Role::Dvm);
    let shared = e
        .store
        .share(&tech(&e), "soap", ShareMode::Replace, None, tech(&e).member, Some(approver.clone()))
        .unwrap();
    assert_eq!(shared.layer, Layer::Team, "the personal version retired");
    let team = e.store.resolve_for("soap", &vet(&e)).unwrap();
    assert_eq!(plan_instruction(&team).as_deref(), Some("Tech plan."));
    assert_eq!(team.origin().saved_by.as_ref().unwrap().name, "Sam");
    assert_eq!(team.origin().approved_by.as_ref().unwrap(), &approver);
    // Add: a new team template, and the personal version stays.
    save(&e.store, &tech(&e), changed_soap("Short plan."), Layer::Personal).unwrap();
    let added = e.store.share(&tech(&e), "soap", ShareMode::Add, Some("Short SOAP"), None, None).unwrap();
    assert_eq!(added.loaded.template.id, "short-soap");
    assert_eq!(added.layer, Layer::Team);
    assert_eq!(e.store.resolve_for("soap", &tech(&e)).unwrap().layer, Layer::Personal);
    // A template only the technician used is shared with its id.
    let mut own = soap();
    own.id = String::new();
    own.name = "Sam's dental".into();
    let id = save(&e.store, &tech(&e), own, Layer::Personal).unwrap().loaded.template.id;
    e.store.share(&tech(&e), &id, ShareMode::Replace, None, None, None).unwrap();
    assert_eq!(e.store.resolve_for(&id, &vet(&e)).unwrap().layer, Layer::Team);
}

#[test]
fn a_profile_summary_counts_and_removal_takes_it_all() {
    let _plain = plain();
    let e = env();
    save(&e.store, &tech(&e), changed_soap("x"), Layer::Personal).unwrap();
    let mut own = soap();
    own.id = String::new();
    own.name = "Mine".into();
    save(&e.store, &tech(&e), own, Layer::Personal).unwrap();
    e.store.add_standard_for(&tech(&e), "dental", "", "Triadan numbers", None, Scope::Personal).unwrap();
    let summary = e.store.profile_summary(&e.profiles, "0199-tech");
    assert_eq!(summary, ProfileSummary { versions: 1, own_templates: 1, standards: 1 });
    e.store.remove_profile(&e.profiles, "0199-tech").unwrap();
    assert!(!e.profiles.join("0199-tech").exists());
    assert!(e.store.remove_profile(&e.profiles, "..").is_err());
}

#[test]
fn a_release_build_ignores_the_test_builtins_folder() {
    let var = Some(std::ffi::OsString::from("C:/anywhere"));
    if cfg!(feature = "test-builtins") {
        assert!(test_builtins_dir(var).is_some());
    } else {
        assert!(test_builtins_dir(var).is_none());
    }
}

#[test]
fn existing_files_load_unchanged() {
    let _plain = plain();
    let e = env();
    // A team template and a standards file in today's format.
    let dir = e.store.root().join("dental-procedure");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("template.yaml"), "id: dental-procedure\nversion: '3'\nname: Dental procedure\nsections:\n  - { title: Anaesthesia, kind: medications }\n").unwrap();
    std::fs::write(
        dir.join("standards.yaml"),
        "- id: s1\n  heading: anaesthesia\n  text: Doses in mg/kg\n  added_at: 2026-10-01T10:00:00-04:00\n",
    )
    .unwrap();
    let r = e.store.resolve_for("dental-procedure", &vet(&e)).unwrap();
    assert_eq!((r.layer, r.loaded.template.version.as_str()), (Layer::Team, "3"));
    assert_eq!(r.loaded.template.standards[0].scope, Scope::Team);
    assert!(r.original.is_none());
    // A template.yaml copied by hand into a built-in's folder is the team's version.
    std::fs::create_dir_all(e.store.root().join("soap")).unwrap();
    std::fs::write(
        e.store.root().join("soap").join("template.yaml"),
        changed_soap("By hand.").to_yaml_for_test(),
    )
    .unwrap();
    let soap_now = e.store.resolve_for("soap", &vet(&e)).unwrap();
    assert_eq!(soap_now.layer, Layer::Team);
    assert!(!soap_now.has_base && soap_now.original_changed.is_none());
    assert!(e.store.compare(&vet(&e), "soap", true).is_err(), "no copy of the original");
    assert!(e.store.compare(&vet(&e), "soap", false).is_ok());
}

/// A template as YAML (tests).
trait ToYaml {
    fn to_yaml_for_test(&self) -> String;
}

impl ToYaml for Template {
    fn to_yaml_for_test(&self) -> String {
        serde_yaml_ng::to_string(self).unwrap()
    }
}
