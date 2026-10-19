//! The clinical re-attack on the tooth checks (ADR-0021 decision 7), kept as
//! permanent tests: each phrasing that once reached the chart with no flag,
//! now with the signal it gets, and the correct reading of the same line,
//! which stays clean. The vet's confirmation of every drafted tooth (decision
//! 10) is the gate; these flags tell the vet where to look.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::*;
use crate::faithfulness::{FaithfulnessReport, check_for_pet, check_full};

const DOG: Option<Dentition> = Some(Dentition::Canine);
const CAT: Option<Dentition> = Some(Dentition::Feline);

fn transcript(lines: &[&str]) -> Transcript {
    Transcript {
        segments: lines
            .iter()
            .enumerate()
            .map(|(i, text)| Segment {
                id: i as u32,
                start: i as f32,
                end: i as f32 + 1.0,
                speaker: Some(0),
                text: text.to_string(),
                hints: Vec::new(),
                check: None,
            })
            .collect(),
    }
}

/// A drafted item: `fields` holds any of detail, grade, mm, planned.
fn item(tooth: &str, finding: &str, fields: Value, evidence: &[u32]) -> Value {
    let mut v = json!({"tooth": tooth, "text": format!("{tooth} {finding}"), "finding": finding,
        "detail": null, "grade": null, "mm": null, "planned": false, "evidence": evidence});
    for (k, x) in fields.as_object().cloned().unwrap_or_default() {
        v[k] = x;
    }
    v
}

fn run(lines: &[&str], items: Vec<Value>, dentition: Option<Dentition>) -> FaithfulnessReport {
    check_full(&json!({ "dental_chart": items }), &transcript(lines), &HashSet::new(), true, dentition)
}

/// Every signal on item `n`: its tooth flags, and "unsupported" or
/// "mismatch" for a tooth said nowhere or on another line.
fn signals(r: &FaithfulnessReport, n: usize) -> Vec<String> {
    let p = format!("/dental_chart/{n}");
    let mut out: Vec<String> = r
        .tooth_flags
        .iter()
        .filter(|f| f.path == p)
        .map(|f| serde_json::to_value(f.kind).unwrap().as_str().unwrap().to_string())
        .collect();
    if r.unsupported_numbers.iter().any(|u| u.path == p) {
        out.push("unsupported".into());
    }
    if r.citation_mismatches.iter().any(|u| u.path == p) {
        out.push("mismatch".into());
    }
    out
}

/// One drafted item on one line: its signals.
fn one(line: &str, tooth: &str, finding: &str, fields: Value, dentition: Option<Dentition>) -> Vec<String> {
    signals(&run(&[line], vec![item(tooth, finding, fields, &[0])], dentition), 0)
}

#[track_caller]
fn flagged_as(got: Vec<String>, kind: &str, line: &str) {
    assert!(got.iter().any(|k| k == kind), "{line}: expected {kind}, got {got:?}");
}

#[track_caller]
fn clean(got: Vec<String>, line: &str) {
    assert!(got.is_empty(), "{line}: expected no flag, got {got:?}");
}

fn read(text: &str) -> Vec<String> {
    teeth_in(text).into_iter().flat_map(|m| m.teeth).collect()
}

// --- Blocker: vague amounts and counts around a group ---

