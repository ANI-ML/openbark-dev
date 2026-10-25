//! `openbark eval`: score drafted notes against the facts each synthetic
//! consult script declares (packages/eval/fixtures/scripts/*.yaml, `expected:`).
//!
//! Checks, per fixture:
//! - vitals: each expected value appears in the matching note field (skipped
//!   when the template has no field of that name, e.g. client documents)
//! - medications: each expected drug is listed, under any heading, with its
//!   expected number
//! - must_mention / must_not_mention: phrases in or absent from the note
//! - invented numbers (hard fail) and citation errors, from the faithfulness pass
//! - tooth charts (ADR-0021): each expected finding (`chart`), per-tooth and
//!   per-finding precision and recall, invented teeth (charted but not
//!   expected: errors), invented normals, wrong values (an expected tooth and
//!   finding charted with the wrong type, grade, mm or plan: errors), findings that must not be charted
//!   (`chart_not_allowed`), items without evidence, and expected flags
//!   (`chart_flags`, such as a name conflict)
//!
//! Transcripts are cached in target/eval-cache so prompt changes re-run only
//! the note step (`--retranscribe` to refresh).
//!
//! `--from-ref` drafts from each fixture's ground truth instead
//! (`audio/<id>.ref.json`: speaker, start, end and text per line), with the
//! speakers numbered and the timings kept as the app would have them. It
//! needs no audio and no speech models, so it compares note models alone
//! (docs/eval/phone-models.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use os_engine::live::{LiveOptions, LiveRecorder, LiveSink, LiveSource};
use os_engine::{LoadedTemplate, Segment, Transcript, generate_note};
use os_models::ModelStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{LlmArgs, load_llm, note_options};

#[derive(clap::Args)]
pub struct EvalArgs {
    /// Fixture ids (default: every script with audio present, or with a
    /// reference transcript under `--from-ref`).
    pub fixtures: Vec<String>,
    #[arg(long, default_value = "packages/eval/fixtures")]
    pub dir: PathBuf,
    /// Re-transcribe instead of using cached transcripts.
    #[arg(long)]
    pub retranscribe: bool,
    /// Write the full report as JSON here.
    #[arg(long)]
    pub json: Option<PathBuf>,
    /// Draft every fixture with --template, even one whose script names its
    /// own (e.g. a client document from the euthanasia consult).
    #[arg(long)]
    pub force_template: bool,
    /// Leave out the patient details a script declares (`patient:`). By
    /// default they go into the prompt as the app sends them, as entered by
    /// the clinic: name, reason for visit and species.
    #[arg(long)]
    pub no_patient: bool,
    /// Draft from the fixture's reference transcript (`audio/<id>.ref.json`)
    /// instead of transcribing its audio: the same lines, speakers and
    /// timings, with no recognition errors, so only the note model differs.
    #[arg(long)]
    pub from_ref: bool,
    /// Run one share of the fixtures, `K/N` (for example `2/3`): every N-th
    /// fixture in id order, starting at the K-th. For parallel CI jobs.
    #[arg(long, value_parser = parse_shard)]
    pub shard: Option<(usize, usize)>,
    #[command(flatten)]
    pub llm: LlmArgs,
}

/// `K/N` with 1 <= K <= N.
fn parse_shard(s: &str) -> Result<(usize, usize), String> {
    let (k, n) = s.split_once('/').ok_or("expected K/N, for example 2/3")?;
    let k: usize = k.trim().parse().map_err(|_| "K must be a number")?;
    let n: usize = n.trim().parse().map_err(|_| "N must be a number")?;
    if n == 0 || k == 0 || k > n {
        return Err("expected 1 <= K <= N".into());
    }
    Ok((k, n))
}

/// One line of a reference transcript (`audio/<id>.ref.json`), as
/// tools/fixture-tts writes it.
#[derive(Deserialize)]
struct RefLine {
    speaker: String,
    start: f32,
    end: f32,
    text: String,
}

/// A transcript from a reference: one segment per line, ids from 0, and the
/// speakers numbered in the order they first speak, as the app numbers them
/// after diarization (whoever speaks first is Speaker 1).
fn transcript_from_ref(json: &str) -> Result<Transcript> {
    let lines: Vec<RefLine> = serde_json::from_str(json).context("reading the reference transcript")?;
    let mut speakers: Vec<String> = Vec::new();
    let segments = lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let n = speakers.iter().position(|s| *s == l.speaker).unwrap_or_else(|| {
                speakers.push(l.speaker.clone());
                speakers.len() - 1
            });
            Segment {
                id: i as u32,
                start: l.start,
                end: l.end,
                speaker: Some(n as u32),
                text: l.text,
                hints: Vec::new(),
                check: None,
            }
        })
        .collect();
    Ok(Transcript { segments })
}

/// The category of scripts that declare none.
const MAIN: &str = "main";

