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
//!   expected: errors), invented normals, findings that must not be charted
//!   (`chart_not_allowed`), items without evidence, and expected flags
//!   (`chart_flags`, such as a name conflict)
//!
//! Transcripts are cached in target/eval-cache so prompt changes re-run only
//! the note step (`--retranscribe` to refresh).

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
    /// Fixture ids (default: every script with audio present).
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
    #[command(flatten)]
    pub llm: LlmArgs,
}

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
    for want in &expected.chart {
        let hit = items.iter().enumerate().find(|(i, got)| !used[*i] && chart_match(want, got, transcript));
        match hit {
            Some((i, _)) => {
                used[i] = true;
                c.found += 1;
            }
            None => c.misses.push(format!("chart {}", serde_json::to_string(want).unwrap_or_default())),
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
    /// The consult's species ("feline"): headings for other species are left
    /// out, as in the app (M2 item 4).
    #[serde(default)]
    species: Option<String>,
    /// The template this consult is meant for (M2 specialty library); the
    /// `--template` option is used when absent.
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    expected: Expected,
}

#[derive(Serialize, Default)]
struct Score {
    fixture: String,
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
    /// The tooth chart, when the template has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    chart: Option<ChartScore>,
    note_secs: f64,
    completion_tokens: u32,
    /// The template that drafted it, "soap@0.1.1", and its fingerprint, so a
    /// run can be traced to the exact template text (ADR-0024 step 12).
    template: String,
    fingerprint: String,
}

struct Quiet;
impl LiveSink for Quiet {
    fn level(&mut self, _: f32, _: f32) {}
    fn segment(&mut self, _: &Segment) {}
}

fn transcript_for(store: &ModelStore, wav: &Path, cache: &Path, refresh: bool) -> Result<Transcript> {
    if !refresh && let Ok(text) = std::fs::read_to_string(cache) {
        return Ok(Transcript::from_jsonl(&text)?);
    }
    let tmp = tempfile::tempdir()?;
    let source = LiveSource::File { path: wav.to_path_buf(), realtime: false };
    let result =
        LiveRecorder::start(store, tmp.path(), source, LiveOptions::microphone(), Box::new(Quiet))?.wait()?;
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

fn score(id: &str, expected: &Expected, note: &Value, faith: &os_engine::FaithfulnessReport) -> Score {
    let mut s = Score { fixture: id.into(), ..Default::default() };
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
    llm: &os_llm::LlamaCppBackend,
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
    let mut scripts: Vec<(Script, PathBuf)> = std::fs::read_dir(&scripts_dir)
        .with_context(|| format!("reading {}", scripts_dir.display()))?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "yaml"))
        .filter_map(|e| {
            let s: Script = serde_yaml_ng::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let wav = audio_dir.join(format!("{}.wav", s.id));
            wav.is_file().then_some((s, wav))
        })
        .filter(|(s, _)| args.fixtures.is_empty() || args.fixtures.contains(&s.id))
        .collect();
    scripts.sort_by(|a, b| a.0.id.cmp(&b.0.id));

    let default_template = LoadedTemplate::resolve(&args.llm.template)?;
    let llm = load_llm(store, &args.llm)?;
    let mut report = Vec::new();
    println!(
        "{:<18} {:>9} {:>9} {:>10} {:>8}  misses",
        "fixture", "facts", "invented", "citations", "note s"
    );
    for (script, wav) in scripts {
        let cache = PathBuf::from("target/eval-cache").join(format!("{}.jsonl", script.id));
        let transcript = transcript_for(store, &wav, &cache, args.retranscribe)?;
        std::fs::create_dir_all("target/eval")?;
        let template = match &script.template {
            Some(id) if !args.force_template => LoadedTemplate::resolve(id)?,
            _ => default_template.clone(),
        }
        .for_species(script.species.as_deref());
        let s = if script.expected.vitals_by_patient.is_empty() {
            let result =
                generate_note(&llm, &template, &transcript, &note_options(&args.llm), &mut |_| true)?;
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
            s
        } else {
            score_pets(&llm, &template, &transcript, &script, &args.llm)?
        };
        let mut s = s;
        s.template = template.label();
        s.fingerprint = os_engine::template::fingerprint(&template.template);
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
        for number in &s.invented {
            println!("    invented: {number}");
        }
        for flag in &s.word_flags {
            println!("    check: {flag}");
        }
        if let Some(c) = &s.chart {
            println!(
                "    chart: findings {}/{} (recall {}, precision {}), teeth recall {} precision {}, invented teeth {:?}, invented normals {:?}, not allowed {:?}, without evidence {}, flags {}/{}, {} completion tokens",
                c.found,
                c.expected,
                pct(c.found, c.expected),
                pct(c.matched, c.charted),
                pct(c.teeth_hit, c.teeth_expected),
                pct(c.teeth_hit, c.teeth_charted),
                c.invented_teeth,
                c.invented_normals,
                c.not_allowed,
                c.no_evidence,
                c.flags_found,
                c.flags_expected,
                s.completion_tokens,
            );
        }
        report.push(s);
    }
    let (p, c): (u32, u32) = report.iter().fold((0, 0), |a, s| (a.0 + s.passed, a.1 + s.checks));
    let invented: usize = report.iter().map(|s| s.invented_numbers).sum();
    println!("TOTAL facts {p}/{c} ({:.0}%), invented numbers {invented}", 100.0 * p as f64 / c.max(1) as f64);
    let mut chart = ChartScore::default();
    let mut charts = 0;
    for c in report.iter().filter_map(|s| s.chart.as_ref()).filter(|c| c.expected > 0) {
        chart.add(c);
        charts += 1;
    }
    if charts > 0 {
        println!(
            "TOTAL chart ({charts} fixtures): findings {}/{} (recall {}, precision {}), teeth recall {} precision {}, invented teeth {}, invented normals {}, not allowed {}, without evidence {}, flags {}/{}",
            chart.found,
            chart.expected,
            pct(chart.found, chart.expected),
            pct(chart.matched, chart.charted),
            pct(chart.teeth_hit, chart.teeth_expected),
            pct(chart.teeth_hit, chart.teeth_charted),
            chart.invented_teeth.len(),
            chart.invented_normals.len(),
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
        assert_eq!(c.not_allowed, vec!["206 normal"]);
        assert_eq!((c.flags_found, c.flags_expected), (0, 1));
        assert!(note_text(&serde_json::json!([items[2]])).contains("probing depth 7 mm"));
    }
}