#[test]
fn an_amount_of_a_group_names_no_tooth_and_gets_no_hint() {
    for text in [
        "A handful of the lower premolars are mobile.",
        "A pair of lower incisors are loose.",
        "A pair of upper incisors are missing.",
        "A handful of upper premolars are mobile.",
        "A number of upper premolars have stage three disease.",
        "The majority of the lower premolars are missing.",
        "All the lower incisors but one are missing.",
        "The lower incisors are mostly missing.",
        "The lower incisors, two of them are missing.",
        "At least two upper incisors are loose.",
        "A smattering of the lower incisors are loose.",
        "Some of the lower incisors are missing, the others are fine.",
    ] {
        assert!(teeth_in(text).is_empty(), "{text}: {:?}", teeth_in(text));
        assert!(hint_teeth(text).is_empty(), "{text}: hint {:?}", hint_teeth(text));
    }
    // Charted anyway: a tooth said nowhere blocks signing.
    let r = run(
        &["A handful of the lower premolars are mobile."],
        vec![item("305", "mobility", json!({}), &[0]), item("408", "mobility", json!({}), &[0])],
        DOG,
    );
    assert!(!r.passed);
    flagged_as(signals(&r, 0), "unsupported", "a handful of");
    flagged_as(signals(&r, 1), "unsupported", "a handful of");
    flagged_as(
        one("A pair of lower incisors are loose.", "301", "mobility", json!({}), CAT),
        "unsupported",
        "a pair of",
    );
    // The whole group still reads as the group.
    assert_eq!(read("All the lower incisors are mobile."), ["301", "302", "303", "401", "402", "403"]);
    assert_eq!(read("Both lower canines are fractured."), ["304", "404"]);
    assert_eq!(read("The six lower incisors are loose."), ["301", "302", "303", "401", "402", "403"]);
    assert_eq!(read("Extraction of the lower incisors, closed."), ["301", "302", "303", "401", "402", "403"]);
    assert_eq!(
        read("The lower incisors are all mobile, grade three."),
        ["301", "302", "303", "401", "402", "403"]
    );
    assert_eq!(
        read("The lower incisors, all of them are loose."),
        ["301", "302", "303", "401", "402", "403"]
    );
}

// --- Blocker: exceptions to a group ---

#[test]
fn teeth_named_as_exceptions_leave_the_group() {
    assert_eq!(
        read("All the lower incisors are missing except four oh one."),
        ["301", "302", "303", "402", "403", "401"]
    );
    assert_eq!(
        read("Every upper premolar is fractured apart from one oh six."),
        ["105", "107", "108", "205", "206", "207", "208", "106"]
    );
    assert!(teeth_in("All the lower incisors are missing except one.").is_empty(), "which one isn't said");
    let cases: &[(&str, &str, &str, &str)] = &[
        // (line, wrong tooth, wrong finding, the right finding for that tooth or "")
        ("All the lower incisors are missing except four oh one.", "401", "missing", ""),
        ("Every upper premolar is fractured apart from one oh six.", "106", "fracture", ""),
        ("The lower incisors are all missing other than four oh one.", "401", "missing", ""),
        (
            "All the upper premolars are normal except two oh eight, which is fractured.",
            "208",
            "normal",
            "fracture",
        ),
        ("All the upper premolars are normal. Two oh eight is fractured.", "208", "normal", "fracture"),
    ];
    for (line, tooth, wrong, right) in cases {
        let got = one(line, tooth, wrong, json!({}), DOG);
        assert!(!got.is_empty(), "{line}: {tooth} {wrong} unflagged");
        if !right.is_empty() {
            clean(one(line, tooth, right, json!({}), DOG), line);
        }
    }
    // The rest of the group keeps the group's finding.
    clean(
        one("All the lower incisors are missing except four oh one.", "402", "missing", json!({}), DOG),
        "402",
    );
    clean(
        one("Every upper premolar is fractured apart from one oh six.", "107", "fracture", json!({}), DOG),
        "107",
    );
    // The E4B cat draft: 102 charted normal from "all fine except one oh two".
    let line = "Most of the lower incisors are missing; the upper incisors are all fine except one oh two.";
    assert!(!one(line, "102", "normal", json!({}), CAT).is_empty(), "{line}");
    clean(one(line, "101", "normal", json!({}), CAT), line);
}

// --- Blocker: negation in every position ---

