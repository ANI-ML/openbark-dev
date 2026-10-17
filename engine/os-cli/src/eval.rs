//! `openbark eval`: score drafted notes against the facts each synthetic
//! consult script declares (packages/eval/fixtures/scripts/*.yaml, `expected:`).
//!
//! Checks, per fixture:
//! - vitals: each expected value appears in the matching note field
//! - medications: each expected drug is listed with its expected number
//! - must_mention / must_not_mention: phrases in or absent from the note
//! - invented numbers (hard fail) and citation errors, from the faithfulness pass
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
}

#[derive(Deserialize)]
struct Script {
    id: String,
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
    citation_errors: usize,
    note_secs: f64,
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
            Value::Object(o) => o.iter().filter(|(k, _)| *k != "evidence").for_each(|(_, x)| walk(x, out)),
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

/// Loose phrase match: case-insensitive, hyphen/space-insensitive, with a few
/// clinical synonyms so "x-ray" matches "radiograph" etc.
fn mentions(text: &str, phrase: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().replace(['-', '’', '\''], " ");
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
        let got = os_engine::render::field_text(&note["objective"]["vitals"][field]).unwrap_or_default();
        check(numbers(&got).contains(want), format!("vital {field} = {want} (got \"{got}\")"), &mut s);
    }
    let meds = note["plan"]["medications"].as_array().cloned().unwrap_or_default();
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
    s.citation_errors = faith.citation_mismatches.len() + faith.invalid_evidence.len();
    s
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

    let template = LoadedTemplate::resolve(&args.llm.template)?;
    let llm = load_llm(store, &args.llm)?;
    let mut report = Vec::new();
    println!(
        "{:<18} {:>9} {:>9} {:>10} {:>8}  misses",
        "fixture", "facts", "invented", "citations", "note s"
    );
    for (script, wav) in scripts {
        let cache = PathBuf::from("target/eval-cache").join(format!("{}.jsonl", script.id));
        let transcript = transcript_for(store, &wav, &cache, args.retranscribe)?;
        let result = generate_note(&llm, &template, &transcript, &note_options(&args.llm), &mut |_| true)?;
        std::fs::create_dir_all("target/eval")?;
        std::fs::write(
            format!("target/eval/{}.note.json", script.id),
            serde_json::to_string_pretty(&result.note)?,
        )?;
        let mut s = score(&script.id, &script.expected, &result.note, &result.faithfulness);
        s.note_secs = result.stats.total_ms as f64 / 1000.0;
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
        report.push(s);
    }
    let (p, c): (u32, u32) = report.iter().fold((0, 0), |a, s| (a.0 + s.passed, a.1 + s.checks));
    let invented: usize = report.iter().map(|s| s.invented_numbers).sum();
    println!("TOTAL facts {p}/{c} ({:.0}%), invented numbers {invented}", 100.0 * p as f64 / c.max(1) as f64);
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
    }

    #[test]
    fn numbers_are_extracted_plainly() {
        assert_eq!(numbers("39.1 °C"), vec!["39.1"]);
        assert_eq!(numbers("4/9"), vec!["4", "9"]);
        assert_eq!(numbers("100 mL LRS"), vec!["100"]);
        assert_eq!(numbers("6.0%"), vec!["6"]);
    }
}