#[derive(Deserialize, Default)]
struct Expected {
    #[serde(default)]
    vitals: BTreeMap<String, String>,
    #[serde(default)]
    medications: Vec<BTreeMap<String, String>>,
    #[serde(default)]
    must_mention: Vec<String>,
    #[serde(default)]
    must_not_mention: Vec<String>,
    /// Multi-pet visits: the expected vitals of each animal. When present,
    /// the eval drafts one note per animal and checks for cross-contamination.
    #[serde(default)]
    vitals_by_patient: BTreeMap<String, BTreeMap<String, String>>,
    /// Tooth chart findings, one fact each: `tooth` and `finding`, and any of
    /// `detail`, `grade`, `mm`, `planned` (a key given as null must be null).
    #[serde(default)]
    chart: Vec<serde_json::Map<String, Value>>,
    /// Tooth flags the faithfulness pass must raise: `tooth` and `kind`.
    #[serde(default)]
    chart_flags: Vec<ExpectedFlag>,
    /// Findings that must not be charted, except on the teeth listed.
    #[serde(default)]
    chart_not_allowed: Vec<NotAllowed>,
    /// The species the note's Patient section must give ("dog" or "cat"),
    /// for consults that say only a breed ("a Yorkshire terrier"). Scored on
    /// its own, outside the facts: right word, and not flagged.
    #[serde(default)]
    patient_species: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ExpectedFlag {
    tooth: String,
    kind: String,
}

#[derive(Deserialize, Clone)]
struct NotAllowed {
    finding: String,
    #[serde(default)]
    except: Vec<String>,
}

/// How a drafted tooth chart compares with the script's expected chart.
#[derive(Serialize, Default, Clone)]
struct ChartScore {
    /// Expected findings found / expected.
    found: u32,
    expected: u32,
    /// Charted findings that match an expected one / charted.
    matched: u32,
    charted: u32,
    /// Distinct teeth: charted and expected, charted, expected.
    teeth_hit: u32,
    teeth_charted: u32,
    teeth_expected: u32,
    /// Charted teeth that aren't in any expected finding (errors).
    invented_teeth: Vec<String>,
    /// `normal` findings that weren't expected.
    invented_normals: Vec<String>,
    /// Charted findings on an expected tooth and finding whose type, grade,
    /// millimetres or plan is wrong ("108 extraction detail closed, expected
    /// open"): errors, like an invented tooth, not only a precision miss.
    wrong_values: Vec<String>,
    /// Findings charted where `chart_not_allowed` forbids them.
    not_allowed: Vec<String>,
    /// Model findings citing no evidence.
    no_evidence: u32,
    /// Expected flags raised / expected.
    flags_found: u32,
    flags_expected: u32,
    misses: Vec<String>,
}

impl ChartScore {
    fn add(&mut self, o: &ChartScore) {
        self.found += o.found;
        self.expected += o.expected;
        self.matched += o.matched;
        self.charted += o.charted;
        self.teeth_hit += o.teeth_hit;
        self.teeth_charted += o.teeth_charted;
        self.teeth_expected += o.teeth_expected;
        self.invented_teeth.extend(o.invented_teeth.iter().cloned());
        self.invented_normals.extend(o.invented_normals.iter().cloned());
        self.wrong_values.extend(o.wrong_values.iter().cloned());
        self.not_allowed.extend(o.not_allowed.iter().cloned());
        self.no_evidence += o.no_evidence;
        self.flags_found += o.flags_found;
        self.flags_expected += o.flags_expected;
    }
}

fn pct(n: u32, d: u32) -> String {
    if d == 0 { "-".into() } else { format!("{:.0}%", 100.0 * n as f64 / d as f64) }
}

/// Does a charted item match an expected one? Tooth and finding, and every
/// other key given (numbers compared as numbers, null as null). `cites`: a
/// word one of its cited lines must hold ("radiograph").
fn chart_match(want: &serde_json::Map<String, Value>, got: &Value, transcript: &Transcript) -> bool {
    want.iter().all(|(k, w)| {
        if k == "cites" {
            let word = w.as_str().unwrap_or_default().to_lowercase();
            return got["evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .any(|id| transcript.get(id as u32).is_some_and(|s| s.text.to_lowercase().contains(&word)));
        }
        let g = &got[k.as_str()];
        match (w, g) {
            (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
            (Value::String(a), Value::Number(b)) => a.parse::<f64>().ok() == b.as_f64(),
            _ => w == g,
        }
    })
}

/// Score a drafted chart against the expected one (ADR-0021 plan step 15).
fn score_chart(
    expected: &Expected,
    items: &[Value],
    faith: &os_engine::FaithfulnessReport,
    transcript: &Transcript,
) -> ChartScore {
    use std::collections::BTreeSet;
    let mut c = ChartScore {
        expected: expected.chart.len() as u32,
        charted: items.len() as u32,
        flags_expected: expected.chart_flags.len() as u32,
        ..Default::default()
    };
    let mut used = vec![false; items.len()];
    let mut hit_wants = vec![false; expected.chart.len()];
    for (w, want) in expected.chart.iter().enumerate() {
        let hit = items.iter().enumerate().find(|(i, got)| !used[*i] && chart_match(want, got, transcript));
        match hit {
            Some((i, _)) => {
                used[i] = true;
                hit_wants[w] = true;
                c.found += 1;
            }
            None => c.misses.push(format!("chart {}", serde_json::to_string(want).unwrap_or_default())),
        }
    }
    // A charted finding on an expected tooth and finding, with the wrong
    // type, grade, millimetres or plan: an error in the record.
    for got in items.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(_, got)| got) {
        let same = |want: &serde_json::Map<String, Value>| {
            want.get("tooth") == Some(&got["tooth"]) && want.get("finding") == Some(&got["finding"])
        };
        let Some((w, want)) =
            expected.chart.iter().enumerate().find(|(w, want)| !hit_wants[*w] && same(want))
        else {
            continue;
        };
        hit_wants[w] = true;
        let wrong: Vec<String> = ["detail", "grade", "mm", "planned"]
            .into_iter()
            .filter(|k| want.contains_key(*k))
            .filter(|k| {
                let one = serde_json::Map::from_iter([(k.to_string(), want[*k].clone())]);
                !chart_match(&one, got, transcript)
            })
            .map(|k| format!("{k} {}, expected {}", got[k], want[k]))
            .collect();
        if !wrong.is_empty() {
            c.wrong_values.push(format!(
                "{} {} {}",
                got["tooth"].as_str().unwrap_or_default(),
                got["finding"].as_str().unwrap_or_default(),
                wrong.join(", ")
            ));
        }
    }
    c.matched = used.iter().filter(|u| **u).count() as u32;
    let tooth = |v: &Value| v["tooth"].as_str().unwrap_or_default().to_string();
    let want_teeth: BTreeSet<String> = expected
        .chart
        .iter()
        .filter_map(|w| w.get("tooth").and_then(Value::as_str).map(String::from))
        .collect();
    let got_teeth: BTreeSet<String> = items.iter().map(tooth).collect();
    c.teeth_expected = want_teeth.len() as u32;
    c.teeth_charted = got_teeth.len() as u32;
    c.teeth_hit = got_teeth.intersection(&want_teeth).count() as u32;
    c.invented_teeth = got_teeth.difference(&want_teeth).cloned().collect();
    for (i, item) in items.iter().enumerate() {
        let t = tooth(item);
        let finding = item["finding"].as_str().unwrap_or_default();
        if finding == "normal" && !used[i] {
            c.invented_normals.push(t.clone());
        }
        for rule in &expected.chart_not_allowed {
            if rule.finding == finding && !rule.except.contains(&t) {
                c.not_allowed.push(format!("{t} {finding}"));
            }
        }
        let vet = item["entered_by"].as_str().is_some_and(|b| !b.is_empty());
        if !vet && item["evidence"].as_array().is_none_or(|e| e.is_empty()) {
            c.no_evidence += 1;
        }
    }
    for want in &expected.chart_flags {
        let raised = faith.tooth_flags.iter().any(|f| {
            f.tooth == want.tooth
                && serde_json::to_value(f.kind).ok().and_then(|k| k.as_str().map(String::from)).as_deref()
                    == Some(want.kind.as_str())
        });
        if raised {
            c.flags_found += 1;
        } else {
            c.misses.push(format!("flag {} on {}", want.kind, want.tooth));
        }
    }
    c
}

#[derive(Deserialize)]
struct Script {
    id: String,
    /// A group reported with its own subtotal ("speakerphone"); the rest are
    /// the main fixtures.
    #[serde(default)]
    category: Option<String>,
    /// A speakerphone consult (tools/fixture-tts renders the phone voice):
    /// transcribed with "Phone on speaker" on, as the app would be.
    #[serde(default)]
    phone: Option<serde_yaml_ng::Value>,
    /// The consult's species ("feline"): headings for other species are left
    /// out, as in the app (M2 item 4).
    #[serde(default)]
    species: Option<String>,
    /// The template this consult is meant for (M2 specialty library); the
    /// `--template` option is used when absent.
    #[serde(default)]
    template: Option<String>,
    /// What the clinic would have entered before the visit. The app sends
    /// these details with every draft (`PatientInfo::prompt_context`), so the
    /// eval does too, unless `--no-patient`.
    #[serde(default)]
    patient: Option<ScriptPatient>,
    #[serde(default)]
    expected: Expected,
}

/// The patient details a script declares, as the clinic types them.
#[derive(Deserialize, Default)]
struct ScriptPatient {
    name: String,
    #[serde(default)]
    reason: String,
    /// As typed ("Dog"); the script's `species` when absent.
    #[serde(default)]
    species: Option<String>,
}

/// The draft options for a script: the app's prompt context when the script
/// declares its patient, so the eval drafts from the prompt the app sends.
fn script_options(args: &EvalArgs, script: &Script) -> os_engine::NoteOptions {
    let mut options = note_options(&args.llm);
    if !args.no_patient
        && let Some(p) = &script.patient
    {
        let info = os_engine::session::PatientInfo {
            name: p.name.clone(),
            reason: p.reason.clone(),
            species: p.species.clone().or_else(|| script.species.clone()).unwrap_or_default(),
            ..Default::default()
        };
        options.context = Some(info.prompt_context()).filter(|c| !c.is_empty());
    }
    options
}

#[derive(Serialize, Default)]
struct Score {
    fixture: String,
    category: String,
    checks: u32,
    passed: u32,
    misses: Vec<String>,
    invented_numbers: usize,
    /// Which numbers, and where ("/plan/medications/0: 1"), for review.
    invented: Vec<String>,
    citation_errors: usize,
    /// Statements whose words aren't in their cited lines (M2 alignment),
    /// with the statement, for review.
    word_flags: Vec<String>,
    /// Multi-pet visits: another animal's value found in this animal's note.
    contaminated: Vec<String>,
    /// `patient_species`: what the note says, and whether it is the expected
    /// species and unflagged.
    #[serde(skip_serializing_if = "Option::is_none")]
    species: Option<SpeciesScore>,
    /// The tooth chart, when the template has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    chart: Option<ChartScore>,
    note_secs: f64,
    completion_tokens: u32,
    /// The template that drafted it, "soap@0.1.1", and its fingerprint, so a
    /// run can be traced to the exact template text (ADR-0024 step 12).
    template: String,
    fingerprint: String,
    /// The patient details sent with the draft, as the app sends them
    /// (none with `--no-patient`, or when the script declares none).
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
    /// The note model (`--llm`), and where the transcript came from:
    /// "reference" (`--from-ref`) or "audio".
    llm: String,
    transcript: String,
    /// The draft failed: why. Every check of the fixture counts as missed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// The score of a fixture whose draft failed: every declared check missed
/// (and every expected tooth finding, when the template has a chart).
fn failed_score(script: &Script, chart: bool, why: &str) -> Score {
    let e = &script.expected;
    let checks = e.vitals.len()
        + e.medications.len()
        + e.must_mention.len()
        + e.must_not_mention.len()
        + e.vitals_by_patient.values().map(BTreeMap::len).sum::<usize>();
    Score {
        fixture: script.id.clone(),
        checks: checks as u32,
        misses: vec![format!("draft failed: {why}")],
        chart: chart.then(|| ChartScore {
            expected: e.chart.len() as u32,
            teeth_expected: e
                .chart
                .iter()
                .filter_map(|w| w.get("tooth").and_then(Value::as_str))
                .collect::<std::collections::BTreeSet<_>>()
                .len() as u32,
            flags_expected: e.chart_flags.len() as u32,
            ..Default::default()
        }),
        error: Some(why.to_string()),
        ..Default::default()
    }
}

struct Quiet;
impl LiveSink for Quiet {
    fn level(&mut self, _: f32, _: f32) {}
    fn segment(&mut self, _: &Segment) {}
}

fn transcript_for(
    store: &ModelStore,
    wav: &Path,
    cache: &Path,
    refresh: bool,
    speakerphone: bool,
) -> Result<Transcript> {
    if !refresh && let Ok(text) = std::fs::read_to_string(cache) {
        return Ok(Transcript::from_jsonl(&text)?);
    }
    let tmp = tempfile::tempdir()?;
    let source = LiveSource::File { path: wav.to_path_buf(), realtime: false };
    let options = LiveOptions { speakerphone, ..LiveOptions::microphone() };
    let result = LiveRecorder::start(store, tmp.path(), source, options, Box::new(Quiet))?.wait()?;
    std::fs::create_dir_all(cache.parent().expect("cache has a parent"))?;
    std::fs::write(cache, result.transcript.to_jsonl())?;
    Ok(result.transcript)
}

/// All text in the note, lowercased, for phrase checks.
fn note_text(v: &Value) -> String {
    let mut out = String::new();
    fn walk(v: &Value, out: &mut String) {
        match v {
            Value::String(s) => {
                out.push_str(&s.to_lowercase());
                out.push('\n');
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => {
                o.iter().filter(|(k, _)| *k != "evidence").for_each(|(_, x)| walk(x, out));
                // A charted tooth also reads as the record prints it ("Probing depth 6 mm").
                if os_engine::dental::is_tooth_item(o) {
                    out.push_str(&os_engine::dental::code_of(v).english().to_lowercase());
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

/// Loose phrase match: case-insensitive, hyphen/space-insensitive, with a few
/// clinical synonyms so "x-ray" matches "radiograph" etc.
fn mentions(text: &str, phrase: &str) -> bool {
    // Spoken and written numbers alike: "three out of five" is "3/5", "fourteen days" is "14 days".
    let norm = |s: &str| {
        let s = s.to_lowercase().replace(['-', '’', '\''], " ").replace('/', " out of ");
        let s = os_engine::numbers::normalize_spoken(&s);
        // "6mm" and "6 mm" alike.
        let mut out = String::new();
        let chars: Vec<char> = s.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            let between = *c == ' '
                && i > 0
                && chars[i - 1].is_ascii_digit()
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic());
            if !between {
                out.push(*c);
            }
        }
        out
    };
    let t = norm(text);
    let p = norm(phrase);
    let synonyms: &[(&str, &[&str])] = &[
        ("x ray", &["radiograph", "x rays", "xray"]),
        ("declined", &["declin", "hold off", "wait", "deferred", "not today"]),
        ("hair tie", &["hair ties", "hair elastic", "elastic"]),
        ("fvrcp", &["f v r c p", "fvrcp", "rhinotracheitis"]),
        ("three days", &["3 days"]),
        ("ten percent", &["10%", "10 percent"]),
        ("once a week", &["weekly", "once weekly", "1 time per week"]),
    ];
    t.contains(&p) || synonyms.iter().any(|(k, alts)| p == *k && alts.iter().any(|a| t.contains(&norm(a))))
}

fn numbers(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || (c == '.' && !cur.is_empty()) {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(cur.trim_end_matches('.').trim_end_matches(".0").to_string());
            cur.clear();
        }
    }
    out
}

/// The first value under `key` anywhere in the note.
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find_key(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

/// Every medication item (an object with "drug") anywhere in the note.
fn medications(v: &Value) -> Vec<Value> {
    match v {
        Value::Object(m) if m.contains_key("drug") => vec![v.clone()],
        Value::Object(m) => m.values().flat_map(medications).collect(),
        Value::Array(a) => a.iter().flat_map(medications).collect(),
        _ => Vec::new(),
    }
}

/// The Patient section's species against a script's `patient_species`.
#[derive(Serialize, Default, Clone)]
struct SpeciesScore {
    expected: String,
    got: Option<String>,
    flagged: bool,
    ok: bool,
}

fn score_species(want: &str, note: &Value, faith: &os_engine::FaithfulnessReport) -> SpeciesScore {
    let got = note.pointer("/patient/species/value").and_then(Value::as_str).map(str::to_string);
    let flagged = faith.unverified_values.iter().any(|p| p == "/patient/species");
    let words: &[&str] = match want {
        "dog" => &["dog", "canine"],
        "cat" => &["cat", "feline"],
        _ => &[],
    };
    let right = got.as_deref().is_some_and(|g| words.contains(&g.trim().to_lowercase().as_str()));
    SpeciesScore { expected: want.to_string(), got, flagged, ok: right && !flagged }
}

fn score(id: &str, expected: &Expected, note: &Value, faith: &os_engine::FaithfulnessReport) -> Score {
    let mut s = Score { fixture: id.into(), ..Default::default() };
    s.species = expected.patient_species.as_deref().map(|want| score_species(want, note, faith));
    let text = note_text(note);
    let check = |ok: bool, what: String, s: &mut Score| {
        s.checks += 1;
        if ok {
            s.passed += 1;
        } else {
            s.misses.push(what);
        }
    };
    for (field, want) in &expected.vitals {
        let Some(value) = find_key(note, field) else { continue };
        let got = os_engine::render::field_text(value).unwrap_or_default();
        check(numbers(&got).contains(want), format!("vital {field} = {want} (got \"{got}\")"), &mut s);
    }
    let meds = medications(note);
    for m in &expected.medications {
        let drug = m.get("drug").map(|d| d.to_lowercase()).unwrap_or_default();
        let key = drug.split_whitespace().next().unwrap_or("").to_string();
        let found = meds.iter().find(|x| note_text(x).contains(&key));
        let number = m.iter().find(|(k, _)| *k != "drug").map(|(_, v)| v.clone());
        let ok = match (found, &number) {
            (Some(x), Some(n)) => numbers(&note_text(x)).contains(n),
            (Some(_), None) => true,
            _ => false,
        };
        check(
            ok,
            format!("medication {drug}{}", number.map(|n| format!(" with {n}")).unwrap_or_default()),
            &mut s,
        );
    }
    for p in &expected.must_mention {
        check(mentions(&text, p), format!("mentions \"{p}\""), &mut s);
    }
    for p in &expected.must_not_mention {
        check(!mentions(&text, p), format!("does not say \"{p}\""), &mut s);
    }
    s.invented_numbers = faith.unsupported_numbers.len();
    s.invented = faith.unsupported_numbers.iter().map(|u| format!("{}: {}", u.path, u.number)).collect();
    s.citation_errors = faith.citation_mismatches.len() + faith.invalid_evidence.len();
    s.word_flags = faith
        .unsupported_statements
        .iter()
        .map(|u| {
            let v = note.pointer(&u.path).cloned().unwrap_or_default();
            let text = v["text"]
                .as_str()
                .or(v["drug"].as_str())
                .or(v["value"].as_str())
                .unwrap_or_default()
                .to_string();
            let hint =
                if u.suggested.is_empty() { String::new() } else { format!(" (said in {:?})", u.suggested) };
            format!("{}: \"{text}\" missing {:?}{hint}", u.path, u.missing)
        })
        .collect();
    s
}

/// Multi-pet visit: one note per animal, each scored against its own vitals;
/// another animal's values in a note count as contamination.
fn score_pets(
    llm: &dyn os_llm::LlmBackend,
    template: &LoadedTemplate,
    transcript: &Transcript,
    script: &Script,
    args: &LlmArgs,
) -> Result<Score> {
    let pets: Vec<String> = script.expected.vitals_by_patient.keys().cloned().collect();
    let mut total = Score { fixture: script.id.clone(), ..Default::default() };
    for pet in &pets {
        let mut options = note_options(args);
        options.pet = Some(pet.clone());
        options.other_pets = pets.iter().filter(|p| *p != pet).cloned().collect();
        let result = generate_note(llm, template, transcript, &options, &mut |_| true)?;
        std::fs::write(
            format!("target/eval/{}.{}.note.json", script.id, pet.to_lowercase()),
            serde_json::to_string_pretty(&result.note)?,
        )?;
        // This animal's vitals, plus the visit-wide phrase checks.
        let expected = Expected {
            vitals: script.expected.vitals_by_patient[pet].clone(),
            must_mention: Vec::new(),
            must_not_mention: script.expected.must_not_mention.clone(),
            ..Default::default()
        };
        let s = score(&format!("{}/{pet}", script.id), &expected, &result.note, &result.faithfulness);
        total.checks += s.checks;
        total.passed += s.passed;
        total.misses.extend(s.misses.into_iter().map(|m| format!("{pet}: {m}")));
        total.invented_numbers += s.invented_numbers;
        total.invented.extend(s.invented.into_iter().map(|i| format!("{pet}: {i}")));
        total.citation_errors += s.citation_errors;
        total.word_flags.extend(s.word_flags.into_iter().map(|w| format!("{pet}: {w}")));
        total.note_secs += result.stats.total_ms as f64 / 1000.0;
        // Another animal's value under this animal's vitals (when they differ).
        for (other, theirs) in script.expected.vitals_by_patient.iter().filter(|(o, _)| *o != pet) {
            for (field, value) in theirs {
                let mine = script.expected.vitals_by_patient[pet].get(field);
                if mine == Some(value) {
                    continue;
                }
                if let Some(v) = find_key(&result.note, field) {
                    let got = os_engine::render::field_text(v).unwrap_or_default();
                    if numbers(&got).contains(value) {
                        total.contaminated.push(format!("{pet}'s {field} is {other}'s ({value})"));
                    }
                }
            }
        }
    }
    // Phrases that only need to appear somewhere in the visit's notes.
    total.checks += script.expected.must_mention.len() as u32;
    let all: String = pets
        .iter()
        .filter_map(|p| {
            std::fs::read_to_string(format!("target/eval/{}.{}.note.json", script.id, p.to_lowercase())).ok()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = all.to_lowercase();
    for p in &script.expected.must_mention {
        if mentions(&text, p) {
            total.passed += 1;
        } else {
            total.misses.push(format!("mentions \"{p}\""));
        }
    }
    for c in &total.contaminated {
        total.misses.push(format!("contaminated: {c}"));
    }
    Ok(total)
}

pub fn run(store: &ModelStore, args: EvalArgs) -> Result<()> {
    let scripts_dir = args.dir.join("scripts");
    let audio_dir = args.dir.join("audio");
    // The input each fixture needs: its audio, or its reference transcript.
    let input_name = |id: &str| if args.from_ref { format!("{id}.ref.json") } else { format!("{id}.wav") };
    let mut scripts: Vec<(Script, PathBuf)> = std::fs::read_dir(&scripts_dir)
        .with_context(|| format!("reading {}", scripts_dir.display()))?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "yaml"))
        .filter_map(|e| {
            let s: Script = serde_yaml_ng::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let input = audio_dir.join(input_name(&s.id));
            input.is_file().then_some((s, input))
        })
        .filter(|(s, _)| args.fixtures.is_empty() || args.fixtures.contains(&s.id))
        .collect();
    scripts.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    if let Some((k, n)) = args.shard {
        scripts = scripts.into_iter().enumerate().filter(|(i, _)| i % n == k - 1).map(|(_, s)| s).collect();
    }
    if scripts.is_empty() {
        anyhow::bail!(
            "no fixtures to run in {}: each needs {}",
            args.dir.display(),
            if args.from_ref { "audio/<id>.ref.json" } else { "audio/<id>.wav" }
        );
    }

    let default_template = LoadedTemplate::resolve(&args.llm.template)?;
    let llm = load_llm(store, &args.llm)?;
    let mut report = Vec::new();
    println!(
        "{:<18} {:>9} {:>9} {:>10} {:>8}  misses",
        "fixture", "facts", "invented", "citations", "note s"
    );
    for (script, input) in scripts {
        let transcript = if args.from_ref {
            transcript_from_ref(&std::fs::read_to_string(&input)?)
                .with_context(|| input.display().to_string())?
        } else {
            let cache = PathBuf::from("target/eval-cache").join(format!("{}.jsonl", script.id));
            transcript_for(store, &input, &cache, args.retranscribe, script.phone.is_some())?
        };
        std::fs::create_dir_all("target/eval")?;
        let template = match &script.template {
            Some(id) if !args.force_template => LoadedTemplate::resolve(id)?,
            _ => default_template.clone(),
        }
        .for_species(script.species.as_deref());
        let options = script_options(&args, &script);
        let draft = || -> Result<Score> {
            if !script.expected.vitals_by_patient.is_empty() {
                return score_pets(&*llm, &template, &transcript, &script, &args.llm);
            }
            let result = generate_note(&*llm, &template, &transcript, &options, &mut |_| true)?;
            std::fs::write(
                format!("target/eval/{}.note.json", script.id),
                serde_json::to_string_pretty(&result.note)?,
            )?;
            let mut s = score(&script.id, &script.expected, &result.note, &result.faithfulness);
            s.note_secs = result.stats.total_ms as f64 / 1000.0;
            s.completion_tokens = result.stats.completion_tokens;
            if let Some((path, _)) = template.chart() {
                let pointer = format!("/{path}");
                let items =
                    result.note.pointer(&pointer).and_then(Value::as_array).cloned().unwrap_or_default();
                let c = score_chart(&script.expected, &items, &result.faithfulness, &transcript);
                s.misses.extend(c.misses.iter().cloned());
                s.chart = Some(c);
            }
            Ok(s)
        };
        let drafted = draft();
        // A draft that fails (past the token limit, or not the template's
        // JSON) is scored as every check missed, and the run goes on.
        let mut s = match drafted {
            Ok(s) => s,
            Err(e) => failed_score(&script, template.chart().is_some(), &format!("{e:#}")),
        };
        s.category = script.category.clone().unwrap_or_else(|| MAIN.into());
        s.context = options.context.clone();
        s.template = template.label();
        s.fingerprint = os_engine::template::fingerprint(&template.template);
        s.llm = args.llm.llm.clone();
        s.transcript = if args.from_ref { "reference" } else { "audio" }.into();
        println!(
            "{:<18} {:>4}/{:<4} {:>9} {:>10} {:>8.1}  {}",
            s.fixture,
            s.passed,
            s.checks,
            s.invented_numbers,
            s.citation_errors,
            s.note_secs,
            s.misses.join("; ")
        );
        println!("    template: {} ({})", s.template, &s.fingerprint[..12]);
        if let Some(context) = &s.context {
            println!("    patient details in the prompt: {context}");
        }
        for number in &s.invented {
            println!("    invented: {number}");
        }
        for flag in &s.word_flags {
            println!("    check: {flag}");
        }
        if let Some(sp) = &s.species {
            println!(
                "    species: {:?}, expected {}{}{}",
                sp.got,
                sp.expected,
                if sp.flagged { ", flagged" } else { "" },
                if sp.ok { " (ok)" } else { " (miss)" }
            );
        }
        if let Some(c) = &s.chart {
            println!(
                "    chart: findings {}/{} (recall {}, precision {}), teeth recall {} precision {}, invented teeth {:?}, invented normals {:?}, wrong values {:?}, not allowed {:?}, without evidence {}, flags {}/{}, {} completion tokens",
                c.found,
                c.expected,
                pct(c.found, c.expected),
                pct(c.matched, c.charted),
                pct(c.teeth_hit, c.teeth_expected),
                pct(c.teeth_hit, c.teeth_charted),
                c.invented_teeth,
                c.invented_normals,
                c.wrong_values,
                c.not_allowed,
                c.no_evidence,
                c.flags_found,
                c.flags_expected,
                s.completion_tokens,
            );
        }
        report.push(s);
        // After every fixture, so a run stopped part-way (a CI time limit)
        // still leaves what it finished.
        if let Some(path) = &args.json {
            std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        }
    }
    let (p, c): (u32, u32) = report.iter().fold((0, 0), |a, s| (a.0 + s.passed, a.1 + s.checks));
    let invented: usize = report.iter().map(|s| s.invented_numbers).sum();
    println!("TOTAL facts {p}/{c} ({:.0}%), invented numbers {invented}", 100.0 * p as f64 / c.max(1) as f64);
    let categories: std::collections::BTreeSet<&str> = report.iter().map(|s| s.category.as_str()).collect();
    if categories.len() > 1 {
        for cat in categories {
            let of: Vec<&Score> = report.iter().filter(|s| s.category == cat).collect();
            let (p, c) = of.iter().fold((0, 0), |a, s| (a.0 + s.passed, a.1 + s.checks));
            let invented: usize = of.iter().map(|s| s.invented_numbers).sum();
            println!(
                "TOTAL {cat} ({} fixtures): facts {p}/{c} ({:.0}%), invented numbers {invented}",
                of.len(),
                100.0 * p as f64 / c.max(1) as f64
            );
        }
    }
    let species: Vec<&SpeciesScore> = report.iter().filter_map(|s| s.species.as_ref()).collect();
    if !species.is_empty() {
        println!(
            "TOTAL species from the breed: {}/{} right and unflagged",
            species.iter().filter(|s| s.ok).count(),
            species.len()
        );
    }
    let mut chart = ChartScore::default();
    let mut charts = 0;
    for c in report.iter().filter_map(|s| s.chart.as_ref()).filter(|c| c.expected > 0) {
        chart.add(c);
        charts += 1;
    }
    if charts > 0 {
        println!(
            "TOTAL chart ({charts} fixtures): findings {}/{} (recall {}, precision {}), teeth recall {} precision {}, invented teeth {}, invented normals {}, wrong values {}, not allowed {}, without evidence {}, flags {}/{}",
            chart.found,
            chart.expected,
            pct(chart.found, chart.expected),
            pct(chart.matched, chart.charted),
            pct(chart.teeth_hit, chart.teeth_expected),
            pct(chart.teeth_hit, chart.teeth_charted),
            chart.invented_teeth.len(),
            chart.invented_normals.len(),
            chart.wrong_values.len(),
            chart.not_allowed.len(),
            chart.no_evidence,
            chart.flags_found,
            chart.flags_expected,
        );
    }
    if let Some(path) = &args.json {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_matching_is_forgiving_but_not_loose() {
        assert!(mentions("abdominal radiographs declined today", "x-ray"));
        assert!(mentions("Client will hold off on x-rays", "declined"));
        assert!(mentions("FVRCP booster given", "FVRCP"));
        assert!(!mentions("bloodwork submitted", "x-ray"));
        assert!(mentions("Grade three out of five lame", "3/5"));
        assert!(mentions("a 6mm pocket", "6 mm"));
        assert!(mentions("Cone on for 14 days", "fourteen days"));
    }

    #[test]
    fn reference_transcripts_number_speakers_as_the_app_does() {
        let t = transcript_from_ref(
            r#"[
              {"speaker": "client", "start": 0.0, "end": 2.5, "text": "She started vomiting on Monday."},
              {"speaker": "dvm", "start": 3.0, "end": 4.0, "text": "Three days, then."},
              {"speaker": "tech", "start": 4.5, "end": 5.0, "text": "Weight is 4.2 kilos."},
              {"speaker": "client", "start": 5.5, "end": 6.0, "text": "Yes."}
            ]"#,
        )
        .unwrap();
        let ids: Vec<u32> = t.segments.iter().map(|s| s.id).collect();
        assert_eq!(ids, [0, 1, 2, 3]);
        // Whoever speaks first is Speaker 1, whatever the label.
        let speakers: Vec<Option<u32>> = t.segments.iter().map(|s| s.speaker).collect();
        assert_eq!(speakers, [Some(0), Some(1), Some(2), Some(0)]);
        assert_eq!(t.speaker_count(), 3);
        assert_eq!((t.segments[1].start, t.segments[1].end), (3.0, 4.0));
        assert!(t.for_prompt().starts_with("[0] Speaker 1 (00:00): She started vomiting on Monday.\n"));
        assert!(transcript_from_ref("{}").is_err());
    }

    #[test]
    fn every_fixture_has_a_reference_transcript() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/eval/fixtures");
        let mut n = 0;
        for e in std::fs::read_dir(dir.join("scripts")).unwrap().flatten() {
            let script: Script =
                serde_yaml_ng::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            let r = dir.join("audio").join(format!("{}.ref.json", script.id));
            let t = transcript_from_ref(&std::fs::read_to_string(&r).unwrap()).unwrap();
            assert!(!t.is_empty(), "{}", r.display());
            n += 1;
        }
        assert!(n > 10, "only {n} fixtures found");
    }

    /// packages/eval/phone-models.json: every candidate is pinned, fits the
    /// tiers it is compared for, and those also in the catalog match it.
    #[test]
    fn phone_candidates_are_pinned_fit_their_tiers_and_match_the_catalog() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/eval/phone-models.json");
        let plan: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let catalog = os_models::Catalog::builtin();
        let candidates = plan["candidates"].as_array().unwrap();
        assert!(candidates.len() >= 3);
        for c in candidates {
            let id = c["id"].as_str().unwrap();
            let file = c["file"].as_str().unwrap();
            let size = c["size"].as_u64().unwrap();
            let sha = c["sha256"].as_str().unwrap();
            assert!(sha.len() == 64 && sha.chars().all(|ch| ch.is_ascii_hexdigit()), "{id}");
            assert_eq!(c["revision"].as_str().unwrap().len(), 40, "{id} must pin a full commit");
            let params = c["params_b"].as_f64();
            for tier in c["tiers"].as_array().unwrap() {
                let tier = tier.as_u64().unwrap() as u32;
                let t = os_models::platform::TIERS.iter().find(|t| t.ram_gb == tier).copied();
                assert!(t.is_some(), "{id}: no {tier} GB tier");
                assert_eq!(
                    os_models::platform::check_gguf(t, file, size, params),
                    Ok(()),
                    "{id} on {tier} GB"
                );
            }
            if c["catalog"].as_bool() == Some(true) {
                let m = catalog.get(id).unwrap_or_else(|| panic!("{id} isn't in catalog.json"));
                assert_eq!(m.files.len(), 1, "{id}");
                assert_eq!(
                    (m.files[0].path.as_str(), m.files[0].size, m.files[0].sha256.as_str()),
                    (file, size, sha)
                );
                match &m.source {
                    os_models::Source::Huggingface { repo, revision } => {
                        assert_eq!(repo, c["repo"].as_str().unwrap(), "{id}");
                        assert_eq!(revision, c["revision"].as_str().unwrap(), "{id}");
                    }
                    _ => panic!("{id} isn't from Hugging Face"),
                }
            }
        }
    }

    #[test]
    fn a_failed_draft_misses_every_check() {
        let script: Script = serde_yaml_ng::from_str(
            "id: x
expected:
  vitals: {weight: '4.2'}
  must_mention: [otitis]
  must_not_mention: [x-ray]
  chart:
    - { tooth: '101', finding: missing }
",
        )
        .unwrap();
        let s = failed_score(&script, true, "model output is not valid JSON (output hit max_tokens)");
        assert_eq!((s.checks, s.passed), (3, 0));
        assert_eq!(s.chart.as_ref().map(|c| (c.expected, c.found, c.teeth_expected)), Some((1, 0, 1)));
        assert!(s.error.as_deref().unwrap().contains("max_tokens"));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["misses"][0], "draft failed: model output is not valid JSON (output hit max_tokens)");
    }

    #[test]
    fn shards_split_the_fixtures() {
        assert_eq!(parse_shard("2/3"), Ok((2, 3)));
        assert!(parse_shard("0/3").is_err() && parse_shard("4/3").is_err() && parse_shard("3").is_err());
    }

    #[test]
    fn numbers_are_extracted_plainly() {
        assert_eq!(numbers("39.1 °C"), vec!["39.1"]);
        assert_eq!(numbers("4/9"), vec!["4", "9"]);
        assert_eq!(numbers("100 mL LRS"), vec!["100"]);
        assert_eq!(numbers("6.0%"), vec!["6"]);
    }

    #[test]
    fn chart_scoring_counts_found_invented_and_forbidden_findings() {
        let expected: Expected = serde_yaml_ng::from_str(
            "chart:\n  - { tooth: '101', finding: missing }\n  - { tooth: '304', finding: fracture, detail: null }\n  - { tooth: '309', finding: probing_depth, mm: 7 }\nchart_flags:\n  - { tooth: '108', kind: name_conflict }\nchart_not_allowed:\n  - { finding: normal, except: ['104'] }\n",
        )
        .unwrap();
        let item = |t: &str, f: &str, detail: Value, mm: Value| {
            serde_json::json!({"tooth": t, "text": "", "finding": f, "detail": detail, "grade": null,
                "mm": mm, "planned": false, "evidence": [1]})
        };
        let items = vec![
            item("101", "missing", Value::Null, Value::Null),
            // A guessed type doesn't match "detail: null".
            item("304", "fracture", serde_json::json!("complicated_crown"), Value::Null),
            item("309", "probing_depth", Value::Null, serde_json::json!(7.0)),
            // An invented tooth, charted normal.
            item("206", "normal", Value::Null, Value::Null),
        ];
        let c =
            score_chart(&expected, &items, &os_engine::FaithfulnessReport::default(), &Transcript::default());
        assert_eq!((c.found, c.expected, c.matched, c.charted), (2, 3, 2, 4));
        assert_eq!((c.teeth_hit, c.teeth_expected, c.teeth_charted), (3, 3, 4));
        assert_eq!(c.invented_teeth, vec!["206"]);
        assert_eq!(c.invented_normals, vec!["206"]);
        assert_eq!(c.wrong_values, vec!["304 fracture detail \"complicated_crown\", expected null"]);
        assert_eq!(c.not_allowed, vec!["206 normal"]);
        assert_eq!((c.flags_found, c.flags_expected), (0, 1));
        assert!(note_text(&serde_json::json!([items[2]])).contains("probing depth 7 mm"));
    }
}