#[test]
fn a_finding_said_to_be_absent_is_negated_wherever_the_negation_sits() {
    let cases: &[(&str, &str, &str, Option<Dentition>)] = &[
        ("Neither one oh eight nor two oh eight is fractured.", "208", "fracture", DOG),
        ("Neither one oh eight nor two oh eight is fractured.", "108", "fracture", DOG),
        ("Two oh six, mobility not detected.", "206", "mobility", DOG),
        ("Resorption not seen on four oh seven.", "407", "resorption", CAT),
        ("Resorption on three oh seven was ruled out.", "307", "resorption", CAT),
        ("We ruled out resorption on three oh seven.", "307", "resorption", CAT),
        ("No fracture, mobility, or resorption on three oh seven.", "307", "mobility", CAT),
        ("No fracture, mobility, or resorption on three oh seven.", "307", "resorption", CAT),
        ("No fracture, mobility, or resorption on three oh seven.", "307", "fracture", CAT),
        ("Three oh nine, mobility: none.", "309", "mobility", DOG),
        ("Three oh nine mobility negative.", "309", "mobility", DOG),
        ("Three oh seven, resorption: absent.", "307", "resorption", CAT),
        ("Absence of resorption on three oh seven.", "307", "resorption", CAT),
        ("Nothing broken on one oh eight.", "108", "fracture", DOG),
        ("One oh eight is fracture-free.", "108", "fracture", DOG),
        ("One oh eight has a stain rather than a fracture.", "108", "fracture", DOG),
        ("Is one oh eight fractured? No, it's just stained.", "108", "fracture", DOG),
        ("Three oh nine lacks any mobility.", "309", "mobility", DOG),
        ("Three oh nine is non-mobile.", "309", "mobility", DOG),
        ("Three oh nine, mobility wasn't detected.", "309", "mobility", DOG),
    ];
    for (line, tooth, finding, d) in cases {
        flagged_as(one(line, tooth, finding, json!({}), *d), "negated", line);
    }
    // A type ruled out by its own words.
    for (line, tooth, fields) in [
        ("Two oh four has a crown fracture, pulp intact.", "204", json!({"detail": "complicated_crown"})),
        (
            "Two oh four has a crown fracture, the pulp wasn't exposed.",
            "204",
            json!({"detail": "complicated_crown"}),
        ),
        ("Extracted three oh eight, a flap wasn't needed.", "308", json!({"detail": "open"})),
    ] {
        let finding = if tooth == "308" { "extraction" } else { "fracture" };
        flagged_as(one(line, tooth, finding, fields, DOG), "negated", line);
    }
    // What was said stays clean.
    clean(one("Three oh nine is mobile.", "309", "mobility", json!({}), DOG), "mobile");
    clean(
        one(
            "Two oh four has a crown fracture, pulp intact.",
            "204",
            "fracture",
            json!({"detail": "uncomplicated_crown"}),
            DOG,
        ),
        "pulp intact",
    );
    clean(
        one("Extracted three oh eight, a flap wasn't needed.", "308", "extraction", json!({}), DOG),
        "no flap",
    );
}

fn one_dog(line: &str, tooth: &str, finding: &str, fields: Value) -> Vec<String> {
    one(line, tooth, finding, fields, DOG)
}

// --- Blocker: planned, declined and earlier extractions ---

#[test]
fn plans_declines_and_earlier_visits_are_not_done_today() {
    for line in [
        "Consider extracting three oh eight.",
        "Three oh eight will need to be extracted.",
        "Three oh eight should be extracted.",
        "Three oh eight needs extraction.",
        "Extraction is indicated for three oh eight.",
        "Three oh eight is to be extracted.",
        "Three oh eight, extraction pending owner approval.",
        "Extract three oh eight at the next dental.",
        "Discussed extracting three oh eight with the owner.",
    ] {
        flagged_as(one_dog(line, "308", "extraction", json!({})), "planned", line);
        clean(one_dog(line, "308", "extraction", json!({"planned": true})), line);
    }
    flagged_as(
        one_dog("One oh four is a candidate for crown amputation.", "104", "crown_amputation", json!({})),
        "planned",
        "candidate",
    );
    for line in [
        "Owner declined extraction of three oh eight.",
        "We decided against extracting three oh eight.",
        "Elected to monitor three oh eight instead of extracting.",
    ] {
        flagged_as(one_dog(line, "308", "extraction", json!({})), "negated", line);
    }
    for (line, tooth) in [
        ("Three oh eight was extracted at her last dental two years ago.", "308"),
        ("Two oh nine was extracted at her last dental.", "209"),
    ] {
        flagged_as(one_dog(line, tooth, "extraction", json!({})), "history", line);
        // History is about treatments: a missing tooth now is not flagged as history.
        assert!(!one_dog(line, tooth, "missing", json!({})).iter().any(|k| k == "history"), "{line}");
    }
    flagged_as(
        one_dog("Three oh eight was extracted.", "308", "extraction", json!({"planned": true})),
        "not_planned",
        "done as plan",
    );
    // Done today stays clean.
    clean(one_dog("Three oh eight was extracted.", "308", "extraction", json!({})), "extracted");
    clean(
        one_dog(
            "Extracting two oh eight surgically with a flap.",
            "208",
            "extraction",
            json!({"detail": "open"}),
        ),
        "surgical",
    );
    clean(
        one_dog(
            "The lower incisors come out closed, no sectioning.",
            "401",
            "extraction",
            json!({"detail": "closed"}),
        ),
        "come out",
    );
}

// --- Major: possible, conditional and reported findings ---

#[test]
fn a_finding_said_only_as_possible_is_uncertain() {
    let cases: &[(&str, &str, &str, Value, Option<Dentition>)] = &[
        (
            "Possible root fracture on one oh four, we'll radiograph it.",
            "104",
            "fracture",
            json!({"detail": "root"}),
            DOG,
        ),
        (
            "Need to radiograph one oh eight to rule out a root fracture.",
            "108",
            "fracture",
            json!({"detail": "root"}),
            DOG,
        ),
        ("Possible resorption on three oh seven, we'll radiograph it.", "307", "resorption", json!({}), CAT),
        (
            "If three oh seven has resorption on the radiograph, we'll extract it.",
            "307",
            "resorption",
            json!({}),
            CAT,
        ),
        ("Check three oh nine for mobility.", "309", "mobility", json!({}), DOG),
        ("Owner thinks the upper left canine is broken.", "204", "fracture", json!({}), DOG),
        ("Is one oh eight loose?", "108", "mobility", json!({}), DOG),
    ];
    for (line, tooth, finding, fields, d) in cases {
        flagged_as(one(line, tooth, finding, fields.clone(), *d), "uncertain", line);
    }
    clean(
        one_dog("One oh four has a root fracture.", "104", "fracture", json!({"detail": "root"})),
        "root fracture",
    );
}

// --- Major: cue words that meant something else ---

#[test]
fn cue_words_mean_their_finding_only() {
    let cases: &[(&str, &str, &str, Value, Option<Dentition>)] = &[
        (
            "Socket at one oh six left open to heal after the extraction.",
            "106",
            "extraction",
            json!({"detail": "open"}),
            DOG,
        ),
        ("Removed the calculus from three oh eight.", "308", "extraction", json!({}), DOG),
        ("Can you pull up the radiograph of three oh eight?", "308", "extraction", json!({}), DOG),
        ("Pull up the radiograph of three oh eight.", "308", "extraction", json!({}), DOG),
        ("Three oh eight, fracture ruled out.", "308", "extraction", json!({}), DOG),
        ("Four oh four sticks out a bit.", "404", "extraction", json!({}), DOG),
        ("The swelling around two oh eight is gone.", "208", "missing", json!({}), DOG),
        ("One oh four, the gum is pink and healthy.", "104", "discoloured", json!({}), DOG),
        ("Stomatitis, with an ulcerated lesion next to three oh seven.", "307", "resorption", json!({}), CAT),
        ("Three oh seven has stage two resorption.", "307", "periodontal_stage", json!({"grade": 2}), CAT),
    ];
    for (line, tooth, finding, fields, d) in cases {
        let got = one(line, tooth, finding, fields.clone(), *d);
        assert!(!got.is_empty(), "{line}: {tooth} {finding} unflagged");
    }
    // The gum, not the tooth.
    flagged_as(
        one_dog(
            "One oh eight has a slab fracture, but the gum around it looks healthy.",
            "108",
            "normal",
            json!({}),
        ),
        "cue_missing",
        "gum",
    );
    // What each line does say stays clean.
    clean(
        one("Three oh seven has stage two resorption.", "307", "resorption", json!({"grade": 2}), CAT),
        "TR2",
    );
    clean(
        one_dog(
            "Extracting one oh eight surgically with a flap.",
            "108",
            "extraction",
            json!({"detail": "open"}),
        ),
        "open",
    );
    clean(one_dog("One oh four is a pink tooth.", "104", "discoloured", json!({})), "pink tooth");
    clean(
        one_dog(
            "Three oh nine has stage three periodontal disease.",
            "309",
            "periodontal_stage",
            json!({"grade": 3}),
        ),
        "PD3",
    );
}

// --- Major: findings, grades and millimetres said about the next tooth ---

#[test]
fn words_just_before_a_tooth_belong_to_it() {
    let cases: &[(&str, &str, &str, Value, &str, &str, Value)] = &[
        // (line, wrong tooth, wrong finding, fields, right tooth, right finding, fields)
        (
            "Two oh nine looks fine next to the fractured two oh eight.",
            "209",
            "fracture",
            json!({}),
            "208",
            "fracture",
            json!({}),
        ),
        (
            "Fracture on one oh eight with a missing two oh eight.",
            "108",
            "missing",
            json!({}),
            "208",
            "missing",
            json!({}),
        ),
        (
            "Severe calculus on one oh eight with a slab fracture on one oh nine.",
            "108",
            "fracture",
            json!({}),
            "109",
            "fracture",
            json!({}),
        ),
        (
            "A nine millimetre pocket on three oh nine with three millimetres on four oh nine.",
            "309",
            "probing_depth",
            json!({"mm": 3.0}),
            "409",
            "probing_depth",
            json!({"mm": 3.0}),
        ),
        (
            "Three oh nine is fractured, and four oh nine is also mobile.",
            "409",
            "fracture",
            json!({}),
            "409",
            "mobility",
            json!({}),
        ),
        (
            "Three oh nine has grade two mobility, and four oh nine has grade two furcation.",
            "409",
            "mobility",
            json!({"grade": 2}),
            "409",
            "furcation",
            json!({"grade": 2}),
        ),
    ];
    for (line, wt, wf, wfields, rt, rf, rfields) in cases {
        assert!(!one_dog(line, wt, wf, wfields.clone()).is_empty(), "{line}: {wt} {wf} unflagged");
        clean(one_dog(line, rt, rf, rfields.clone()), line);
    }
    // The other way round on the first line, and the elliptical clauses.
    clean(
        one_dog("Two oh nine looks fine next to the fractured two oh eight.", "209", "normal", json!({})),
        "209 fine",
    );
    clean(
        one_dog(
            "A nine millimetre pocket on three oh nine with three millimetres on four oh nine.",
            "309",
            "probing_depth",
            json!({"mm": 9.0}),
        ),
        "309 9 mm",
    );
    clean(one_dog("Three oh nine is fractured and four oh nine too.", "409", "fracture", json!({})), "too");
    // Paired by order: flagged, both ways.
    for (line, items) in [
        (
            "Three oh nine and four oh nine are mobile, grade one and grade three respectively.",
            vec![("309", json!({"grade": 3})), ("409", json!({"grade": 1})), ("309", json!({"grade": 1}))],
        ),
        (
            "One oh eight and two oh eight: the first is fractured, the second is missing.",
            vec![("208", json!({})), ("108", json!({}))],
        ),
    ] {
        for (tooth, fields) in items {
            let finding = if line.contains("mobile") { "mobility" } else { "fracture" };
            flagged_as(one_dog(line, tooth, finding, fields), "ambiguous", line);
        }
    }
}

#[test]
fn a_grade_or_measure_belongs_to_the_finding_said_next_to_it() {
    let cases: &[(&str, &str, &str, Value, Option<Dentition>)] = &[
        (
            "Three oh nine, stage three periodontal disease, mobility grade one.",
            "309",
            "periodontal_stage",
            json!({"grade": 1}),
            DOG,
        ),
        (
            "Three oh nine, stage three periodontal disease, mobility grade one.",
            "309",
            "mobility",
            json!({"grade": 3}),
            DOG,
        ),
        ("Three oh seven has stage two resorption, type one.", "307", "resorption", json!({"grade": 1}), CAT),
        (
            "Three oh seven has stage two resorption, type one.",
            "307",
            "resorption_type",
            json!({"grade": 2}),
            CAT,
        ),
        (
            "Four oh nine is grade one mobile with a three millimetre pocket.",
            "409",
            "probing_depth",
            json!({"mm": 1.0}),
            DOG,
        ),
        (
            "Four oh nine is grade one mobile with a three millimetre pocket.",
            "409",
            "mobility",
            json!({"grade": 3}),
            DOG,
        ),
        (
            "Two oh eight has grade three furcation and is mobile.",
            "208",
            "mobility",
            json!({"grade": 3}),
            DOG,
        ),
        (
            "Four oh nine has been loose for three weeks, grade one.",
            "409",
            "mobility",
            json!({"grade": 3}),
            DOG,
        ),
        (
            "Three oh nine, behind the fourth premolar, is stage two.",
            "309",
            "periodontal_stage",
            json!({"grade": 4}),
            DOG,
        ),
    ];
    for (line, tooth, finding, fields, d) in cases {
        let got = one(line, tooth, finding, fields.clone(), *d);
        assert!(
            got.iter().any(|k| k == "other_type" || k == "cue_missing" || k == "other_tooth"),
            "{line}: {tooth} {finding} {fields}: {got:?}"
        );
    }
    // The pairs as said are clean.
    for (line, tooth, finding, fields, d) in [
        (
            "Three oh nine, stage three periodontal disease, mobility grade one.",
            "309",
            "periodontal_stage",
            json!({"grade": 3}),
            DOG,
        ),
        (
            "Three oh nine, stage three periodontal disease, mobility grade one.",
            "309",
            "mobility",
            json!({"grade": 1}),
            DOG,
        ),
        ("Three oh seven has stage two resorption, type one.", "307", "resorption", json!({"grade": 2}), CAT),
        (
            "Three oh seven has stage two resorption, type one.",
            "307",
            "resorption_type",
            json!({"grade": 1}),
            CAT,
        ),
        (
            "Four oh nine is grade one mobile with a three millimetre pocket.",
            "409",
            "mobility",
            json!({"grade": 1}),
            DOG,
        ),
        (
            "Four oh nine is grade one mobile with a three millimetre pocket.",
            "409",
            "probing_depth",
            json!({"mm": 3.0}),
            DOG,
        ),
        (
            "Two oh eight has grade three furcation and is mobile.",
            "208",
            "furcation",
            json!({"grade": 3}),
            DOG,
        ),
        (
            "Four oh nine has been loose for three weeks, grade one.",
            "409",
            "mobility",
            json!({"grade": 1}),
            DOG,
        ),
    ] {
        clean(one(line, tooth, finding, fields, d), line);
    }
}

// --- Major: a number and a name that disagree ---

#[test]
fn a_number_and_a_name_that_disagree_are_flagged_however_they_are_joined() {
    for (line, tooth, other) in [
        ("One oh eight, I mean the upper left fourth premolar, is fractured.", "208", "108"),
        ("One oh eight, I mean the upper left fourth premolar, is fractured.", "108", "208"),
        ("One oh eight, which I think is the upper left fourth premolar, is fractured.", "208", "108"),
        ("One oh eight, which I think is the upper left fourth premolar, is fractured.", "108", "208"),
        ("The upper left fourth premolar, so one oh eight, has a slab fracture.", "108", "208"),
        ("The upper left fourth premolar, so one oh eight, has a slab fracture.", "208", "108"),
        ("One oh eight, on the upper left, the fourth premolar, has a slab fracture.", "108", "208"),
        ("One oh eight, on the upper left, the fourth premolar, has a slab fracture.", "208", "108"),
        ("The upper left fourth premolar, or one oh eight, is fractured.", "108", "208"),
    ] {
        let r = run(&[line], vec![item(tooth, "fracture", json!({}), &[0])], DOG);
        let f = r.tooth_flags.iter().find(|f| f.kind == ToothFlagKind::NameConflict);
        assert_eq!(
            f.and_then(|f| f.other.as_deref()),
            Some(other),
            "{line} charted {tooth}: {:?}",
            r.tooth_flags
        );
    }
    // On two lines: "Next is 108." then the name.
    let lines = ["Next is one oh eight.", "The upper left fourth premolar has a slab fracture."];
    let by_name = run(&lines, vec![item("208", "fracture", json!({}), &[1])], DOG);
    flagged_as(signals(&by_name, 0), "name_conflict", "two lines, name charted");
    let by_number = run(&lines, vec![item("108", "fracture", json!({}), &[0, 1])], DOG);
    flagged_as(signals(&by_number, 0), "name_conflict", "two lines, number charted");
    // Different teeth in different clauses are no conflict.
    for (lines, tooth, finding) in [
        (&["Two oh six is missing, the upper left canine looks healthy."][..], "204", "normal"),
        (&["Two oh six is missing, and the upper left canine looks healthy."][..], "204", "normal"),
        (&["Two oh six is missing.", "The upper left canine looks healthy."][..], "204", "normal"),
    ] {
        let cited = (lines.len() - 1) as u32;
        let r = run(lines, vec![item(tooth, finding, json!({}), &[cited])], DOG);
        clean(signals(&r, 0), lines[0]);
    }
}

// --- Minor: retractions, hearsay, "appeared ... but" ---

#[test]
fn a_finding_withdrawn_or_reported_by_someone_else_is_flagged() {
    let r = run(
        &["I thought one oh eight was fractured.", "Actually one oh eight is fine, it's just stain."],
        vec![item("108", "fracture", json!({}), &[0])],
        DOG,
    );
    flagged_as(signals(&r, 0), "contradicted", "retraction");
    let r = run(
        &["Owner thinks the upper left canine is broken.", "On exam the upper left canine is fine."],
        vec![item("204", "fracture", json!({}), &[0])],
        DOG,
    );
    flagged_as(signals(&r, 0), "contradicted", "owner thinks");
    flagged_as(signals(&r, 0), "uncertain", "owner thinks");
    flagged_as(
        one_dog(
            "Two oh nine appeared missing, but it's just unerupted on the radiograph.",
            "209",
            "missing",
            json!({}),
        ),
        "negated",
        "appeared",
    );
    clean(
        one_dog(
            "Two oh nine appeared missing, but it's just unerupted on the radiograph.",
            "209",
            "unerupted",
            json!({}),
        ),
        "unerupted",
    );
    flagged_as(
        one_dog(
            "Five oh four was retained but has now fallen out.",
            "504",
            "persistent_deciduous",
            json!({}),
        ),
        "negated",
        "fallen out",
    );
}

// --- Minor: another animal's tooth on this animal's chart ---

#[test]
fn another_animals_tooth_is_flagged_on_this_animals_chart() {
    let others = ["Rex".to_string()];
    for (lines, cited) in [
        (&["Rex has a fractured one oh eight, Bella doesn't."][..], 0),
        (&["Rex has a fractured one oh eight.", "Bella's mouth looks good, all her teeth are fine."][..], 0),
    ] {
        let note = json!({"dental_chart": [item("108", "fracture", json!({}), &[cited])]});
        let r = check_for_pet(&note, &transcript(lines), &HashSet::new(), true, DOG, &others);
        flagged_as(signals(&r, 0), "other_pet", lines[0]);
        // Bella's own tooth is clean.
        let t = transcript(&["Bella has a fractured one oh eight."]);
        let r = check_for_pet(
            &json!({"dental_chart": [item("108", "fracture", json!({}), &[0])]}),
            &t,
            &HashSet::new(),
            true,
            DOG,
            &others,
        );
        clean(signals(&r, 0), "Bella's tooth");
    }
}

// --- Minor: side and jaw words that start the next clause ---

#[test]
fn a_place_starting_the_next_clause_is_not_the_tooths() {
    assert!(teeth_in("There's a fractured canine, upper right is fine.").is_empty());
    assert!(hint_teeth("There's a fractured canine, upper right is fine.").is_empty());
    flagged_as(
        one_dog("There's a fractured canine, upper right is fine.", "104", "fracture", json!({})),
        "unsupported",
        "next clause",
    );
    assert_eq!(read("The canine on the lower right is fine."), ["404"]);
    assert_eq!(read("First molar, lower left, is mobile."), ["309"]);
}

// --- Regression: tooth numbers in other headings (the Juno draft) ---

#[test]
fn tooth_numbers_in_any_heading_are_supported_by_a_group_said_in_the_cited_line() {
    let t = transcript(&[
        "The lower incisors come out closed, no sectioning.",
        "Her temperature is thirty-eight point five.",
        "All the upper incisors are missing.",
    ]);
    let note = json!({
        "treatment_performed": [
            {"text": "Extraction of 301, 302, 303, 401, 402, 403 closed, no sectioning.", "evidence": [0]}
        ],
        "dental_chart": [item("301", "extraction", json!({"detail": "closed"}), &[0])]
    });
    let r = check_full(&note, &t, &HashSet::new(), true, DOG);
    assert!(r.passed && r.unsupported_numbers.is_empty(), "{r:?}");
    // A tooth outside the group is still a number said nowhere.
    let wrong = json!({"treatment_performed": [{"text": "Extraction of 304 closed.", "evidence": [0]}]});
    let r = check_full(&wrong, &t, &HashSet::new(), true, DOG);
    assert!(r.unsupported_numbers.iter().any(|u| u.number == "304"), "{r:?}");
    // A number the statement doesn't read as a tooth isn't excused.
    let temp = json!({"vitals": {"temperature": {"value": "Temperature 101", "evidence": [2]}}});
    let r = check_full(&temp, &t, &HashSet::new(), true, DOG);
    assert!(r.unsupported_numbers.iter().any(|u| u.number == "101"), "{r:?}");
}

// --- Minor: a runaway repeat ---

#[test]
fn a_runaway_repeat_becomes_one_item_with_every_citation() {
    let mut items: Vec<Value> = (0..17).map(|i| item("404", "normal", json!({}), &[5 + (i % 2)])).collect();
    items.insert(0, item("101", "missing", json!({}), &[1]));
    let mut vet = item("404", "normal", json!({}), &[]);
    vet["entered_by"] = json!("Dr Okafor");
    items.push(vet);
    let mut note = json!({"dental_chart": items, "plan": [{"text": "Recheck", "evidence": [2]}]});
    merge_repeats(&mut note);
    let chart = note["dental_chart"].as_array().unwrap();
    assert_eq!(chart.len(), 3, "{chart:?}");
    assert_eq!(chart[1]["evidence"], json!([5, 6]));
    assert_eq!(chart[2]["entered_by"], "Dr Okafor", "the vet's own item stays");
    assert_eq!(note["plan"].as_array().unwrap().len(), 1);
}
